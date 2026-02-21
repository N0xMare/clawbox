//! Configuration loading for clawbox.

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Errors from configuration loading.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum ConfigError {
    /// Failed to read the configuration file.
    #[error("failed to read config from {path}: {source}")]
    Read {
        path: String,
        source: std::io::Error,
    },
    /// Failed to parse the configuration file.
    #[error("failed to parse config from {path}: {source}")]
    Parse {
        path: String,
        source: toml::de::Error,
    },
    /// Configuration value is invalid.
    #[error("config validation error: {0}")]
    Validation(String),
}

#[derive(Debug, Clone, Deserialize)]
#[non_exhaustive]
pub struct ClawboxConfig {
    /// HTTP server configuration (host, port, auth).
    #[serde(default)]
    pub server: ServerConfig,
    /// WASM sandbox configuration (fuel, timeouts, tool directory).
    #[serde(default)]
    pub sandbox: SandboxConfig,
    /// Outbound HTTP proxy configuration (response limits, timeouts).
    #[serde(default)]
    pub proxy: ProxyConfig,
    /// Credential store configuration (encryption, storage path).
    #[serde(default)]
    pub credentials: CredentialsConfig,
    /// Logging and audit trail configuration.
    #[serde(default)]
    pub logging: LoggingConfig,
    /// Container management configuration (limits, workspace).
    #[serde(default)]
    pub containers: ContainerConfig,
    /// Server-side container security policy.
    #[serde(default)]
    pub container_policy: ContainerPolicy,
    #[serde(default)]
    pub tools: ToolsConfig,
    #[serde(default)]
    pub images: ImagesConfig,
}

#[derive(Clone, Deserialize)]
#[non_exhaustive]
pub struct ServerConfig {
    /// Address to bind the HTTP listener to.
    #[serde(default = "default_host")]
    pub host: String,
    /// TCP port for the HTTP listener.
    #[serde(default = "default_port")]
    pub port: u16,
    /// Bearer token required for API authentication.
    #[serde(default = "default_token")]
    pub auth_token: String,
    /// Path for Unix domain socket listener (same-machine fast path).
    /// If set, the server listens on both TCP and this socket.
    #[serde(default)]
    pub unix_socket: Option<String>,
    /// Maximum number of concurrent tool executions.
    #[serde(default = "default_max_concurrent_executions")]
    pub max_concurrent_executions: usize,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            host: default_host(),
            port: default_port(),
            auth_token: default_token(),
            unix_socket: None,
            max_concurrent_executions: default_max_concurrent_executions(),
        }
    }
}

fn default_max_concurrent_executions() -> usize {
    10
}

impl std::fmt::Debug for ServerConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ServerConfig")
            .field("host", &self.host)
            .field("port", &self.port)
            .field("auth_token", &"[REDACTED]")
            .field("unix_socket", &self.unix_socket)
            .field("max_concurrent_executions", &self.max_concurrent_executions)
            .finish()
    }
}

