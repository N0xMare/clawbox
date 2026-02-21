//! Endpoint allowlist enforcement.

use std::collections::HashSet;
use tracing::warn;

/// Enforces domain-level allowlisting for outbound HTTP requests.
///
/// # Security Warning
/// Including `"*"` in the allowed domains disables all domain filtering.
/// This should **never** be used in production. The server-side default
/// should be an empty allowlist (deny-all).
#[derive(Debug, Clone)]
#[non_exhaustive]
#[must_use]
pub struct AllowlistEnforcer {
    allowed_domains: HashSet<String>,
}

impl AllowlistEnforcer {
    /// Create a new allowlist from a list of domain patterns.
    pub fn new(domains: impl IntoIterator<Item = impl Into<String>>) -> Self {
        let allowed_domains: HashSet<String> = domains.into_iter().map(Into::into).collect();
        if allowed_domains.contains("*") {
            warn!(
                "Allowlist contains wildcard '*' - ALL domain filtering is disabled. This is dangerous and should not be used in production."
            );
        }
        Self { allowed_domains }
    }

    /// Returns `true` if the wildcard `*` is set, meaning all domains are allowed.
    ///
    /// # Security Warning
    /// When this returns , no domain filtering is applied.
    /// Returns true if the allowlist contains a wildcard entry.
    pub fn allows_all(&self) -> bool {
        self.allowed_domains.contains("*")
    }

    /// Check if a URL is allowed by the current allowlist.
    /// Check if a URL is permitted by the allowlist.
    pub fn is_allowed(&self, url: &str) -> bool {
        // WARNING: "*" disables all security filtering. Never use as a default.
        if self.allowed_domains.contains("*") {
            return true;
        }

        let Ok(parsed) = url::Url::parse(url) else {
            return false;
        };
        let Some(host) = parsed.host_str() else {
            return false;
        };
        self.allowed_domains.contains(host)
            || self.allowed_domains.iter().any(|d| {
                d.starts_with("*.") && (host == &d[2..] || host.ends_with(&format!(".{}", &d[2..])))
            })
    }

    /// Get the set of allowed domain patterns.
    pub fn allowed_domains(&self) -> &HashSet<String> {
        &self.allowed_domains
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_exact_match() {
        let enforcer = AllowlistEnforcer::new(["api.github.com"]);
        assert!(enforcer.is_allowed("https://api.github.com/repos"));
        assert!(!enforcer.is_allowed("https://evil.com/repos"));
    }

    #[test]
    fn test_wildcard_match() {
        let enforcer = AllowlistEnforcer::new(["*.github.com"]);
        assert!(enforcer.is_allowed("https://api.github.com/repos"));
        assert!(!enforcer.is_allowed("https://raw.githubusercontent.com"));
    }

    #[test]
    fn test_wildcard_boundary() {
        let enforcer = AllowlistEnforcer::new(["*.github.com"]);
        // Must NOT match — no dot boundary
        assert!(!enforcer.is_allowed("https://evilgithub.com/steal"));
        // Bare domain should match
        assert!(enforcer.is_allowed("https://github.com/repos"));
        // Subdomain should match
        assert!(enforcer.is_allowed("https://sub.github.com/repos"));
    }

    #[test]
    fn test_wildcard_all() {
        let enforcer = AllowlistEnforcer::new(["*"]);
        assert!(enforcer.is_allowed("https://anything.example.com/path"));
        assert!(enforcer.is_allowed("https://evil.com/steal"));
    }

    #[test]
    fn test_empty_allowlist_blocks_all() {
        let enforcer = AllowlistEnforcer::new(Vec::<String>::new());
        assert!(!enforcer.is_allowed("https://api.github.com/repos"));
    }
}
