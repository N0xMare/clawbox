//! Sandbox policies defining the security envelope for executions.

use serde::{Deserialize, Serialize};

/// Security policy controlling isolation level.
///
/// ```text
/// ┌─────────────────┬──────────┬──────────┬───────────────────────────┐
/// │ Policy          │ Docker   │ WASM     │ Network                   │
/// ├─────────────────┼──────────┼──────────┼───────────────────────────┤
/// │ WasmOnly        │ No       │ Yes      │ Proxied (allowlist only)  │
/// │ Container       │ Yes      │ Yes      │ Proxied (allowlist only)  │
/// │ ContainerDirect │ Yes      │ No       │ Direct (no proxy)          │
/// └─────────────────┴──────────┴──────────┴───────────────────────────┘
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum SandboxPolicy {
    /// WASM sandbox only, no Docker container. Lightweight tool calls.
    #[default]
    WasmOnly,
    /// Docker container with WASM tool sandboxing inside. Full sub-agent runtime.
    Container,
    /// Docker container without WASM. For heavy tasks needing full Linux env.
    ContainerDirect,
}

impl SandboxPolicy {
    /// Whether this policy uses Docker containers.
    pub fn uses_docker(&self) -> bool {
        matches!(self, Self::Container | Self::ContainerDirect)
    }

    /// Whether this policy uses WASM sandboxing for tool execution.
    pub fn uses_wasm(&self) -> bool {
        matches!(self, Self::WasmOnly | Self::Container)
    }

    /// Returns whether this policy requires proxied network access.
    ///
    /// `ContainerDirect` intentionally bypasses the proxy for trusted
    /// workloads that need unrestricted network access.
    pub fn is_proxied(&self) -> bool {
        matches!(self, Self::WasmOnly | Self::Container)
    }
}

impl std::fmt::Display for SandboxPolicy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::WasmOnly => write!(f, "wasm_only"),
            Self::Container => write!(f, "container"),
            Self::ContainerDirect => write!(f, "container_direct"),
        }
    }
}

impl std::str::FromStr for SandboxPolicy {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().replace('-', "_").as_str() {
            "wasm_only" | "wasm" | "wasmonly" => Ok(Self::WasmOnly),
            "container" => Ok(Self::Container),
            "container_direct" | "direct" | "containerdirect" => Ok(Self::ContainerDirect),
            _ => Err(format!(
                "invalid policy '{s}', expected: wasm_only, container, container_direct"
            )),
        }
    }
}

/// Resource limits for sandboxed execution.
///
/// Controls the CPU, memory, time, and output budgets available to a
/// single tool invocation or container.
#[non_exhaustive]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceLimits {
    /// Maximum execution time in milliseconds. Defaults to 120000 (2 minutes).
    pub timeout_ms: u64,
    /// Maximum memory in megabytes. Defaults to 512.
    pub memory_mb: u64,
    /// CPU shares (relative weight). Defaults to 1024. Higher values get
    /// proportionally more CPU time when contending with other containers.
    pub cpu_shares: u32,
    /// Maximum output size in bytes. Defaults to 262144 (256 KB).
    pub max_output_bytes: usize,
}

impl ResourceLimits {
    /// Create resource limits with the given timeout and memory.
    pub fn new(timeout_ms: u64, memory_mb: u64) -> Self {
        Self {
            timeout_ms,
            memory_mb,
            ..Self::default()
        }
    }

    /// Set CPU shares.
    pub fn with_cpu_shares(mut self, cpu_shares: u32) -> Self {
        self.cpu_shares = cpu_shares;
        self
    }

    /// Set maximum output size in bytes.
    pub fn with_max_output_bytes(mut self, max_output_bytes: usize) -> Self {
        self.max_output_bytes = max_output_bytes;
        self
    }
}

impl Default for ResourceLimits {
    fn default() -> Self {
        Self {
            timeout_ms: 120_000,
            memory_mb: 512,
            cpu_shares: 1024,
            max_output_bytes: 256 * 1024,
        }
    }
}

