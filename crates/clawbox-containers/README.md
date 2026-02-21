# clawbox-containers

Docker container lifecycle management for clawbox agent sandboxing.

## Overview

`clawbox-containers` manages the full lifecycle of Docker containers used as agent sandboxes. It handles container creation with security hardening, workspace isolation, per-container authentication, agent-level orchestration, and automatic cleanup of orphaned containers. This crate is used by `clawbox-server` to provide the `Container` and `ContainerDirect` sandbox policies.

## Usage

```rust,ignore
use clawbox_containers::{DockerBackend, ContainerBackend, AgentOrchestrator};
use clawbox_types::ContainerSpawnRequest;
use std::sync::Arc;
use std::path::PathBuf;

// Create a Docker backend (requires Docker daemon)
let backend = DockerBackend::new().await?;

// Spawn a sandboxed container via the ContainerBackend trait
use clawbox_types::Capabilities;
let request = ContainerSpawnRequest::new("Run code analysis", Capabilities::default())
    .with_image("alpine:latest");
let container_info = backend.spawn(request, 18080, None).await?;

// Or use the AgentOrchestrator for agent-level lifecycle management
let orchestrator = AgentOrchestrator::new(
    Arc::new(backend) as Arc<dyn ContainerBackend>,
    PathBuf::from("/tmp/workspaces"),
);
```

## Features

- **Container lifecycle** — Create, start, monitor, stop, and remove containers with timeout enforcement
- **Security hardening** — Read-only root filesystem, dropped capabilities, no-new-privileges flag, seccomp profiles
- **Image allowlisting** — Only pre-approved image prefixes can be used (`ghcr.io/n0xmare/`, `alpine:`, `ubuntu:`, `debian:`)
- **Workspace isolation** — Per-agent host directories mounted into containers with configurable read-only mode
- **Per-container auth** — Each container receives a unique bearer token for proxy authentication
- **Agent orchestration** — Register, start, stop, and track agents with idle timeouts and crash recovery
- **Orphan reaper** — Background task scans for containers with clawbox labels not tracked by the manager, stops and removes them
- **Graceful shutdown** — Containers are stopped cleanly on server shutdown

## Architecture

| Module         | Purpose                                                   |
|----------------|-----------------------------------------------------------|
| `manager`      | Core container lifecycle (spawn, stop, remove, list) via `DockerBackend` |
| `orchestrator` | Agent-level state management over containers               |
| `lifecycle`    | Background monitoring for timeouts and status transitions  |
| `reaper`       | Periodic cleanup of orphaned Docker containers             |
| `config`       | Security settings, defaults, and image allowlists          |
| `auth`         | Per-container bearer token generation and validation       |
| `backend`      | `ContainerBackend` trait for abstracting container runtimes |

## Safety / Security

- **Requires Docker daemon** — The Docker socket must be accessible
- **Least privilege** — Containers run with dropped capabilities, read-only rootfs, and `no-new-privileges`
- **Image allowlisting** — Prevents spawning arbitrary images; only approved prefixes are accepted
- **Isolation** — Each container gets its own network namespace, workspace mount, and auth token
- **Reaper** — Prevents resource leaks by cleaning up containers that outlive their manager

## License

MIT
