//! Leak detection — scans outbound request bodies for credential patterns.
//!
//! Prevents WASM guests or containers from exfiltrating credentials
//! through request parameters, headers, or body content.

use std::sync::LazyLock;

use base64::Engine as _;
use percent_encoding::percent_decode_str;
use regex::Regex;
use tracing::warn;
use zeroize::Zeroizing;

use clawbox_types::patterns::CREDENTIAL_PATTERNS;

/// Compiled default credential patterns (compiled once, reused across all instances).
static DEFAULT_PATTERNS: LazyLock<Vec<Regex>> = LazyLock::new(|| {
    CREDENTIAL_PATTERNS
        .iter()
        .filter_map(|p| Regex::new(p).ok())
        .collect()
});

/// Scans outbound requests for credential leaks.
#[non_exhaustive]
#[must_use]
pub struct LeakDetector {
    /// Known credential values to watch for (zeroized on drop).
    known_secrets: Vec<Zeroizing<String>>,
    /// Regex patterns that look like credentials.
    patterns: Vec<Regex>,
}

impl LeakDetector {
    pub fn new() -> Self {
        Self {
            known_secrets: Vec::new(),
            patterns: DEFAULT_PATTERNS.clone(),
        }
    }

    /// Add a known secret value to watch for in outbound requests.
    pub fn add_known_secret(&mut self, secret: impl Into<String>) {
        self.known_secrets.push(Zeroizing::new(secret.into()));
    }

    /// Check if content contains any known secrets or credential patterns.
    /// Checks URL-decoded, double URL-decoded, and base64-encoded variants
    /// to prevent encoding-based evasion.
    pub fn scan(&self, content: &str) -> Vec<LeakFinding> {
        let mut findings = Vec::new();

        // Iterative URL decoding (up to 10 rounds) to defeat multi-layer encoding
        let mut variants = vec![content.to_string()];
        let mut current = content.to_string();
        for _ in 0..10 {
            let decoded = percent_decode_str(&current).decode_utf8_lossy().to_string();
            if decoded == current {
                break;
            }
            variants.push(decoded.clone());
            current = decoded;
        }

        for secret in &self.known_secrets {
            // Check plaintext in all decoded variants
            if variants.iter().any(|c| c.contains(secret.as_str())) {
                findings.push(LeakFinding {
                    kind: LeakKind::KnownSecret,
                    description: "Known credential value found in content".into(),
                });
                warn!("Known secret detected in outbound content");
                continue;
            }
            // Check base64-encoded variants of the secret in all content variants
            let b64 = base64::engine::general_purpose::STANDARD.encode(secret.as_bytes());
            let b64_urlsafe = base64::engine::general_purpose::URL_SAFE.encode(secret.as_bytes());
            if variants
                .iter()
                .any(|c| c.contains(&b64) || c.contains(&b64_urlsafe))
            {
                findings.push(LeakFinding {
                    kind: LeakKind::KnownSecret,
                    description: "Base64-encoded credential value found".into(),
                });
                warn!("Base64-encoded secret detected in outbound content");
            }
            // Check hex-encoded variants of the secret
            if secret.len() >= 8 {
                let hex_lower: String = secret.bytes().map(|b| format!("{b:02x}")).collect();
                let hex_upper: String = secret.bytes().map(|b| format!("{b:02X}")).collect();
                if variants
                    .iter()
                    .any(|c| c.contains(&hex_lower) || c.contains(&hex_upper))
                {
                    findings.push(LeakFinding {
                        kind: LeakKind::KnownSecret,
                        description: "Hex-encoded credential value found".into(),
                    });
                    warn!("Hex-encoded secret detected in outbound content");
                }
            }
        }

        for pattern in &self.patterns {
            if variants.iter().any(|c| pattern.is_match(c)) {
                findings.push(LeakFinding {
                    kind: LeakKind::PatternMatch,
                    description: format!("Credential pattern matched: {}", pattern.as_str()),
                });
            }
        }

        findings
    }

    /// Redact known secrets from content, replacing with `REDACTED`.
    pub fn redact(&self, content: &str) -> String {
        let mut result = content.to_string();
        // Replace known secret values
        for secret in &self.known_secrets {
            result = result.replace(secret.as_str(), "[REDACTED]");
        }
        // Replace pattern matches
        for pattern in &self.patterns {
            result = pattern.replace_all(&result, "[REDACTED]").to_string();
        }
        result
    }
}

