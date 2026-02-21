#![doc = include_str!("../README.md")]

pub mod agent;
pub mod api;
pub mod host;
pub mod manifest;
pub mod patterns;
pub mod policy;

pub use agent::{AgentConfig, AgentInfo, AgentStatus, LifecycleConfig, WorkspaceConfig};
pub use api::{
    ApiError, ComponentHealth, ContainerInfo, ContainerSpawnRequest, ContainerStatus,
    ExecuteRequest, ExecuteResponse, ExecutionMetadata, ExecutionStatus, HealthComponents,
    HealthResponse, ResourceUsage, SanitizationReport,
};
pub use host::HostCallHandler;
pub use manifest::{
    ToolCredentialConfig, ToolManifest, ToolMeta, ToolNetworkConfig, ToolResourceConfig,
};
pub use policy::{Capabilities, NetworkCapabilities, ResourceLimits, SandboxPolicy};

/// Default maximum concurrent connections, shared across policy and manifest.
pub const DEFAULT_MAX_CONCURRENT: usize = 5;
