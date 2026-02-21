# clawbox-sandbox

WASM sandboxing engine for clawbox using wasmtime, with fuel metering, epoch-based interruption, memory limits, and host call rate limiting.

## Overview

`clawbox-sandbox` provides the core WASM execution environment for clawbox tools. Each tool is a pre-compiled WASM module that runs in a fully isolated sandbox with strict resource limits. The engine uses wasmtime's fuel metering for instruction budgets, epoch-based interruption for timeouts, linear memory caps, and host call rate limiting to prevent abuse. Tools communicate with the host only through a single `host_call` import.

## Usage

```rust,ignore
use clawbox_sandbox::{SandboxEngine, SandboxConfig, NoOpHandler};
use std::sync::Arc;

// Configure the sandbox
let config = SandboxConfig::new("/path/to/tools");

// Create the engine
let engine = SandboxEngine::new(config)?;

// Execute a tool
let handler = Arc::new(NoOpHandler);
let output = engine.execute(
    "my_tool",
    serde_json::json!({"input": "hello"}),
    handler,
).await?;

println!("Result: {}", output.output);
println!("Fuel used: {}", output.fuel_consumed);
println!("Time: {}ms", output.execution_time_ms);
```

## Features

- **Fuel metering** — Configurable instruction budget per execution (default: ~100M instructions)
- **Epoch interruption** — Time-based interruption via wasmtime epochs (100ms tick interval, 30s default deadline)
- **Memory limits** — Configurable WASM linear memory cap (default: 64 MB) and table element limits
- **Host call rate limiting** — Maximum host calls per execution (default: 100) to prevent abuse
- **WASI support** — WASI preview 1 for filesystem and stdio access within the sandbox
- **Hot reloading** — Filesystem watcher for automatic tool module reloading on changes
- **Extensible host calls** — `HostCallHandler` trait for custom host function implementations

## Architecture

| Module            | Purpose                                           |
|-------------------|---------------------------------------------------|
| `engine`          | Core WASM execution engine (load, compile, run)    |
| `host_functions`  | Host call dispatch, logging, and rate limiting     |
| `resource_limits` | `SandboxConfig` and default constants              |
| `watcher`         | Filesystem watcher for hot-reloading tool modules  |

## Safety / Security

- **Full isolation** — Each execution runs in its own wasmtime `Store` with independent limits
- **No ambient capabilities** — Tools can only interact with the host through the `host_call` import
- **Defense in depth** — Three independent kill mechanisms: fuel exhaustion, epoch deadline, and host call limit
- **Memory bounded** — Linear memory growth is capped by `ResourceLimiter` to prevent OOM
- **No filesystem escape** — WASI access is scoped; tools cannot access the host filesystem directly

## License

MIT
