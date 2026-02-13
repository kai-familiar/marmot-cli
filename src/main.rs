// Marmot CLI - E2E encrypted messaging over Nostr for agents
// Uses the Marmot Development Kit (MDK) for MLS protocol
// Compatible with Whitenoise (uses same MDK version)
//
// Supports two signing modes:
// - Direct nsec (legacy, convenient for development)
// - NIP-46 remote signing via bunker:// (recommended for production/agents)

mod nip46;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use mdk_core::prelude::*;
use mdk_sqlite_storage::MdkSqliteStorage;
use nostr::prelude::*;
use nostr_sdk::prelude::*;
use serde::Serialize;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::io::Write;
use tokio::sync::Mutex;

use nip46::{AuditLog, BunkerConfig, MarmotSigner, SigningMode};

/// JSON payload for --on-message callback
#[derive(Serialize)]
struct MessagePayload {
    message_id: String,
    group_id: String,
    group_name: String,
    sender: String,
    sender_hex: String,
    content: String,
    timestamp: u64,
    is_me: bool,
}

#[derive(Parser)]
#[command(name = "marmot-cli")]
#[command(about = "E2E encrypted messaging over Nostr using Marmot/MLS protocol")]
#[command(version)]
struct Cli {
    /// Path to the database file
    #[arg(short, long, default_value = "~/.marmot-cli/marmot.db")]
    db: String,

    /// Nostr private key (nsec or hex) — use bunker mode for production
    #[arg(short, long, env = "NOSTR_NSEC", hide_env_values = true)]
    nsec: Option<String>,

    /// Bunker URI for NIP-46 remote signing (recommended for agents)
    #[arg(short, long, env = "NOSTR_BUNKER")]
    bunker: Option<String>,

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
        /// Use NIP-46 bunker for remote signing (recommended for agents)
        #[arg(long)]
        bunker: Option<String>,
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
        /// Script/command to execute for each message (receives JSON via stdin)
        #[arg(long)]
        on_message: Option<String>,
    },
    /// Fetch key package for a user
    FetchKeyPackage {
        /// The npub to fetch key package for
        npub: String,
    },
    /// Migrate from direct nsec to NIP-46 bunker signing (atomic)
    MigrateToBunker {
        /// Bunker URI: bunker://<pubkey>?relay=wss://...&secret=TOKEN
        bunker: String,
    },
    /// Show signing mode and bunker connection status
    SignerStatus,
}

struct MarmotCli {
    signer: MarmotSigner,
    mdk: MDK<MdkSqliteStorage>,
    relays: Vec<RelayUrl>,
    client: Client,
    #[allow(dead_code)]
    db_path: PathBuf,
}

impl MarmotCli {
    async fn new(
        db_path: PathBuf,
        nsec: Option<String>,
        bunker_uri: Option<String>,
        relay_urls: Vec<String>,
    ) -> Result<Self> {
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let audit = Arc::new(Mutex::new(AuditLog::new(&db_path)));

        let signing_mode = SigningMode::resolve(
            nsec.as_deref(),
            bunker_uri.as_deref(),
            &db_path,
        )?;

        let signer = MarmotSigner::new(signing_mode, &db_path, audit).await?;

        let storage = MdkSqliteStorage::new_unencrypted(&db_path)
            .context("Failed to create SQLite storage")?;
        let mdk = MDK::new(storage);

        let relays: Vec<RelayUrl> = relay_urls
            .iter()
            .filter_map(|url| RelayUrl::parse(url).ok())
            .collect();

        let client = signer.build_client().await?;
        for relay in &relays {
            client.add_relay(relay.as_str()).await?;
        }
        client.connect().await;
        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

        Ok(Self { signer, mdk, relays, client, db_path })
    }