/// Capability requirements for an execution.
///
/// Combines network access rules, credential access, and resource limits
/// into a single capability set that can be attached to a request or agent.
#[non_exhaustive]
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Capabilities {
    /// Network access configuration (domain allowlist, concurrency).
    #[serde(default)]
    pub network: NetworkCapabilities,
    /// Names of credentials the execution may use (e.g., `"openai"`, `"github"`).
    #[serde(default)]
    pub credentials: Vec<String>,
    /// Resource limits (timeout, memory, CPU, output size).
    #[serde(default)]
    pub resources: ResourceLimits,
}

impl Capabilities {
    /// Create capabilities with the given network configuration.
    pub fn new(network: NetworkCapabilities) -> Self {
        Self {
            network,
            credentials: Vec::new(),
            resources: ResourceLimits::default(),
        }
    }

    /// Add a credential name to the allowed set.
    pub fn with_credential(mut self, name: impl Into<String>) -> Self {
        self.credentials.push(name.into());
        self
    }

    /// Set resource limits.
    pub fn with_resources(mut self, resources: ResourceLimits) -> Self {
        self.resources = resources;
        self
    }
}

/// Network access capabilities.
///
/// Controls which external domains a sandboxed execution can reach
/// and how many concurrent connections it may open.
#[non_exhaustive]
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NetworkCapabilities {
    /// Allowlisted domains. An empty list means no network access.
    #[serde(default)]
    pub allowlist: Vec<String>,
    /// Maximum concurrent outbound connections. Defaults to [`DEFAULT_MAX_CONCURRENT`](crate::DEFAULT_MAX_CONCURRENT).
    #[serde(default = "default_max_concurrent")]
    pub max_concurrent: usize,
}

impl NetworkCapabilities {
    /// Create network capabilities with the given domain allowlist.
    pub fn new(allowlist: Vec<String>) -> Self {
        Self {
            allowlist,
            max_concurrent: default_max_concurrent(),
        }
    }

    /// Set the maximum concurrent connections.
    pub fn with_max_concurrent(mut self, max: usize) -> Self {
        self.max_concurrent = max;
        self
    }
}

