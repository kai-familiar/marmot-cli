//! Unified signer abstraction for marmot-cli
//!
//! MarmotSigner wraps both direct Keys and NostrConnect (NIP-46) signing,
//! providing a consistent interface for the rest of the application.
//! 
//! Key design decisions:
//! - The nostr-sdk Client uses NostrSigner trait for event signing
//! - For direct mode, we use Keys (which impl NostrSigner)
//! - For bunker mode, we use NostrConnect (which impl NostrSigner)
//! - MDK operations need a PublicKey, not the full signer
//! - Gift-wrap operations (NIP-59) need the full signer for encryption

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use nostr::prelude::*;
use nostr_connect::prelude::*;
use nostr_sdk::prelude::*;
use tokio::sync::Mutex;

use super::audit::AuditLog;
use super::config::{BunkerConfig, SigningMode};

/// NIP-46 connection timeout
const BUNKER_TIMEOUT: Duration = Duration::from_secs(30);

/// Unified signer that supports both direct keys and NIP-46 remote signing
pub struct MarmotSigner {
    /// The signing mode
    mode: SignerMode,
    /// User's public key (available in both modes)
    public_key: PublicKey,
    /// Audit logger for signing operations
    audit: Arc<Mutex<AuditLog>>,
    /// DB path for persisting config updates
    #[allow(dead_code)]
    db_path: std::path::PathBuf,
}

enum SignerMode {
    /// Direct nsec signing (keys available locally)
    Direct {
        keys: Keys,
    },
    /// NIP-46 remote signing via bunker
    Bunker {
        connect: NostrConnect,
        #[allow(dead_code)]
        config: BunkerConfig,
    },
}

impl MarmotSigner {
    /// Create a new signer from the resolved signing mode
    pub async fn new(
        signing_mode: SigningMode,
        db_path: &Path,
        audit: Arc<Mutex<AuditLog>>,
    ) -> Result<Self> {
        match signing_mode {
            SigningMode::DirectKey(keys) => {
                let public_key = keys.public_key();

                // Warn about direct key usage for long-running processes
                if std::env::var("MARMOT_NO_NSEC_WARNING").is_err() {
                    eprintln!(
                        "⚠️  Using direct nsec signing. For long-running agents, consider \
                         bunker mode:\n   marmot-cli init --bunker \"bunker://...\"\n"
                    );
                }

                Ok(Self {
                    mode: SignerMode::Direct { keys },
                    public_key,
                    audit,
                    db_path: db_path.to_path_buf(),
                })
            }
            SigningMode::Bunker(mut config) => {
                // Create NostrConnect client
                let uri = config.to_nostr_connect_uri()?;
                let client_keys = config.client_keys()?;

                eprintln!("🔐 Connecting to bunker...");

                let connect = NostrConnect::new(uri, client_keys, BUNKER_TIMEOUT, None)
                    .map_err(|e| anyhow::anyhow!("Failed to create NIP-46 client: {}", e))?;

                // If we have a cached user pubkey, set it to avoid an extra round trip
                if let Some(cached_pk) = config.cached_user_pubkey() {
                    let _ = connect.non_secure_set_user_public_key(cached_pk);
                }

                // Get the user's public key from bunker (or use cached)
                let public_key = connect
                    .get_public_key()
                    .await
                    .map_err(|e| anyhow::anyhow!(
                        "Failed to get public key from bunker: {}\n\
                         \n\
                         Is the bunker online? Check:\n\
                         - Bunker process is running\n\
                         - Relay {} is accessible\n\
                         - Connection token is still valid",
                        e,
                        config.relays.first().unwrap_or(&"(none)".to_string())
                    ))?;

                eprintln!("✓ Connected to bunker (user: {})", &public_key.to_bech32().unwrap_or_default()[..20]);

                // Update config with successful connection
                config.update_connected(Some(public_key));
                config.save(db_path)?;

                {
                    let mut log = audit.lock().await;
                    log.record("bunker_connect", &format!("Connected to bunker, user pubkey: {}", public_key.to_hex()));
                }

                Ok(Self {
                    mode: SignerMode::Bunker { connect, config },
                    public_key,
                    audit,
                    db_path: db_path.to_path_buf(),
                })
            }
        }
    }

    /// Get the user's public key
    pub fn public_key(&self) -> PublicKey {
        self.public_key
    }

    /// Get Keys if in direct mode (needed for MDK operations that require Keys)
    #[allow(dead_code)]
    pub fn direct_keys(&self) -> Option<&Keys> {
        match &self.mode {
            SignerMode::Direct { keys } => Some(keys),
            SignerMode::Bunker { .. } => None,
        }
    }

    /// Check if we're in bunker mode
    pub fn is_bunker(&self) -> bool {
        matches!(self.mode, SignerMode::Bunker { .. })
    }

