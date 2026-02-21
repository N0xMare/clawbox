//! Tool capability manifests — define what a WASM tool can do.
//!
//! Each tool registers a [`ToolManifest`] that declares its metadata,
//! network requirements, credential needs, and resource defaults.

use serde::{Deserialize, Serialize};

/// Capability manifest for a registered WASM tool.
///
/// Loaded from a tool's `manifest.toml` or provided via the registration API.
/// The server uses this to pre-configure sandbox capabilities before execution.
#[non_exhaustive]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolManifest {
    /// Tool identity and description.
    pub tool: ToolMeta,
    /// Network capabilities. `None` means no network access.
    #[serde(default)]
    pub network: Option<ToolNetworkConfig>,
    /// Credential access configuration. `None` means no credentials.
    #[serde(default)]
    pub credentials: Option<ToolCredentialConfig>,
    /// Resource defaults and limits. `None` uses server defaults.
    #[serde(default)]
    pub resources: Option<ToolResourceConfig>,
    /// JSON Schema describing the tool's expected input parameters.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_schema: Option<serde_json::Value>,
    /// JSON Schema describing the tool's output format.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_schema: Option<serde_json::Value>,
}

impl ToolManifest {
    /// Create a manifest with the given metadata and no extra capabilities.
    pub fn new(tool: ToolMeta) -> Self {
        Self {
            tool,
            network: None,
            credentials: None,
            resources: None,
            input_schema: None,
            output_schema: None,
        }
    }

    /// Set network configuration.
    pub fn with_network(mut self, network: ToolNetworkConfig) -> Self {
        self.network = Some(network);
        self
    }

    /// Set credential configuration.
    pub fn with_credentials(mut self, credentials: ToolCredentialConfig) -> Self {
        self.credentials = Some(credentials);
        self
    }

    /// Set resource configuration.
    pub fn with_resources(mut self, resources: ToolResourceConfig) -> Self {
        self.resources = Some(resources);
        self
    }

    /// Set input schema.
    pub fn with_input_schema(mut self, schema: serde_json::Value) -> Self {
        self.input_schema = Some(schema);
        self
    }

    /// Set output schema.
    pub fn with_output_schema(mut self, schema: serde_json::Value) -> Self {
        self.output_schema = Some(schema);
        self
    }
}

/// Tool identity metadata.
#[non_exhaustive]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolMeta {
    /// Unique tool name (used in API calls). Must be a valid identifier.
    pub name: String,
    /// Human-readable description shown in tool listings.
    pub description: String,
    /// Semantic version string (e.g., `"0.1.0"`). Defaults to `"0.1.0"`.
    #[serde(default = "default_version")]
    pub version: String,
}

impl ToolMeta {
    /// Create tool metadata with the given name and description.
    pub fn new(name: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            version: default_version(),
        }
    }

    /// Set the version string.
    pub fn with_version(mut self, version: impl Into<String>) -> Self {
        self.version = version.into();
        self
    }
}

fn default_version() -> String {
    "0.1.0".to_string()
}

/// Network configuration for a tool.
///
/// Defines which external domains the tool may contact and connection limits.
#[non_exhaustive]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolNetworkConfig {
    /// Default domain allowlist. Can be narrowed (but not widened) per-call.
    #[serde(default)]
    pub allowlist: Vec<String>,
    /// Maximum concurrent outbound connections. Defaults to [`DEFAULT_MAX_CONCURRENT`](crate::DEFAULT_MAX_CONCURRENT).
    #[serde(default = "default_max_concurrent")]
    pub max_concurrent: usize,
}

impl ToolNetworkConfig {
    /// Create a network config with the given domain allowlist.
    pub fn new(allowlist: Vec<String>) -> Self {
        Self {
            allowlist,
            max_concurrent: default_max_concurrent(),
        }
    }
}

impl Default for ToolNetworkConfig {
    fn default() -> Self {
        Self {
            allowlist: Vec::new(),
            max_concurrent: default_max_concurrent(),
        }
    }
}

fn default_max_concurrent() -> usize {
    crate::DEFAULT_MAX_CONCURRENT
}

/// Credential configuration for a tool.
///
/// Lists which named credentials (managed by the server's credential store)
/// this tool is allowed to request at runtime.
#[non_exhaustive]
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ToolCredentialConfig {
    /// Credential names this tool may request (e.g., `"openai"`, `"github"`).
    #[serde(default)]
    pub available: Vec<String>,
}

impl ToolCredentialConfig {
    /// Create a credential config with the given available credential names.
    pub fn new(available: Vec<String>) -> Self {
        Self { available }
    }
}