    fn whoami(&self) {
        println!("=== Marmot CLI Identity ===");
        println!("npub:   {}", self.signer.public_key().to_bech32().unwrap());
        println!("hex:    {}", self.signer.public_key());
        println!("signer: {}", self.signer.mode_description());
        if self.signer.is_bunker() {
            println!("\n🔐 Using NIP-46 remote signing (bunker mode)");
            println!("   Private key never leaves the bunker.");
        }
        println!("\nRelays:");
        for relay in &self.relays {
            println!("  - {}", relay);
        }
    }

    async fn publish_key_package(&self) -> Result<()> {
        println!("Creating and publishing key package...");
        if self.signer.is_bunker() {
            println!("   (bunker must be online for signing)");
        }

        let (key_package_encoded, tags) = self.mdk
            .create_key_package_for_event(&self.signer.public_key(), self.relays.clone())?;

        let builder = EventBuilder::new(Kind::MlsKeyPackage, key_package_encoded)
            .tags(tags)
            .tag(Tag::custom(TagKind::custom("encoding"), ["hex"]));

        let event = self.signer.sign_event(builder).await?;
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

        let filter = Filter::new().kind(Kind::MlsKeyPackage).author(pubkey).limit(1);
        let events = self.client
            .fetch_events(filter, std::time::Duration::from_secs(10))
            .await?;

        let event = events.first().cloned()
            .context("No key package found for this user. They need to run `publish-key-package` first.")?;
        println!("✓ Found key package (event: {})", event.id.to_hex());
        Ok(event)
    }

    async fn create_chat(&self, npub: &str, name: Option<String>) -> Result<()> {
        let other_key_package_event = self.fetch_key_package(npub).await?;
        let other_pubkey = other_key_package_event.pubkey;
        let group_name = name.unwrap_or_else(|| {
            format!("Chat with {}", &other_pubkey.to_bech32().unwrap()[..20])
        });
        println!("Creating group '{}'...", group_name);

        let config = NostrGroupConfigData::new(
            group_name.clone(), "Marmot CLI chat".to_string(),
            None, None, None, self.relays.clone(), vec![self.signer.public_key()],
        );
        let result = self.mdk.create_group(
            &self.signer.public_key(), vec![other_key_package_event], config,
        )?;

        for welcome_rumor in &result.welcome_rumors {
            let gift_wrap = self.signer.gift_wrap(&other_pubkey, welcome_rumor.clone()).await?;
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
                    let is_me = *member == self.signer.public_key();
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
        let rumor = EventBuilder::new(Kind::Custom(9), message)
            .build(self.signer.public_key());
        let message_event = self.mdk.create_message(&mls_group_id, rumor)?;
        let send_result = self.client.send_event(&message_event).await?;
        println!("✓ Message sent to {} relays", send_result.success.len());
        Ok(())
    }

    async fn receive_messages(&self) -> Result<(usize, usize, Vec<MessagePayload>)> {
        let mut welcomes_found = 0;
        let mut messages_found = 0;
        let mut payloads: Vec<MessagePayload> = Vec::new();

        // Phase 1: Fetch and process gift-wrapped welcome messages
        let filter = Filter::new()
            .kind(Kind::GiftWrap)
            .pubkey(self.signer.public_key())
            .limit(100);
        let events = self.client
            .fetch_events(filter, std::time::Duration::from_secs(10))
            .await?;

        for event in events.iter() {
            match self.signer.extract_rumor(event).await {
                Ok(unwrapped) => {
                    if unwrapped.rumor.kind == Kind::MlsWelcome {
                        match self.mdk.process_welcome(&event.id, &unwrapped.rumor) {
                            Ok(_) => {
                                welcomes_found += 1;
                                println!("📨 New welcome received (event: {})", event.id.to_hex());
                            }
                            Err(e) => { tracing::debug!("Welcome processing: {}", e); }
                        }
                    }
                }
                Err(e) => { tracing::debug!("Could not unwrap gift-wrap: {}", e); }
            }
        }

        // Phase 2: Check for pending welcomes
        if let Ok(pending) = self.mdk.get_pending_welcomes(None) {
            for welcome in &pending {
                println!("⏳ Pending welcome: '{}' (event: {})", welcome.group_name, &welcome.id.to_hex()[..16]);
                println!("   Run: marmot-cli accept-welcome {}", welcome.id.to_hex());
            }
        }

        // Phase 3: Fetch and process group messages
        let groups = self.mdk.get_groups()?;
        for group in &groups {
            let nostr_group_id = hex::encode(&group.nostr_group_id);
            let filter = Filter::new()
                .kind(Kind::MlsGroupMessage)
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
                                let sender = msg.pubkey.to_bech32().unwrap_or_else(|_| "unknown".to_string());
                                let is_me = msg.pubkey == self.signer.public_key();
                                let prefix = if is_me { "→ You".to_string() } else { sender.clone() };
                                println!("[{}] {}: {}", group.name, prefix, msg.content);
                                payloads.push(MessagePayload {
                                    message_id: event.id.to_hex(),
                                    group_id: hex::encode(group.mls_group_id.as_slice()),
                                    group_name: group.name.clone(),
                                    sender, sender_hex: msg.pubkey.to_hex(),
                                    content: msg.content.clone(),
                                    timestamp: event.created_at.as_secs(),
                                    is_me,
                                });
                            }
                            MessageProcessingResult::Commit { .. } => {
                                tracing::debug!("Processed commit for group {}", group.name);
                            }
                            _ => {}
                        }
                    }
                    Err(e) => { tracing::debug!("Message processing: {}", e); }
                }
            }
        }

        Ok((welcomes_found, messages_found, payloads))
    }

    fn invoke_callback(script: &str, payload: &MessagePayload) -> Result<i32> {
        let json = serde_json::to_string(payload)?;
        let mut child = Command::new("sh")
            .arg("-c").arg(script)
            .stdin(Stdio::piped())
            .spawn().context("Failed to spawn callback process")?;
        if let Some(mut stdin) = child.stdin.take() {
            stdin.write_all(json.as_bytes())?;
        }
        let status = child.wait()?;
        Ok(status.code().unwrap_or(-1))
    }

    async fn accept_welcome(&self, event_id_str: &str) -> Result<()> {
        let event_id = EventId::from_hex(event_id_str)
            .or_else(|_| EventId::from_bech32(event_id_str))
            .context("Invalid event ID")?;
        let welcome = self.mdk.get_welcome(&event_id)?
            .context("Welcome not found. Run `receive` first to fetch pending welcomes.")?;
        self.mdk.accept_welcome(&welcome)?;
        println!("✓ Welcome accepted! You've joined the group.");
        let groups = self.mdk.get_groups()?;
        if let Some(latest) = groups.last() {
            println!("  Group: {}", latest.name);
            println!("  MLS ID: {}", hex::encode(latest.mls_group_id.as_slice()));
        }
        Ok(())
    }
}