    /// Get the signing mode description for display
    pub fn mode_description(&self) -> &str {
        match &self.mode {
            SignerMode::Direct { .. } => "direct (nsec)",
            SignerMode::Bunker { .. } => "NIP-46 bunker",
        }
    }

    /// Sign an event using the appropriate method
    ///
    /// In direct mode: signs locally
    /// In bunker mode: sends sign_event request to bunker
    pub async fn sign_event(&self, builder: EventBuilder) -> Result<Event> {
        {
            let mut log = self.audit.lock().await;
            log.record("sign_event_request", "signing event");
        }

        let event = match &self.mode {
            SignerMode::Direct { keys } => {
                builder.sign(keys).await
                    .context("Failed to sign event with local keys")?
            }
            SignerMode::Bunker { connect, .. } => {
                let unsigned = builder.build(self.public_key);
                connect.sign_event(unsigned).await
                    .map_err(|e| anyhow::anyhow!("Bunker signing failed: {}\nIs the bunker still online?", e))?
            }
        };

        {
            let mut log = self.audit.lock().await;
            log.record("sign_event_success", &format!("event_id: {}, kind: {}", event.id.to_hex(), event.kind.as_u16()));
        }

        Ok(event)
    }

    /// Sign multiple events (batched for efficiency with bunker)
    ///
    /// In direct mode: signs all locally (fast)
    /// In bunker mode: signs sequentially (each requires bunker round-trip)
    #[allow(dead_code)]
    pub async fn sign_events(&self, builders: Vec<EventBuilder>) -> Result<Vec<Event>> {
        let count = builders.len();

        {
            let mut log = self.audit.lock().await;
            log.record("sign_batch_request", &format!("count: {}", count));
        }

        let mut events = Vec::with_capacity(count);
        for builder in builders {
            events.push(self.sign_event(builder).await?);
        }

        {
            let mut log = self.audit.lock().await;
            log.record("sign_batch_complete", &format!("count: {}", count));
        }

        Ok(events)
    }

    /// Create a gift-wrapped event (used for MLS welcome messages)
    ///
    /// This is one of the most critical operations for NIP-46 since gift-wrap
    /// requires NIP-44 encryption, which the bunker handles remotely.
    pub async fn gift_wrap(
        &self,
        receiver: &PublicKey,
        rumor: UnsignedEvent,
    ) -> Result<Event> {
        {
            let mut log = self.audit.lock().await;
            log.record("gift_wrap_request", &format!("receiver: {}", &receiver.to_hex()[..16]));
        }

        let event = match &self.mode {
            SignerMode::Direct { keys } => {
                EventBuilder::gift_wrap(keys, receiver, rumor, [])
                    .await
                    .context("Failed to create gift-wrapped event")?
            }
            SignerMode::Bunker { connect, .. } => {
                // Gift-wrap with NIP-46 signer
                // The nostr crate's gift_wrap function accepts NostrSigner
                EventBuilder::gift_wrap(connect, receiver, rumor, [])
                    .await
                    .map_err(|e| anyhow::anyhow!(
                        "Bunker gift-wrap failed: {}\n\
                         Gift wrapping requires NIP-44 encryption via the bunker.",
                        e
                    ))?
            }
        };

        {
            let mut log = self.audit.lock().await;
            log.record("gift_wrap_success", &format!("event_id: {}", event.id.to_hex()));
        }

        Ok(event)
    }

    /// Extract a rumor from a gift-wrapped event
    ///
    /// In direct mode: decrypts locally
    /// In bunker mode: uses bunker for NIP-44 decryption
    pub async fn extract_rumor(&self, event: &Event) -> Result<UnwrappedGift> {
        match &self.mode {
            SignerMode::Direct { keys } => {
                nostr::nips::nip59::extract_rumor(keys, event)
                    .await
                    .context("Failed to extract rumor from gift-wrap")
            }
            SignerMode::Bunker { connect, .. } => {
                nostr::nips::nip59::extract_rumor(connect, event)
                    .await
                    .map_err(|e| anyhow::anyhow!(
                        "Bunker gift-unwrap failed: {}\n\
                         Decrypting gift-wrapped messages requires the bunker to be online.",
                        e
                    ))
            }
        }
    }

    /// Build a nostr-sdk Client with the appropriate signer
    pub async fn build_client(&self) -> Result<Client> {
        let client = match &self.mode {
            SignerMode::Direct { keys } => {
                Client::builder().signer(keys.clone()).build()
            }
            SignerMode::Bunker { connect, .. } => {
                Client::builder().signer(connect.clone()).build()
            }
        };
        Ok(client)
    }

    /// Shutdown bunker connection (if in bunker mode)
    #[allow(dead_code)]
    pub async fn shutdown(self) {
        if let SignerMode::Bunker { connect, .. } = self.mode {
            connect.shutdown().await;
        }
    }
}
