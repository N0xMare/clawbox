//! Agent-level types for container orchestration.
//!
//! Agents are long-lived identities that own containers. Each agent has
//! a configuration, lifecycle policy, and runtime status.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::policy::{Capabilities, SandboxPolicy};

/// Configuration for registering an agent.
///
/// Defines the agent's identity, sandbox policy, capabilities, and
/// lifecycle rules. Submitted via the agent registration API.
#[non_exhaustive]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConfig {
    /// Unique agent identifier (alphanumeric + hyphens, max 64 chars).
    pub agent_id: String,
    /// Human-readable display name.
    pub name: String,
    /// Sandbox policy for this agent's container. Defaults to `WasmOnly`.
    #[serde(default)]
    pub policy: SandboxPolicy,
    /// Capabilities (network, credentials, resources).
    #[serde(default)]
    pub capabilities: Capabilities,
    /// Workspace mount configuration. `None` for no persistent workspace.
    #[serde(default)]
    pub workspace: Option<WorkspaceConfig>,
    /// Lifecycle configuration (idle timeout, restart policy).
    #[serde(default)]
    pub lifecycle: LifecycleConfig,
    /// Extra environment variables injected into the agent's container.
    #[serde(default)]
    pub env: std::collections::HashMap<String, String>,
    /// Docker image override. `None` uses the server default.
    pub image: Option<String>,
}

impl AgentConfig {
    /// Create an agent config with the given ID and name, using defaults for everything else.
    pub fn new(agent_id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            agent_id: agent_id.into(),
            name: name.into(),
            policy: SandboxPolicy::default(),
            capabilities: Capabilities::default(),
            workspace: None,
            lifecycle: LifecycleConfig::default(),
            env: std::collections::HashMap::new(),
            image: None,
        }
    }

    /// Set the sandbox policy.
    pub fn with_policy(mut self, policy: SandboxPolicy) -> Self {
        self.policy = policy;
        self
    }

    /// Set capabilities.
    pub fn with_capabilities(mut self, capabilities: Capabilities) -> Self {
        self.capabilities = capabilities;
        self
    }

    /// Set workspace configuration.
    pub fn with_workspace(mut self, workspace: WorkspaceConfig) -> Self {
        self.workspace = Some(workspace);
        self
    }

    /// Set lifecycle configuration.
    pub fn with_lifecycle(mut self, lifecycle: LifecycleConfig) -> Self {
        self.lifecycle = lifecycle;
        self
    }

    /// Set the Docker image.
    pub fn with_image(mut self, image: impl Into<String>) -> Self {
        self.image = Some(image.into());
        self
    }
}

/// Workspace mount configuration for an agent.
///
/// Controls how the host filesystem is exposed inside the container.
#[non_exhaustive]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceConfig {
    /// Host path to mount. If `None`, auto-generated under the server's workspace root.
    pub host_path: Option<String>,
    /// Mount point inside the container. Defaults to `"/workspace"`.
    #[serde(default = "default_mount_point")]
    pub mount_point: String,
    /// Whether the mount is read-only. Defaults to `false`.
    #[serde(default)]
    pub read_only: bool,
}

impl WorkspaceConfig {
    /// Create a workspace config with defaults (auto-generated path, `/workspace` mount, read-write).
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the host path.
    pub fn with_host_path(mut self, path: impl Into<String>) -> Self {
        self.host_path = Some(path.into());
        self
    }

    /// Set the container mount point.
    pub fn with_mount_point(mut self, mount_point: impl Into<String>) -> Self {
        self.mount_point = mount_point.into();
        self
    }

    /// Make the mount read-only.
    pub fn read_only(mut self) -> Self {
        self.read_only = true;
        self
    }
}

impl Default for WorkspaceConfig {
    fn default() -> Self {
        Self {
            host_path: None,
            mount_point: default_mount_point(),
            read_only: false,
        }
    }
}

fn default_mount_point() -> String {
    "/workspace".into()
}

/// Lifecycle configuration for an agent container.
///
/// Controls idle timeouts, maximum lifetime, and crash recovery.
#[non_exhaustive]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LifecycleConfig {
    /// Maximum idle time before auto-stop, in milliseconds. Defaults to 3600000 (1 hour).
    #[serde(default = "default_max_idle_ms")]
    pub max_idle_ms: u64,
    /// Maximum total lifetime in milliseconds. `None` means unlimited.
    pub max_lifetime_ms: Option<u64>,
    /// Whether to restart the container on crash. Defaults to `false`.
    #[serde(default)]
    pub restart_on_crash: bool,
    /// Maximum restart attempts before giving up. Defaults to 3.
    #[serde(default = "default_max_restarts")]
    pub max_restarts: u32,
}

impl LifecycleConfig {
    /// Create a lifecycle config with the given idle timeout in milliseconds.
    pub fn new(max_idle_ms: u64) -> Self {
        Self {
            max_idle_ms,
            ..Self::default()
        }
    }

    /// Enable crash restart with the given max attempts.
    pub fn with_restart(mut self, max_restarts: u32) -> Self {
        self.restart_on_crash = true;
        self.max_restarts = max_restarts;
        self
    }

    /// Set maximum total lifetime in milliseconds.
    pub fn with_max_lifetime(mut self, ms: u64) -> Self {
        self.max_lifetime_ms = Some(ms);
        self
    }
}

impl Default for LifecycleConfig {
    fn default() -> Self {
        Self {
            max_idle_ms: default_max_idle_ms(),
            max_lifetime_ms: None,
            restart_on_crash: false,
            max_restarts: default_max_restarts(),
        }
    }
}

fn default_max_idle_ms() -> u64 {
    3_600_000
}

