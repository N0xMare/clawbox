//! Container manager — coordinates Docker container lifecycle.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use bollard::Docker;
use bollard::models::ContainerCreateBody as Config;
use bollard::models::HostConfig;
use bollard::query_parameters::{
    CreateContainerOptions, LogsOptions, RemoveContainerOptions, StartContainerOptions,
    StopContainerOptions,
};
use futures_util::StreamExt;
use tokio::sync::RwLock;
use tracing::{info, warn};
use uuid::Uuid;

use clawbox_types::{ContainerInfo, ContainerSpawnRequest, ContainerStatus};

use crate::auth::ContainerTokenStore;
use crate::backend::ContainerBackend;
use crate::config::{ContainerSecurityConfig, DEFAULT_AGENT_IMAGE};
use crate::error::{ContainerError, ContainerResult};
use crate::lifecycle;

/// A container tracked by the manager.
#[allow(dead_code)]
pub(crate) struct ManagedContainer {
    pub(crate) info: ContainerInfo,
    /// Docker's internal container ID (hex hash).
    pub(crate) docker_id: String,
    /// Host-side directory containing the proxy Unix socket.
    pub(crate) proxy_socket_dir: PathBuf,
    /// Unique bearer token for proxy authentication.
    pub(crate) proxy_token: String,
}

/// Manages sandboxed Docker containers for sub-agent execution.
#[non_exhaustive]
pub struct DockerBackend {
    docker: Docker,
    /// Active containers tracked by clawbox container ID.
    containers: Arc<RwLock<HashMap<String, ManagedContainer>>>,
    /// Security configuration applied to all containers.
    security: ContainerSecurityConfig,
    /// Per-container authentication token store.
    token_store: Arc<ContainerTokenStore>,
}

impl DockerBackend {
    /// Create a new container manager, connecting to the Docker daemon.
    /// Succeeds even if Docker is not available (WASM-only mode).
    pub async fn new() -> ContainerResult<Self> {
        let docker = Docker::connect_with_local_defaults().map_err(ContainerError::Docker)?;
        match docker.ping().await {
            Ok(_) => info!("Container manager initialized, Docker daemon reachable"),
            Err(e) => warn!("Docker daemon not responding ({e}), container features disabled"),
        }
        Ok(Self {
            docker,
            containers: Arc::new(RwLock::new(HashMap::new())),
            security: ContainerSecurityConfig::default(),
            token_store: Arc::new(ContainerTokenStore::new()),
        })
    }

    /// Get a reference to the token store for proxy validation.
    pub fn token_store(&self) -> &Arc<ContainerTokenStore> {
        &self.token_store
    }

    /// Spawn the background reaper task that cleans up orphaned containers.
    pub fn spawn_reaper(
        &self,
        shutdown: tokio::sync::watch::Receiver<bool>,
    ) -> tokio::task::JoinHandle<()> {
        crate::reaper::spawn_reaper(self.docker.clone(), self.containers.clone(), shutdown)
    }

    /// Quick Docker availability check.
    pub async fn is_available(&self) -> bool {
        self.docker.ping().await.is_ok()
    }

    /// Check if an image is in the allowed list.
    pub(crate) fn is_allowed_image(&self, image: &str) -> bool {
        // Strip digest (@sha256:...) before tag to avoid mis-splitting on digest colon
        let name = if let Some((n, _digest)) = image.split_once('@') {
            n
        } else if let Some((n, _tag)) = image.rsplit_once(':') {
            n
        } else {
            image
        };
        self.security.allowed_image_prefixes.iter().any(|allowed| {
            let allowed_name = allowed.strip_suffix(":").unwrap_or(allowed);
            if allowed.contains("/") && !allowed.ends_with(":") {
                // Registry prefix: require path boundary after prefix
                if allowed.ends_with('/') {
                    // Prefix already includes trailing slash — starts_with is sufficient
                    name.starts_with(allowed.as_str())
                } else if name.starts_with(allowed.as_str()) {
                    let rest = &name[allowed.len()..];
                    rest.is_empty() || rest.starts_with('/')
                } else {
                    false
                }
            } else {
                name == allowed_name
            }
        })
    }

    /// Look up a container by its clawbox ID.
    pub async fn get(&self, id: &str) -> Option<ContainerInfo> {
        let lock = self.containers.read().await;
        lock.get(id).map(|m| m.info.clone())
    }