fn default_host() -> String {
    "127.0.0.1".into()
}
fn default_port() -> u16 {
    9800
}
fn default_token() -> String {
    use rand::RngCore;
    let mut bytes = [0u8; 32];
    rand::rng().fill_bytes(&mut bytes);
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[derive(Debug, Clone, Deserialize)]
#[non_exhaustive]
pub struct SandboxConfig {
    /// Directory containing WASM tool modules.
    #[serde(default = "default_tool_dir")]
    pub tool_dir: String,
    /// Default fuel limit for WASM execution.
    #[serde(default = "default_fuel")]
    pub default_fuel: u64,
    /// Default execution timeout in milliseconds.
    #[serde(default = "default_timeout")]
    pub default_timeout_ms: u64,
    /// Whether to watch the tool directory for hot-reload.
    #[serde(default = "default_watch_tools")]
    pub watch_tools: bool,
}

impl Default for SandboxConfig {
    fn default() -> Self {
        Self {
            tool_dir: default_tool_dir(),
            default_fuel: default_fuel(),
            default_timeout_ms: default_timeout(),
            watch_tools: default_watch_tools(),
        }
    }
}

fn default_tool_dir() -> String {
    "./tools/wasm".into()
}
fn default_fuel() -> u64 {
    100_000_000
}
fn default_timeout() -> u64 {
    30_000
}
fn default_watch_tools() -> bool {
    true
}

#[derive(Debug, Clone, Deserialize)]
#[non_exhaustive]
pub struct ProxyConfig {
    /// Maximum response body size in bytes.
    #[serde(default = "default_max_response")]
    pub max_response_bytes: usize,
    /// Default proxy request timeout in milliseconds.
    #[serde(default = "default_timeout")]
    pub default_timeout_ms: u64,
}

impl Default for ProxyConfig {
    fn default() -> Self {
        Self {
            max_response_bytes: default_max_response(),
            default_timeout_ms: default_timeout(),
        }
    }
}

fn default_max_response() -> usize {
    1_048_576
}

#[derive(Debug, Clone, Deserialize)]
#[non_exhaustive]
pub struct CredentialsConfig {
    /// Path to the encrypted credential store file.
    #[serde(default = "default_store_path")]
    pub store_path: String,
}

impl Default for CredentialsConfig {
    fn default() -> Self {
        Self {
            store_path: default_store_path(),
        }
    }
}

fn default_store_path() -> String {
    "~/.clawbox/credentials.enc".into()
}

#[derive(Debug, Clone, Deserialize)]
#[non_exhaustive]
pub struct LoggingConfig {
    /// Log output format (`json` or `text`).
    #[serde(default = "default_format")]
    pub format: String,
    /// Minimum log level (`trace`, `debug`, `info`, `warn`, `error`).
    #[serde(default = "default_level")]
    pub level: String,
    /// Directory for audit log files.
    #[serde(default = "default_audit_dir")]
    pub audit_dir: String,
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            format: default_format(),
            level: default_level(),
            audit_dir: default_audit_dir(),
        }
    }
}

fn default_format() -> String {
    "json".into()
}
fn default_level() -> String {
    "info".into()
}
fn default_audit_dir() -> String {
    "./audit".into()
}

impl ClawboxConfig {
    pub fn load(path: &str) -> Result<Self, ConfigError> {
        let contents = std::fs::read_to_string(path).map_err(|source| ConfigError::Read {
            path: path.to_string(),
            source,
        })?;
        let config: Self = toml::from_str(&contents).map_err(|source| ConfigError::Parse {
            path: path.to_string(),
            source,
        })?;
        Ok(config)
    }

