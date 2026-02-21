//! Encrypted file-based credential storage.
//!
//! Uses AES-256-GCM to encrypt credentials at rest. The master key is
//! sourced from the `CLAWBOX_MASTER_KEY` env var (hex-encoded, 32 bytes).

use aes_gcm::{
    Aes256Gcm, Nonce,
    aead::{Aead, KeyInit},
};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use zeroize::{Zeroize, Zeroizing};

use crate::credentials::CredentialInjector;

fn ser_zeroizing<S: serde::Serializer>(val: &Zeroizing<String>, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_str(val)
}

fn de_zeroizing<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Zeroizing<String>, D::Error> {
    String::deserialize(d).map(Zeroizing::new)
}

/// Errors from credential store operations.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum StoreError {
    /// I/O error reading or writing the credential file.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    /// Encryption failed.
    #[error("encryption error: {0}")]
    Encryption(String),
    /// Decryption failed (wrong key or corrupted data).
    #[error("decryption error: {0}")]
    Decryption(String),
    /// JSON serialization/deserialization error.
    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
    /// Invalid master key format.
    #[error("invalid master key: {0}")]
    InvalidKey(String),
}

/// A single stored credential entry.
#[derive(Debug, Serialize, Deserialize)]
#[non_exhaustive]
pub struct CredentialEntry {
    /// Human-readable name for this credential.
    pub name: String,
    /// The secret value. Wrapped in `Zeroizing` so it is cleared from memory on drop.
    #[serde(serialize_with = "ser_zeroizing", deserialize_with = "de_zeroizing")]
    pub value: Zeroizing<String>,
    /// Domain this credential applies to.
    pub domain: String,
    /// HTTP header name to inject.
    pub header_name: String,
    /// Prefix for the header value (e.g. "Bearer ").
    pub header_prefix: String,
}

/// Encrypted file-based credential storage.
#[non_exhaustive]
pub struct CredentialStore {
    entries: Vec<CredentialEntry>,
    path: PathBuf,
    key: [u8; 32],
}

impl CredentialStore {
    /// Load from an encrypted file. If the file doesn't exist, returns an empty store.
    pub fn load(path: impl AsRef<Path>, key: [u8; 32]) -> Result<Self, StoreError> {
        let path = path.as_ref().to_path_buf();
        let entries = if path.exists() {
            let ciphertext = std::fs::read(&path)?;
            if ciphertext.len() < 12 {
                return Err(StoreError::Decryption("credential file too short".into()));
            }
            let (nonce_bytes, ct) = ciphertext.split_at(12);
            let cipher = Aes256Gcm::new_from_slice(&key)
                .map_err(|e| StoreError::InvalidKey(e.to_string()))?;
            let nonce = Nonce::from_slice(nonce_bytes);
            let plaintext = cipher
                .decrypt(nonce, ct)
                .map_err(|_| StoreError::Decryption("decryption failed — wrong key?".into()))?;
            serde_json::from_slice(&plaintext)?
        } else {
            Vec::new()
        };
        Ok(Self { entries, path, key })
    }

    /// Save the store to its encrypted file.
    pub fn save(&self) -> Result<(), StoreError> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let plaintext = serde_json::to_vec(&self.entries)?;
        let cipher = Aes256Gcm::new_from_slice(&self.key)
            .map_err(|e| StoreError::InvalidKey(e.to_string()))?;
        let mut nonce_bytes = [0u8; 12];
        rand::rng().fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::from_slice(&nonce_bytes);
        let ciphertext = cipher
            .encrypt(nonce, plaintext.as_ref())
            .map_err(|_| StoreError::Encryption("encryption failed".into()))?;
        let mut output = nonce_bytes.to_vec();
        output.extend(ciphertext);
        std::fs::write(&self.path, output)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&self.path, std::fs::Permissions::from_mode(0o600))?;
        }
        Ok(())
    }

    /// Add a credential. Overwrites if name already exists.
    pub fn add(
        &mut self,
        name: impl Into<String>,
        value: impl Into<String>,
        domain: impl Into<String>,
        header_name: impl Into<String>,
        header_prefix: impl Into<String>,
    ) {
        let name = name.into();
        self.entries.retain(|e| e.name != name);
        self.entries.push(CredentialEntry {
            name,
            value: Zeroizing::new(value.into()),
            domain: domain.into(),
            header_name: header_name.into(),
            header_prefix: header_prefix.into(),
        });
    }

    /// Remove a credential by name.
    pub fn remove(&mut self, name: &str) {
        self.entries.retain(|e| e.name != name);
    }

    /// List credential names (never exposes values).
    pub fn list_names(&self) -> Vec<String> {
        self.entries.iter().map(|e| e.name.clone()).collect()
    }

    /// Build a `CredentialInjector` with only the requested credentials.
    pub fn build_injector(&self, cred_names: &[String]) -> CredentialInjector {
        let mut injector = CredentialInjector::new();
        for entry in &self.entries {
            if cred_names.contains(&entry.name) {
                let value = Zeroizing::new(format!("{}{}", entry.header_prefix, &*entry.value));
                injector.add_mapping(&entry.domain, &entry.header_name, &*value);
            }
        }
        injector
    }

    /// Get all known secret values (for leak detection).
    /// Returns `Zeroizing<String>` wrappers that automatically zero memory on drop.
    pub fn secret_values(&self) -> Vec<Zeroizing<String>> {
        self.entries
            .iter()
            .map(|e| Zeroizing::new((*e.value).clone()))
            .collect()
    }
}

