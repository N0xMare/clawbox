#![doc = include_str!("../README.md")]

pub mod auth;
pub mod backend;
pub mod config;
pub mod error;
pub mod lifecycle;
pub mod manager;
pub mod orchestrator;
pub mod reaper;

pub use backend::ContainerBackend;
pub use error::{ContainerError, ContainerResult};
pub use manager::DockerBackend;
pub use orchestrator::AgentOrchestrator;
