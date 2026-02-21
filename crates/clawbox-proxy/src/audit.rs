//! Audit trail — structured append-only logging for proxy requests.

use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Instant;

/// Maximum audit writes per second before throttling.
const AUDIT_MAX_WRITES_PER_SEC: usize = 100;
/// Window size for rate limit tracking.
const AUDIT_RATE_WINDOW_SIZE: usize = 100;

/// Maximum log file size before rotation (10 MB).
const MAX_LOG_SIZE: u64 = 10 * 1024 * 1024;
/// Maximum number of rotated log files to keep.
const MAX_ROTATED_FILES: u32 = 5;

/// Errors from audit logging operations.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum AuditError {
    /// I/O error writing the audit log.
    #[error("audit I/O error: {0}")]
    Io(#[from] std::io::Error),
    /// Serialization error encoding an audit entry.
    #[error("audit serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
}

/// A single audit log entry for a proxied request.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct AuditEntry {
    /// ISO 8601 timestamp.
    pub timestamp: String,
    /// The request URL (query parameter values are redacted when logged).
    pub url: String,
    /// HTTP method.
    pub method: String,
    /// Response status code.
    pub status: u16,
    /// Request duration in milliseconds.
    pub duration_ms: u64,
    /// Whether the request was blocked by the allowlist.
    pub blocked: bool,
    /// Whether a credential leak was detected.
    pub leak_detected: bool,
    /// Domain for which a credential was injected, if any.
    pub credential_injected: Option<String>,
}

impl AuditEntry {
    /// Create a new entry with the current timestamp.
    /// Create a new audit entry for a proxy request.
    pub fn new(url: String, method: String) -> Self {
        Self {
            timestamp: Utc::now().to_rfc3339(),
            url,
            method,
            status: 0,
            duration_ms: 0,
            blocked: false,
            leak_detected: false,
            credential_injected: None,
        }
    }
}

/// Redact query parameter values in a URL, replacing them with `[REDACTED]`.
fn redact_query_params(url: &str) -> String {
    match url::Url::parse(url) {
        Ok(parsed) => {
            let pairs: Vec<(String, String)> = parsed
                .query_pairs()
                .map(|(k, _v)| (k.into_owned(), "[REDACTED]".to_string()))
                .collect();
            if pairs.is_empty() {
                return url.to_string();
            }
            // Build query string manually to avoid percent-encoding the brackets
            let query = pairs
                .iter()
                .map(|(k, v)| format!("{}={}", k, v))
                .collect::<Vec<_>>()
                .join("&");
            let mut result = parsed.clone();
            result.set_query(Some(&query));
            result.to_string()
        }
        Err(_) => url.to_string(),
    }
}

/// Append-only structured log that writes JSON lines to a file.
///
/// Supports automatic log rotation: when the file exceeds 10 MB,
/// existing logs are rotated (`{name}.1` through `{name}.5`) and
/// a fresh file is started.
#[non_exhaustive]
pub struct AuditLog {
    path: PathBuf,
    /// Timestamps of recent writes for rate limiting.
    recent_writes: Mutex<Vec<Instant>>,
    /// Count of entries dropped due to rate limiting since last logged warning.
    dropped_count: Mutex<u64>,
}

