//! Bunker configuration and signing mode management
//!
//! Handles parsing bunker:// URIs, storing/loading bunker config from a JSON
//! sidecar file alongside the marmot.db, and determining which signing mode
//! to use.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use nostr::prelude::*;
use serde::{Deserialize, Serialize};

/// Persistent bunker connection configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BunkerConfig {
    /// The remote signer's public key (hex)
    pub remote_signer_pubkey: String,
    /// Relay URLs the bunker listens on
    pub relays: Vec<String>,
    /// Connection secret/token (optional, used for initial connect)
    pub secret: Option<String>,
    /// Our local client keypair (hex) for NIP-46 communication
    /// Generated once and persisted so the bunker recognizes us
    pub client_secret_key: String,
    /// The user's public key as reported by the bunker (hex)
    /// Cached after first successful connection
    pub user_pubkey: Option<String>,
    /// When this config was created
    pub created_at: String,
    /// When last successfully connected to bunker
    pub last_connected: Option<String>,
}

impl BunkerConfig {
    /// Parse a bunker:// URI into a BunkerConfig
    ///
    /// Format: bunker://<remote-signer-pubkey>?relay=wss://...&secret=TOKEN
    pub fn from_bunker_uri(uri: &str) -> Result<Self> {
        // Validate it parses as a NostrConnectURI
        let parsed = NostrConnectURI::parse(uri)
            .context("Invalid bunker:// URI format")?;

        // Extract components
        let (remote_pubkey, relays, secret) = match &parsed {
            NostrConnectURI::Bunker {
                remote_signer_public_key,
                relays,
                secret,
            } => (
                remote_signer_public_key.to_hex(),
                relays.iter().map(|r| r.to_string()).collect::<Vec<_>>(),
                secret.clone(),
            ),
            NostrConnectURI::Client { .. } => {
                anyhow::bail!(
                    "Expected bunker:// URI, got nostrconnect:// URI.\n\
                     Use format: bunker://<pubkey>?relay=wss://...&secret=TOKEN"
                );
            }
        };

        if relays.is_empty() {
            anyhow::bail!(
                "Bunker URI must include at least one relay.\n\
                 Example: bunker://<pubkey>?relay=wss://relay.nsec.app&secret=TOKEN"
            );
        }

        // Generate a fresh client keypair for NIP-46 communication
        let client_keys = Keys::generate();

        Ok(BunkerConfig {
            remote_signer_pubkey: remote_pubkey,
            relays,
            secret,
            client_secret_key: client_keys.secret_key().to_secret_hex(),
            user_pubkey: None,
            created_at: chrono::Utc::now().to_rfc3339(),
            last_connected: None,
        })
    }

    /// Reconstruct the NostrConnectURI from stored config
    pub fn to_nostr_connect_uri(&self) -> Result<NostrConnectURI> {
        let pubkey = PublicKey::from_hex(&self.remote_signer_pubkey)
            .context("Invalid stored remote signer pubkey")?;
        let relays: Vec<RelayUrl> = self
            .relays
            .iter()
            .filter_map(|r| RelayUrl::parse(r).ok())
            .collect();

        Ok(NostrConnectURI::Bunker {
            remote_signer_public_key: pubkey,
            relays,
            secret: self.secret.clone(),
        })
    }

    /// Get the client Keys for NIP-46 communication
    pub fn client_keys(&self) -> Result<Keys> {
        let sk = SecretKey::from_hex(&self.client_secret_key)
            .context("Invalid stored client secret key")?;
        Ok(Keys::new(sk))
    }

    /// Get the cached user public key (if available)
    pub fn cached_user_pubkey(&self) -> Option<PublicKey> {
        self.user_pubkey
            .as_ref()
            .and_then(|hex| PublicKey::from_hex(hex).ok())
    }

    /// Config file path derived from the database path
    pub fn config_path(db_path: &Path) -> PathBuf {
        db_path.with_extension("bunker.json")
    }

    /// Load bunker config from disk
    pub fn load(db_path: &Path) -> Result<Option<Self>> {
        let path = Self::config_path(db_path);
        if !path.exists() {
            return Ok(None);
        }
        let content = std::fs::read_to_string(&path)
            .context("Failed to read bunker config")?;
        let config: BunkerConfig = serde_json::from_str(&content)
            .context("Failed to parse bunker config")?;
        Ok(Some(config))
    }

