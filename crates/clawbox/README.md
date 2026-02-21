# clawbox

CLI binary for the clawbox sandboxed agent execution service.

## Overview

`clawbox` is the command-line interface for managing the clawbox service. It provides commands for initializing the environment, starting the server, executing tools, managing credentials, checking health, and reading audit logs.

## Quick Start

```bash
# Initialize clawbox (creates config, generates auth token)
clawbox init

# Set the master key for credential encryption
export CLAWBOX_MASTER_KEY=$(openssl rand -hex 32)

# Store a credential
clawbox creds add --name github --domain api.github.com --header Authorization --prefix "token "

# Start the server
clawbox serve --config ~/.clawbox/config/clawbox.toml

# Execute a tool
clawbox run --token <auth-token> my_tool '{"param": "value"}'

# Check health
clawbox health
```

## Subcommands

| Command  | Description                                              |
|----------|----------------------------------------------------------|
| `init`   | Initialize clawbox — creates config, generates auth token, sets up directories |
| `serve`  | Start the HTTP server (use `--insecure` for dev with default token) |
| `health` | Query the server health endpoint                          |
| `run`    | Execute a tool on a running server (params as JSON or stdin with `-`) |
| `tools`  | Manage tools: `list`, `register <manifest>`, `reload`     |
| `creds`  | Manage credentials: `add`, `remove`, `list`               |
| `status` | Show detailed server status (version, uptime, WASM engine, Docker) |
| `logs`   | Read audit logs with optional `--follow` and `--tail N`   |

## Features

- **Zero-config start** — `clawbox init` generates a secure config with random auth token
- **Credential management** — Add/remove/list encrypted credentials (values read from stdin for security)
- **Structured log viewer** — Parses JSON audit log lines into human-readable format
- **Log following** — `clawbox logs --follow` for real-time audit monitoring
- **Auth token resolution** — Pass `--token`, set `CLAWBOX_AUTH_TOKEN`, or let it fail clearly
- **Graceful shutdown** — SIGTERM and Ctrl+C trigger clean container teardown

## License

MIT
