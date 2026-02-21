# clawbox API Reference

All protected endpoints require the `Authorization: Bearer <token>` header.

Base URL: `http://localhost:9800`

---

## GET /health

Public endpoint. No authentication required.

**Response** `200 OK`
```json
{
  "status": "healthy",
  "version": "0.1.0",
  "docker_available": false,
  "active_containers": 0,
  "uptime_seconds": 42,
  "components": {
    "wasm_engine": { "status": "healthy", "detail": { "tools_loaded": 3 } },
    "docker": { "status": "healthy", "detail": { "active_containers": 0 } },
    "agents": { "status": "healthy", "detail": { "active_agents": 0 } }
  }
}
```

| Field | Type | Description |
|---|---|---|
| `status` | string | `"healthy"` or `"degraded"` (degraded when Docker unavailable) |
| `version` | string | Server version from Cargo.toml |
| `docker_available` | bool | Whether Docker daemon is reachable |
| `active_containers` | integer | Number of running containers |
| `uptime_seconds` | integer | Seconds since server start |
| `components` | object | Per-component health (`wasm_engine`, `docker`, `agents`) |

---

## GET /metrics

Public endpoint. Prometheus-format metrics.

**Response** `200 OK` — `text/plain` Prometheus exposition format.

---

## POST /execute

Execute a WASM tool in the sandbox.

**Request**
```json
{
  "tool": "echo",
  "params": { "hello": "world" },
  "capabilities": {
    "network": { "allowlist": ["api.github.com"] },
    "credentials": ["github-token"]
  }
}
```

| Field | Type | Required | Description |
|---|---|---|---|
| `tool` | string | yes | Tool name (matches `.wasm` filename without extension) |
| `params` | object | yes | JSON parameters passed to the tool via stdin |
| `capabilities` | object | no | Requested capabilities (filtered against tool manifest) |

> **Security:** The `capabilities.network.allowlist` from the request is **ignored**. The allowlist is always taken from the registered tool manifest. The request `capabilities.credentials` are filtered to only those listed in the manifest.

**Response** `200 OK`
```json
{
  "status": "ok",
  "request_id": "550e8400-e29b-41d4-a716-446655440000",
  "output": {
    "tool": "echo",
    "echo": { "hello": "world" }
  },
  "error": null,
  "metadata": {
    "execution_time_ms": 12,
    "fuel_consumed": 45000,
    "logs": [],
    "sanitization": {
      "issues_found": 0,
      "actions_taken": []
    }
  }
}
```

| Field | Type | Description |
|---|---|---|
| `status` | string | `"ok"` on success |
| `request_id` | string | UUID for this execution |
| `output` | object \| string | Tool's stdout parsed as JSON (see note below) |
| `error` | string \| null | Error message if execution failed |
| `metadata.execution_time_ms` | integer | Wall-clock execution time |
| `metadata.fuel_consumed` | integer | WASM fuel units consumed |
| `metadata.logs` | array | Log entries emitted by the tool via `host_call` |
| `metadata.sanitization.issues_found` | integer | Number of credential/injection patterns detected |
| `metadata.sanitization.actions_taken` | array | Redaction actions applied to output |

> **Non-JSON stdout:** If your tool writes plain text instead of JSON to stdout, the `output` field will be a JSON string containing the raw text (not `null`). For example, if the tool prints `hello world`, the response will contain `"output": "hello world"`. Stdout is also truncated to ~1MB if it exceeds the maximum size.

### Error Responses

| Code | HTTP Status | Description |
|---|---|---|
| `tool_not_found` | 404 | WASM module not loaded for this tool name |
| `timeout` | 408 | Execution exceeded time limit |
| `fuel_exhausted` | 408 | WASM fuel limit reached |
| `execution_error` | 500 | WASM execution failed or panicked |
| `internal_error` | 500 | Proxy setup or other server error |

---

## GET /tools

List registered tool manifests.