    /// List all tracked containers.
    pub async fn list(&self) -> Vec<ContainerInfo> {
        let lock = self.containers.read().await;
        lock.values().map(|m| m.info.clone()).collect()
    }
}

#[async_trait]
impl ContainerBackend for DockerBackend {
    fn pre_generate_id(&self) -> (String, String) {
        let clawbox_id = format!("clawbox-{}", Uuid::new_v4());
        let proxy_token = self.token_store.generate(&clawbox_id);
        (clawbox_id, proxy_token)
    }

    async fn spawn(
        &self,
        req: ContainerSpawnRequest,
        proxy_socket_path: &Path,
        pre_generated: Option<(String, String)>,
    ) -> ContainerResult<ContainerInfo> {
        let (clawbox_id, proxy_token) = match pre_generated {
            Some((id, token)) => (id, token),
            None => {
                let id = format!("clawbox-{}", Uuid::new_v4());
                let token = self.token_store.generate(&id);
                (id, token)
            }
        };

        let image = req
            .image
            .as_deref()
            .unwrap_or(DEFAULT_AGENT_IMAGE)
            .to_string();

        if !self.is_allowed_image(&image) {
            return Err(ContainerError::ImageNotAllowed {
                image,
                allowed: self.security.allowed_image_prefixes.clone(),
            });
        }

        const RESERVED_ENV_KEYS: &[&str] = &[
            "HTTP_PROXY",
            "HTTPS_PROXY",
            "http_proxy",
            "https_proxy",
            "CLAWBOX_PROXY_SOCKET",
            "CLAWBOX_PROXY_TOKEN",
            "CLAWBOX_CONTAINER_ID",
            "LD_PRELOAD",
            "LD_LIBRARY_PATH",
            "BASH_ENV",
            "ENV",
            "PYTHONSTARTUP",
        ];

        let mut env: Vec<String> = req
            .env
            .iter()
            .filter(|(k, _)| !RESERVED_ENV_KEYS.contains(&k.as_str()))
            .map(|(k, v)| format!("{k}={v}"))
            .collect();

        env.push("CLAWBOX_PROXY_SOCKET=/run/clawbox/proxy.sock".to_string());
        env.push(format!("CLAWBOX_PROXY_TOKEN={proxy_token}"));
        env.push(format!("CLAWBOX_CONTAINER_ID={clawbox_id}"));

        let tmpfs: HashMap<String, String> = self
            .security
            .tmpfs_mounts
            .iter()
            .filter_map(|m| {
                let (path, opts) = m.split_once(":")?;
                Some((path.to_string(), opts.to_string()))
            })
            .collect();

        let cap_drop = if self.security.drop_all_caps {
            Some(vec!["ALL".to_string()])
        } else {
            None
        };

        let memory_bytes = (req.capabilities.resources.memory_mb as i64) * 1024 * 1024;
        let cpu_shares = req.capabilities.resources.cpu_shares as i64;

        let mut labels = HashMap::new();
        labels.insert("clawbox.container_id".into(), clawbox_id.clone());
        labels.insert("clawbox.task".into(), req.task.clone());
        labels.insert("clawbox.policy".into(), req.policy.to_string());

        let host_config = HostConfig {
            memory: Some(memory_bytes),
            cpu_shares: Some(cpu_shares),
            readonly_rootfs: Some(self.security.readonly_rootfs),
            cap_drop,
            security_opt: if self.security.no_new_privileges {
                Some(vec!["no-new-privileges:true".into()])
            } else {
                None
            },
            tmpfs: Some(tmpfs),
            // Prevent fork bombs — 256 is a reasonable default for agent workloads
            pids_limit: Some(256),
            // Containers have NO network access. The only way out is through the
            // bind-mounted Unix socket proxy, which enforces allowlists and
            // credential injection.
            network_mode: Some("none".into()),
            binds: Some(vec![format!(
                "{}:/run/clawbox:ro",
                proxy_socket_path
                    .parent()
                    .unwrap_or(Path::new("/tmp"))
                    .display()
            )]),
            ..Default::default()
        };

        let container_config = Config {
            image: Some(image.clone()),
            user: Some(self.security.user.clone()),
            cmd: req.command.clone(),
            env: Some(env),
            labels: Some(labels),
            host_config: Some(host_config),
            ..Default::default()
        };

        let create_opts = CreateContainerOptions {
            name: Some(clawbox_id.clone()),
            platform: String::new(),
        };

        let create_response = self
            .docker
            .create_container(Some(create_opts), container_config)
            .await?;

        let docker_id = create_response.id;

        self.docker
            .start_container(&docker_id, None::<StartContainerOptions>)
            .await?;

        let info = ContainerInfo::new(
            clawbox_id.clone(),
            ContainerStatus::Running,
            req.policy,
            req.task.clone(),
            "/run/clawbox/proxy.sock",
        );

        {
            let mut lock = self.containers.write().await;
            lock.insert(
                clawbox_id.clone(),
                ManagedContainer {
                    info: info.clone(),
                    docker_id: docker_id.clone(),
                    proxy_socket_dir: proxy_socket_path
                        .parent()
                        .unwrap_or(Path::new("/tmp"))
                        .to_path_buf(),
                    proxy_token,
                },
            );
        }

        let timeout_ms = req.capabilities.resources.timeout_ms;
        tokio::spawn(lifecycle::monitor_container(
            self.docker.clone(),
            docker_id,
            clawbox_id,
            timeout_ms,
            Arc::clone(&self.containers),
            Arc::clone(&self.token_store),
        ));

        info!(
            container = %info.container_id,
            image = %image,
            "Container spawned"
        );

        Ok(info)
    }