/// Migrate from nsec to bunker mode (standalone, doesn't need full MarmotCli)
async fn migrate_to_bunker(db_path: &PathBuf, bunker_uri: &str, current_nsec: Option<&str>) -> Result<()> {
    // Step 1: Parse the bunker URI
    println!("🔐 Migrating to NIP-46 bunker signing...\n");
    let mut config = BunkerConfig::from_bunker_uri(bunker_uri)?;

    // Step 2: Check if there's already a bunker config
    if BunkerConfig::load(db_path)?.is_some() {
        anyhow::bail!(
            "A bunker configuration already exists.\n\
             Delete it first with: rm {}\n\
             Or use init --bunker to start fresh.",
            BunkerConfig::config_path(db_path).display()
        );
    }

    // Step 3: Connect to bunker and get user pubkey
    println!("   Connecting to bunker...");
    let uri = config.to_nostr_connect_uri()?;
    let client_keys = config.client_keys()?;
    let connect = nostr_connect::prelude::NostrConnect::new(
        uri, client_keys, std::time::Duration::from_secs(30), None,
    ).map_err(|e| anyhow::anyhow!("Failed to create NIP-46 client: {}", e))?;

    let bunker_pubkey = connect.get_public_key().await
        .map_err(|e| anyhow::anyhow!("Failed to connect to bunker: {}", e))?;
    println!("   Bunker user pubkey: {}", bunker_pubkey.to_bech32().unwrap_or_default());

    // Step 4: If we have the current nsec, verify identity matches
    if let Some(nsec) = current_nsec {
        let current_keys = if nsec.starts_with("nsec") {
            Keys::parse(nsec)?
        } else {
            Keys::new(SecretKey::from_hex(nsec)?)
        };

        if current_keys.public_key() != bunker_pubkey {
            anyhow::bail!(
                "Identity mismatch!\n\
                 Current nsec pubkey: {}\n\
                 Bunker pubkey:       {}\n\n\
                 The bunker must control the same Nostr identity.\n\
                 Your MLS group state is tied to your public key.",
                current_keys.public_key().to_bech32().unwrap_or_default(),
                bunker_pubkey.to_bech32().unwrap_or_default()
            );
        }
        println!("   ✓ Identity verified (pubkeys match)");
    } else {
        println!("   ⚠️  No current nsec provided — cannot verify identity match.");
        println!("   Make sure the bunker controls the same key used for your MLS groups.");
    }

    // Step 5: Save config atomically
    config.update_connected(Some(bunker_pubkey));
    config.save(db_path)?;
    println!("   ✓ Bunker config saved to {}", BunkerConfig::config_path(db_path).display());

    // Step 6: Shutdown bunker connection
    connect.shutdown().await;

    println!("\n✅ Migration complete!");
    println!("\nNext steps:");
    println!("  1. Remove NOSTR_NSEC from your environment");
    println!("  2. Remove nsec from .credentials/nostr.json (if used)");
    println!("  3. Test: marmot-cli whoami  (should show bunker mode)");
    println!("  4. Keep your bunker process running for signing operations");
    println!("\n🔒 Your nsec is no longer needed by marmot-cli.");
    Ok(())
}

