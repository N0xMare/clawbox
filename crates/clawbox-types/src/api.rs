//! API request and response types for the clawbox HTTP interface.

use serde::{Deserialize, Serialize};

use crate::policy::{Capabilities, SandboxPolicy};

/// Request to execute a single tool call in a WASM sandbox.
///
/// At minimum, specify the `tool` name and `params`. Optionally override
/// the caller's default capabilities for this invocation.
#[non_exhaustive]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecuteRequest {
    /// Tool name to invoke (must match a registered manifest).
    pub tool: String,
    /// JSON parameters passed to the tool's entry point.
    #[serde(default)]
    pub params: serde_json::Value,
    /// Optional capability overrides for this execution.
    /// When `None`, the server applies the tool's default capabilities.
    #[serde(default)]
    pub capabilities: Option<Capabilities>,
}

impl ExecuteRequest {
    /// Create a new execution request for the given tool with the supplied parameters.
    pub fn new(tool: impl Into<String>, params: serde_json::Value) -> Self {
        Self {
            tool: tool.into(),
            params,
            capabilities: None,
        }
    }

    /// Override the default capabilities for this execution.
    pub fn with_capabilities(mut self, capabilities: Capabilities) -> Self {
        self.capabilities = Some(capabilities);
        self
    }
}

/// Response from a tool execution.
///
/// Contains the execution result, any errors, and metadata about
/// resource consumption and sanitization.
#[non_exhaustive]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecuteResponse {
    /// Caller-assigned request ID for correlation. Echoed back if provided.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    /// Overall execution outcome.
    pub status: ExecutionStatus,
    /// Tool output on success. `None` when `status` is not `Ok`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<serde_json::Value>,
    /// Human-readable error message on failure.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Execution telemetry (timing, fuel, logs, sanitization).
    pub metadata: ExecutionMetadata,
}

impl ExecuteResponse {
    /// Create a successful response with the given output and metadata.
    pub fn ok(output: serde_json::Value, metadata: ExecutionMetadata) -> Self {
        Self {
            request_id: None,
            status: ExecutionStatus::Ok,
            output: Some(output),
            error: None,
            metadata,
        }
    }

    /// Create an error response with the given message and metadata.
    pub fn error(error: impl Into<String>, metadata: ExecutionMetadata) -> Self {
        Self {
            request_id: None,
            status: ExecutionStatus::Error,
            output: None,
            error: Some(error.into()),
            metadata,
        }
    }

    /// Set the request ID for correlation.
    pub fn with_request_id(mut self, id: impl Into<String>) -> Self {
        self.request_id = Some(id.into());
        self
    }
}

/// Outcome status of a tool execution.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ExecutionStatus {
    /// Tool completed successfully.
    Ok,
    /// Tool returned an error.
    Error,
    /// Execution exceeded its time limit.
    Timeout,
    /// Execution was blocked by policy (e.g., disallowed network access).
    Blocked,
}

/// Telemetry collected during a tool execution.
#[non_exhaustive]
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ExecutionMetadata {
    /// Wall-clock execution time in milliseconds.
    pub execution_time_ms: u64,
    /// WASM fuel (instruction budget) consumed. 0 for container-direct executions.
    pub fuel_consumed: u64,
    /// Structured log entries emitted by the tool during execution.
    #[serde(default)]
    pub logs: Vec<serde_json::Value>,
    /// Output sanitization report (credential scrubbing, etc.).
    pub sanitization: SanitizationReport,
}

impl ExecutionMetadata {
    /// Create metadata with the given execution time and fuel consumption.
    pub fn new(execution_time_ms: u64, fuel_consumed: u64) -> Self {
        Self {
            execution_time_ms,
            fuel_consumed,
            logs: Vec::new(),
            sanitization: SanitizationReport::default(),
        }
    }
}

/// Report of output sanitization actions taken after execution.
///
/// The clawbox proxy scans tool output for leaked credentials and other
/// sensitive patterns, redacting them before returning results.
#[must_use]
#[non_exhaustive]
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SanitizationReport {
    /// Number of sensitive patterns detected in the output.
    pub issues_found: u32,
    /// Human-readable descriptions of sanitization actions taken.
    pub actions_taken: Vec<String>,
}

impl SanitizationReport {
    /// Create a report with the given number of issues and actions.
    pub fn new(issues_found: u32, actions_taken: Vec<String>) -> Self {
        Self {
            issues_found,
            actions_taken,
        }
    }
}

/// Request to spawn a new agent container.
#[non_exhaustive]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContainerSpawnRequest {
    /// Human-readable task description for this container.
    pub task: String,
    /// Docker image override. When `None`, uses the server default.
    #[serde(default)]
    pub image: Option<String>,
    /// Sandbox policy for the container. Defaults to `WasmOnly`.
    #[serde(default)]
    pub policy: SandboxPolicy,
    /// Capability requirements for the container.
    #[serde(default)]
    pub capabilities: Capabilities,
    /// Extra environment variables injected into the container.
    #[serde(default)]
    pub env: std::collections::HashMap<String, String>,
    /// Optional command to run in the container (overrides image CMD).
    #[serde(default)]
    pub command: Option<Vec<String>>,
}

