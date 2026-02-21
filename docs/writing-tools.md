# Writing Tools for clawbox

clawbox tools are WebAssembly (WASM) modules that run inside a sandboxed environment. Tools communicate via stdin/stdout using JSON and can make host calls for logging and HTTP requests.

## Overview

1. Write a Rust binary that reads JSON from stdin and writes JSON to stdout
2. Compile to `wasm32-wasip1` target
3. Copy the `.wasm` file to the `tools/wasm/` directory
4. The filename (without `.wasm`) becomes the tool name

## Protocol

### Input (stdin)

The tool receives a JSON object on stdin with the execution parameters:

```json
{"hello": "world", "count": 42}
```

### Output (stdout)

The tool writes a JSON object to stdout with its results:

```json
{"tool": "echo", "echo": {"hello": "world", "count": 42}}
```

### Host Calls

Tools can invoke host-provided functions via the `clawbox::host_call` import. This is a **clawbox module import** (not a WASI import). The interface uses a simple request/response pattern:

```rust
#[link(wasm_import_module = "clawbox")]
extern "C" {
    fn host_call(request_ptr: *const u8, request_len: i32, response_ptr: *mut u8, response_cap: i32) -> i32;
}
```

The request is a JSON object written to `request_ptr`/`request_len`. The response is written back to `response_ptr` with a maximum of `response_cap` bytes. The return value is the number of bytes written, or `-1` on error.

**Limit:** Each execution is limited to **100 host calls** (configurable via `max_host_calls` in the sandbox config). Exceeding this limit returns an error response:
```json
{"error": "host_call limit exceeded (max 100)"}
```

#### Response Wrapping

All host call responses are wrapped in a result envelope:

- **Success:** `{"ok": <value>}`
- **Error:** `{"error": "<message>"}`

Tools should unwrap this envelope to get the actual result.

#### `log`

Write a log entry (captured in execution metadata).

**Request:**
```json
{"method": "log", "params": {"level": "info", "message": "Processing request..."}}
```

**Response:** `{"ok": null}`

Valid log levels: `trace`, `debug`, `info`, `warn`, `error`.

#### `http_request`

Make an HTTP request through the network proxy. **Only works if the target domain is in the tool's registered allowlist** (deny-all by default).

**Request:**
```json
{
  "method": "http_request",
  "params": {
    "url": "https://api.github.com/repos/owner/repo",
    "method": "GET",
    "headers": {"Accept": "application/json"},
    "body": null
  }
}
```

**Response (success):**
```json
{
  "ok": {
    "status": 200,
    "headers": {"content-type": "application/json"},
    "body": "..."
  }
}
```

**Response (blocked):**
```json
{"error": "domain not in allowlist: evil.com"}
```

## Example: Echo Tool

The simplest possible tool — reads params and echoes them back.

```rust
// tools/examples/rust/echo/src/main.rs
use serde_json::Value;
use std::io::Read;

fn main() {
    let mut input = String::new();
    std::io::stdin().read_to_string(&mut input).unwrap();
    let params: Value = serde_json::from_str(&input).unwrap_or(Value::Null);

    let output = serde_json::json!({
        "tool": "echo",
        "echo": params
    });

    println!("{}", serde_json::to_string(&output).unwrap());
}
```

## Example: HTTP Request Tool

A tool that makes HTTP requests via the host call interface.

See `tools/examples/rust/http-request/` for the full implementation.

## CLI Shortcuts

The fastest way to create and build tools:

```bash
# Scaffold a new Rust tool
clawbox new-tool my-tool --lang rust

# Scaffold a JS tool
clawbox new-tool my-tool --lang js

# Build any tool (compiles to WASM and installs to tools/wasm/)
clawbox build my-tool
```

The sections below explain the manual build process for reference.

## Build Instructions

```bash
# Install the WASM target (one-time)
rustup target add wasm32-wasip1

# Build the tool
cd tools/examples/rust/echo
cargo build --target wasm32-wasip1 --release

# Copy to tools directory (filename = tool name)
cp target/wasm32-wasip1/release/clawbox-tool-echo.wasm ../../wasm/echo.wasm
```

## Registering a Tool Manifest

To grant a tool network access or credential injection, register a manifest via the API:

```bash
curl -X POST http://localhost:9800/tools/register \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer <token>" \
  -d '{"tool":{"name":"http_request","description":"Make HTTP requests to allowed endpoints","version":"0.1.0"},"network":{"allowlist":["api.github.com","httpbin.org"],"max_concurrent":5},"credentials":{"available":["github-token"]}}'
```

Without a manifest, tools run in **deny-all** mode: no network access, no credentials.

## Security Notes