fn default_max_concurrent() -> usize {
    crate::DEFAULT_MAX_CONCURRENT
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_from_str_roundtrip() {
        for policy in [
            SandboxPolicy::WasmOnly,
            SandboxPolicy::Container,
            SandboxPolicy::ContainerDirect,
        ] {
            let s = policy.to_string();
            let parsed: SandboxPolicy = s.parse().unwrap();
            assert_eq!(parsed, policy);
        }
    }

    #[test]
    fn test_from_str_invalid() {
        assert!("garbage".parse::<SandboxPolicy>().is_err());
    }

    #[test]
    fn test_uses_docker() {
        assert!(!SandboxPolicy::WasmOnly.uses_docker());
        assert!(SandboxPolicy::Container.uses_docker());
        assert!(SandboxPolicy::ContainerDirect.uses_docker());
    }

    #[test]
    fn test_uses_wasm() {
        assert!(SandboxPolicy::WasmOnly.uses_wasm());
        assert!(SandboxPolicy::Container.uses_wasm());
        assert!(!SandboxPolicy::ContainerDirect.uses_wasm());
    }

    #[test]
    fn test_all_proxied() {
        assert!(SandboxPolicy::WasmOnly.is_proxied());
        assert!(SandboxPolicy::Container.is_proxied());
        assert!(!SandboxPolicy::ContainerDirect.is_proxied());
    }

    #[test]
    fn test_default_is_wasm_only() {
        assert_eq!(SandboxPolicy::default(), SandboxPolicy::WasmOnly);
    }

    #[test]
    fn test_capabilities_default() {
        let cap = Capabilities::default();
        assert!(cap.credentials.is_empty());
        assert!(cap.network.allowlist.is_empty());
    }

    #[test]
    fn test_resource_limits_new() {
        let rl = ResourceLimits::new(5000, 256);
        assert_eq!(rl.timeout_ms, 5000);
        assert_eq!(rl.memory_mb, 256);
        assert_eq!(rl.cpu_shares, 1024);
    }

    #[test]
    fn test_resource_limits_builder() {
        let rl = ResourceLimits::new(1000, 128)
            .with_cpu_shares(512)
            .with_max_output_bytes(1024);
        assert_eq!(rl.cpu_shares, 512);
        assert_eq!(rl.max_output_bytes, 1024);
    }

    #[test]
    fn test_resource_limits_default() {
        let rl = ResourceLimits::default();
        assert_eq!(rl.timeout_ms, 120_000);
        assert_eq!(rl.memory_mb, 512);
    }

    #[test]
    fn test_capabilities_builder() {
        let cap = Capabilities::new(NetworkCapabilities::new(vec!["api.github.com".into()]))
            .with_credential("github")
            .with_resources(ResourceLimits::new(5000, 128));
        assert_eq!(cap.network.allowlist, vec!["api.github.com"]);
        assert_eq!(cap.credentials, vec!["github"]);
    }

    #[test]
    fn test_network_capabilities_new() {
        let nc = NetworkCapabilities::new(vec!["example.com".into()]).with_max_concurrent(10);
        assert_eq!(nc.allowlist, vec!["example.com"]);
        assert_eq!(nc.max_concurrent, 10);
    }

    #[test]
    fn test_sandbox_policy_serde_roundtrip() {
        for policy in [
            SandboxPolicy::WasmOnly,
            SandboxPolicy::Container,
            SandboxPolicy::ContainerDirect,
        ] {
            let json = serde_json::to_string(&policy).unwrap();
            let parsed: SandboxPolicy = serde_json::from_str(&json).unwrap();
            assert_eq!(parsed, policy);
        }
    }

    #[test]
    fn test_from_str_aliases() {
        assert_eq!(
            "wasm".parse::<SandboxPolicy>().unwrap(),
            SandboxPolicy::WasmOnly
        );
        assert_eq!(
            "direct".parse::<SandboxPolicy>().unwrap(),
            SandboxPolicy::ContainerDirect
        );
    }
}

#[cfg(test)]
mod config_policy_tests {
    use super::*;

    /// C1: Verify that default allowed_policies match SandboxPolicy::to_string() output.
    #[test]
    fn test_default_allowed_policies_match_display() {
        // These are the defaults from config.rs
        let allowed = vec!["wasm_only".to_string(), "container".to_string()];

        let wasm_only_str = SandboxPolicy::WasmOnly.to_string();
        let container_str = SandboxPolicy::Container.to_string();

        assert!(
            allowed.contains(&wasm_only_str),
            "WasmOnly display '{}' not in allowed policies",
            wasm_only_str
        );
        assert!(
            allowed.contains(&container_str),
            "Container display '{}' not in allowed policies",
            container_str
        );
    }

    /// C1: Case-insensitive comparison works for same-format strings.
    #[test]
    fn test_policy_case_insensitive_match() {
        let policy_str = SandboxPolicy::WasmOnly.to_string();
        let allowed = vec!["WASM_ONLY".to_string(), "CONTAINER".to_string()];
        assert!(allowed.iter().any(|p| p.eq_ignore_ascii_case(&policy_str)));

        let allowed_snake = vec!["wasm_only".to_string(), "container".to_string()];
        assert!(
            allowed_snake
                .iter()
                .any(|p| p.eq_ignore_ascii_case(&policy_str))
        );
    }

    #[test]
    fn test_from_str_pascal_case() {
        // FromStr should handle lowercased PascalCase
        assert_eq!(
            "wasmonly".parse::<SandboxPolicy>().unwrap(),
            SandboxPolicy::WasmOnly
        );
        assert_eq!(
            "containerdirect".parse::<SandboxPolicy>().unwrap(),
            SandboxPolicy::ContainerDirect
        );
    }
}