impl Drop for CredentialStore {
    fn drop(&mut self) {
        self.key.zeroize();
        // Zeroize credential values in entries
        // entry.value is Zeroizing<String> and handles its own zeroization on drop
    }
}

/// Parse a hex-encoded 32-byte key.
pub fn parse_master_key(hex: &str) -> Result<[u8; 32], StoreError> {
    let hex = hex.trim();
    if hex.len() != 64 {
        return Err(StoreError::InvalidKey(
            "CLAWBOX_MASTER_KEY must be 64 hex chars (32 bytes)".into(),
        ));
    }
    let mut key = [0u8; 32];
    for i in 0..32 {
        key[i] = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16)
            .map_err(|e| StoreError::InvalidKey(e.to_string()))?;
    }
    Ok(key)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_key() -> [u8; 32] {
        let mut key = [0u8; 32];
        for (i, b) in key.iter_mut().enumerate() {
            *b = i as u8;
        }
        key
    }

    fn temp_path(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("clawbox_test_{}_{}", name, std::process::id()))
    }

    #[test]
    fn test_store_roundtrip() {
        let path = temp_path("creds_rt.enc");
        let key = test_key();

        {
            let mut store = CredentialStore::load(&path, key).unwrap();
            store.add(
                "GITHUB_TOKEN",
                "ghp_abc123",
                "api.github.com",
                "Authorization",
                "token ",
            );
            store.add(
                "ANTHROPIC_API_KEY",
                "sk-ant-xyz",
                "api.anthropic.com",
                "x-api-key",
                "",
            );
            store.save().unwrap();
        }

        let store = CredentialStore::load(&path, key).unwrap();
        assert_eq!(store.list_names().len(), 2);
        assert!(store.list_names().contains(&"GITHUB_TOKEN".to_string()));
    }

    #[test]
    fn test_store_remove() {
        let path = temp_path("creds_rm.enc");
        let key = test_key();

        let mut store = CredentialStore::load(&path, key).unwrap();
        store.add("A", "val", "d.com", "Auth", "");
        store.add("B", "val2", "d2.com", "Auth", "");
        store.remove("A");
        assert_eq!(store.list_names(), vec!["B".to_string()]);
    }

    #[test]
    fn test_build_injector() {
        let path = temp_path("creds_inj.enc");
        let key = test_key();

        let mut store = CredentialStore::load(&path, key).unwrap();
        store.add(
            "GITHUB_TOKEN",
            "ghp_abc123",
            "api.github.com",
            "Authorization",
            "token ",
        );
        store.add("OTHER", "secret", "other.com", "X-Key", "");

        let injector = store.build_injector(&["GITHUB_TOKEN".to_string()]);
        assert!(injector.get_mapping("api.github.com").is_some());
        assert!(injector.get_mapping("other.com").is_none());
    }

    #[test]
    fn test_wrong_key_fails() {
        let path = temp_path("creds_wk.enc");
        let key = test_key();

        let mut store = CredentialStore::load(&path, key).unwrap();
        store.add("A", "val", "d.com", "Auth", "");
        store.save().unwrap();

        let mut bad_key = [0u8; 32];
        bad_key[0] = 0xFF;
        assert!(CredentialStore::load(&path, bad_key).is_err());
    }

    #[test]
    fn test_parse_master_key() {
        let hex = "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f";
        let key = parse_master_key(hex).unwrap();
        assert_eq!(key[0], 0);
        assert_eq!(key[31], 0x1f);
    }

    #[test]
    fn test_store_error_io() {
        let err = StoreError::Io(std::io::Error::new(std::io::ErrorKind::NotFound, "gone"));
        assert!(err.to_string().contains("I/O error"));
    }

    #[test]
    fn test_store_error_invalid_key() {
        let err = StoreError::InvalidKey("bad hex".into());
        assert!(err.to_string().contains("invalid master key"));
    }

    #[test]
    fn test_store_error_encryption() {
        let err = StoreError::Encryption("failed".into());
        assert!(err.to_string().contains("encryption error"));
    }

    #[test]
    fn test_store_error_decryption() {
        let err = StoreError::Decryption("corrupt".into());
        assert!(err.to_string().contains("decryption error"));
    }

    #[test]
    fn test_parse_master_key_wrong_length() {
        assert!(parse_master_key("0011").is_err());
    }

    #[test]
    fn test_parse_master_key_invalid_hex() {
        let bad = "zz".repeat(32);
        assert!(parse_master_key(&bad).is_err());
    }

    #[test]
    fn test_secret_values() {
        let path = temp_path("creds_sv.enc");
        let key = test_key();
        let mut store = CredentialStore::load(&path, key).unwrap();
        store.add("A", "secret1", "d.com", "Auth", "");
        store.add("B", "secret2", "d2.com", "Auth", "");
        let secrets = store.secret_values();
        assert_eq!(secrets.len(), 2);
    }
}
