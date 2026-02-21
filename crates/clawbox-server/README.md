# clawbox-server

HTTP API server for the clawbox sandboxed agent execution service.

## Overview

`clawbox-server` is the Axum-based HTTP server that ties all clawbox components together. It exposes REST endpoints for tool execution, container management, agent orchestration, health checking, and Prometheus metrics. The server manages shared application state including the WASM sandbox engine, container manager, credential store, output scanner, and audit log.

## Usage

```rust,ignore
use clawbox_server::{ClawboxConfig, AppState, build_router};
use std::sync::Arc;

let config = ClawboxConfig::load("config/clawbox.toml")?;
let state = Arc::new(AppState::new(config).await?);
let app = build_router(state);

let listener = tokio::net::TcpListener::bind("127.0.0.1:9800").await?;
axum::serve(listener, app).await?;
```

## API Routes

| Method | Path                    | Auth     | Description                        |
|--------|-------------------------|----------|------------------------------------|
| GET    | `/health`               | Public   | Health check with component status |
| GET    | `/metrics`              | Public   | Prometheus metrics endpoint        |
| POST   | `/execute`              | Bearer   | Execute a tool in WASM sandbox     |
| GET    | `/tools`                | Bearer   | List registered tool manifests     |
| POST   | `/tools/register`       | Bearer   | Register a tool manifest           |
| POST   | `/tools/reload`         | Bearer   | Hot-reload WASM tools from disk    |
| GET    | `/containers`           | Bearer   | List active containers             |
| POST   | `/containers/spawn`     | Bearer   | Spawn a new container              |
| GET    | `/containers/{id}`      | Bearer   | Get container details              |
| DELETE | `/containers/{id}`      | Bearer   | Kill and remove a container        |
| POST   | `/agents`               | Bearer   | Register a new agent               |
| GET    | `/agents`               | Bearer   | List registered agents             |
| GET    | `/agents/{id}`          | Bearer   | Get agent details                  |
| POST   | `/agents/{id}/start`    | Bearer   | Start an agent's container         |
| POST   | `/agents/{id}/stop`     | Bearer   | Stop an agent's container          |
| DELETE | `/agents/{id}`          | Bearer   | Remove an agent                    |

## Features

- **Axum HTTP framework** — Async, tower-based middleware stack
- **Bearer token auth** — Constant-time token comparison via `subtle::ConstantTimeEq`
- **Concurrency limiting** — Tower `ConcurrencyLimitLayer` on protected routes (max 10)
- **Request size limiting** — 10 MB body limit on protected endpoints
- **Prometheus metrics** — Request counts, durations, container gauges, execution histograms
- **Graceful shutdown** — SIGTERM/Ctrl+C handling with container cleanup
- **TOML configuration** — Layered config for server, sandbox, proxy, credentials, containers, and logging

## Architecture

| Module          | Purpose                                          |
|-----------------|--------------------------------------------------|
| `routes/`       | Route handlers (execute, containers, tools, agents, health, metrics) |
| `auth`          | Bearer token middleware with constant-time comparison |
| `config`        | TOML config loading and defaults                  |
| `state`         | `AppState` — shared state (engine, manager, store, scanner) |
| `metrics`       | Prometheus recorder initialization and helpers    |
| `proxy_handler` | Per-container proxy request forwarding            |
| `container_proxy` | Container-level proxy configuration             |

## License

MIT