**Response** `200 OK`
```json
[
  {
    "tool": {
      "name": "http_request",
      "description": "Make HTTP requests",
      "version": "0.1.0"
    },
    "network": {
      "allowlist": ["api.github.com"],
      "max_concurrent": 5
    },
    "credentials": {
      "available": ["github-token"]
    },
    "resources": null
  }
]
```

> **Note:** This returns registered manifests, not all loaded WASM files. Tools loaded from the `tool_dir` are executable even without a manifest, but they will have no network access (deny-all) and no credential access.

Manifests may include optional `input_schema` and `output_schema` fields containing JSON Schema objects that describe the tool's expected input parameters and output format respectively.

---

## GET /tools/{name}

Get a single tool's manifest by name.

**Response** `200 OK` — Returns a single `ToolManifest` object (same shape as items in `GET /tools`).

**Response** `404 Not Found`
```json
{
  "error": "tool 'echo' not found",
  "code": "tool_not_found",
  "details": null
}
```

---

## POST /tools/register

Register a tool manifest (defines network allowlist, credential access, etc.).

The tool `name` must match a loaded WASM file. Names must contain only alphanumeric characters, hyphens, and underscores (max 64 chars).

**Request**
```json
{
  "tool": {
    "name": "http_request",
    "description": "Make HTTP requests to allowed endpoints",
    "version": "0.1.0"
  },
  "network": {
    "allowlist": ["api.github.com", "httpbin.org"],
    "max_concurrent": 5
  },
  "credentials": {
    "available": ["github-token"]
  }
}
```

**Response** `201 Created` (new) / `200 OK` (updated)
```json
{
  "status": "registered",
  "tool": "http_request"
}
```

---

## POST /tools/reload

Hot-reload all WASM tool modules from the tool directory.

**Response** `200 OK`
```json
{
  "reloaded": 3,
  "tools": ["echo", "http_request", "summarize"]
}
```

---

## POST /containers/spawn

Spawn a sandboxed Docker container for long-running or sub-agent tasks.

**Request**
```json
{
  "task": "Summarize the latest commits",
  "image": "ghcr.io/n0xmare/clawbox-agent:latest",
  "policy": "container",
  "capabilities": {
    "network": {
      "allowlist": ["api.github.com"]
    },
    "credentials": ["github-token"],
    "resources": {
      "timeout_ms": 120000,
      "memory_mb": 512,
      "cpu_shares": 1024,
      "max_output_bytes": 262144
    }
  },
  "env": { "TASK_MODE": "summary" }
}
```

| Field | Type | Required | Description |
|---|---|---|---|
| `task` | string | yes | Task description for the container |
| `image` | string | no | Docker image (default: `ghcr.io/n0xmare/clawbox-agent:latest`) |
| `policy` | string | no | `wasm_only`, `container`, or `container_direct` (default: `wasm_only`) |
| `capabilities` | object | no | Network allowlist, credentials, and resource limits. Defaults to no network access and no credentials if omitted. |
| `env` | object | no | Environment variables passed to the container |

> **Security:** The `capabilities.network.allowlist` from the request is **ignored**. The server uses the `[container_policy].network_allowlist` from config. Credentials are filtered against `[container_policy].allowed_credentials`.

> **Note:** The `policy` must be in the `[container_policy].allowed_policies` list (default: `["wasm_only", "container"]`). Requesting a disallowed policy returns `403 Forbidden`.

**Response** `201 Created`
```json
{
  "container_id": "clawbox-550e8400-e29b-41d4-a716-446655440000",
  "status": "running",
  "policy": "container",
  "created_at": "2026-02-19T21:00:00Z",
  "task_summary": "Summarize the latest commits",
  "proxy_socket": "/run/clawbox/proxy.sock",
  "resource_usage": null
}
```

| Field | Type | Description |
|---|---|---|
| `container_id` | string | Unique clawbox container identifier |
| `status` | string | Container lifecycle status (`running`, `completed`, `failed`, `timed_out`) |
| `policy` | string | Sandbox policy in effect |
| `created_at` | string | ISO 8601 timestamp |
| `task_summary` | string | Brief description of the container's task |
| `proxy_socket` | string | In-container Unix socket path for the proxy |
| `resource_usage` | object \| null | Resource consumption snapshot (see below), `null` initially |