impl AuditLog {
    /// Create a new audit log at the given path.
    /// Create a new audit log writing to the given file path.
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            recent_writes: Mutex::new(Vec::with_capacity(AUDIT_RATE_WINDOW_SIZE)),
            dropped_count: Mutex::new(0),
        }
    }

    /// Check if we are within the rate limit. Returns true if the write should proceed.
    fn check_rate_limit(&self) -> bool {
        let now = Instant::now();
        let mut recent = self.recent_writes.lock().unwrap_or_else(|e| e.into_inner());
        // Remove entries older than 1 second
        recent.retain(|t| now.duration_since(*t).as_secs_f64() < 1.0);
        if recent.len() >= AUDIT_MAX_WRITES_PER_SEC {
            return false;
        }
        recent.push(now);
        true
    }

    /// Rotate log files: `path` → `path.1`, `path.1` → `path.2`, etc.
    /// Keeps at most `MAX_ROTATED_FILES` rotated files.
    fn rotate(&self) -> Result<(), AuditError> {
        // Remove the oldest rotated file if it exists
        let oldest = format!("{}.{}", self.path.display(), MAX_ROTATED_FILES);
        let _ = fs::remove_file(&oldest);

        // Shift existing rotated files up by one
        for i in (1..MAX_ROTATED_FILES).rev() {
            let from = format!("{}.{}", self.path.display(), i);
            let to = format!("{}.{}", self.path.display(), i + 1);
            if std::path::Path::new(&from).exists() {
                fs::rename(&from, &to)?;
            }
        }

        // Rotate current file to .1
        let first = format!("{}.1", self.path.display());
        if self.path.exists() {
            fs::rename(&self.path, &first)?;
        }

        Ok(())
    }

    /// Record an audit entry by appending a JSON line.
    /// Query parameter values in the URL are redacted before logging.
    /// If the log file exceeds 10 MB, it is rotated first.
    pub fn record(&self, entry: &AuditEntry) -> Result<(), AuditError> {
        if !self.check_rate_limit() {
            let mut dropped = self.dropped_count.lock().unwrap_or_else(|e| e.into_inner());
            *dropped += 1;
            if *dropped == 1 || (*dropped).is_multiple_of(1000) {
                tracing::warn!(
                    dropped = *dropped,
                    "Audit log rate limit exceeded, entries dropped"
                );
            }
            return Ok(());
        }
        // Flush any pending dropped count
        {
            let mut dropped = self.dropped_count.lock().unwrap_or_else(|e| e.into_inner());
            if *dropped > 0 {
                tracing::info!(
                    dropped = *dropped,
                    "Audit log rate limit recovered, total entries dropped"
                );
                *dropped = 0;
            }
        }

        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }

        // Check if rotation is needed
        if let Ok(meta) = fs::metadata(&self.path)
            && meta.len() >= MAX_LOG_SIZE
        {
            self.rotate()?;
        }

        // Redact query params in the URL before logging
        let mut redacted_entry = entry.clone();
        redacted_entry.url = redact_query_params(&entry.url);

        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        let line = serde_json::to_string(&redacted_entry)?;
        writeln!(file, "{}", line)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp_path(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("clawbox_test_{}_{}", name, std::process::id()))
    }

    #[test]
    fn test_audit_log_roundtrip() {
        let path = temp_path("audit.jsonl");
        let log = AuditLog::new(&path);

        let mut entry = AuditEntry::new("https://api.github.com/repos".into(), "GET".into());
        entry.status = 200;
        entry.duration_ms = 42;
        entry.credential_injected = Some("GITHUB_TOKEN".into());

        log.record(&entry).unwrap();
        log.record(&entry).unwrap();

        let content = fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = content.trim().lines().collect();
        assert_eq!(lines.len(), 2);

        let parsed: AuditEntry = serde_json::from_str(lines[0]).unwrap();
        assert_eq!(parsed.status, 200);
        assert_eq!(parsed.credential_injected.as_deref(), Some("GITHUB_TOKEN"));
    }

    #[test]
    fn test_query_params_redacted() {
        let path = temp_path("audit_redact.jsonl");
        let log = AuditLog::new(&path);

        let entry = AuditEntry::new(
            "https://api.example.com/data?token=secret123&user=admin".into(),
            "GET".into(),
        );
        log.record(&entry).unwrap();

        let content = fs::read_to_string(&path).unwrap();
        assert!(!content.contains("secret123"));
        assert!(!content.contains("admin"));
        assert!(content.contains("[REDACTED]"));
        assert!(content.contains("token"));
        assert!(content.contains("user"));
    }

    #[test]
    fn test_url_without_query_unchanged() {
        let url = "https://api.github.com/repos";
        assert_eq!(redact_query_params(url), url);
    }

    #[test]
    fn test_log_rotation() {
        let path = temp_path("audit_rotate.jsonl");
        let log = AuditLog::new(&path);

        // Write enough to trigger rotation (just test the rotation logic directly)
        log.rotate().unwrap();

        // After rotation, original file should be gone (it didn't exist)
        // Just verify it doesn't panic
    }

    #[test]
    fn test_audit_error_display() {
        let err = AuditError::Io(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "nope",
        ));
        assert!(err.to_string().contains("audit I/O error"));
    }

    #[test]
    fn test_log_rotation_with_large_file() {
        let path = temp_path("audit_rot_large.jsonl");
        let log = AuditLog::new(&path);

        // Create a file larger than MAX_LOG_SIZE (10MB)
        {
            let mut f = std::fs::OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(true)
                .open(&path)
                .unwrap();
            use std::io::Write;
            let big_line = "x".repeat(1024);
            for _ in 0..(11 * 1024) {
                writeln!(f, "{}", big_line).unwrap();
            }
        }

        // Now record should trigger rotation
        let entry = AuditEntry::new("https://example.com".into(), "GET".into());
        log.record(&entry).unwrap();

        // Original file should be small now (just the new entry)
        let meta = fs::metadata(&path).unwrap();
        assert!(
            meta.len() < 1024,
            "After rotation, main file should be small, got {}",
            meta.len()
        );

        // Rotated file should exist
        let rotated = format!("{}.1", path.display());
        assert!(
            std::path::Path::new(&rotated).exists(),
            "Rotated file .1 should exist"
        );

        // Cleanup
        let _ = fs::remove_file(&path);
        let _ = fs::remove_file(&rotated);
    }

    #[test]
    fn test_audit_entry_new_has_timestamp() {
        let entry = AuditEntry::new("https://example.com".into(), "POST".into());
        assert!(!entry.timestamp.is_empty());
        assert_eq!(entry.method, "POST");
        assert_eq!(entry.status, 0);
        assert!(!entry.blocked);
        assert!(!entry.leak_detected);
    }
}