impl Default for LeakDetector {
    fn default() -> Self {
        Self::new()
    }
}

/// A detected leak.
#[derive(Debug)]
#[non_exhaustive]
pub struct LeakFinding {
    pub kind: LeakKind,
    pub description: String,
}

/// Type of leak detected.
#[derive(Debug)]
#[non_exhaustive]
pub enum LeakKind {
    KnownSecret,
    PatternMatch,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_url_encoded_secret_detected() {
        let mut d = LeakDetector::new();
        d.add_known_secret("my_secret");
        assert!(!d.scan("my%5Fsecret").is_empty());
    }

    #[test]
    fn test_double_url_encoded_secret_detected() {
        let mut d = LeakDetector::new();
        d.add_known_secret("my_secret");
        // "my_secret" -> URL-encoded underscore: "my%5Fsecret" -> double: "my%255Fsecret"
        assert!(!d.scan("my%255Fsecret").is_empty());
    }

    #[test]
    fn test_base64_encoded_secret_detected() {
        let mut d = LeakDetector::new();
        d.add_known_secret("super_secret_key");
        let b64 = base64::engine::general_purpose::STANDARD.encode(b"super_secret_key");
        assert!(!d.scan(&b64).is_empty());
    }

    #[test]
    fn test_base64_urlsafe_encoded_secret_detected() {
        let mut d = LeakDetector::new();
        d.add_known_secret("secret+value/here");
        let b64 = base64::engine::general_purpose::URL_SAFE.encode(b"secret+value/here");
        assert!(!d.scan(&b64).is_empty());
    }

    #[test]
    fn test_plain_secret_still_detected() {
        let mut d = LeakDetector::new();
        d.add_known_secret("plain_secret");
        assert!(!d.scan("contains plain_secret here").is_empty());
    }

    #[test]
    fn test_no_false_positive() {
        let mut d = LeakDetector::new();
        d.add_known_secret("real_secret");
        assert!(d.scan("nothing here").is_empty());
    }

    #[test]
    fn test_lazy_lock_patterns_work() {
        // Verify that DEFAULT_PATTERNS are usable and detect an OpenAI key pattern
        let d = LeakDetector::new();
        let findings = d.scan("token: sk-abcdefghijklmnopqrstuvwxyz1234567890");
        assert!(!findings.is_empty());
    }

    #[test]
    fn test_redact_known_secrets() {
        let mut d = LeakDetector::new();
        d.add_known_secret("my_api_key_123");
        let result = d.redact("the key is my_api_key_123 in this response");
        assert_eq!(result, "the key is [REDACTED] in this response");
        assert!(!result.contains("my_api_key_123"));
    }

    #[test]
    fn test_redact_pattern_matches() {
        let d = LeakDetector::new();
        let result = d.redact("token: sk-abcdefghijklmnopqrstuvwxyz1234567890");
        assert!(result.contains("[REDACTED]"));
        assert!(!result.contains("sk-abcdefghijklmnopqrstuvwxyz1234567890"));
    }

    #[test]
    fn test_hex_encoded_secret_detected() {
        let mut d = LeakDetector::new();
        d.add_known_secret("my_secret_key");
        let hex: String = "my_secret_key"
            .bytes()
            .map(|b| format!("{b:02x}"))
            .collect();
        assert!(!d.scan(&hex).is_empty());
    }

    #[test]
    fn test_hex_upper_encoded_secret_detected() {
        let mut d = LeakDetector::new();
        d.add_known_secret("my_secret_key");
        let hex: String = "my_secret_key"
            .bytes()
            .map(|b| format!("{b:02X}"))
            .collect();
        assert!(!d.scan(&hex).is_empty());
    }

    #[test]
    fn test_hex_short_secret_skipped() {
        let mut d = LeakDetector::new();
        d.add_known_secret("short");
        let hex: String = "short".bytes().map(|b| format!("{b:02x}")).collect();
        // Short secrets (<8 chars) should not trigger hex detection
        // (but may still trigger plain detection)
        let findings: Vec<_> = d
            .scan(&hex)
            .into_iter()
            .filter(|f| f.description.contains("Hex"))
            .collect();
        assert!(findings.is_empty());
    }

    #[test]
    fn test_redact_no_secrets_unchanged() {
        let d = LeakDetector::new();
        let input = "this is clean content";
        assert_eq!(d.redact(input), input);
    }
}