#### ResourceUsage Object

```json
{
  "memory_bytes": 52428800,
  "cpu_percent": 12.5,
  "network_requests": 7,
  "duration_ms": 45000
}
```

| Field | Type | Description |
|---|---|---|
| `memory_bytes` | integer | Current memory usage in bytes |
| `cpu_percent` | float | CPU utilization (0.0–100.0) |
| `network_requests` | integer | Total outbound network requests made |
| `duration_ms` | integer | Wall-clock time the container has been running |

### Container Networking

Containers run with `network_mode: "none"` — they have **no direct internet access**. All outbound HTTP must go through the clawbox proxy via a **Unix domain socket** mounted into the container.

The host creates a per-container proxy socket at `~/.clawbox/proxies/{container_id}/proxy.sock` and bind-mounts it into the container at `/run/clawbox/proxy.sock`.

The container receives these environment variables automatically:

| Variable | Value | Description |
|---|---|---|
| `CLAWBOX_PROXY_SOCKET` | `/run/clawbox/proxy.sock` | Unix socket path for the proxy |
| `CLAWBOX_PROXY_TOKEN` | `{container_token}` | Bearer token for proxy authentication |
| `CLAWBOX_CONTAINER_ID` | `{clawbox_id}` | This container's clawbox ID |

> **Reserved env vars:** The following variables cannot be overridden via the `env` field — they are either set by clawbox or filtered out:

| Variable | Behavior |
|---|---|
| `CLAWBOX_PROXY_SOCKET` | Set by clawbox (always `/run/clawbox/proxy.sock`) |
| `CLAWBOX_PROXY_TOKEN` | Set by clawbox (unique per container) |
| `CLAWBOX_CONTAINER_ID` | Set by clawbox (container ID) |
| `HTTP_PROXY` | Filtered out (blocked) |
| `HTTPS_PROXY` | Filtered out (blocked) |
| `http_proxy` | Filtered out (blocked) |
| `https_proxy` | Filtered out (blocked) |

To make HTTP requests from inside a container, use the Unix socket directly:

```bash
# Shell — use curl with --unix-socket
curl --unix-socket /run/clawbox/proxy.sock \
  -H "Authorization: Bearer $CLAWBOX_PROXY_TOKEN" \
  http://localhost/https://api.github.com/repos/owner/repo
```

```python
# Python — use httpx with Unix domain socket transport
import httpx
import os

transport = httpx.HTTPTransport(uds="/run/clawbox/proxy.sock")
client = httpx.Client(transport=transport)
response = client.get(
    "http://localhost/https://api.github.com/repos/owner/repo",
    headers={"Authorization": f"Bearer {os.environ['CLAWBOX_PROXY_TOKEN']}"}
)
print(response.json())
```

The proxy enforces the server-side network allowlist and injects credentials automatically. Requests to non-allowlisted hosts are blocked.

### Error Responses

| Code | HTTP Status | Description |
|---|---|---|
| `policy_denied` | 403 | Requested sandbox policy not in `allowed_policies` |
| `resource_exhausted` | 429 | Container limit reached |
| `proxy_error` | 500 | Failed to spawn per-container proxy |
| `container_error` | 500 | Failed to create/start Docker container |

---

## GET /containers

List all active containers.

**Response** `200 OK`
```json
[
  {
    "container_id": "clawbox-550e8400-...",
    "status": "running",
    "policy": "container",
    "created_at": "2026-02-19T21:00:00Z",
    "task_summary": "Summarize the latest commits",
    "proxy_socket": "/run/clawbox/proxy.sock",
    "resource_usage": null
  }
]
```

---

## GET /containers/{id}

Get details for a specific container.

**Response** `200 OK` — Returns a `ContainerInfo` object (same shape as items in `GET /containers`).

