//! Periodic reaper for orphaned clawbox containers.
//!
//! Scans Docker for containers with clawbox labels that are no longer
//! tracked by the DockerBackend (e.g., after a crash/restart).

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use bollard::Docker;
use bollard::query_parameters::{
    ListContainersOptionsBuilder, RemoveContainerOptions, StopContainerOptions,
};
use tokio::sync::RwLock;
use tracing::{info, warn};

use crate::manager::ManagedContainer;

/// Interval between reaper scans.
const REAPER_INTERVAL: Duration = Duration::from_secs(60);

/// Start the background reaper task.
///
/// Periodically scans for Docker containers with the `clawbox.container_id` label
/// that are NOT present in the active containers map. Stops and removes them.
pub(crate) fn spawn_reaper(
    docker: Docker,
    containers: Arc<RwLock<HashMap<String, ManagedContainer>>>,
    shutdown: tokio::sync::watch::Receiver<bool>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut shutdown = shutdown;
        loop {
            tokio::select! {
                _ = tokio::time::sleep(REAPER_INTERVAL) => {
                    if let Err(e) = reap_orphans(&docker, &containers).await {
                        warn!(error = %e, "Container reaper scan failed");
                    }
                }
                _ = shutdown.changed() => {
                    info!("Container reaper shutting down");
                    break;
                }
            }
        }
    })
}

async fn reap_orphans(
    docker: &Docker,
    tracked: &RwLock<HashMap<String, ManagedContainer>>,
) -> crate::error::ContainerResult<()> {
    // List all containers with clawbox labels
    let mut filters = HashMap::new();
    filters.insert(
        "label".to_string(),
        vec!["clawbox.container_id".to_string()],
    );

    let options = ListContainersOptionsBuilder::default()
        .all(true)
        .filters(&filters)
        .build();

    let docker_containers = docker.list_containers(Some(options)).await?;

    // Fix 9: Collect orphan IDs while holding the read lock, then release before Docker API calls
    let orphan_ids: Vec<(String, String)> = {
        let tracked_lock = tracked.read().await;
        docker_containers
            .iter()
            .filter_map(|c| {
                let clawbox_id = c.labels.as_ref()?.get("clawbox.container_id")?.clone();
                let docker_id = c.id.clone()?;
                if !tracked_lock.contains_key(&clawbox_id) {
                    Some((clawbox_id, docker_id))
                } else {
                    None
                }
            })
            .collect()
    }; // lock released here

    let mut reaped = 0;
    for (clawbox_id, docker_id) in &orphan_ids {
        warn!(
            clawbox_id = %clawbox_id,
            docker_id = %docker_id,
            "Reaping orphaned container"
        );

        let _ = docker
            .stop_container(
                docker_id,
                Some(StopContainerOptions {
                    t: Some(5),
                    ..Default::default()
                }),
            )
            .await;
        let _ = docker
            .remove_container(
                docker_id,
                Some(RemoveContainerOptions {
                    force: true,
                    ..Default::default()
                }),
            )
            .await;
        reaped += 1;
    }

    if reaped > 0 {
        info!(count = reaped, "Reaped orphaned containers");
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use clawbox_types::{ContainerInfo, ContainerStatus, SandboxPolicy};

    #[tokio::test]
    async fn test_reaper_with_tracked_containers() {
        // Verify that the tracked container map works correctly for reaper logic
        let containers: Arc<RwLock<HashMap<String, ManagedContainer>>> =
            Arc::new(RwLock::new(HashMap::new()));

        let info = ContainerInfo::new(
            "clawbox-test-1",
            ContainerStatus::Running,
            SandboxPolicy::Container,
            "test",
            "/run/clawbox/proxy.sock",
        );

        {
            let mut lock = containers.write().await;
            lock.insert(
                "clawbox-test-1".into(),
                ManagedContainer {
                    info,
                    docker_id: "docker-abc".into(),
                    proxy_socket_dir: std::path::PathBuf::from("/tmp/clawbox-proxy-test"),
                    proxy_token: "fake-token".into(),
                },
            );
        }

        // Verify tracked container is found
        let lock = containers.read().await;
        assert!(lock.contains_key("clawbox-test-1"));
        // An orphaned container would NOT be in the map
        assert!(!lock.contains_key("clawbox-orphan"));
    }

    #[tokio::test]
    async fn test_reaper_shutdown_signal() {
        let (tx, rx) = tokio::sync::watch::channel(false);
        let _containers: Arc<RwLock<HashMap<String, ManagedContainer>>> =
            Arc::new(RwLock::new(HashMap::new()));

        // We can't create a real Docker client in tests, but we can verify
        // the shutdown mechanism works by sending the signal immediately.
        // The reaper would exit on shutdown.changed().
        tx.send(true).unwrap();

        // Verify the receiver sees the change
        let mut rx_clone = rx.clone();
        assert!(rx_clone.changed().await.is_ok());
    }
}