    async fn kill(&self, id: &str) -> ContainerResult<()> {
        let docker_id = {
            let lock = self.containers.read().await;
            lock.get(id)
                .map(|m| m.docker_id.clone())
                .ok_or_else(|| ContainerError::NotFound(id.to_string()))?
        };

        let _ = self
            .docker
            .stop_container(
                &docker_id,
                Some(StopContainerOptions {
                    t: Some(5),
                    ..Default::default()
                }),
            )
            .await;

        self.docker
            .remove_container(
                &docker_id,
                Some(RemoveContainerOptions {
                    force: true,
                    ..Default::default()
                }),
            )
            .await?;

        {
            let mut lock = self.containers.write().await;
            lock.remove(id);
        }
        self.token_store.remove(id);

        info!(container = %id, "Container killed and removed");
        Ok(())
    }

    async fn collect_output(&self, id: &str) -> ContainerResult<String> {
        let docker_id = {
            let lock = self.containers.read().await;
            lock.get(id)
                .map(|m| m.docker_id.clone())
                .ok_or_else(|| ContainerError::NotFound(id.to_string()))?
        };

        let opts = LogsOptions {
            stdout: true,
            stderr: true,
            follow: false,
            ..Default::default()
        };

        let mut output = String::new();
        let mut stream = self.docker.logs(&docker_id, Some(opts));

        while let Some(chunk) = stream.next().await {
            match chunk {
                Ok(log_output) => {
                    output.push_str(&log_output.to_string());
                }
                Err(e) => {
                    warn!(container = %id, error = %e, "Error reading container logs");
                    break;
                }
            }
        }

        Ok(output)
    }