fn default_max_restarts() -> u32 {
    3
}

/// Runtime information about a registered agent.
///
/// Returned by the agent listing and status APIs.
#[non_exhaustive]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentInfo {
    /// Unique agent identifier.
    pub agent_id: String,
    /// Human-readable display name.
    pub name: String,
    /// Current lifecycle status.
    pub status: AgentStatus,
    /// Docker container ID, if the agent has a running container.
    pub container_id: Option<String>,
    /// When this agent was registered.
    pub created_at: DateTime<Utc>,
    /// Timestamp of the most recent execution or heartbeat.
    pub last_activity: DateTime<Utc>,
    /// Total number of tool executions performed by this agent.
    pub execution_count: u64,
    /// Host-side workspace path, if configured.
    pub workspace_path: Option<String>,
}

impl AgentInfo {
    /// Create agent info with the given ID, name, and status.
    pub fn new(agent_id: impl Into<String>, name: impl Into<String>, status: AgentStatus) -> Self {
        let now = Utc::now();
        Self {
            agent_id: agent_id.into(),
            name: name.into(),
            status,
            container_id: None,
            created_at: now,
            last_activity: now,
            execution_count: 0,
            workspace_path: None,
        }
    }
}

/// Current lifecycle status of an agent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum AgentStatus {
    /// Agent is registered but not executing anything.
    Idle,
    /// Agent is actively executing a tool or task.
    Running,
    /// Agent's container is being created.
    Starting,
    /// Agent's container is shutting down.
    Stopping,
    /// Agent's container crashed or encountered an unrecoverable error.
    Failed,
    /// Agent was explicitly deregistered or its container was removed.
    Terminated,
}

impl std::fmt::Display for AgentStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Idle => write!(f, "idle"),
            Self::Running => write!(f, "running"),
            Self::Starting => write!(f, "starting"),
            Self::Stopping => write!(f, "stopping"),
            Self::Failed => write!(f, "failed"),
            Self::Terminated => write!(f, "terminated"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lifecycle_defaults() {
        let lc = LifecycleConfig::default();
        assert_eq!(lc.max_idle_ms, 3_600_000);
        assert_eq!(lc.max_restarts, 3);
        assert!(!lc.restart_on_crash);
        assert!(lc.max_lifetime_ms.is_none());
    }

    #[test]
    fn test_agent_status_display() {
        assert_eq!(AgentStatus::Idle.to_string(), "idle");
        assert_eq!(AgentStatus::Running.to_string(), "running");
    }

    #[test]
    fn test_agent_config_serde() {
        let json = r#"{
            "agent_id": "test-agent",
            "name": "Test Agent",
            "policy": "container",
            "capabilities": {},
            "lifecycle": {},
            "env": {},
            "image": null
        }"#;
        let config: AgentConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.agent_id, "test-agent");
        assert_eq!(config.policy, SandboxPolicy::Container);
    }

    #[test]
    fn test_agent_config_builder() {
        let config = AgentConfig::new("my-agent", "My Agent").with_policy(SandboxPolicy::Container);
        assert_eq!(config.agent_id, "my-agent");
        assert_eq!(config.policy, SandboxPolicy::Container);
    }

    #[test]
    fn test_agent_config_new_defaults() {
        let config = AgentConfig::new("test", "Test");
        assert_eq!(config.agent_id, "test");
        assert_eq!(config.policy, SandboxPolicy::WasmOnly);
        assert!(config.workspace.is_none());
        assert!(config.image.is_none());
    }

    #[test]
    fn test_agent_config_full_builder() {
        let config = AgentConfig::new("a1", "Agent One")
            .with_policy(SandboxPolicy::Container)
            .with_capabilities(Capabilities::default())
            .with_workspace(WorkspaceConfig::new().with_host_path("/data").read_only())
            .with_lifecycle(LifecycleConfig::new(60000).with_restart(5))
            .with_image("alpine:latest");
        assert_eq!(config.policy, SandboxPolicy::Container);
        assert!(config.workspace.as_ref().unwrap().read_only);
        assert!(config.lifecycle.restart_on_crash);
        assert_eq!(config.lifecycle.max_restarts, 5);
    }

    #[test]
    fn test_workspace_config_defaults() {
        let ws = WorkspaceConfig::default();
        assert!(ws.host_path.is_none());
        assert_eq!(ws.mount_point, "/workspace");
        assert!(!ws.read_only);
    }

    #[test]
    fn test_workspace_config_builder() {
        let ws = WorkspaceConfig::new()
            .with_mount_point("/custom")
            .read_only();
        assert_eq!(ws.mount_point, "/custom");
        assert!(ws.read_only);
    }

    #[test]
    fn test_agent_info_new() {
        let info = AgentInfo::new("agent-1", "My Agent", AgentStatus::Idle);
        assert_eq!(info.agent_id, "agent-1");
        assert_eq!(info.status, AgentStatus::Idle);
        assert!(info.container_id.is_none());
        assert_eq!(info.execution_count, 0);
    }

    #[test]
    fn test_lifecycle_config_builder() {
        let lc = LifecycleConfig::new(30000)
            .with_restart(10)
            .with_max_lifetime(600000);
        assert_eq!(lc.max_idle_ms, 30000);
        assert!(lc.restart_on_crash);
        assert_eq!(lc.max_lifetime_ms, Some(600000));
    }

    #[test]
    fn test_agent_status_all_variants() {
        for v in [
            AgentStatus::Idle,
            AgentStatus::Running,
            AgentStatus::Starting,
            AgentStatus::Stopping,
            AgentStatus::Failed,
            AgentStatus::Terminated,
        ] {
            assert!(!v.to_string().is_empty());
        }
    }
}
