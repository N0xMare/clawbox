//! Shared application state.

use crate::config::ClawboxConfig;
#[cfg(feature = "docker")]
use crate::container_proxy::ContainerProxy;
#[cfg(feature = "docker")]
use clawbox_containers::{AgentOrchestrator, ContainerBackend, DockerBackend};
use clawbox_proxy::OutputScanner;
use clawbox_proxy::{AuditLog, CredentialStore, RateLimiter, parse_master_key};
use clawbox_sandbox::{SandboxEngine, ToolWatcherHandle, start_watching};
use clawbox_types::{ToolManifest, ToolMeta};
use metrics_exporter_prometheus::PrometheusHandle;
use thiserror::Error;

/// Errors from application state initialization.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum StateError {
    /// Sandbox engine initialization failed.
    #[error("sandbox error: {0}")]
    Sandbox(#[from] clawbox_sandbox::SandboxError),
    /// Container backend initialization failed.
    #[cfg(feature = "docker")]
    #[error("container error: {0}")]
    Container(#[from] clawbox_containers::ContainerError),
    /// I/O error.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    /// Watcher initialization failed.
    #[error("watcher error: {0}")]
    Watcher(#[from] clawbox_sandbox::WatcherError),
}
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;
use tokio::sync::RwLock;
use tracing::{info, warn};

/// Returns the proxy socket directory for a container.
///
/// Uses `~/.clawbox/proxies/` instead of `/tmp` to avoid predictable paths.
/// Creates the base directory with mode 0o700 if it does not exist.
pub fn proxy_socket_dir(container_id: &str) -> std::path::PathBuf {
    let base = crate::config::expand_tilde("~/.clawbox/proxies");
    if !base.exists() {
        if let Err(e) = std::fs::create_dir_all(&base) {
            tracing::warn!("Failed to create proxy base dir {}: {e}", base.display());
        } else {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(&base, std::fs::Permissions::from_mode(0o700));
            }
        }
    }
    base.join(container_id)
}

/// Returns the proxy socket path for a container.
pub fn proxy_socket_path(container_id: &str) -> std::path::PathBuf {
    proxy_socket_dir(container_id).join("proxy.sock")
}

/// Docker/container-specific state, only compiled when `docker` feature is enabled.
#[cfg(feature = "docker")]
pub struct DockerState {
    pub container_manager: Arc<DockerBackend>,
    pub agent_orchestrator: Arc<AgentOrchestrator>,
    pub container_proxies: RwLock<HashMap<String, ContainerProxy>>,
    reaper_shutdown: tokio::sync::watch::Sender<bool>,
}

/// Shared state for the clawbox HTTP server.
#[non_exhaustive]
pub struct AppState {
    pub sandbox_engine: Arc<SandboxEngine>,
    pub output_scanner: OutputScanner,
    pub config: ClawboxConfig,
    pub tools: RwLock<HashMap<String, ToolManifest>>,
    pub start_time: std::time::Instant,
    pub credential_store: Option<CredentialStore>,
    pub audit_log: AuditLog,
    pub rate_limiter: Arc<RateLimiter>,
    pub metrics_handle: PrometheusHandle,
    /// Handle to the filesystem watcher (if enabled).
    watcher_handle: Mutex<Option<ToolWatcherHandle>>,
    /// Docker/container state (only present with `docker` feature).
    #[cfg(feature = "docker")]
    pub docker: DockerState,
}

impl AppState {
    pub async fn new(config: ClawboxConfig) -> Result<Self, StateError> {
        let metrics_handle = crate::metrics::init_metrics();
        let epoch_interval_ms = clawbox_sandbox::resource_limits::EPOCH_INTERVAL_MS;
        let sandbox_config = clawbox_sandbox::SandboxConfig::new(&config.sandbox.tool_dir)
            .with_fuel_limit(config.sandbox.default_fuel)
            .with_epoch_deadline((config.sandbox.default_timeout_ms / epoch_interval_ms).max(1))
            .with_epoch_interval_ms(epoch_interval_ms)
            .with_max_host_calls(100)
            .with_max_memory_bytes(clawbox_sandbox::resource_limits::DEFAULT_MAX_MEMORY_BYTES)
            .with_max_table_elements(clawbox_sandbox::resource_limits::DEFAULT_MAX_TABLE_ELEMENTS);

        let engine = Arc::new(SandboxEngine::new(sandbox_config)?);

        // Load all .wasm modules from the tool directory
        let count = engine.load_all_modules()?;
        info!(count, tool_dir = %config.sandbox.tool_dir, "loaded WASM tools at startup");

        // D4: Auto-register minimal manifests for loaded WASM tools
        let mut auto_tools = HashMap::new();
        for name in engine.list_modules() {
            let meta =
                ToolMeta::new(&name, "Auto-loaded WASM tool").with_version("0.0.0".to_string());
            auto_tools.insert(name, ToolManifest::new(meta));
        }

        // Start filesystem watcher if enabled
        let watcher_handle = if config.sandbox.watch_tools {
            let tool_dir = std::path::PathBuf::from(&config.sandbox.tool_dir);
            if tool_dir.exists() {
                match start_watching(Arc::clone(&engine), tool_dir) {
                    Ok(handle) => Some(handle),
                    Err(e) => {
                        warn!("Failed to start tool watcher: {e}. Hot-reload disabled.");
                        None
                    }
                }
            } else {
                warn!(dir = %config.sandbox.tool_dir, "tool directory does not exist, skipping watcher");
                None
            }
        } else {
            None
        };

        // Load credential store if CLAWBOX_MASTER_KEY is set
        let credential_store = match std::env::var("CLAWBOX_MASTER_KEY") {
            Ok(hex_key) => match parse_master_key(&hex_key) {
                Ok(key) => {
                    let cred_path = crate::config::expand_tilde(&config.credentials.store_path);
                    match CredentialStore::load(cred_path.to_string_lossy().as_ref(), key) {
                        Ok(store) => {
                            info!(
                                "Credential store loaded from {}",
                                config.credentials.store_path
                            );
                            Some(store)
                        }
                        Err(e) => {
                            warn!(
                                "Failed to load credential store: {e}. Continuing without credentials."
                            );
                            None
                        }
                    }
                }
                Err(e) => {
                    warn!("Invalid CLAWBOX_MASTER_KEY: {e}. Continuing without credentials.");
                    None
                }
            },
            Err(_) => {
                warn!("CLAWBOX_MASTER_KEY not set. Credential injection disabled.");
                None
            }
        };

        // Initialize audit log
        let audit_log = AuditLog::new(format!("{}/proxy.jsonl", config.logging.audit_dir));

        // Fix 5: Create shared rate limiter
        let rate_limiter = Arc::new(RateLimiter::new(50, 10.0)); // 50 burst, 10/sec refill

        // Docker/container initialization
        #[cfg(feature = "docker")]
        let docker = {
            let container_manager = Arc::new(DockerBackend::new().await?);

            let workspace_root = crate::config::expand_tilde(&config.containers.workspace_root)
                .to_string_lossy()
                .into_owned();
            let workspace_root = std::path::PathBuf::from(&workspace_root);
            std::fs::create_dir_all(&workspace_root)?;
            let agent_orchestrator = Arc::new(AgentOrchestrator::new(
                Arc::clone(&container_manager) as Arc<dyn clawbox_containers::ContainerBackend>,
                workspace_root,
            ));

            let (reaper_shutdown_tx, reaper_shutdown_rx) = tokio::sync::watch::channel(false);
            container_manager.spawn_reaper(reaper_shutdown_rx);

            DockerState {
                container_manager,
                agent_orchestrator,
                container_proxies: RwLock::new(HashMap::new()),
                reaper_shutdown: reaper_shutdown_tx,
            }
        };

        // Spawn periodic rate limiter cleanup
        let cleanup_limiter = Arc::clone(&rate_limiter);
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(300));
            loop {
                interval.tick().await;
                cleanup_limiter.cleanup_stale(std::time::Duration::from_secs(600));
            }
        });

        Ok(Self {
            sandbox_engine: engine,
            output_scanner: OutputScanner::new(),
            config,
            tools: RwLock::new(auto_tools),
            start_time: std::time::Instant::now(),
            credential_store,
            audit_log,
            rate_limiter,
            metrics_handle,
            watcher_handle: Mutex::new(watcher_handle),
            #[cfg(feature = "docker")]
            docker,
        })
    }

    /// Returns `(docker_available, active_containers, active_agents)`.
    /// When compiled without the `docker` feature this always returns `(false, 0, 0)`.
    #[cfg(feature = "docker")]
    pub async fn docker_status(&self) -> (bool, usize, usize) {
        let available = self.docker.container_manager.is_available().await;
        let containers = self.docker.container_manager.list().await.len();
        let agents = self.docker.agent_orchestrator.list_agents().await.len();
        (available, containers, agents)
    }

    #[cfg(not(feature = "docker"))]
    pub async fn docker_status(&self) -> (bool, usize, usize) {
        (false, 0, 0)
    }

    /// Gracefully shut down all managed resources.
    pub async fn shutdown(&self) {
        // Shut down the tool watcher
        if let Some(handle) = self
            .watcher_handle
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take()
        {
            handle.shutdown();
        }

        // Shut down Docker resources
        #[cfg(feature = "docker")]
        {
            let _ = self.docker.reaper_shutdown.send(true);

            let mut proxies = self.docker.container_proxies.write().await;
            for (id, proxy) in proxies.iter_mut() {
                tracing::info!(container = %id, "Shutting down container proxy");
                proxy.shutdown();
            }
            proxies.clear();

            let containers = self.docker.container_manager.list().await;
            for info in &containers {
                if info.status == clawbox_types::ContainerStatus::Running {
                    tracing::info!(container = %info.container_id, "Killing container on shutdown");
                    let _ = self.docker.container_manager.kill(&info.container_id).await;
                }
            }
        }

        // Stop the sandbox epoch ticker
        self.sandbox_engine.shutdown();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_proxy_socket_dir() {
        let dir = proxy_socket_dir("clawbox-abc123");
        let home = std::env::var("HOME").unwrap_or_else(|_| "/home/test".into());
        assert_eq!(
            dir,
            std::path::PathBuf::from(format!("{home}/.clawbox/proxies/clawbox-abc123"))
        );
    }

    #[test]
    fn test_proxy_socket_path() {
        let path = proxy_socket_path("clawbox-abc123");
        let home = std::env::var("HOME").unwrap_or_else(|_| "/home/test".into());
        assert_eq!(
            path,
            std::path::PathBuf::from(format!("{home}/.clawbox/proxies/clawbox-abc123/proxy.sock"))
        );
    }
}