impl ContainerSpawnRequest {
    /// Create a container spawn request for the given task with specified capabilities.
    pub fn new(task: impl Into<String>, capabilities: Capabilities) -> Self {
        Self {
            task: task.into(),
            image: None,
            policy: SandboxPolicy::default(),
            capabilities,
            env: std::collections::HashMap::new(),
            command: None,
        }
    }

    /// Override the sandbox policy.
    pub fn with_policy(mut self, policy: SandboxPolicy) -> Self {
        self.policy = policy;
        self
    }

    /// Set the Docker image.
    pub fn with_image(mut self, image: impl Into<String>) -> Self {
        self.image = Some(image.into());
        self
    }

    /// Set the container command.
    pub fn with_command(mut self, cmd: Vec<String>) -> Self {
        self.command = Some(cmd);
        self
    }

    /// Add an environment variable.
    pub fn with_env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.env.insert(key.into(), value.into());
        self
    }
}

/// Lifecycle status of a container.
#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, Clone)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ContainerStatus {
    /// Container is being created.
    Creating,
    /// Container is running and accepting requests.
    Running,
    /// Container finished its task normally.
    Completed,
    /// Container exited with an error.
    Failed,
    /// Container exceeded its time limit.
    TimedOut,
    /// Container was explicitly killed.
    Killed,
}

/// Runtime information about an active or recently-active container.
#[non_exhaustive]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContainerInfo {
    /// Unique container identifier.
    pub container_id: String,
    /// Current lifecycle status.
    pub status: ContainerStatus,
    /// Sandbox policy in effect.
    pub policy: SandboxPolicy,
    /// When this container was created.
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// Brief description of the container's task.
    pub task_summary: String,
    /// Unix socket path for the container proxy (in-container path).
    pub proxy_socket: String,
    /// Current resource usage snapshot. `None` if not yet available.
    pub resource_usage: Option<ResourceUsage>,
}

impl ContainerInfo {
    /// Create container info with the minimum required fields.
    pub fn new(
        container_id: impl Into<String>,
        status: ContainerStatus,
        policy: SandboxPolicy,
        task_summary: impl Into<String>,
        proxy_socket: impl Into<String>,
    ) -> Self {
        Self {
            container_id: container_id.into(),
            status,
            policy,
            created_at: chrono::Utc::now(),
            task_summary: task_summary.into(),
            proxy_socket: proxy_socket.into(),
            resource_usage: None,
        }
    }
}

/// Snapshot of a container's resource consumption.
#[non_exhaustive]
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ResourceUsage {
    /// Current memory usage in bytes.
    pub memory_bytes: u64,
    /// CPU utilization as a percentage (0.0–100.0).
    pub cpu_percent: f64,
    /// Total outbound network requests made.
    pub network_requests: u32,
    /// Wall-clock time the container has been running, in milliseconds.
    pub duration_ms: u64,
}

impl ResourceUsage {
    /// Create a new resource usage snapshot.
    pub fn new(
        memory_bytes: u64,
        cpu_percent: f64,
        network_requests: u32,
        duration_ms: u64,
    ) -> Self {
        Self {
            memory_bytes,
            cpu_percent,
            network_requests,
            duration_ms,
        }
    }
}

/// Health check response from the clawbox server.
#[non_exhaustive]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthResponse {
    /// Overall health status (e.g., `"healthy"`, `"degraded"`).
    pub status: String,
    /// Server version string.
    pub version: String,
    /// Whether the Docker daemon is reachable.
    pub docker_available: bool,
    /// Whether the WASM execution engine is initialized.
    pub wasm_engine_ready: bool,
    /// Number of currently active containers.
    pub active_containers: usize,
    /// Server uptime in seconds.
    pub uptime_seconds: u64,
    /// Detailed per-component health. `None` if not requested.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub components: Option<HealthComponents>,
}

impl HealthResponse {
    /// Create a healthy response with the given version and uptime.
    pub fn healthy(version: impl Into<String>, uptime_seconds: u64) -> Self {
        Self {
            status: "healthy".into(),
            version: version.into(),
            docker_available: true,
            wasm_engine_ready: true,
            active_containers: 0,
            uptime_seconds,
            components: None,
        }
    }
}

/// Per-component health breakdown.
#[non_exhaustive]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthComponents {
    /// WASM engine health.
    pub wasm_engine: ComponentHealth,
    /// Docker daemon health.
    pub docker: ComponentHealth,
    /// Agent manager health.
    pub agents: ComponentHealth,
}

impl HealthComponents {
    /// Create a health components report.
    pub fn new(
        wasm_engine: ComponentHealth,
        docker: ComponentHealth,
        agents: ComponentHealth,
    ) -> Self {
        Self {
            wasm_engine,
            docker,
            agents,
        }
    }
}

/// Health status of an individual component.
#[non_exhaustive]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComponentHealth {
    /// Component status (e.g., `"ok"`, `"error"`, `"unavailable"`).
    pub status: String,
    /// Optional structured detail about the component's state.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<serde_json::Value>,
}

