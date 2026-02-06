// Marmot CLI - E2E encrypted messaging over Nostr for agents
// Uses the Marmot Development Kit (MDK) for MLS protocol
// Compatible with Whitenoise (uses same MDK version)

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use mdk_core::prelude::*;
use mdk_sqlite_storage::MdkSqliteStorage;
use nostr::prelude::*;
use nostr::nips::nip59;
use nostr_sdk::prelude::*;
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "marmot-cli")]
#[command(about = "E2E encrypted messaging over Nostr using Marmot/MLS protocol")]
struct Cli {
    /// Path to the database file
    #[arg(short, long, default_value = "~/.marmot-cli/marmot.db")]
    db: String,

    /// Nostr private key (nsec or hex)
    #[arg(short, long, env = "NOSTR_NSEC", hide_env_values = true)]
    nsec: Option<String>,

    /// Relay URLs (comma-separated)
    #[arg(short, long, default_value = "wss://relay.damus.io,wss://relay.primal.net,wss://nos.lol")]
    relays: String,

    /// Suppress relay connection logs
    #[arg(short, long, default_value_t = false)]
    quiet: bool,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Initialize with a new or existing Nostr identity
    Init {
        /// Use existing nsec (otherwise generates new keys)
        #[arg(long)]
        nsec: Option<String>,
    },

    /// Show current identity info
    Whoami,

    /// Publish key package to relays (required before others can message you)
    PublishKeyPackage,

    /// Create a new group/chat with another user
    CreateChat {
        /// The npub of the user to chat with
        npub: String,
        /// Optional group name
        #[arg(short, long)]
        name: Option<String>,
    },

    /// List all groups/chats
    ListChats,

    /// Send a message to a group
    Send {
        /// Group ID (hex, from list-chats). Can be partial.
        #[arg(short, long)]
        group: String,
        /// Message content
        message: String,
    },

    /// Receive and process pending messages
    Receive,

    /// Accept a pending welcome (join a group you've been invited to)
    AcceptWelcome {
        /// Welcome event ID (from receive output)
        event_id: String,
    },

    /// Listen for incoming messages (runs continuously)
    Listen {
        /// Poll interval in seconds
        #[arg(short, long, default_value_t = 5)]
        interval: u64,
    },

    /// Fetch key package for a user
    FetchKeyPackage {
        /// The npub to fetch key package for
        npub: String,
    },
}

struct MarmotCli {
    keys: Keys,
    mdk: MDK<MdkSqliteStorage>,
    relays: Vec<RelayUrl>,
    client: Client,
}

impl MarmotCli {
    async fn new(db_path: PathBuf, nsec: Option<String>, relay_urls: Vec<String>) -> Result<Self> {
        // Create database directory if needed
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        // Load or generate keys
        let keys = if let Some(nsec) = nsec {
            if nsec.starts_with("nsec") {
                Keys::parse(&nsec)?
            } else {
                // Try as hex
                let secret_key = SecretKey::from_hex(&nsec)?;
                Keys::new(secret_key)
            }
        } else {
            Keys::generate()
        };

        // Initialize MDK with SQLite storage
        let storage = MdkSqliteStorage::new(&db_path)
            .context("Failed to create SQLite storage")?;
        let mdk = MDK::new(storage);

        // Parse relay URLs
        let relays: Vec<RelayUrl> = relay_urls
            .iter()
            .filter_map(|url| RelayUrl::parse(url).ok())
            .collect();

        // Initialize Nostr client
        let client = Client::builder().signer(keys.clone()).build();
        for relay in &relays {
            client.add_relay(relay.as_str()).await?;
        }
        client.connect().await;
        
        // Wait for relay connections to establish
        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

        Ok(Self {
            keys,
            mdk,
            relays,
            client,
        })
    }

    fn whoami(&self) {
        println!("=== Marmot CLI Identity ===");
        println!("npub: {}", self.keys.public_key().to_bech32().unwrap());
        println!("hex:  {}", self.keys.public_key());
        println!("\nRelays:");
        for relay in &self.relays {
            println!("  - {}", relay);
        }
    }