    /// Save bunker config to disk atomically
    pub fn save(&self, db_path: &Path) -> Result<()> {
        let path = Self::config_path(db_path);
        let tmp_path = path.with_extension("json.tmp");

        let content = serde_json::to_string_pretty(self)
            .context("Failed to serialize bunker config")?;

        // Write to temp file first
        std::fs::write(&tmp_path, &content)
            .context("Failed to write bunker config temp file")?;

        // Atomic rename
        std::fs::rename(&tmp_path, &path)
            .context("Failed to atomically save bunker config")?;

        // Set restrictive permissions (config contains client secret key)
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = std::fs::Permissions::from_mode(0o600);
            std::fs::set_permissions(&path, perms)?;
        }

        Ok(())
    }

    /// Delete bunker config from disk
    #[allow(dead_code)]
    pub fn delete(db_path: &Path) -> Result<()> {
        let path = Self::config_path(db_path);
        if path.exists() {
            std::fs::remove_file(&path)
                .context("Failed to delete bunker config")?;
        }
        Ok(())
    }

    /// Update last_connected timestamp and optionally user pubkey
    pub fn update_connected(&mut self, user_pubkey: Option<PublicKey>) {
        self.last_connected = Some(chrono::Utc::now().to_rfc3339());
        if let Some(pk) = user_pubkey {
            self.user_pubkey = Some(pk.to_hex());
        }
    }
}

/// Signing mode for the CLI
#[derive(Debug, Clone)]
pub enum SigningMode {
    /// Direct nsec — keys available locally
    DirectKey(Keys),
    /// NIP-46 remote signing via bunker
    Bunker(BunkerConfig),
}

