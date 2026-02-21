# clawbox-proxy

Network proxy layer for clawbox providing endpoint allowlisting, credential injection, leak detection, rate limiting, and audit logging.

## Overview

`clawbox-proxy` is the security boundary between sandboxed tool executions and the outside network. All outbound HTTP requests from WASM tools and containers pass through this proxy. It enforces domain allowlists, injects credentials at the last mile (so tools never see raw API keys), scans for credential leaks in both requests and responses, and logs every request to an append-only audit trail.

## Usage

```rust,ignore
use clawbox_proxy::{ProxyConfig, ProxyService, CredentialInjector, LeakDetector};

// Configure the proxy
let config = ProxyConfig {
    allowlist: vec!["api.github.com".into(), "api.openai.com".into()],
    max_response_bytes: 10 * 1024 * 1024,
    timeout_ms: 30_000,
};

// Set up credential injection
let mut injector = CredentialInjector::new();
injector.add_mapping("api.github.com", "Authorization", "token ghp_...");

// Set up leak detection
let mut detector = LeakDetector::new();
detector.add_known_secret("ghp_...");

// Create the proxy service
let proxy = ProxyService::new(config, injector, detector)?;

// Forward a request through the pipeline
let response = proxy.forward_request(
    "https://api.github.com/repos",
    "GET",
    Default::default(),
    None,
).await?;
```

## Features

- **Domain allowlisting** — Only explicitly allowed domains can be contacted; supports exact match and wildcard (`*.github.com`)
- **Credential injection** — API keys injected into outbound requests at the proxy boundary; tools never see raw secrets
- **Leak detection** — Scans URLs, headers, and bodies for known secrets and credential patterns; checks URL-encoded, double-encoded, and base64 variants
- **Output redaction** — Automatically redacts leaked credentials from response bodies
- **Private IP blocking** — Blocks requests to loopback, RFC 1918, link-local, and other internal addresses
- **DNS rebinding protection** — Pre-resolves hostnames and pins validated IPs to prevent TOCTOU attacks
- **Rate limiting** — Per-tool token bucket rate limiter with configurable burst and refill rates
- **Audit logging** — Append-only JSON lines log with automatic rotation (10 MB, 5 rotated files)
- **Encrypted credential storage** — AES-256-GCM encrypted file store with zeroizing memory handling

## Architecture

| Module           | Purpose                                              |
|------------------|------------------------------------------------------|
| `proxy`          | Core forward proxy pipeline (allowlist → leak scan → IP check → inject → forward → audit) |
| `allowlist`      | Domain-level allowlist enforcement                    |
| `credentials`    | Domain-to-header credential mapping with `Zeroizing` memory |
| `leak_detection` | Outbound content scanning for secrets (regex + known values) |
| `rate_limiter`   | Token bucket rate limiter per tool/container          |
| `audit`          | Append-only structured JSON log with rotation          |
| `store`          | AES-256-GCM encrypted credential file storage          |

## Safety / Security

- **Credentials never reach the sandbox** — Injection happens at the proxy boundary after all security checks pass
- **Constant-time auth** — Bearer token validation uses `subtle::ConstantTimeEq` (in the server layer)
- **DNS rebinding protection** — Hostnames are resolved and validated before connecting; the resolved IP is pinned via `reqwest::resolve()` to prevent TOCTOU attacks
- **Private IP blocking** — Prevents SSRF to internal services, cloud metadata endpoints (169.254.169.254), and loopback
- **IPv6-aware** — Handles IPv4-mapped IPv6 addresses and unique-local/link-local ranges
- **Encoding evasion prevention** — Leak detection checks URL-decoded, double-decoded, base64, and base64-urlsafe variants
- **Memory safety** — Credential values use `zeroize::Zeroizing` to clear secrets from memory on drop
- **Redirect blocking** — HTTP redirects are not followed to prevent redirect-based SSRF

## License

MIT
