# Security Policy

> **⚠️ Alpha Software** — clawbox is in early development. Use at your own risk.
> There are no guarantees of stability, backward compatibility, or security completeness.

## Reporting Issues

Report bugs and security vulnerabilities as [GitHub Issues](https://github.com/N0xMare/clawbox/issues).

Given the alpha status, there is no private disclosure process — all issues
are tracked publicly. If you believe a vulnerability is critical, note that
in the issue title.

## Security Model

clawbox uses a three-layer defense-in-depth model:

### Layer 1: WASM Sandbox
- Fuel metering (instruction budget)
- Epoch-based timeout interruption
- Linear memory caps
- Host call rate limiting
- No ambient capabilities — tools interact only via `host_call` FFI

### Layer 2: Network Proxy
- Domain allowlist enforcement (server-side only)
- Credential injection at the proxy boundary (tools never see raw secrets)
- Leak detection with iterative decoding (URL, base64, double-encoded)
- Output redaction of detected credentials
- Private IP blocking (SSRF protection)
- DNS rebinding protection with IP pinning
- HTTP redirect blocking
- Per-tool rate limiting

### Layer 3: Docker Container Isolation
- `network_mode: none` — no direct network access
- All capabilities dropped
- Read-only root filesystem
- `no-new-privileges` flag
- Non-root user (1000:1000)
- PID limit (256)
- Memory limit (512MB default)
- Per-container bearer token authentication via Unix domain socket proxy

## Known Limitations

- **Docker socket access:** The clawbox server requires access to the Docker socket, which is effectively root-equivalent on the host. Run clawbox on a dedicated machine or use rootless Docker.
- **WASM host_call interface:** The `host_call` FFI uses a fixed-size response buffer. Extremely large responses may be truncated.
- **No mTLS:** Communication between containers and the proxy uses bearer tokens over Unix sockets, not mTLS.
- **Single-tenant:** clawbox is designed for single-tenant use. There is no multi-user isolation or RBAC.
