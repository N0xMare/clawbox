# clawbox

Sandboxed execution for AI agents.

> **⚠️ Alpha** — Early development. APIs may change. Use at your own risk.

## What it does

clawbox sits between your AI agent(s) and the internet. Every HTTP call goes through an allowlisted proxy with credential injection and leak detection. Code runs in WASM sandboxes or hardened Docker containers.

```
Agent Framework ──► clawbox ──► WASM Sandbox ──► Proxy ──► Internet
                          └──► Docker Container ─┘
```

## Quick Start

### Install

**Option 1 — One-liner (Linux & macOS):**

```bash
curl -sSf https://raw.githubusercontent.com/N0xMare/clawbox/main/install.sh | bash
```

**Option 2 — Cargo:**

```bash
cargo install clawbox
```

**Option 3 — Build from source:**

```bash
git clone https://github.com/N0xMare/clawbox.git
cd clawbox
cargo build --release
# Binary at target/release/clawbox
```

Then initialize:

```bash
clawbox init                    # Creates ~/.clawbox/ with config and auth token
export CLAWBOX_AUTH_TOKEN=$(grep auth_token ~/.clawbox/config/clawbox.toml | cut -d\"  -f2)
clawbox serve                   # Starts HTTP API on :9800
```

### Usage

**CLI:**

```bash
clawbox list tools              # Discover available tools
clawbox run echo '{"message": "hello"}'
clawbox health                  # Check server status
```

**HTTP API:**

```bash
# List available tools
curl -H "Authorization: Bearer $CLAWBOX_AUTH_TOKEN" http://localhost:9800/tools

# Execute a tool
curl -X POST http://localhost:9800/execute \
  -H "Authorization: Bearer $CLAWBOX_AUTH_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"tool": "echo", "params": {"message": "hello"}}'
```

## Writing Tools

### Rust

```bash
clawbox new-tool my-tool --lang rust
cd my-tool && clawbox build my-tool
```

### JavaScript / TypeScript

```bash
clawbox new-tool my-tool --lang js
clawbox build my-tool
```

### Adding tools and images

```bash
# Add a pre-built WASM tool
clawbox add tool my-tool /path/to/tool.wasm

# Add a container image template
clawbox add image researcher --image python:3.12-slim \
  --allowlist api.search.brave.com --credential brave_search

# List what's available
clawbox list tools
clawbox list images
```

See [Writing Tools Guide](docs/writing-tools.md) for details.

## Integration

| Framework | How |
|-----------|-----|
| **Claude Code** | `claude mcp add clawbox -- clawbox mcp` |
| **Cursor** | Add MCP server in settings → `clawbox mcp` |
| **Claude Desktop** | Add to MCP config → `clawbox mcp` |
| **OpenClaw** | HTTP API: `GET /tools`, `POST /execute` |
| **Any framework** | [HTTP API docs](docs/api.md) |

## Security Model

- **Network allowlists** — Tools can only reach URLs defined in their manifest
- **Credential injection** — Secrets injected at proxy boundary, never visible to agent code
- **Leak detection** — Output scanned for credential patterns, auto-redacted
- **WASM sandbox** — CPU (fuel), memory, and time limits enforced
- **Docker containers** — network=none, no caps, readonly rootfs, PID limit, non-root
- **Unix socket proxy** — Containers communicate via mounted Unix domain socket, not TCP

See [SECURITY.md](SECURITY.md) for the full security model and vulnerability reporting.

## Architecture

```
┌─────────────────────────────────┐
│         clawbox serve           │
│                                 │
│  WASM Engine  │  Docker Manager │
│  (wasmtime)   │  (bollard)      │
│               │                 │
│       Proxy Pipeline            │
│  (allowlist → creds → leak)     │
│                                 │
│  HTTP API (:9800)               │
│  Unix Socket (optional)         │
└─────────────────────────────────┘
```

## Configuration

See [config/clawbox.toml](config/clawbox.toml) for all options. Key settings:

| Setting | Env Override | Default | Description |
|---------|-------------|---------|-------------|
| `server.port` | `CLAWBOX_PORT` | 9800 | HTTP API port |
| `server.auth_token` | `CLAWBOX_AUTH_TOKEN` | (generated) | Bearer token |
| `sandbox.tool_dir` | `CLAWBOX_TOOL_DIR` | ./tools/wasm | WASM tool directory |
| `server.unix_socket` | — | (disabled) | Unix socket path |

See [docs/api.md](docs/api.md) for the full configuration reference.

## Docs

- [API Reference](docs/api.md)
- [Writing Tools](docs/writing-tools.md)
- [Configuration](config/clawbox.toml)
- [Security Policy](SECURITY.md)
- [Contributing](CONTRIBUTING.md)

## License

[MIT](LICENSE)
