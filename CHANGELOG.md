# Changelog

## Unreleased

### Changed
- **Container proxy: TCP ports → Unix domain sockets.** Containers now communicate with the proxy via a Unix socket mounted at `/run/clawbox/proxy.sock` instead of per-container TCP ports. The `CLAWBOX_PROXY_SOCKET` env var replaces `HTTP_PROXY`/`HTTPS_PROXY`. Config fields `proxy_base_port` and `proxy_port_range` are removed.
- **Javy v8+ API for JS tools.** JavaScript tool examples updated to use the two-argument `Javy.IO.readSync(fd, buf)` API (the one-argument form is deprecated in Javy v8).

## v0.1.0

Initial release.

### Features
- WASM tool sandbox with fuel, memory, and time limits
- Docker container isolation (network=none, no caps, readonly rootfs)
- Network proxy with domain allowlists and credential injection
- Leak detection with iterative decoding and pattern matching
- MCP server for Claude Code / Cursor integration
- Hot-reload for WASM tools
- Agent orchestration with lifecycle management
- CLI: init, serve, mcp, build, new-tool, health, status, logs
- Prometheus metrics endpoint
- Audit logging with rotation

### Security
- Server-side policy enforcement (client allowlists ignored)
- Constant-time token comparison
- DNS rebinding protection with IP pinning
- Credential zeroization (Zeroizing<String>)
- Container PID limits and network isolation
- Response header leak redaction
