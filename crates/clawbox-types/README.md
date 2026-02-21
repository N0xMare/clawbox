# clawbox-types

Shared type definitions for the clawbox sandboxed agent execution service.

## Overview

`clawbox-types` is the foundational crate in the clawbox ecosystem. It defines the core API types, tool manifests, sandbox policies, agent configuration, and host call interfaces used by every other clawbox crate. By centralizing these definitions, all crates stay in sync without circular dependencies.

## Usage

```rust,ignore
use clawbox_types::{
    ExecuteRequest, ExecuteResponse, ExecutionMetadata,
    SandboxPolicy, Capabilities, NetworkCapabilities, ResourceLimits,
    AgentConfig, ToolManifest, ToolMeta,
};

// Build an execution request
let req = ExecuteRequest::new("web_search", serde_json::json!({"query": "rust wasm"}))
    .with_capabilities(Capabilities::new(
        NetworkCapabilities::new(vec!["api.example.com".into()])
    ));

// Configure an agent with builder pattern
let agent = AgentConfig::new("my-agent", "My Agent")
    .with_policy(SandboxPolicy::Container)
    .with_capabilities(
        Capabilities::new(NetworkCapabilities::new(vec!["api.github.com".into()]))
            .with_credential("github")
            .with_resources(ResourceLimits::new(60_000, 512))
    );
```

## Features

- **API types** — `ExecuteRequest`, `ExecuteResponse`, `ContainerSpawnRequest`, `HealthResponse`, and more
- **Tool manifests** — `ToolManifest` with network, credential, and resource configurations
- **Sandbox policies** — `WasmOnly`, `Container`, and `ContainerDirect` isolation levels
- **Agent types** — `AgentConfig`, `AgentInfo`, `AgentStatus` for lifecycle management
- **Resource limits** — Timeout, memory, CPU, and output size budgets
- **Host call trait** — `HostCallHandler` for WASM-host RPC dispatch
- **Credential patterns** — Shared regex patterns for leak detection
- **Builder patterns** — Ergonomic construction for all major types
- **`#[non_exhaustive]`** — All structs and enums are non-exhaustive for semver safety

## Architecture

| Module     | Contents                                              |
|------------|-------------------------------------------------------|
| `api`      | HTTP request/response types (`ExecuteRequest`, etc.)  |
| `policy`   | `SandboxPolicy`, `Capabilities`, `ResourceLimits`     |
| `manifest` | `ToolManifest`, `ToolMeta`, network/credential config  |
| `agent`    | `AgentConfig`, `AgentInfo`, lifecycle types            |
| `host`     | `HostCallHandler` trait for WASM-host RPC              |
| `patterns` | Shared credential detection regex patterns             |

## License

MIT