/// Resource configuration for a tool.
///
/// Defines default and maximum resource budgets. The server clamps
/// per-call overrides to the maximums defined here.
#[non_exhaustive]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResourceConfig {
    /// Default timeout in milliseconds. Defaults to 30000 (30 seconds).
    #[serde(default = "default_timeout")]
    pub default_timeout_ms: u64,
    /// Maximum allowed timeout in milliseconds. Defaults to 120000 (2 minutes).
    #[serde(default = "max_timeout")]
    pub max_timeout_ms: u64,
    /// Default memory limit in megabytes. Defaults to 256.
    #[serde(default = "default_memory")]
    pub default_memory_mb: u64,
    /// Maximum allowed memory in megabytes. Defaults to 1024.
    #[serde(default = "max_memory")]
    pub max_memory_mb: u64,
}

impl ToolResourceConfig {
    /// Create a resource config with custom default timeout and memory.
    pub fn new(default_timeout_ms: u64, default_memory_mb: u64) -> Self {
        Self {
            default_timeout_ms,
            max_timeout_ms: max_timeout(),
            default_memory_mb,
            max_memory_mb: max_memory(),
        }
    }
}

impl Default for ToolResourceConfig {
    fn default() -> Self {
        Self {
            default_timeout_ms: default_timeout(),
            max_timeout_ms: max_timeout(),
            default_memory_mb: default_memory(),
            max_memory_mb: max_memory(),
        }
    }
}

fn default_timeout() -> u64 {
    30_000
}
fn max_timeout() -> u64 {
    120_000
}
fn default_memory() -> u64 {
    256
}
fn max_memory() -> u64 {
    1024
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_manifest_serde_roundtrip() {
        let manifest =
            ToolManifest::new(ToolMeta::new("test-tool", "A test tool").with_version("1.2.3"))
                .with_network(ToolNetworkConfig::new(vec!["example.com".into()]))
                .with_credentials(ToolCredentialConfig::new(vec!["github".into()]))
                .with_resources(ToolResourceConfig::new(5000, 128));

        let json = serde_json::to_string(&manifest).unwrap();
        let deserialized: ToolManifest = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.tool.name, "test-tool");
        assert_eq!(deserialized.tool.version, "1.2.3");
        assert_eq!(
            deserialized.network.as_ref().unwrap().allowlist,
            vec!["example.com"]
        );
        assert_eq!(
            deserialized.credentials.as_ref().unwrap().available,
            vec!["github"]
        );
        assert_eq!(
            deserialized.resources.as_ref().unwrap().default_timeout_ms,
            5000
        );
        assert_eq!(
            deserialized.resources.as_ref().unwrap().default_memory_mb,
            128
        );
    }

    #[test]
    fn test_manifest_defaults() {
        let json = r#"{"tool": {"name": "minimal", "description": "bare minimum"}}"#;
        let manifest: ToolManifest = serde_json::from_str(json).unwrap();

        assert_eq!(manifest.tool.name, "minimal");
        assert_eq!(manifest.tool.version, "0.1.0");
        assert!(manifest.network.is_none());
        assert!(manifest.credentials.is_none());
        assert!(manifest.resources.is_none());
    }

    #[test]
    fn test_manifest_with_schemas_roundtrip() {
        let input_schema = serde_json::json!({
            "type": "object",
            "properties": { "message": { "type": "string" } }
        });
        let output_schema = serde_json::json!({
            "type": "object",
            "properties": { "result": { "type": "string" } }
        });
        let manifest = ToolManifest::new(ToolMeta::new("schema-tool", "has schemas"))
            .with_input_schema(input_schema.clone())
            .with_output_schema(output_schema.clone());

        let json = serde_json::to_string(&manifest).unwrap();
        let de: ToolManifest = serde_json::from_str(&json).unwrap();
        assert_eq!(de.input_schema.unwrap(), input_schema);
        assert_eq!(de.output_schema.unwrap(), output_schema);
    }

    #[test]
    fn test_manifest_without_schemas_backward_compat() {
        let json = r#"{"tool": {"name": "old-tool", "description": "no schemas"}}"#;
        let manifest: ToolManifest = serde_json::from_str(json).unwrap();
        assert!(manifest.input_schema.is_none());
        assert!(manifest.output_schema.is_none());

        // Roundtrip should not include schema keys
        let serialized = serde_json::to_string(&manifest).unwrap();
        assert!(!serialized.contains("input_schema"));
        assert!(!serialized.contains("output_schema"));
    }

    #[test]
    fn test_manifest_builder() {
        let manifest = ToolManifest::new(ToolMeta::new("builder-test", "desc"))
            .with_network(ToolNetworkConfig::new(vec!["api.com".into()]))
            .with_credentials(ToolCredentialConfig::new(vec!["openai".into()]))
            .with_resources(ToolResourceConfig::new(10000, 512));

        assert_eq!(manifest.tool.name, "builder-test");
        assert!(manifest.network.is_some());
        assert!(manifest.credentials.is_some());
        assert_eq!(
            manifest.resources.as_ref().unwrap().default_timeout_ms,
            10000
        );
        assert_eq!(manifest.resources.as_ref().unwrap().default_memory_mb, 512);
    }
}