    async fn cleanup_stopped(&self) -> ContainerResult<usize> {
        let to_remove: Vec<(String, String)> = {
            let lock = self.containers.read().await;
            lock.iter()
                .filter(|(_, m)| {
                    matches!(
                        m.info.status,
                        ContainerStatus::Completed
                            | ContainerStatus::Failed
                            | ContainerStatus::TimedOut
                            | ContainerStatus::Killed
                    )
                })
                .map(|(id, m)| (id.clone(), m.docker_id.clone()))
                .collect()
        };

        let count = to_remove.len();

        for (clawbox_id, docker_id) in &to_remove {
            let _ = self
                .docker
                .remove_container(
                    docker_id,
                    Some(RemoveContainerOptions {
                        force: true,
                        ..Default::default()
                    }),
                )
                .await;

            info!(container = %clawbox_id, "Cleaned up stopped container");
        }

        {
            let mut lock = self.containers.write().await;
            for (id, _) in &to_remove {
                lock.remove(id);
            }
        }

        Ok(count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ContainerSecurityConfig;
    use clawbox_types::{Capabilities, ContainerSpawnRequest, ContainerStatus, SandboxPolicy};

    #[test]
    fn test_security_config_defaults() {
        let config = ContainerSecurityConfig::default();
        assert_eq!(config.user, "1000:1000");
        assert!(config.readonly_rootfs);
        assert!(config.drop_all_caps);
        assert!(config.no_new_privileges);
        assert!(!config.tmpfs_mounts.is_empty());
        assert!(config.tmpfs_mounts[0].starts_with("/tmp:"));
    }

    #[test]
    fn test_managed_container_tracks_status() {
        let info = ContainerInfo::new(
            "clawbox-test-123",
            ContainerStatus::Creating,
            SandboxPolicy::Container,
            "test task",
            "/run/clawbox/proxy.sock",
        );

        let mut managed = ManagedContainer {
            info,
            docker_id: "abc123".into(),
            proxy_socket_dir: PathBuf::from("/tmp/clawbox-proxy-test"),
            proxy_token: "test-token".into(),
        };

        assert_eq!(managed.info.status, ContainerStatus::Creating);

        managed.info.status = ContainerStatus::Running;
        assert_eq!(managed.info.status, ContainerStatus::Running);

        managed.info.status = ContainerStatus::Completed;
        assert_eq!(managed.info.status, ContainerStatus::Completed);
    }

    #[tokio::test]
    async fn test_container_manager_fields() {
        let containers: Arc<RwLock<HashMap<String, ManagedContainer>>> =
            Arc::new(RwLock::new(HashMap::new()));

        let info = ContainerInfo::new(
            "clawbox-test",
            ContainerStatus::Running,
            SandboxPolicy::Container,
            "test",
            "/run/clawbox/proxy.sock",
        );

        {
            let mut lock = containers.write().await;
            lock.insert(
                "clawbox-test".into(),
                ManagedContainer {
                    info: info.clone(),
                    docker_id: "docker-abc".into(),
                    proxy_socket_dir: PathBuf::from("/tmp/clawbox-proxy-test"),
                    proxy_token: "test-token".into(),
                },
            );
        }

        let lock = containers.read().await;
        assert_eq!(lock.len(), 1);
        assert!(lock.contains_key("clawbox-test"));
        assert_eq!(lock["clawbox-test"].info.status, ContainerStatus::Running);
    }

    #[tokio::test]
    #[ignore] // Requires Docker daemon
    async fn test_spawn_and_kill_real_container() {
        let manager = DockerBackend::new().await.expect("Docker not available");
        assert!(manager.is_available().await);

        let req = ContainerSpawnRequest::new("integration-test", Capabilities::default())
            .with_image("alpine:latest");

        let socket_path = std::path::Path::new("/tmp/clawbox-proxy-test/proxy.sock");
        let info = manager
            .spawn(req, socket_path, None)
            .await
            .expect("Failed to spawn");
        assert_eq!(info.status, ContainerStatus::Running);

        let listed = manager.list().await;
        assert_eq!(listed.len(), 1);

        manager
            .kill(&info.container_id)
            .await
            .expect("Failed to kill");

        // After kill(), the container is removed from the map entirely (H3 fix)
        assert!(manager.get(&info.container_id).await.is_none());
        assert!(manager.list().await.is_empty());
    }

    #[test]
    fn test_allowed_image_prefixes_include_registry() {
        let config = ContainerSecurityConfig::default();
        assert!(
            config
                .allowed_image_prefixes
                .iter()
                .any(|p| p == "ghcr.io/n0xmare/")
        );
    }

    #[test]
    fn test_default_allowed_images_complete() {
        let config = ContainerSecurityConfig::default();
        let prefixes = &config.allowed_image_prefixes;
        assert!(prefixes.contains(&"alpine:".to_string()));
        assert!(prefixes.contains(&"ubuntu:".to_string()));
        assert!(prefixes.contains(&"debian:".to_string()));
        assert!(!prefixes.contains(&"evil.io/".to_string()));
    }

    /// Shared helper for image allowlist tests — mirrors `DockerBackend::is_allowed_image`.
    fn check_image_allowed(security: &ContainerSecurityConfig, image: &str) -> bool {
        let name = if let Some((n, _)) = image.split_once('@') {
            n
        } else if let Some((n, _)) = image.rsplit_once(':') {
            n
        } else {
            image
        };
        security.allowed_image_prefixes.iter().any(|allowed| {
            let allowed_name = allowed.strip_suffix(":").unwrap_or(allowed);
            if allowed.contains("/") && !allowed.ends_with(":") {
                if allowed.ends_with('/') {
                    name.starts_with(allowed.as_str())
                } else if name.starts_with(allowed.as_str()) {
                    let rest = &name[allowed.len()..];
                    rest.is_empty() || rest.starts_with('/')
                } else {
                    false
                }
            } else {
                name == allowed_name
            }
        })
    }

    #[test]
    fn test_image_allowlist_prefix_boundary() {
        let mut security = ContainerSecurityConfig::default();
        security.allowed_image_prefixes =
            vec!["ghcr.io/n0xmare/".to_string(), "alpine:".to_string()];

        // Should match
        assert!(
            check_image_allowed(&security, "ghcr.io/n0xmare/tool:latest"),
            "exact prefix + tool should match"
        );
        assert!(
            check_image_allowed(&security, "ghcr.io/n0xmare/tool@sha256:abc123"),
            "prefix + digest should match"
        );
        assert!(
            check_image_allowed(&security, "alpine:3.18"),
            "exact name with tag should match"
        );

        // Should NOT match
        assert!(
            !check_image_allowed(&security, "ghcr.io/n0xmare-evil/malicious:latest"),
            "similar prefix should NOT match"
        );
        assert!(
            !check_image_allowed(&security, "ghcr.io/n0xmarex/tool:latest"),
            "extended prefix should NOT match"
        );
    }

    #[test]
    fn test_image_digest_handling() {
        // Verify @sha256: images don't get mis-split on the digest colon
        let image = "myimage@sha256:abcdef1234567890";
        let name = if let Some((n, _)) = image.split_once('@') {
            n
        } else if let Some((n, _)) = image.rsplit_once(':') {
            n
        } else {
            image
        };
        assert_eq!(name, "myimage");
    }

    #[test]
    fn test_reserved_env_vars_filtered() {
        use std::collections::HashMap;

        const RESERVED_ENV_KEYS: &[&str] = &[
            "HTTP_PROXY",
            "HTTPS_PROXY",
            "http_proxy",
            "https_proxy",
            "CLAWBOX_PROXY_SOCKET",
            "CLAWBOX_PROXY_TOKEN",
            "CLAWBOX_CONTAINER_ID",
            "LD_PRELOAD",
            "LD_LIBRARY_PATH",
            "BASH_ENV",
            "ENV",
            "PYTHONSTARTUP",
        ];

        let mut env: HashMap<String, String> = HashMap::new();
        env.insert("HTTP_PROXY".into(), "evil".into());
        env.insert("HTTPS_PROXY".into(), "evil".into());
        env.insert("CLAWBOX_PROXY_TOKEN".into(), "evil".into());
        env.insert("CLAWBOX_CONTAINER_ID".into(), "evil".into());
        env.insert("MY_VAR".into(), "safe".into());
        env.insert("ANOTHER_VAR".into(), "safe".into());

        let filtered: Vec<String> = env
            .iter()
            .filter(|(k, _)| !RESERVED_ENV_KEYS.contains(&k.as_str()))
            .map(|(k, v)| format!("{k}={v}"))
            .collect();

        assert_eq!(filtered.len(), 2);
        assert!(filtered.iter().all(|e| !e.starts_with("HTTP_PROXY=")
            && !e.starts_with("HTTPS_PROXY=")
            && !e.starts_with("CLAWBOX_PROXY_TOKEN=")
            && !e.starts_with("CLAWBOX_CONTAINER_ID=")));
    }

    #[test]
    fn test_image_allowlist_boundary() {
        let security = ContainerSecurityConfig::default();
        assert!(!check_image_allowed(
            &security,
            "ghcr.io/n0xmare-evil/image:latest"
        ));
    }

    #[test]
    fn test_image_allowlist_exact_match() {
        let security = ContainerSecurityConfig::default();
        assert!(check_image_allowed(
            &security,
            "ghcr.io/n0xmare/tool:latest"
        ));
    }

    #[test]
    fn test_image_allowlist_empty_blocks_all() {
        let mut security = ContainerSecurityConfig::default();
        security.allowed_image_prefixes = vec![];
        assert!(!check_image_allowed(&security, "anything:latest"));
        assert!(!check_image_allowed(&security, "ghcr.io/n0xmare/tool:v1"));
    }
}