    async fn publish_key_package(&self) -> Result<()> {
        println!("Creating and publishing key package...");
        
        let (key_package_encoded, tags) = self.mdk
            .create_key_package_for_event(&self.keys.public_key(), self.relays.clone())?;

        // Build event with MDK-generated tags + encoding tag
        let event = EventBuilder::new(Kind::MlsKeyPackage, key_package_encoded)
            .tags(tags)
            .tag(Tag::custom(TagKind::custom("encoding"), ["hex"]))
            .sign(&self.keys)
            .await?;

        let output = self.client.send_event(&event).await?;
        
        println!("✓ Key package published!");
        println!("  Event ID: {}", output.id());
        println!("  Published to {} relays", output.success.len());
        
        Ok(())
    }

    async fn fetch_key_package(&self, npub: &str) -> Result<Event> {
        let pubkey = if npub.starts_with("npub") {
            PublicKey::from_bech32(npub)?
        } else {
            PublicKey::from_hex(npub)?
        };

        println!("Fetching key package for {}...", &pubkey.to_bech32().unwrap()[..20]);

        let filter = Filter::new()
            .kind(Kind::MlsKeyPackage)
            .author(pubkey)
            .limit(1);

        let events = self.client
            .fetch_events(filter, std::time::Duration::from_secs(10))
            .await?;

        let event = events
            .first()
            .cloned()
            .context("No key package found for this user. They need to run `publish-key-package` first.")?;

        println!("✓ Found key package (event: {})", &event.id.to_hex()[..16]);
        Ok(event)
    }

    async fn create_chat(&self, npub: &str, name: Option<String>) -> Result<()> {
        // Fetch the other user's key package
        let other_key_package_event = self.fetch_key_package(npub).await?;
        let other_pubkey = other_key_package_event.pubkey;

        let group_name = name.unwrap_or_else(|| {
            format!("Chat with {}", &other_pubkey.to_bech32().unwrap()[..20])
        });

        println!("Creating group '{}'...", group_name);

        // Creator is the admin; both are members (members are added via key packages)
        let config = NostrGroupConfigData::new(
            group_name.clone(),
            "Marmot CLI chat".to_string(),
            None, None, None,
            self.relays.clone(),
            vec![self.keys.public_key()], // only creator is admin
        );

        let result = self.mdk.create_group(
            &self.keys.public_key(),
            vec![other_key_package_event],
            config,
        )?;

        // Gift-wrap and send welcome to each invited member
        for welcome_rumor in &result.welcome_rumors {
            let gift_wrap = EventBuilder::gift_wrap(
                &self.keys,
                &other_pubkey,
                welcome_rumor.clone(),
                [],
            ).await?;
            
            let send_result = self.client.send_event(&gift_wrap).await?;
            println!("✓ Welcome sent to {} relays", send_result.success.len());
        }

        let mls_group_id = hex::encode(result.group.mls_group_id.as_slice());
        let nostr_group_id = hex::encode(&result.group.nostr_group_id);

        println!("\n✓ Group created!");
        println!("  Name:           {}", group_name);
        println!("  MLS Group ID:   {}", mls_group_id);
        println!("  Nostr Group ID: {}", nostr_group_id);
        println!("\nUse this to send messages:");
        println!("  marmot-cli send -g {} \"Hello!\"", &mls_group_id[..16]);

        Ok(())
    }

    fn list_chats(&self) -> Result<()> {
        let groups = self.mdk.get_groups()?;
        
        if groups.is_empty() {
            println!("No chats found. Create one with: marmot-cli create-chat <npub>");
            return Ok(());
        }

        println!("=== Your Chats ({}) ===\n", groups.len());
        for group in groups {
            let mls_id = hex::encode(group.mls_group_id.as_slice());
            println!("📱 {} [epoch {}]", group.name, group.epoch);
            println!("   MLS ID: {} (use first 8+ chars with -g)", mls_id);
            println!("   Nostr ID: {}", hex::encode(&group.nostr_group_id));
            
            if let Ok(members) = self.mdk.get_members(&group.mls_group_id) {
                println!("   Members: {}", members.len());
                for member in &members {
                    let bech32 = member.to_bech32().unwrap_or_else(|_| member.to_string());
                    let is_me = *member == self.keys.public_key();
                    println!("     {} {}", if is_me { "→" } else { " " }, &bech32[..20]);
                }
            }

            if let Some(last) = &group.last_message_at {
                println!("   Last message: {}", last);
            }
            println!();
        }

        Ok(())
    }

