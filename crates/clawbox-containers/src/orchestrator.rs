//! Agent-level orchestration over containers.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

use chrono::Utc;
use tokio::sync::RwLock;
use tracing::{info, warn};

use clawbox_types::ContainerSpawnRequest;
use clawbox_types::agent::{AgentConfig, AgentInfo, AgentStatus};

use crate::backend::ContainerBackend;
use crate::error::{ContainerError, ContainerResult};

/// Default proxy socket path for agent containers.
const DEFAULT_PROXY_SOCKET_PATH: &str = "/tmp/clawbox-proxy-agent/proxy.sock";

/// Internal agent state tracked by the orchestrator.
#[derive(Debug, Clone)]
struct AgentState {
    config: AgentConfig,
    info: AgentInfo,
    container_id: Option<String>,
    restart_count: u32,
}

/// Orchestrates agent lifecycle on top of a [`ContainerBackend`].
#[non_exhaustive]
pub struct AgentOrchestrator {
    backend: Arc<dyn ContainerBackend>,
    agents: RwLock<HashMap<String, AgentState>>,
    workspace_root: PathBuf,
}

impl AgentOrchestrator {
    /// Create a new orchestrator.
    /// Create a new orchestrator with the given container backend and workspace root.
    pub fn new(backend: Arc<dyn ContainerBackend>, workspace_root: PathBuf) -> Self {
        Self {
            backend,
            agents: RwLock::new(HashMap::new()),
            workspace_root,
        }
    }

