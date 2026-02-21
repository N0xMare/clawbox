//! Container lifecycle management — timeout monitoring and status updates.

use std::collections::HashMap;
use std::sync::Arc;

use bollard::Docker;
use bollard::query_parameters::{
    RemoveContainerOptions, StopContainerOptions, WaitContainerOptions,
};
use futures_util::StreamExt;
use tokio::sync::RwLock;
use tracing::{info, warn};

use clawbox_types::ContainerStatus;

use crate::auth::ContainerTokenStore;
use crate::manager::ManagedContainer;

/// Monitor a container for completion or timeout.
///
/// Spawned as a background task by `DockerBackend::spawn`.
/// Updates the container's status in the shared state when it finishes.
pub(crate) async fn monitor_container(
    docker: Docker,
    docker_container_id: String,
    clawbox_id: String,
    timeout_ms: u64,
    containers: Arc<RwLock<HashMap<String, ManagedContainer>>>,
    token_store: Arc<ContainerTokenStore>,
) {
    let wait_fut = async {
        let mut stream = docker.wait_container(
            &docker_container_id,
            Some(WaitContainerOptions {
                condition: "not-running".to_string(),
            }),
        );
        // Get the first (and typically only) result from the wait stream
        if let Some(result) = stream.next().await {
            match result {
                Ok(response) => response.status_code,
                Err(e) => {
                    warn!(container = %clawbox_id, error = %e, "Error waiting for container");
                    -1
                }
            }
        } else {
            -1
        }
    };

    let status = tokio::select! {
        exit_code = wait_fut => {
            if exit_code == 0 {
                info!(container = %clawbox_id, "Container completed successfully");
                ContainerStatus::Completed
            } else {
                warn!(container = %clawbox_id, exit_code, "Container failed");
                ContainerStatus::Failed
            }
        }
        _ = tokio::time::sleep(std::time::Duration::from_millis(timeout_ms)) => {
            warn!(container = %clawbox_id, timeout_ms, "Container timed out, killing");
            // Kill the container
            let _ = docker.stop_container(
                &docker_container_id,
                Some(StopContainerOptions { t: Some(5), ..Default::default() }),
            ).await;
            let _ = docker.remove_container(
                &docker_container_id,
                Some(RemoveContainerOptions {
                    force: true,
                    ..Default::default()
                }),
            ).await;
            ContainerStatus::TimedOut
        }
    };

    // Update status in shared state
    let mut lock = containers.write().await;
    if let Some(managed) = lock.get_mut(&clawbox_id) {
        managed.info.status = status.clone();
    }

    // Clean up token for timed-out or completed containers
    if matches!(
        status,
        ContainerStatus::TimedOut | ContainerStatus::Completed | ContainerStatus::Failed
    ) {
        token_store.remove(&clawbox_id);
        info!(container = %clawbox_id, "Cleaned up auth token for finished container");
    }
}
