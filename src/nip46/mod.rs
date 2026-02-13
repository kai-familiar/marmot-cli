//! NIP-46 Remote Signing Support
//!
//! This module provides bunker:// URI handling, persistent storage of bunker
//! connection parameters, and a unified signer abstraction that supports both
//! direct nsec and NIP-46 remote signing modes.

pub mod config;
pub mod signer;
pub mod audit;

pub use config::{BunkerConfig, SigningMode};
pub use signer::MarmotSigner;
pub use audit::AuditLog;
