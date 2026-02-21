//! Backend trait for container operations.

use async_trait::async_trait;

use std::path::Path;

use crate::error::ContainerResult;
use clawbox_types::{ContainerInfo, ContainerSpawnRequest};

/// Trait abstracting container runtime operations.
///
/// The default implementation is [`DockerBackend`](crate::DockerBackend)
/// which uses Docker via the bollard crate.
#[async_trait]
pub trait ContainerBackend: Send + Sync + 'static {
    /// Spawn a new sandboxed container.
    async fn spawn(
        &self,
        req: ContainerSpawnRequest,
        proxy_socket_path: &Path,
        pre_generated: Option<(String, String)>,
    ) -> ContainerResult<ContainerInfo>;

    /// Stop and remove a container by its clawbox ID.
    async fn kill(&self, id: &str) -> ContainerResult<()>;

    /// Collect stdout/stderr output from a container.
    async fn collect_output(&self, id: &str) -> ContainerResult<String>;

    /// Remove containers that have exited. Returns count cleaned up.
    async fn cleanup_stopped(&self) -> ContainerResult<usize>;

    /// Pre-generate a container ID and its proxy token.
    fn pre_generate_id(&self) -> (String, String);
}