    /// Create a default config (useful for tests).
    pub fn default_config() -> Self {
        Self {
            server: ServerConfig::default(),
            sandbox: SandboxConfig::default(),
            proxy: ProxyConfig::default(),
            credentials: CredentialsConfig::default(),
            logging: LoggingConfig::default(),
            containers: ContainerConfig::default(),
            container_policy: ContainerPolicy::default(),
            tools: ToolsConfig::default(),
            images: ImagesConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[non_exhaustive]
pub struct ContainerConfig {
    /// Maximum number of concurrent containers.
    #[serde(default = "default_max_containers")]
    pub max_containers: usize,
    /// Root directory for container workspace mounts.
    #[serde(default = "default_workspace_root")]
    pub workspace_root: String,
}

impl Default for ContainerConfig {
    fn default() -> Self {
        Self {
            max_containers: default_max_containers(),
            workspace_root: default_workspace_root(),
        }
    }
}

fn default_max_containers() -> usize {
    10
}
fn default_workspace_root() -> String {
    "~/.clawbox/workspaces".into()
}

/// Server-side policy for container capabilities.
///
/// These settings override any client-requested values, preventing
/// authenticated clients from escalating their own privileges.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct ContainerPolicy {
    /// Server-side network allowlist for containers (overrides client requests).
    #[serde(default)]
    pub network_allowlist: Vec<String>,
    /// Credentials containers are allowed to request.
    #[serde(default)]
    pub allowed_credentials: Vec<String>,
    /// Sandbox policies that containers are allowed to use.
    /// Defaults to [WasmOnly, Container] — excludes ContainerDirect.
    #[serde(default = "default_allowed_policies")]
    pub allowed_policies: Vec<String>,
}

fn default_allowed_policies() -> Vec<String> {
    vec!["wasm_only".into(), "container".into()]
}

impl Default for ContainerPolicy {
    fn default() -> Self {
        Self {
            network_allowlist: Vec::new(),
            allowed_credentials: Vec::new(),
            allowed_policies: default_allowed_policies(),
        }
    }
}

/// Expand leading `~/` in a path to the user's home directory.
pub fn expand_tilde(path: &str) -> std::path::PathBuf {
    if let Some(rest) = path.strip_prefix("~/")
        && let Ok(home) = std::env::var("HOME")
    {
        return std::path::PathBuf::from(home).join(rest);
    }
    std::path::PathBuf::from(path)
}

impl ClawboxConfig {
    /// Apply environment variable overrides to the loaded configuration.
    pub fn apply_env_overrides(&mut self) {
        if let Ok(host) = std::env::var("CLAWBOX_HOST") {
            self.server.host = host;
        }
        if let Ok(port) = std::env::var("CLAWBOX_PORT")
            && let Ok(p) = port.parse::<u16>()
        {
            self.server.port = p;
        }
        if let Ok(token) = std::env::var("CLAWBOX_AUTH_TOKEN") {
            self.server.auth_token = token;
        }
        if let Ok(tool_dir) = std::env::var("CLAWBOX_TOOL_DIR") {
            self.sandbox.tool_dir = tool_dir;
        }
        if let Ok(level) = std::env::var("CLAWBOX_LOG_LEVEL") {
            self.logging.level = level;
        }
    }

    /// Validate configuration values. Returns actionable error messages.
    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.server.port == 0 {
            return Err(ConfigError::Validation(
                "invalid port 0 in [server]: must be 1-65535".into(),
            ));
        }
        if self.server.auth_token.is_empty() {
            return Err(ConfigError::Validation(
                "auth_token in [server] must not be empty".into(),
            ));
        }
        if self.sandbox.default_fuel == 0 {
            return Err(ConfigError::Validation(
                "default_fuel in [sandbox] must be positive".into(),
            ));
        }
        if self.sandbox.default_timeout_ms == 0 {
            return Err(ConfigError::Validation(
                "default_timeout_ms in [sandbox] must be positive".into(),
            ));
        }
        if self.server.max_concurrent_executions == 0 {
            return Err(ConfigError::Validation(
                "max_concurrent_executions in [server] must be positive".into(),
            ));
        }
        Ok(())
    }

    /// Expand tilde in all path-like config fields.
    pub fn expand_paths(&mut self) {
        self.sandbox.tool_dir = expand_tilde(&self.sandbox.tool_dir)
            .to_string_lossy()
            .into_owned();
        self.credentials.store_path = expand_tilde(&self.credentials.store_path)
            .to_string_lossy()
            .into_owned();
        self.logging.audit_dir = expand_tilde(&self.logging.audit_dir)
            .to_string_lossy()
            .into_owned();
        self.containers.workspace_root = expand_tilde(&self.containers.workspace_root)
            .to_string_lossy()
            .into_owned();
    }
}

/// Configuration for tool management.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[non_exhaustive]
pub struct ToolsConfig {
    /// Default language for scaffolding new tools (rust, js, ts).
    #[serde(default = "default_tool_language")]
    pub default_language: String,
}

impl Default for ToolsConfig {
    fn default() -> Self {
        Self {
            default_language: default_tool_language(),
        }
    }
}
fn default_tool_language() -> String {
    "rust".into()
}

/// Configuration for container image templates.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[non_exhaustive]
#[derive(Default)]
pub struct ImagesConfig {
    /// Named image templates for easy container spawning.
    #[serde(default)]
    pub templates: std::collections::HashMap<String, ImageTemplate>,
}

/// A named Docker image template with pre-configured capabilities.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[non_exhaustive]
pub struct ImageTemplate {
    /// Docker image name and tag.
    pub image: String,
    /// Human-readable description of this template.
    #[serde(default)]
    pub description: String,
    /// Network allowlist for this template.
    #[serde(default)]
    pub network_allowlist: Vec<String>,
    /// Credentials this template can access.
    #[serde(default)]
    pub credentials: Vec<String>,
    /// Override command for the container.
    #[serde(default)]
    pub command: Option<Vec<String>>,
    /// Maximum container lifetime in milliseconds.
    #[serde(default)]
    pub max_lifetime_ms: Option<u64>,
}

#[cfg(test)]
mod config_tests {
    use super::*;
    use serial_test::serial;