    /// Resolve a potentially partial group ID to a full MLS group ID
    fn resolve_group_id(&self, partial: &str) -> Result<GroupId> {
        let groups = self.mdk.get_groups()?;
        let partial_lower = partial.to_lowercase();
        
        let matches: Vec<_> = groups.iter().filter(|g| {
            let mls_hex = hex::encode(g.mls_group_id.as_slice());
            let nostr_hex = hex::encode(&g.nostr_group_id);
            mls_hex.starts_with(&partial_lower) || nostr_hex.starts_with(&partial_lower)
        }).collect();

        match matches.len() {
            0 => anyhow::bail!("No group found matching '{}'", partial),
            1 => Ok(matches[0].mls_group_id.clone()),
            n => {
                eprintln!("Ambiguous group ID '{}' matches {} groups:", partial, n);
                for g in &matches {
                    eprintln!("  - {} ({})", g.name, hex::encode(g.mls_group_id.as_slice()));
                }
                anyhow::bail!("Use a longer prefix to disambiguate")
            }
        }
    }

    async fn send_message(&self, group_id_str: &str, message: &str) -> Result<()> {
        let mls_group_id = self.resolve_group_id(group_id_str)?;
        
        // Create the inner message event (Kind 9 = chat message per NIP-EE/Marmot spec)
        // This MUST be unsigned per spec
        let rumor = EventBuilder::new(Kind::Custom(9), message)
            .build(self.keys.public_key());

        let message_event = self.mdk.create_message(&mls_group_id, rumor)?;

        let send_result = self.client.send_event(&message_event).await?;
        
        println!("✓ Message sent to {} relays", send_result.success.len());
        Ok(())
    }

    async fn receive_messages(&self) -> Result<(usize, usize)> {
        let mut welcomes_found = 0;
        let mut messages_found = 0;

        // === Phase 1: Fetch and process gift-wrapped welcome messages ===
        let filter = Filter::new()
            .kind(Kind::GiftWrap)
            .pubkey(self.keys.public_key())
            .limit(100);

        let events = self.client
            .fetch_events(filter, std::time::Duration::from_secs(10))
            .await?;

        for event in events.iter() {
            match nip59::extract_rumor(&self.keys, event).await {
                Ok(unwrapped) => {
                    // Kind 444 = MLS Welcome
                    if unwrapped.rumor.kind == Kind::MlsWelcome {
                        match self.mdk.process_welcome(&event.id, &unwrapped.rumor) {
                            Ok(_) => {
                                welcomes_found += 1;
                                println!("📨 New welcome received (event: {})", &event.id.to_hex()[..16]);
                            }
                            Err(e) => {
                                // May already be processed
                                tracing::debug!("Welcome processing: {}", e);
                            }
                        }
                    }
                }
                Err(e) => {
                    tracing::debug!("Could not unwrap gift-wrap: {}", e);
                }
            }
        }

        // === Phase 2: Check for pending welcomes and show them ===
        if let Ok(pending) = self.mdk.get_pending_welcomes() {
            for welcome in &pending {
                println!("⏳ Pending welcome: '{}' (event: {})", welcome.group_name, &welcome.id.to_hex()[..16]);
                println!("   Run: marmot-cli accept-welcome {}", welcome.id.to_hex());
            }
        }

        // === Phase 3: Fetch and process group messages for all groups ===
        let groups = self.mdk.get_groups()?;
        for group in &groups {
            let nostr_group_id = hex::encode(&group.nostr_group_id);
            let filter = Filter::new()
                .kind(Kind::MlsGroupMessage) // Kind 445
                .custom_tag(SingleLetterTag::lowercase(Alphabet::H), nostr_group_id.clone())
                .limit(50);

            let events = self.client
                .fetch_events(filter, std::time::Duration::from_secs(10))
                .await?;

            for event in events.iter() {
                match self.mdk.process_message(event) {
                    Ok(result) => {
                        match result {
                            MessageProcessingResult::ApplicationMessage(msg) => {
                                messages_found += 1;
                                let sender = msg.pubkey.to_bech32()
                                    .unwrap_or_else(|_| "unknown".to_string());
                                let is_me = msg.pubkey == self.keys.public_key();
                                let prefix = if is_me { "→ You" } else { &sender[..16] };
                                println!("[{}] {}: {}", group.name, prefix, msg.content);
                            }
                            MessageProcessingResult::Commit { .. } => {
                                tracing::debug!("Processed commit for group {}", group.name);
                            }
                            _ => {}
                        }
                    }
                    Err(e) => {
                        tracing::debug!("Message processing: {}", e);
                    }
                }
            }
        }

        Ok((welcomes_found, messages_found))
    }

