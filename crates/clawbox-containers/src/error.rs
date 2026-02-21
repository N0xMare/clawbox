//! Error types for container management operations.

use thiserror::Error;

/// Errors that can occur during container operations.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum ContainerError {
    /// Docker daemon communication failure.
    #[error("docker error: {0}")]
    Docker(#[from] bollard::errors::Error),

    /// The requested container was not found.
    #[error("container not found: {0}")]
    NotFound(String),

    /// A container with this ID already exists.
    #[error("container already exists: {0}")]
    AlreadyExists(String),

    /// The requested image is not in the allowlist.
    #[error("image not allowed: {image} (allowed: {allowed:?})")]
    ImageNotAllowed { image: String, allowed: Vec<String> },

    /// The container is in an invalid state for the requested operation.
    #[error("invalid state for container {id}: expected {expected}, got {actual}")]
    InvalidState {
        id: String,
        expected: String,
        actual: String,
    },

    /// Agent orchestration error.
    #[error("agent error: {0}")]
    Agent(String),

    /// I/O error during workspace or volume operations.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// Serialization/deserialization error.
    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
}

/// Result type alias for container operations.
pub type ContainerResult<T> = Result<T, ContainerError>;