fn show_signer_status(db_path: &PathBuf, nsec: Option<&str>, bunker_uri: Option<&str>) -> Result<()> {
    println!("=== Marmot CLI Signer Status ===\n");

    // Check for bunker config
    if let Some(config) = BunkerConfig::load(db_path)? {
        println!("Mode: 🔐 NIP-46 Bunker (stored)");
        println!("  Remote signer: {}", &config.remote_signer_pubkey[..16]);
        println!("  Relays: {}", config.relays.join(", "));
        if let Some(ref pk) = config.user_pubkey {
            println!("  User pubkey: {}...", &pk[..16]);
        }
        println!("  Created: {}", config.created_at);
        if let Some(ref last) = config.last_connected {
            println!("  Last connected: {}", last);
        }
        println!("  Config file: {}", BunkerConfig::config_path(db_path).display());
    } else if bunker_uri.is_some() {
        println!("Mode: 🔐 NIP-46 Bunker (from CLI/env, not yet stored)");
    } else if nsec.is_some() {
        println!("Mode: 🔑 Direct nsec");
        println!("  ⚠️  Consider migrating to bunker mode for production use:");
        println!("     marmot-cli migrate-to-bunker --bunker \"bunker://...\"");
    } else {
        println!("Mode: ❌ No credentials configured");
    }

    // Check audit log
    let audit_path = db_path.with_extension("audit.jsonl");
    if audit_path.exists() {
        if let Ok(metadata) = std::fs::metadata(&audit_path) {
            println!("\nAudit log: {} ({} bytes)", audit_path.display(), metadata.len());
        }
    }

    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    use tracing_subscriber::EnvFilter;
    let default_filter = if cli.quiet { "warn,nostr_relay_pool=off" } else { "info" };
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(default_filter));
    let subscriber = tracing_subscriber::FmtSubscriber::builder()
        .with_env_filter(filter)
        .with_target(true)
        .with_writer(std::io::stderr)
        .finish();
    tracing::subscriber::set_global_default(subscriber)?;

    let db_path = PathBuf::from(cli.db.replace("~", &std::env::var("HOME").unwrap_or_default()));
    let relay_urls: Vec<String> = cli.relays.split(',').map(|s| s.trim().to_string()).collect();

    // Handle commands that don't need full MarmotCli initialization
    match &cli.command {
        Commands::MigrateToBunker { bunker } => {
            return migrate_to_bunker(&db_path, bunker, cli.nsec.as_deref()).await;
        }
        Commands::SignerStatus => {
            return show_signer_status(&db_path, cli.nsec.as_deref(), cli.bunker.as_deref());
        }
        _ => {}
    }

    // For Init with bunker, handle specially
    if let Commands::Init { nsec: _init_nsec, bunker: Some(bunker_uri) } = &cli.command {
        // Store bunker config and show identity
        let mut config = BunkerConfig::from_bunker_uri(bunker_uri)?;
        println!("🔐 Initializing with NIP-46 bunker...");

        let uri = config.to_nostr_connect_uri()?;
        let client_keys = config.client_keys()?;
        let connect = nostr_connect::prelude::NostrConnect::new(
            uri, client_keys, std::time::Duration::from_secs(30), None,
        ).map_err(|e| anyhow::anyhow!("Failed to create NIP-46 client: {}", e))?;

        let pubkey = connect.get_public_key().await
            .map_err(|e| anyhow::anyhow!("Failed to connect to bunker: {}", e))?;

        config.update_connected(Some(pubkey));

        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        config.save(&db_path)?;
        connect.shutdown().await;

        println!("✓ Bunker configured!");
        println!("  npub: {}", pubkey.to_bech32().unwrap_or_default());
        println!("  hex:  {}", pubkey);
        println!("  Config: {}", BunkerConfig::config_path(&db_path).display());
        println!("\nNext: marmot-cli publish-key-package");
        return Ok(());
    }

    // Merge init nsec with global nsec
    let effective_nsec = match &cli.command {
        Commands::Init { nsec: Some(init_nsec), .. } => Some(init_nsec.clone()),
        _ => cli.nsec,
    };

    let marmot = MarmotCli::new(db_path, effective_nsec, cli.bunker, relay_urls).await?;

    match cli.command {
        Commands::Init { .. } => {
            println!("Initialized with provided credentials");
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
            let (welcomes, messages, _) = marmot.receive_messages().await?;
            if welcomes == 0 && messages == 0 {
                println!("No new messages.");
            } else {
                println!("\n--- {} welcome(s), {} message(s) ---", welcomes, messages);
            }
        }
        Commands::AcceptWelcome { event_id } => {
            marmot.accept_welcome(&event_id).await?;
        }
        Commands::Listen { interval, on_message } => {
            if let Some(ref script) = on_message {
                println!("Listening for messages with callback (Ctrl+C to stop, poll every {}s)...", interval);
                println!("Callback: {}", script);
            } else {
                println!("Listening for messages (Ctrl+C to stop, poll every {}s)...", interval);
            }
            loop {
                let (w, m, payloads) = marmot.receive_messages().await?;
                if let Some(ref script) = on_message {
                    for payload in &payloads {
                        if payload.is_me { continue; }
                        match MarmotCli::invoke_callback(script, payload) {
                            Ok(0) => { tracing::debug!("Callback succeeded for message {}", payload.message_id); }
                            Ok(code) => { eprintln!("⚠️ Callback exited with code {} for message {}", code, &payload.message_id[..16]); }
                            Err(e) => { eprintln!("❌ Callback failed for message {}: {}", &payload.message_id[..16], e); }
                        }
                    }
                }
                if w > 0 || m > 0 {
                    println!("--- {} welcome(s), {} message(s) ---", w, m);
                }
                tokio::time::sleep(tokio::time::Duration::from_secs(interval)).await;
            }
        }
        Commands::FetchKeyPackage { npub } => {
            marmot.fetch_key_package(&npub).await?;
        }
        Commands::MigrateToBunker { .. } | Commands::SignerStatus => {
            unreachable!("Handled above");
        }
    }

    Ok(())
}