**Response** `404 Not Found`
```json
{
  "error": "Container not found",
  "code": "not_found",
  "details": null
}
```

---

## DELETE /containers/{id}

Kill a running container, shut down its proxy, and collect output.

**Response** `200 OK`
```json
{
  "container_id": "clawbox-550e8400-...",
  "status": "killed",
  "output": "..."
}
```

---

## POST /agents

Register a new agent.

**Request**
```json
{
  "agent_id": "my-agent",
  "name": "My Agent",
  "policy": "container",
  "image": "ghcr.io/n0xmare/clawbox-agent:latest",
  "capabilities": {},
  "env": {},
  "lifecycle": {
    "max_idle_ms": 300000,
    "max_lifetime_ms": 3600000
  }
}
```

**Response** `201 Created` — Returns `AgentInfo`.

---

## GET /agents

List all registered agents.

**Response** `200 OK` — Returns `AgentInfo[]`.

---

## GET /agents/{id}

Get info about a specific agent.

**Response** `200 OK` — Returns `AgentInfo`.

**Response** `404 Not Found`

---

## POST /agents/{id}/start

Start an agent's container.

**Response** `200 OK` — Returns `AgentInfo` with status `"Running"`.

---

## POST /agents/{id}/stop

Stop an agent's container.

**Response** `200 OK` — Returns `AgentInfo` with status `"Idle"`.

---

## DELETE /agents/{id}

Remove an agent entirely.

**Response** `204 No Content`

**Response** `404 Not Found`

---

## Execution Policies

clawbox supports three execution policies that control how tools run:

| Policy | Isolation | Network | Use Case |
|---|---|---|---|
| `wasm_only` | WASM sandbox | Via `host_call` FFI → proxy | Most restrictive. Recommended for untrusted tools. |
| `container` | Docker container | Via Unix socket proxy (`network_mode=none`) | Container isolation + proxy security. For tools needing a full OS environment. |
| `container_direct` | Docker container | Direct (no proxy) | Least restrictive. Requires explicit opt-in in server config. Only for trusted tools needing raw socket access. |

**`wasm_only`** — Tool runs in a WASM sandbox with fuel and time limits. Network requests go through the `host_call` FFI, which routes them through the proxy pipeline (allowlist → credential injection → leak detection). The tool never sees real credentials or makes direct network calls.

**`container`** — Tool runs inside a Docker container with `network_mode: "none"`. All HTTP traffic is routed through the clawbox proxy via a Unix domain socket (`CLAWBOX_PROXY_SOCKET=/run/clawbox/proxy.sock`). The proxy enforces the same allowlist and credential injection as WASM mode.

**`container_direct`** — Tool runs in a Docker container with direct network access (no proxy interception). This bypasses allowlist enforcement and credential injection. Requires explicit opt-in by adding `"container_direct"` to `[container_policy].allowed_policies` in server config. Use only for trusted tools that need raw socket access (e.g., SSH, WebSocket, or non-HTTP protocols).

---

## Configuration Reference

### Server (`[server]`)

| Field | Type | Default | Env Override | Description |
|---|---|---|---|---|
| `host` | string | `"127.0.0.1"` | — | Bind address |
| `port` | integer | `9800` | `CLAWBOX_PORT` | HTTP API port |
| `auth_token` | string | (generated) | `CLAWBOX_AUTH_TOKEN` | Bearer token for API auth |
| `unix_socket` | string | (disabled) | — | Unix socket path for same-machine clients |
| `max_concurrent_executions` | integer | `10` | — | Maximum concurrent WASM executions (tower concurrency limit) |

### Sandbox (`[sandbox]`)

| Field | Type | Default | Description |
|---|---|---|---|
| `tool_dir` | string | `"./tools/wasm"` | Directory containing `.wasm` tool files |
| `default_fuel` | integer | `100000000` | Default fuel limit for WASM execution |
| `default_timeout_ms` | integer | `30000` | Default timeout for WASM execution |
| `watch_tools` | bool | `true` | Watch `tool_dir` for changes and hot-reload WASM modules |

