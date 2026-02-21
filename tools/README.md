# clawbox Tool Examples

Example WASM tools in multiple languages. Each reads JSON from stdin, writes JSON to stdout.

## Structure

- `wasm/` — Compiled .wasm binaries (loaded by the server)
- `examples/` — Source code for reference tools
  - `rust/` — Rust tool sources
  - `js/` — JavaScript tool sources
  - `ts/` — TypeScript tool sources

## Rust Tools

| Tool | Description | Source | Allowlist |
|------|-------------|--------|-----------|
| echo | Echo input back | `rust/echo/` | none |
| http_request | Generic HTTP client | `rust/http-request/` | any (server allowlist) |
| web_search | Brave Search API | `rust/web-search/` | `api.search.brave.com` |
| web_fetch | Fetch URL → clean text | `rust/web-fetch/` | any (server allowlist) |
| github | GitHub REST API v3 | `rust/github/` | `api.github.com` |

Build: `cd examples/rust/<tool> && cargo build --target wasm32-wasip1 --release`

## JavaScript / TypeScript Tools

| Tool | Description | Source | Build |
|------|-------------|--------|-------|
| echo-js | Echo input back | `js/echo/` | `javy build echo.js -o echo-js.wasm` |
| echo-ts | Echo input back | `ts/echo/` | `./build.sh` (esbuild + javy) |
| base64 | Base64 encode/decode | `js/base64/` | `javy build index.js -o base64.wasm` |
| hash | SHA-256 hashing | `js/hash/` | `javy build index.js -o hash.wasm` |
| json-fmt | JSON formatting | `js/json-fmt/` | `javy build index.js -o json-fmt.wasm` |

## Prerequisites

- **Rust:** `rustup target add wasm32-wasip1`
- **JavaScript/TypeScript:** Install [javy](https://github.com/bytecodealliance/javy) v8+
- **TypeScript additionally:** `npm install -g esbuild`
