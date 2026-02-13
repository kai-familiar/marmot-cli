//! Audit logging for signing operations
//!
//! Records all signing requests, bunker connections, and key migrations
//! to a local log file for security review.

use std::path::{Path, PathBuf};
use std::io::Write;

use serde::Serialize;

/// Audit log entry
#[derive(Debug, Serialize)]
struct AuditEntry {
    timestamp: String,
    operation: String,
    details: String,
}

/// Append-only audit log for signing operations
pub struct AuditLog {
    path: PathBuf,
    enabled: bool,
}

impl AuditLog {
    /// Create a new audit log at the given path
    pub fn new(db_path: &Path) -> Self {
        let path = db_path.with_extension("audit.jsonl");
        Self {
            path,
            enabled: true,
        }
    }

    /// Create a disabled audit log (for testing)
    #[allow(dead_code)]
    pub fn disabled() -> Self {
        Self {
            path: PathBuf::from("/dev/null"),
            enabled: false,
        }
    }

    /// Record an audit event
    pub fn record(&mut self, operation: &str, details: &str) {
        if !self.enabled {
            return;
        }

        let entry = AuditEntry {
            timestamp: chrono::Utc::now().to_rfc3339(),
            operation: operation.to_string(),
            details: details.to_string(),
        };

        // Best-effort append — don't fail the operation if audit logging fails
        if let Ok(json) = serde_json::to_string(&entry) {
            if let Ok(mut file) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&self.path)
            {
                let _ = writeln!(file, "{}", json);
            }
        }
    }

    /// Get the audit log file path
    #[allow(dead_code)]
    pub fn path(&self) -> &Path {
        &self.path
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_audit_log_writes() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("marmot.db");

        let mut log = AuditLog::new(&db_path);
        log.record("test_op", "test details");
        log.record("test_op2", "more details");

        let content = std::fs::read_to_string(log.path()).unwrap();
        let lines: Vec<&str> = content.lines().collect();
        assert_eq!(lines.len(), 2);

        let entry: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
        assert_eq!(entry["operation"], "test_op");
        assert_eq!(entry["details"], "test details");
    }

    #[test]
    fn test_audit_log_disabled() {
        let mut log = AuditLog::disabled();
        // Should not panic
        log.record("test", "test");
    }
}