### Proxy (`[proxy]`)

| Field | Type | Default | Description |
|---|---|---|---|
| `max_response_bytes` | integer | `1048576` | Maximum response body size from proxied requests |
| `default_timeout_ms` | integer | `30000` | Default timeout for proxied HTTP requests |

### Containers (`[containers]`)

| Field | Type | Default | Description |
|---|---|---|---|
| `max_containers` | integer | `10` | Maximum number of concurrent containers |
| `workspace_root` | string | `"~/.clawbox/workspaces"` | Root directory for per-agent workspace mounts |

### Container Policy (`[container_policy]`)

| Field | Type | Default | Description |
|---|---|---|---|
| `network_allowlist` | list | `[]` | Server-side network allowlist applied to all containers |
| `allowed_credentials` | list | `[]` | Credential names containers are allowed to request |
| `allowed_policies` | list | `["wasm_only", "container"]` | Which sandbox policies are permitted |

### Credentials (`[credentials]`)

| Field | Type | Default | Description |
|---|---|---|---|
| `store_path` | string | `"~/.clawbox/credentials.enc"` | Path to the encrypted credential store |

### Logging (`[logging]`)

| Field | Type | Default | Description |
|---|---|---|---|
| `format` | string | `"json"` | Log output format (`"json"` or `"text"`) |
| `level` | string | `"info"` | Log level (`trace`, `debug`, `info`, `warn`, `error`) |
| `audit_dir` | string | `"./audit"` | Directory for audit log files |

---

## Rate Limits & Timeouts

| Limit | Default | Configurable Via |
|---|---|---|
| WASM fuel limit | 100,000,000 | `[sandbox].default_fuel` |
| WASM execution timeout | 30s | `[sandbox].default_timeout_ms` |
| Container proxy rate limit | 100 req/s per tool | `[proxy].rate_limit` |
| Request body size | 10 MB max | — |
| Max concurrent WASM executions | 10 | `[server].max_concurrent_executions` |
| Max containers | 10 | `[containers].max_containers` |
| Proxy response size | 1 MB | `[proxy].max_response_bytes` |
| Container idle timeout | Configurable per agent | `lifecycle.max_idle_ms` in agent config |
| HTTP redirect following | Disabled | — (SSRF protection) |

---

## Authentication

All endpoints except `GET /health` and `GET /metrics` require:
```
Authorization: Bearer <token>
```

The `clawbox init` command generates a random 64-character hex token and writes it to `clawbox.toml`. There is no default token — each installation gets a unique one.

Requests without a valid token receive `401 Unauthorized`:
```json
{
  "error": "unauthorized",
  "code": "auth_required",
  "details": null
}
```

---

## Error Format

All errors follow the `ApiError` shape:

```json
{
  "error": "human-readable error message",
  "code": "machine_readable_code",
  "details": null
}
```

| Code | HTTP Status | Description |
|---|---|---|
| `auth_required` | 401 | Missing or invalid bearer token |
| `tool_not_found` | 404 | WASM module not loaded for this tool name |
| `not_found` | 404 | Container or agent not found |
| `invalid_request` | 400 | Invalid agent config or state transition |
| `policy_denied` | 403 | Requested sandbox policy not allowed |
| `timeout` | 408 | Execution exceeded time limit |
| `fuel_exhausted` | 408 | WASM fuel limit reached |
| `execution_error` | 500 | WASM execution failed |
| `internal_error` | 500 | Server-side error (proxy setup, panic, etc.) |
| `proxy_error` | 500 | Failed to spawn container proxy |
| `container_error` | 500 | Failed to spawn Docker container |
| `resource_exhausted` | 429/503 | Container limit or proxy port exhaustion |

## Unix Socket

For same-machine clients, clawbox can listen on a Unix domain socket for lower latency:

```toml
[server]
unix_socket = "/run/clawbox.sock"
```

Connect via:
```bash
curl --unix-socket /run/clawbox.sock http://localhost/health
```