    #[test]
    fn test_default_config_is_valid() {
        let config = ClawboxConfig::default_config();
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_validation_catches_zero_fuel() {
        let mut config = ClawboxConfig::default_config();
        config.sandbox.default_fuel = 0;
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_validation_catches_empty_token() {
        let mut config = ClawboxConfig::default_config();
        config.server.auth_token = String::new();
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_expand_tilde() {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/home/test".into());
        let expanded = expand_tilde("~/foo/bar");
        assert_eq!(
            expanded,
            std::path::PathBuf::from(format!("{home}/foo/bar"))
        );

        let no_tilde = expand_tilde("/abs/path");
        assert_eq!(no_tilde, std::path::PathBuf::from("/abs/path"));
    }

    #[test]
    #[serial]
    fn test_env_overrides() {
        let mut config = ClawboxConfig::default_config();
        unsafe { std::env::set_var("CLAWBOX_PORT", "9999") };
        config.apply_env_overrides();
        assert_eq!(config.server.port, 9999);
        unsafe { std::env::remove_var("CLAWBOX_PORT") };
    }

    #[test]
    fn test_default_allowed_policies_are_snake_case() {
        let policies = default_allowed_policies();
        assert!(policies.contains(&"wasm_only".to_string()));
        assert!(policies.contains(&"container".to_string()));
    }

    #[test]
    fn test_tools_config_default() {
        let config = ToolsConfig::default();
        assert_eq!(config.default_language, "rust");
    }

    #[test]
    fn test_tools_config_parsing() {
        let toml = r#"

    default_language = "js"
    "#;
        let tools: ToolsConfig = toml::from_str(toml).unwrap();
        assert_eq!(tools.default_language, "js");
    }

    #[test]
    fn test_images_config_default() {
        let config = ImagesConfig::default();
        assert!(config.templates.is_empty());
    }

    #[test]
    fn test_image_template_parsing() {
        let toml = r#"
    [templates.example]
    image = "alpine:latest"
    description = "Test template"
    network_allowlist = ["example.com"]
    credentials = ["GITHUB_TOKEN"]
    command = ["/bin/sh"]
    max_lifetime_ms = 60000
    "#;
        let images: ImagesConfig = toml::from_str(toml).unwrap();
        let template = images.templates.get("example").unwrap();
        assert_eq!(template.image, "alpine:latest");
        assert_eq!(template.description, "Test template");
        assert_eq!(template.network_allowlist, vec!["example.com"]);
        assert_eq!(template.credentials, vec!["GITHUB_TOKEN"]);
        assert_eq!(template.command, Some(vec!["/bin/sh".to_string()]));
        assert_eq!(template.max_lifetime_ms, Some(60000));
    }

    #[test]
    fn test_image_template_minimal() {
        let toml = r#"
    [templates.minimal]
    image = "busybox"
    "#;
        let images: ImagesConfig = toml::from_str(toml).unwrap();
        let template = images.templates.get("minimal").unwrap();
        assert_eq!(template.image, "busybox");
        assert!(template.description.is_empty());
        assert!(template.network_allowlist.is_empty());
        assert!(template.credentials.is_empty());
        assert_eq!(template.command, None);
        assert_eq!(template.max_lifetime_ms, None);
    }
}
