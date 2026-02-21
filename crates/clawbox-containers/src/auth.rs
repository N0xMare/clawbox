//! Per-container authentication tokens.
//!
//! Each container gets a unique bearer token on spawn.
//! The token is passed via environment variable and validated on proxy requests.

use std::collections::HashMap;
use std::sync::Mutex;

use rand::RngCore;

/// Store for per-container authentication tokens.
#[non_exhaustive]
pub struct ContainerTokenStore {
    /// Map of container_id → bearer token
    tokens: Mutex<HashMap<String, String>>,
}

impl ContainerTokenStore {
    /// Create a new empty token store.
    pub fn new() -> Self {
        Self {
            tokens: Mutex::new(HashMap::new()),
        }
    }

    /// Generate and store a new token for a container. Returns the token.
    /// Generate and store a new random token for the given container.
    pub fn generate(&self, container_id: &str) -> String {
        let mut bytes = [0u8; 32];
        rand::rng().fill_bytes(&mut bytes);
        let token: String = bytes.iter().map(|b| format!("{b:02x}")).collect();

        self.tokens
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(container_id.to_string(), token.clone());

        token
    }

    /// Validate a token for a container. Returns true if valid.
    /// Validate a token for the given container using constant-time comparison.
    pub fn validate(&self, container_id: &str, token: &str) -> bool {
        use subtle::ConstantTimeEq;
        self.tokens
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(container_id)
            .is_some_and(|stored| stored.as_bytes().ct_eq(token.as_bytes()).into())
    }

    /// Remove the token when a container is killed.
    /// Remove the token for the given container.
    pub fn remove(&self, container_id: &str) {
        self.tokens
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(container_id);
    }
}

impl Default for ContainerTokenStore {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_and_validate() {
        let store = ContainerTokenStore::new();
        let token = store.generate("container-1");
        assert_eq!(token.len(), 64); // 32 bytes * 2 hex chars
        assert!(store.validate("container-1", &token));
    }

    #[test]
    fn test_validate_wrong_token() {
        let store = ContainerTokenStore::new();
        let _token = store.generate("container-1");
        assert!(!store.validate("container-1", "wrong-token"));
    }

    #[test]
    fn test_validate_wrong_container() {
        let store = ContainerTokenStore::new();
        let token = store.generate("container-1");
        assert!(!store.validate("container-2", &token));
    }

    #[test]
    fn test_remove() {
        let store = ContainerTokenStore::new();
        let token = store.generate("container-1");
        assert!(store.validate("container-1", &token));
        store.remove("container-1");
        assert!(!store.validate("container-1", &token));
    }

    #[test]
    fn test_unique_tokens() {
        let store = ContainerTokenStore::new();
        let t1 = store.generate("c1");
        let t2 = store.generate("c2");
        assert_ne!(t1, t2);
    }

    #[test]
    fn test_constant_time_eq_same_length() {
        // Verify that ct_eq works correctly for same-length strings
        use subtle::ConstantTimeEq;
        let a = b"abcdef";
        let b = b"abcdef";
        let c = b"abcdeg";
        assert!(bool::from(a.ct_eq(b)));
        assert!(!bool::from(a.ct_eq(c)));
    }
}