- Tools run in a WASM sandbox with limited fuel (default: 100M instructions)
- Tools have a timeout (default: 30 seconds)
- Host calls are limited to 100 per execution (default, configurable)
- All tool output is scanned for credential leaks and prompt injection patterns
- Detected credentials are automatically redacted with `[REDACTED]`
- Network access requires explicit allowlisting via tool manifest
- Credentials are injected at the proxy boundary — tools never see raw secrets

## JavaScript Tools

You can write clawbox tools in JavaScript using [javy](https://github.com/bytecodealliance/javy) (v8+), which compiles JS to WebAssembly.

JS tools support stdin/stdout for JSON I/O (pure computation). **Host calls (`host_call` FFI) are Rust-only** — JS/TS tools cannot make HTTP requests or use logging via the clawbox host interface.

> **Important:** Javy v8+ changed the `readSync` API to require two arguments: `readSync(fd, buffer)`. The one-argument form is deprecated. All examples below use the v8+ API.

### Stdin Reading Pattern (Javy v8+)

All JS tools must use the two-argument chunked reader pattern:

```javascript
function readStdin() {
  const chunks = [];
  const buf = new Uint8Array(4096);
  while (true) {
    const n = Javy.IO.readSync(0, buf);
    if (n === 0) break;
    chunks.push(buf.slice(0, n));
  }
  const total = chunks.reduce((s, c) => s + c.length, 0);
  const result = new Uint8Array(total);
  let offset = 0;
  for (const chunk of chunks) {
    result.set(chunk, offset);
    offset += chunk.length;
  }
  return new TextDecoder().decode(result);
}
```

### Example: Echo Tool (JavaScript)

```javascript
// tools/examples/js/echo/echo.js

function readStdin() {
  const chunks = [];
  const buf = new Uint8Array(4096);
  while (true) {
    const n = Javy.IO.readSync(0, buf);
    if (n === 0) break;
    chunks.push(buf.slice(0, n));
  }
  const total = chunks.reduce((s, c) => s + c.length, 0);
  const result = new Uint8Array(total);
  let offset = 0;
  for (const chunk of chunks) {
    result.set(chunk, offset);
    offset += chunk.length;
  }
  return new TextDecoder().decode(result);
}

const params = JSON.parse(readStdin());

const response = {
  tool: "echo-js",
  version: "0.1.0",
  echo: params,
  message: "Hello from clawbox WASM sandbox! (JavaScript)"
};

const encoder = new TextEncoder();
Javy.IO.writeSync(1, encoder.encode(JSON.stringify(response)));
```

### Build Instructions (JavaScript)

```bash
# Install javy (one-time) — https://github.com/bytecodealliance/javy
# Requires javy v8+

# Build the tool
cd tools/examples/js/echo
javy build echo.js -o echo-js.wasm

# Copy to tools directory
cp echo-js.wasm ../../wasm/echo-js.wasm
```

## TypeScript Tools

TypeScript tools follow the same pattern but require a transpilation step (TS → JS) before javy compilation.

### Example: Echo Tool (TypeScript)

```typescript
// tools/examples/ts/echo/echo.ts
declare const Javy: {
  IO: {
    readSync(fd: number, buf: Uint8Array): number;
    writeSync(fd: number, data: Uint8Array): void;
  };
};

interface EchoParams {
  [key: string]: unknown;
}

function readStdin(): string {
  const chunks: Uint8Array[] = [];
  const buf = new Uint8Array(4096);
  while (true) {
    const n: number = Javy.IO.readSync(0, buf);
    if (n === 0) break;
    chunks.push(buf.slice(0, n));
  }
  const total = chunks.reduce((s, c) => s + c.length, 0);
  const result = new Uint8Array(total);
  let offset = 0;
  for (const chunk of chunks) {
    result.set(chunk, offset);
    offset += chunk.length;
  }
  return new TextDecoder().decode(result);
}

const params: EchoParams = JSON.parse(readStdin());

const response = {
  tool: "echo-ts",
  version: "0.1.0",
  echo: params,
  message: "Hello from clawbox WASM sandbox! (TypeScript)",
};

const encoder = new TextEncoder();
Javy.IO.writeSync(1, encoder.encode(JSON.stringify(response)));
```

### Build Instructions (TypeScript)

```bash
# Step 1: Transpile TS → JS (using esbuild)
esbuild echo.ts --bundle --format=esm --outfile=echo.js --platform=neutral

# Step 2: Compile JS → WASM
javy build echo.js -o echo-ts.wasm

# Or use the build script:
cd tools/examples/ts/echo
./build.sh
```

## Other Languages

### Python

Python-to-WASM toolchains (e.g., componentize-py, py2wasm) are not yet production-ready for this use case. We recommend Rust or JavaScript for now.
