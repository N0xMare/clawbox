//! Credential injection at the network proxy boundary.
//!
//! Credentials are injected into outbound requests based on the destination
//! domain. The WASM guest and container never see raw API keys.

use std::collections::HashMap;
use zeroize::Zeroizing;

/// Maps destination domains to credential injection rules.
#[derive(Debug, Clone)]
#[non_exhaustive]
#[must_use]
pub struct CredentialInjector {
    /// domain → (header_name, credential_value)
    mappings: HashMap<String, CredentialMapping>,
}

/// How to inject a credential into an outbound request.
#[derive(Clone)]
#[non_exhaustive]
pub struct CredentialMapping {
    /// HTTP header name to set.
    pub header: String,
    /// Header value (including any prefix like "Bearer ").
    /// Wrapped in `Zeroizing` so the secret is cleared from memory on drop.
    pub value: Zeroizing<String>,
}

impl std::fmt::Debug for CredentialMapping {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CredentialMapping")
            .field("header", &self.header)
            .field("value", &"[REDACTED]")
            .finish()
    }
}

impl CredentialInjector {
    /// Create a new empty injector.
    pub fn new() -> Self {
        Self {
            mappings: HashMap::new(),
        }
    }

    /// Register a credential mapping for a domain.
    pub fn add_mapping(
        &mut self,
        domain: impl Into<String>,
        header: impl Into<String>,
        value: impl Into<String>,
    ) {
        self.mappings.insert(
            domain.into(),
            CredentialMapping {
                header: header.into(),
                value: Zeroizing::new(value.into()),
            },
        );
    }

    /// Get the credential mapping for a domain, if any.
    pub fn get_mapping(&self, domain: &str) -> Option<&CredentialMapping> {
        self.mappings.get(domain)
    }
}

impl Default for CredentialInjector {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_add_and_get_mapping() {
        let mut injector = CredentialInjector::new();
        injector.add_mapping("api.github.com", "Authorization", "token ghp_abc");

        let mapping = injector.get_mapping("api.github.com").unwrap();
        assert_eq!(mapping.header, "Authorization");
        assert_eq!(&*mapping.value, "token ghp_abc");
    }

    #[test]
    fn test_get_missing_mapping() {
        let injector = CredentialInjector::new();
        assert!(injector.get_mapping("api.github.com").is_none());
    }

    #[test]
    fn test_overwrite_mapping() {
        let mut injector = CredentialInjector::new();
        injector.add_mapping("api.example.com", "X-Key", "old");
        injector.add_mapping("api.example.com", "X-Key", "new");

        let mapping = injector.get_mapping("api.example.com").unwrap();
        assert_eq!(&*mapping.value, "new");
    }
}