    async fn accept_welcome(&self, event_id_str: &str) -> Result<()> {
        let event_id = EventId::from_hex(event_id_str)
            .or_else(|_| EventId::from_bech32(event_id_str))
            .context("Invalid event ID")?;

        let welcome = self.mdk.get_welcome(&event_id)?
            .context("Welcome not found. Run `receive` first to fetch pending welcomes.")?;

        self.mdk.accept_welcome(&welcome)?;

        println!("✓ Welcome accepted! You've joined the group.");
        
        // Show the group we just joined
        let groups = self.mdk.get_groups()?;
        if let Some(latest) = groups.last() {
            println!("  Group: {}", latest.name);
            println!("  MLS ID: {}", hex::encode(latest.mls_group_id.as_slice()));
        }

        Ok(())
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Configure logging based on quiet flag
    use tracing_subscriber::EnvFilter;
    
    let default_filter = if cli.quiet {
        "warn,nostr_relay_pool=off"
    } else {
        "info"
    };
    
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(default_filter));

    let subscriber = tracing_subscriber::FmtSubscriber::builder()
        .with_env_filter(filter)
        .with_target(true)
        .finish();
    tracing::subscriber::set_global_default(subscriber)?;

    // Expand ~ in path
    let db_path = PathBuf::from(cli.db.replace("~", &std::env::var("HOME").unwrap_or_default()));
    
    let relay_urls: Vec<String> = cli.relays.split(',').map(|s| s.trim().to_string()).collect();
    
    let marmot = MarmotCli::new(db_path, cli.nsec, relay_urls).await?;

    match cli.command {
        Commands::Init { nsec } => {
            if nsec.is_some() {
                println!("Initialized with provided nsec");
            } else {
                println!("Generated new identity");
            }
            marmot.whoami();
        }
        Commands::Whoami => {
            marmot.whoami();
        }
        Commands::PublishKeyPackage => {
            marmot.publish_key_package().await?;
        }
        Commands::CreateChat { npub, name } => {
            marmot.create_chat(&npub, name).await?;
        }
        Commands::ListChats => {
            marmot.list_chats()?;
        }
        Commands::Send { group, message } => {
            marmot.send_message(&group, &message).await?;
        }
        Commands::Receive => {
            println!("Checking for new messages...");
            let (welcomes, messages) = marmot.receive_messages().await?;
            if welcomes == 0 && messages == 0 {
                println!("No new messages.");
            } else {
                println!("\n--- {} welcome(s), {} message(s) ---", welcomes, messages);
            }
        }
        Commands::AcceptWelcome { event_id } => {
            marmot.accept_welcome(&event_id).await?;
        }
        Commands::Listen { interval } => {
            println!("Listening for messages (Ctrl+C to stop, poll every {}s)...", interval);
            loop {
                let (w, m) = marmot.receive_messages().await?;
                if w > 0 || m > 0 {
                    println!("--- {} welcome(s), {} message(s) ---", w, m);
                }
                tokio::time::sleep(tokio::time::Duration::from_secs(interval)).await;
            }
        }
        Commands::FetchKeyPackage { npub } => {
            marmot.fetch_key_package(&npub).await?;
        }
    }

    Ok(())
}