impl ComponentHealth {
    /// Create a healthy component status.
    pub fn ok() -> Self {
        Self {
            status: "ok".into(),
            detail: None,
        }
    }

    /// Create an error component status with a detail message.
    pub fn error(detail: impl Into<String>) -> Self {
        Self {
            status: "error".into(),
            detail: Some(serde_json::Value::String(detail.into())),
        }
    }
}

/// Structured API error response.
#[non_exhaustive]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiError {
    /// Human-readable error message.
    pub error: String,
    /// Machine-readable error code (e.g., `"tool_not_found"`, `"policy_violation"`).
    pub code: String,
    /// Optional structured details about the error.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<serde_json::Value>,
}

impl ApiError {
    /// Create an API error with the given message and code.
    pub fn new(error: impl Into<String>, code: impl Into<String>) -> Self {
        Self {
            error: error.into(),
            code: code.into(),
            details: None,
        }
    }

    /// Attach structured details to this error.
    pub fn with_details(mut self, details: serde_json::Value) -> Self {
        self.details = Some(details);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_execute_response_serde_roundtrip() {
        let resp = ExecuteResponse::ok(
            serde_json::json!({"result": 42}),
            ExecutionMetadata::new(150, 10000),
        )
        .with_request_id("req-123");

        let json = serde_json::to_string(&resp).unwrap();
        let deser: ExecuteResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(deser.status, ExecutionStatus::Ok);
        assert_eq!(deser.request_id.as_deref(), Some("req-123"));
        assert_eq!(deser.metadata.fuel_consumed, 10000);
    }

    #[test]
    fn test_execute_request_builder() {
        let req = ExecuteRequest::new("my_tool", serde_json::json!({"key": "value"}))
            .with_capabilities(Capabilities::default());
        assert_eq!(req.tool, "my_tool");
        assert!(req.capabilities.is_some());
    }

    #[test]
    fn test_execute_response_error() {
        let meta = ExecutionMetadata::new(100, 5000);
        let resp = ExecuteResponse::error("something broke", meta);
        assert_eq!(resp.status, ExecutionStatus::Error);
        assert_eq!(resp.error.as_deref(), Some("something broke"));
        assert!(resp.output.is_none());
    }

    #[test]
    fn test_execution_metadata_default() {
        let meta = ExecutionMetadata::default();
        assert_eq!(meta.execution_time_ms, 0);
        assert_eq!(meta.fuel_consumed, 0);
        assert!(meta.logs.is_empty());
    }

    #[test]
    fn test_sanitization_report_new() {
        let report = SanitizationReport::new(3, vec!["redacted".into()]);
        assert_eq!(report.issues_found, 3);
        assert_eq!(report.actions_taken.len(), 1);
    }

    #[test]
    fn test_sanitization_report_default() {
        let report = SanitizationReport::default();
        assert_eq!(report.issues_found, 0);
        assert!(report.actions_taken.is_empty());
    }

    #[test]
    fn test_container_spawn_request_builder() {
        let req = ContainerSpawnRequest::new("my task", Capabilities::default())
            .with_policy(SandboxPolicy::Container)
            .with_image("alpine:latest")
            .with_env("FOO", "bar");
        assert_eq!(req.task, "my task");
        assert_eq!(req.policy, SandboxPolicy::Container);
        assert_eq!(req.image.as_deref(), Some("alpine:latest"));
        assert_eq!(req.env.get("FOO").unwrap(), "bar");
    }

    #[test]
    fn test_container_info_new() {
        let info = ContainerInfo::new(
            "clawbox-123",
            ContainerStatus::Running,
            SandboxPolicy::Container,
            "test task",
            "/run/clawbox/proxy.sock",
        );
        assert_eq!(info.container_id, "clawbox-123");
        assert_eq!(info.status, ContainerStatus::Running);
        assert_eq!(info.proxy_socket, "/run/clawbox/proxy.sock");
        assert!(info.resource_usage.is_none());
    }

    #[test]
    fn test_resource_usage_new() {
        let usage = ResourceUsage::new(1024, 50.0, 10, 5000);
        assert_eq!(usage.memory_bytes, 1024);
        assert_eq!(usage.network_requests, 10);
    }

    #[test]
    fn test_resource_usage_default() {
        let usage = ResourceUsage::default();
        assert_eq!(usage.memory_bytes, 0);
    }

    #[test]
    fn test_health_response_healthy() {
        let health = HealthResponse::healthy("1.0.0", 3600);
        assert_eq!(health.status, "healthy");
        assert!(health.docker_available);
    }

    #[test]
    fn test_component_health_ok_and_error() {
        let ok = ComponentHealth::ok();
        assert_eq!(ok.status, "ok");
        let err = ComponentHealth::error("bad");
        assert_eq!(err.status, "error");
        assert!(err.detail.is_some());
    }

    #[test]
    fn test_api_error_with_details() {
        let err =
            ApiError::new("not found", "not_found").with_details(serde_json::json!({"id": "abc"}));
        assert_eq!(err.code, "not_found");
        assert!(err.details.is_some());
    }
}