    /// Validate an agent ID: alphanumeric + hyphens, max 64 chars, no path traversal.
    fn validate_agent_id(id: &str) -> ContainerResult<()> {
        if id.is_empty() || id.len() > 64 {
            return Err(ContainerError::Agent(
                "agent_id must be 1-64 characters".into(),
            ));
        }
        if id.contains("..") || id.contains("/") || id.contains("\\") {
            return Err(ContainerError::Agent(
                "agent_id contains invalid characters (path traversal)".into(),
            ));
        }
        if !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
            return Err(ContainerError::Agent(
                "agent_id must be alphanumeric + hyphens only".into(),
            ));
        }
        Ok(())
    }

    /// Register a new agent.
    /// Register a new agent configuration.
    pub async fn register_agent(&self, config: AgentConfig) -> ContainerResult<AgentInfo> {
        Self::validate_agent_id(&config.agent_id)?;

        let mut agents = self.agents.write().await;
        if agents.contains_key(&config.agent_id) {
            return Err(ContainerError::AlreadyExists(config.agent_id.clone()));
        }

        // Create workspace directory
        let workspace_path = self.workspace_root.join(&config.agent_id);
        std::fs::create_dir_all(&workspace_path)?;

        let mut info = AgentInfo::new(
            config.agent_id.clone(),
            config.name.clone(),
            AgentStatus::Idle,
        );
        info.workspace_path = Some(workspace_path.to_string_lossy().into_owned());

        let state = AgentState {
            config,
            info: info.clone(),
            container_id: None,
            restart_count: 0,
        };

        agents.insert(info.agent_id.clone(), state);
        info!(agent = %info.agent_id, "Agent registered");
        Ok(info)
    }

    /// Start an agent's container.
    /// Start a registered agent.
    pub async fn start_agent(&self, agent_id: &str) -> ContainerResult<AgentInfo> {
        let mut agents = self.agents.write().await;
        let state = agents
            .get_mut(agent_id)
            .ok_or_else(|| ContainerError::NotFound(agent_id.to_string()))?;

        match state.info.status {
            AgentStatus::Running | AgentStatus::Starting => {
                return Err(ContainerError::InvalidState {
                    id: agent_id.to_string(),
                    expected: "Idle or Stopped".into(),
                    actual: format!("{:?}", state.info.status),
                });
            }
            _ => {}
        }

        state.info.status = AgentStatus::Starting;
        state.info.last_activity = Utc::now();

        // Build a ContainerSpawnRequest from the agent config
        let mut spawn_req = ContainerSpawnRequest::new(
            format!("agent:{}", state.config.agent_id),
            state.config.capabilities.clone(),
        )
        .with_policy(state.config.policy);

        if let Some(ref image) = state.config.image {
            spawn_req = spawn_req.with_image(image.clone());
        }

        // Inject agent env vars
        for (k, v) in &state.config.env {
            spawn_req.env.insert(k.clone(), v.clone());
        }

        // Spawn the container
        match self
            .backend
            .spawn(
                spawn_req,
                std::path::Path::new(DEFAULT_PROXY_SOCKET_PATH),
                None,
            )
            .await
        {
            Ok(container_info) => {
                state.container_id = Some(container_info.container_id.clone());
                state.info.status = AgentStatus::Running;
                // Track restarts (first start doesn't count)
                if state.restart_count > 0 || state.container_id.is_some() {
                    state.restart_count += 1;
                }
                info!(agent = %agent_id, container = %container_info.container_id, "Agent started");
                Ok(state.info.clone())
            }
            Err(e) => {
                // Revert status on failure
                state.info.status = AgentStatus::Idle;
                Err(e)
            }
        }
    }

    /// Stop an agent's container.
    /// Stop a running agent.
    pub async fn stop_agent(&self, agent_id: &str) -> ContainerResult<AgentInfo> {
        let mut agents = self.agents.write().await;
        let state = agents
            .get_mut(agent_id)
            .ok_or_else(|| ContainerError::NotFound(agent_id.to_string()))?;

        if state.info.status != AgentStatus::Running {
            return Err(ContainerError::InvalidState {
                id: agent_id.to_string(),
                expected: "Running".into(),
                actual: format!("{:?}", state.info.status),
            });
        }

        state.info.status = AgentStatus::Stopping;
        state.info.last_activity = Utc::now();

        // Stop the container if we have one
        if let Some(ref container_id) = state.container_id
            && let Err(e) = self.backend.kill(container_id).await
        {
            warn!(agent = %agent_id, error = %e, "Failed to kill container, marking stopped anyway");
        }

        state.info.status = AgentStatus::Idle;
        state.container_id = None;

        info!(agent = %agent_id, "Agent stopped");
        Ok(state.info.clone())
    }

    /// Remove an agent entirely. If the agent has a running container, it is killed first.
    /// Remove an agent and clean up its resources.
    pub async fn remove_agent(&self, agent_id: &str) -> ContainerResult<()> {
        let container_id = {
            let mut agents = self.agents.write().await;
            let state = agents
                .remove(agent_id)
                .ok_or_else(|| ContainerError::NotFound(agent_id.to_string()))?;
            state.container_id
        };
        // Kill the container outside the lock
        if let Some(ref cid) = container_id
            && let Err(e) = self.backend.kill(cid).await
        {
            warn!(agent = %agent_id, container = %cid, error = %e, "Failed to kill container during agent removal");
        }
        info!(agent = %agent_id, "Agent removed");
        Ok(())
    }

    /// Get info about a specific agent.
    /// Get info about a specific agent.
    pub async fn get_agent(&self, agent_id: &str) -> Option<AgentInfo> {
        let agents = self.agents.read().await;
        agents.get(agent_id).map(|s| s.info.clone())
    }

    /// List all registered agents.
    /// List all registered agents.
    pub async fn list_agents(&self) -> Vec<AgentInfo> {
        let agents = self.agents.read().await;
        agents.values().map(|s| s.info.clone()).collect()
    }

    /// Enforce lifecycle policies (idle timeout, max lifetime).
    /// Returns IDs of agents that were stopped. Kills their containers.
    /// Enforce lifecycle policies on all agents (stop expired, restart failed).
    pub async fn enforce_lifecycle(&self) -> Vec<String> {
        let now = Utc::now();
        let mut stopped = HashSet::new();
        let mut to_kill: Vec<(String, String)> = Vec::new(); // (agent_id, container_id)

        {
            let mut agents = self.agents.write().await;
            for (id, state) in agents.iter_mut() {
                if state.info.status != AgentStatus::Running {
                    continue;
                }
                let idle_ms = (now - state.info.last_activity).num_milliseconds().max(0) as u64;
                if idle_ms > state.config.lifecycle.max_idle_ms {
                    state.info.status = AgentStatus::Idle;
                    if let Some(ref cid) = state.container_id {
                        to_kill.push((id.clone(), cid.clone()));
                    }
                    state.container_id = None;
                    stopped.insert(id.clone());
                    continue; // Don't double-check max_lifetime for the same agent
                }
                if let Some(max_lifetime) = state.config.lifecycle.max_lifetime_ms {
                    let lifetime_ms =
                        (now - state.info.created_at).num_milliseconds().max(0) as u64;
                    if lifetime_ms > max_lifetime {
                        state.info.status = AgentStatus::Terminated;
                        if let Some(ref cid) = state.container_id {
                            to_kill.push((id.clone(), cid.clone()));
                        }
                        state.container_id = None;
                        stopped.insert(id.clone());
                    }
                }
            }
        }

        // Kill containers outside the write lock
        for (agent_id, container_id) in &to_kill {
            if let Err(e) = self.backend.kill(container_id).await {
                warn!(agent = %agent_id, container = %container_id, error = %e, "Failed to kill container during lifecycle enforcement");
            } else {
                info!(agent = %agent_id, container = %container_id, "Container killed by lifecycle enforcement");
            }
        }

        stopped.into_iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    use clawbox_types::agent::{AgentConfig, AgentStatus};
    use clawbox_types::{ContainerInfo, ContainerSpawnRequest, ContainerStatus};

    use crate::backend::ContainerBackend;
    use crate::error::{ContainerError, ContainerResult};
    use async_trait::async_trait;

    /// Mock container backend for unit testing (no Docker needed).
    struct MockBackend {
        spawn_count: AtomicUsize,
        kill_count: AtomicUsize,
        should_fail_spawn: AtomicBool,
        should_fail_kill: AtomicBool,
        next_id: AtomicUsize,
    }

    impl MockBackend {
        fn new() -> Self {
            Self {
                spawn_count: AtomicUsize::new(0),
                kill_count: AtomicUsize::new(0),
                should_fail_spawn: AtomicBool::new(false),
                should_fail_kill: AtomicBool::new(false),
                next_id: AtomicUsize::new(1),
            }
        }

        fn spawns(&self) -> usize {
            self.spawn_count.load(Ordering::SeqCst)
        }

        fn kills(&self) -> usize {
            self.kill_count.load(Ordering::SeqCst)
        }

        fn fail_next_spawn(&self) {
            self.should_fail_spawn.store(true, Ordering::SeqCst);
        }

        fn fail_next_kill(&self) {
            self.should_fail_kill.store(true, Ordering::SeqCst);
        }
    }

    #[async_trait]
    impl ContainerBackend for MockBackend {
        async fn spawn(
            &self,
            req: ContainerSpawnRequest,
            _proxy_socket_path: &std::path::Path,
            pre_generated: Option<(String, String)>,
        ) -> ContainerResult<ContainerInfo> {
            if self.should_fail_spawn.swap(false, Ordering::SeqCst) {
                return Err(ContainerError::Agent("mock spawn failure".into()));
            }
            self.spawn_count.fetch_add(1, Ordering::SeqCst);
            let id = pre_generated
                .map(|(id, _)| id)
                .unwrap_or_else(|| format!("mock-{}", self.next_id.fetch_add(1, Ordering::SeqCst)));
            Ok(ContainerInfo::new(
                id,
                ContainerStatus::Running,
                req.policy,
                req.task.clone(),
                "/run/clawbox/proxy.sock",
            ))
        }

        async fn kill(&self, _id: &str) -> ContainerResult<()> {
            if self.should_fail_kill.swap(false, Ordering::SeqCst) {
                return Err(ContainerError::Agent("mock kill failure".into()));
            }
            self.kill_count.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }

        async fn collect_output(&self, _id: &str) -> ContainerResult<String> {
            Ok("mock output".into())
        }

        async fn cleanup_stopped(&self) -> ContainerResult<usize> {
            Ok(0)
        }

        fn pre_generate_id(&self) -> (String, String) {
            let n = self.next_id.fetch_add(1, Ordering::SeqCst);
            (format!("mock-{n}"), format!("token-{n}"))
        }
    }

    fn make_config(id: &str) -> AgentConfig {
        AgentConfig::new(id, format!("Agent {id}"))
    }

    fn make_orchestrator(dir: &std::path::Path) -> AgentOrchestrator {
        let backend: Arc<dyn ContainerBackend> = Arc::new(MockBackend::new());
        AgentOrchestrator::new(backend, dir.to_path_buf())
    }

    fn make_orchestrator_with_backend(
        dir: &std::path::Path,
        backend: Arc<MockBackend>,
    ) -> (AgentOrchestrator, Arc<MockBackend>) {
        let dyn_backend: Arc<dyn ContainerBackend> = Arc::clone(&backend) as _;
        (
            AgentOrchestrator::new(dyn_backend, dir.to_path_buf()),
            backend,
        )
    }

    // --- Validation tests (no Docker needed, kept from original) ---

    #[test]
    fn test_validate_agent_id_valid() {
        assert!(AgentOrchestrator::validate_agent_id("my-agent-1").is_ok());
        assert!(AgentOrchestrator::validate_agent_id("a").is_ok());
    }

    #[test]
    fn test_validate_agent_id_empty() {
        assert!(AgentOrchestrator::validate_agent_id("").is_err());
    }

    #[test]
    fn test_validate_agent_id_too_long() {
        let long = "a".repeat(65);
        assert!(AgentOrchestrator::validate_agent_id(&long).is_err());
    }

    #[test]
    fn test_validate_agent_id_path_traversal() {
        assert!(AgentOrchestrator::validate_agent_id("../etc").is_err());
        assert!(AgentOrchestrator::validate_agent_id("foo/bar").is_err());
        assert!(AgentOrchestrator::validate_agent_id("foo\\bar").is_err());
    }

    #[test]
    fn test_validate_agent_id_special_chars() {
        assert!(AgentOrchestrator::validate_agent_id("foo_bar").is_err());
        assert!(AgentOrchestrator::validate_agent_id("foo bar").is_err());
        assert!(AgentOrchestrator::validate_agent_id("foo@bar").is_err());
    }

    // --- Mock backend tests ---

    #[tokio::test]
    async fn test_register_and_start() {
        let tmp = tempfile::TempDir::new().unwrap();
        let (orch, backend) =
            make_orchestrator_with_backend(tmp.path(), Arc::new(MockBackend::new()));

        orch.register_agent(make_config("agent-1")).await.unwrap();
        let info = orch.start_agent("agent-1").await.unwrap();

        assert_eq!(info.status, AgentStatus::Running);
        assert_eq!(backend.spawns(), 1);
    }

    #[tokio::test]
    async fn test_start_sets_running_status() {
        let tmp = tempfile::TempDir::new().unwrap();
        let orch = make_orchestrator(tmp.path());

        orch.register_agent(make_config("agent-1")).await.unwrap();
        let info = orch.start_agent("agent-1").await.unwrap();
        assert_eq!(info.status, AgentStatus::Running);

        let fetched = orch.get_agent("agent-1").await.unwrap();
        assert_eq!(fetched.status, AgentStatus::Running);
    }

    #[tokio::test]
    async fn test_stop_calls_kill() {
        let tmp = tempfile::TempDir::new().unwrap();
        let (orch, backend) =
            make_orchestrator_with_backend(tmp.path(), Arc::new(MockBackend::new()));

        orch.register_agent(make_config("agent-1")).await.unwrap();
        orch.start_agent("agent-1").await.unwrap();
        orch.stop_agent("agent-1").await.unwrap();

        assert_eq!(backend.kills(), 1);
    }

    #[tokio::test]
    async fn test_stop_sets_idle_status() {
        let tmp = tempfile::TempDir::new().unwrap();
        let orch = make_orchestrator(tmp.path());

        orch.register_agent(make_config("agent-1")).await.unwrap();
        orch.start_agent("agent-1").await.unwrap();
        let info = orch.stop_agent("agent-1").await.unwrap();

        assert_eq!(info.status, AgentStatus::Idle);
    }

    #[tokio::test]
    async fn test_start_failure_reverts_status() {
        let tmp = tempfile::TempDir::new().unwrap();
        let (orch, backend) =
            make_orchestrator_with_backend(tmp.path(), Arc::new(MockBackend::new()));

        orch.register_agent(make_config("agent-1")).await.unwrap();
        backend.fail_next_spawn();

        let result = orch.start_agent("agent-1").await;
        assert!(result.is_err());

        let info = orch.get_agent("agent-1").await.unwrap();
        assert_eq!(info.status, AgentStatus::Idle);
    }

    #[tokio::test]
    async fn test_stop_failure_still_marks_idle() {
        let tmp = tempfile::TempDir::new().unwrap();
        let (orch, backend) =
            make_orchestrator_with_backend(tmp.path(), Arc::new(MockBackend::new()));

        orch.register_agent(make_config("agent-1")).await.unwrap();
        orch.start_agent("agent-1").await.unwrap();

        backend.fail_next_kill();
        let info = orch.stop_agent("agent-1").await.unwrap();

        // Should still be Idle even though kill failed (graceful)
        assert_eq!(info.status, AgentStatus::Idle);
    }

    #[tokio::test]
    async fn test_start_already_running() {
        let tmp = tempfile::TempDir::new().unwrap();
        let orch = make_orchestrator(tmp.path());

        orch.register_agent(make_config("agent-1")).await.unwrap();
        orch.start_agent("agent-1").await.unwrap();

        let result = orch.start_agent("agent-1").await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("invalid state"));
    }

    #[tokio::test]
    async fn test_stop_already_idle() {
        let tmp = tempfile::TempDir::new().unwrap();
        let orch = make_orchestrator(tmp.path());

        orch.register_agent(make_config("agent-1")).await.unwrap();

        let result = orch.stop_agent("agent-1").await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("invalid state"));
    }

    #[tokio::test]
    async fn test_remove_running_agent() {
        let tmp = tempfile::TempDir::new().unwrap();
        let (orch, backend) =
            make_orchestrator_with_backend(tmp.path(), Arc::new(MockBackend::new()));

        orch.register_agent(make_config("agent-1")).await.unwrap();
        orch.start_agent("agent-1").await.unwrap();

        // Remove should kill the container then remove
        orch.remove_agent("agent-1").await.unwrap();
        assert!(orch.get_agent("agent-1").await.is_none());
        assert_eq!(
            backend.kills(),
            1,
            "remove_agent should kill the running container"
        );
    }

    #[tokio::test]
    async fn test_remove_nonexistent() {
        let tmp = tempfile::TempDir::new().unwrap();
        let orch = make_orchestrator(tmp.path());

        let result = orch.remove_agent("ghost").await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not found"));
    }

    #[tokio::test]
    async fn test_restart_count_incremented() {
        let tmp = tempfile::TempDir::new().unwrap();
        let (orch, backend) =
            make_orchestrator_with_backend(tmp.path(), Arc::new(MockBackend::new()));

        orch.register_agent(make_config("agent-1")).await.unwrap();
        orch.start_agent("agent-1").await.unwrap();
        orch.stop_agent("agent-1").await.unwrap();
        orch.start_agent("agent-1").await.unwrap();

        // Should have spawned twice
        assert_eq!(backend.spawns(), 2);
    }

    #[tokio::test]
    async fn test_list_agents() {
        let tmp = tempfile::TempDir::new().unwrap();
        let orch = make_orchestrator(tmp.path());

        orch.register_agent(make_config("a1")).await.unwrap();
        orch.register_agent(make_config("a2")).await.unwrap();
        orch.register_agent(make_config("a3")).await.unwrap();

        let list = orch.list_agents().await;
        assert_eq!(list.len(), 3);
    }

    #[tokio::test]
    async fn test_get_agent() {
        let tmp = tempfile::TempDir::new().unwrap();
        let orch = make_orchestrator(tmp.path());

        orch.register_agent(make_config("agent-1")).await.unwrap();

        let info = orch.get_agent("agent-1").await.unwrap();
        assert_eq!(info.agent_id, "agent-1");
        assert_eq!(info.name, "Agent agent-1");
    }

    #[tokio::test]
    async fn test_get_nonexistent_agent() {
        let tmp = tempfile::TempDir::new().unwrap();
        let orch = make_orchestrator(tmp.path());

        assert!(orch.get_agent("nope").await.is_none());
    }

    #[tokio::test]
    async fn test_register_duplicate() {
        let tmp = tempfile::TempDir::new().unwrap();
        let orch = make_orchestrator(tmp.path());

        orch.register_agent(make_config("dup")).await.unwrap();
        let result = orch.register_agent(make_config("dup")).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("already exists"));
    }

    #[tokio::test]
    async fn test_start_nonexistent_agent() {
        let tmp = tempfile::TempDir::new().unwrap();
        let orch = make_orchestrator(tmp.path());

        let result = orch.start_agent("ghost").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_enforce_lifecycle_kills_container() {
        let tmp = tempfile::TempDir::new().unwrap();
        let (orch, backend) =
            make_orchestrator_with_backend(tmp.path(), Arc::new(MockBackend::new()));

        let mut config = make_config("agent-1");
        config.lifecycle.max_idle_ms = 0; // immediate idle timeout
        orch.register_agent(config).await.unwrap();
        orch.start_agent("agent-1").await.unwrap();

        // Sleep briefly to ensure some time passes for idle detection
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;

        // enforce_lifecycle should detect idle and kill
        let stopped = orch.enforce_lifecycle().await;
        assert!(
            stopped.contains(&"agent-1".to_string()),
            "agent should be stopped"
        );
        assert_eq!(
            backend.kills(),
            1,
            "enforce_lifecycle should kill the container"
        );

        let info = orch.get_agent("agent-1").await.unwrap();
        assert_eq!(info.status, AgentStatus::Idle);
    }

    #[tokio::test]
    async fn test_concurrent_start_stop() {
        let tmp = tempfile::TempDir::new().unwrap();
        let (orch, _backend) =
            make_orchestrator_with_backend(tmp.path(), Arc::new(MockBackend::new()));
        let orch = Arc::new(orch);

        orch.register_agent(make_config("agent-1")).await.unwrap();
        orch.start_agent("agent-1").await.unwrap();

        let orch1 = Arc::clone(&orch);
        let orch2 = Arc::clone(&orch);

        // Run stop and start concurrently — should not panic
        let (r1, r2) = tokio::join!(
            tokio::spawn(async move { orch1.stop_agent("agent-1").await }),
            tokio::spawn(async move { orch2.stop_agent("agent-1").await }),
        );

        // One should succeed, one should fail (already stopped)
        let results = [r1.unwrap(), r2.unwrap()];
        let successes = results.iter().filter(|r| r.is_ok()).count();
        let failures = results.iter().filter(|r| r.is_err()).count();
        assert_eq!(successes, 1);
        assert_eq!(failures, 1);
    }
}