impl SigningMode {
    /// Determine signing mode from CLI args and stored config
    pub fn resolve(
        nsec: Option<&str>,
        bunker_uri: Option<&str>,
        db_path: &Path,
    ) -> Result<Self> {
        // Explicit bunker URI takes highest priority
        if let Some(uri) = bunker_uri {
            let config = BunkerConfig::from_bunker_uri(uri)?;
            return Ok(SigningMode::Bunker(config));
        }

        // Explicit nsec
        if let Some(nsec) = nsec {
            let keys = if nsec.starts_with("nsec") {
                Keys::parse(nsec)?
            } else {
                let secret_key = SecretKey::from_hex(nsec)?;
                Keys::new(secret_key)
            };
            return Ok(SigningMode::DirectKey(keys));
        }

        // Check for stored bunker config
        if let Some(config) = BunkerConfig::load(db_path)? {
            return Ok(SigningMode::Bunker(config));
        }

        // No credentials at all
        anyhow::bail!(
            "No credentials provided. Use one of:\n\
             \n\
             🔒 Bunker mode (recommended for agents):\n\
             - marmot-cli --bunker \"bunker://<pubkey>?relay=wss://...&secret=TOKEN\" <command>\n\
             - Or run: marmot-cli init --bunker \"bunker://...\"\n\
             \n\
             🔑 Direct key mode:\n\
             - Set NOSTR_NSEC environment variable\n\
             - Or: marmot-cli --nsec \"nsec1...\" <command>\n\
             \n\
             ⚠️  For long-running agents, bunker mode is strongly recommended.\n\
             Direct nsec exposes your private key in the process environment."
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_valid_bunker_uri() {
        let uri = "bunker://79dff8f82963424e0bb02708a22e44b4980893e3a4be0fa3cb60a43b946764e3?relay=wss://relay.nsec.app&secret=test123";
        let config = BunkerConfig::from_bunker_uri(uri).unwrap();

        assert_eq!(
            config.remote_signer_pubkey,
            "79dff8f82963424e0bb02708a22e44b4980893e3a4be0fa3cb60a43b946764e3"
        );
        assert_eq!(config.relays, vec!["wss://relay.nsec.app"]);
        assert_eq!(config.secret, Some("test123".to_string()));
        assert!(config.user_pubkey.is_none());
        assert!(!config.client_secret_key.is_empty());
    }

    #[test]
    fn test_parse_bunker_uri_no_secret() {
        let uri = "bunker://79dff8f82963424e0bb02708a22e44b4980893e3a4be0fa3cb60a43b946764e3?relay=wss://relay.nsec.app";
        let config = BunkerConfig::from_bunker_uri(uri).unwrap();

        assert!(config.secret.is_none());
    }

    #[test]
    fn test_parse_bunker_uri_multiple_relays() {
        let uri = "bunker://79dff8f82963424e0bb02708a22e44b4980893e3a4be0fa3cb60a43b946764e3?relay=wss://relay.nsec.app&relay=wss://nos.lol";
        let config = BunkerConfig::from_bunker_uri(uri).unwrap();

        assert_eq!(config.relays.len(), 2);
    }

    #[test]
    fn test_parse_invalid_uri() {
        assert!(BunkerConfig::from_bunker_uri("not-a-bunker-uri").is_err());
        assert!(BunkerConfig::from_bunker_uri("https://example.com").is_err());
    }

    #[test]
    fn test_roundtrip_nostr_connect_uri() {
        let uri = "bunker://79dff8f82963424e0bb02708a22e44b4980893e3a4be0fa3cb60a43b946764e3?relay=wss://relay.nsec.app&secret=test123";
        let config = BunkerConfig::from_bunker_uri(uri).unwrap();
        let reconstructed = config.to_nostr_connect_uri().unwrap();
        assert!(matches!(reconstructed, NostrConnectURI::Bunker { .. }));
    }

    #[test]
    fn test_config_save_load() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("marmot.db");

        let uri = "bunker://79dff8f82963424e0bb02708a22e44b4980893e3a4be0fa3cb60a43b946764e3?relay=wss://relay.nsec.app&secret=test123";
        let config = BunkerConfig::from_bunker_uri(uri).unwrap();
        config.save(&db_path).unwrap();

        let loaded = BunkerConfig::load(&db_path).unwrap().unwrap();
        assert_eq!(loaded.remote_signer_pubkey, config.remote_signer_pubkey);
        assert_eq!(loaded.client_secret_key, config.client_secret_key);
        assert_eq!(loaded.secret, config.secret);
    }

    #[test]
    fn test_config_delete() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("marmot.db");

        let uri = "bunker://79dff8f82963424e0bb02708a22e44b4980893e3a4be0fa3cb60a43b946764e3?relay=wss://relay.nsec.app";
        let config = BunkerConfig::from_bunker_uri(uri).unwrap();
        config.save(&db_path).unwrap();

        assert!(BunkerConfig::load(&db_path).unwrap().is_some());
        BunkerConfig::delete(&db_path).unwrap();
        assert!(BunkerConfig::load(&db_path).unwrap().is_none());
    }

    #[test]
    fn test_signing_mode_resolve_nsec() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("marmot.db");

        let keys = Keys::generate();
        let nsec = keys.secret_key().to_bech32().unwrap();
        let mode = SigningMode::resolve(Some(&nsec), None, &db_path).unwrap();
        assert!(matches!(mode, SigningMode::DirectKey(_)));
    }

    #[test]
    fn test_signing_mode_resolve_bunker() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("marmot.db");

        let uri = "bunker://79dff8f82963424e0bb02708a22e44b4980893e3a4be0fa3cb60a43b946764e3?relay=wss://relay.nsec.app&secret=test";
        let mode = SigningMode::resolve(None, Some(uri), &db_path).unwrap();
        assert!(matches!(mode, SigningMode::Bunker(_)));
    }

    #[test]
    fn test_signing_mode_resolve_stored_bunker() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("marmot.db");

        // Save a bunker config
        let uri = "bunker://79dff8f82963424e0bb02708a22e44b4980893e3a4be0fa3cb60a43b946764e3?relay=wss://relay.nsec.app";
        let config = BunkerConfig::from_bunker_uri(uri).unwrap();
        config.save(&db_path).unwrap();

        // Should auto-detect stored bunker config
        let mode = SigningMode::resolve(None, None, &db_path).unwrap();
        assert!(matches!(mode, SigningMode::Bunker(_)));
    }

    #[test]
    fn test_signing_mode_no_credentials() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("marmot.db");

        let result = SigningMode::resolve(None, None, &db_path);
        assert!(result.is_err());
    }
}