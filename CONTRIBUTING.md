# Contributing to clawbox

## Prerequisites

- **Rust 1.93+** with the `wasm32-wasip1` target:
  ```bash
  rustup target add wasm32-wasip1
  ```
- **Docker** (for integration tests involving containers)
- **Javy v8+** (for building JavaScript tool examples)

## Building

```bash
# Full build
cargo build

# Check only (faster)
cargo check

# Build WASM tools
cargo build --target wasm32-wasip1 --release -p clawbox-tool-echo
```

## Testing

```bash
# Unit tests (no Docker required)
cargo test

# Integration tests (requires Docker daemon)
cargo test --features integration

# Run a specific test
cargo test test_name

# With nextest (recommended)
cargo nextest run
```

## Code Style

- Run `cargo fmt` before committing
- Run `cargo clippy -- -D warnings` — all warnings are errors in CI
- Add doc comments (`///`) on all public items
- Use `#[non_exhaustive]` on public structs and enums
- Use `#[must_use]` where appropriate

## PR Process

1. Fork the repository
2. Create a feature branch (`git checkout -b feature/my-feature`)
3. Make your changes with tests
4. Ensure `cargo fmt`, `cargo clippy`, and `cargo test` pass
5. Open a pull request with a clear description

## Architecture

See the crate-level `README.md` files for each component:

- [`clawbox`](crates/clawbox/README.md) — CLI binary
- [`clawbox-server`](crates/clawbox-server/README.md) — HTTP API server
- [`clawbox-sandbox`](crates/clawbox-sandbox/README.md) — WASM engine
- [`clawbox-proxy`](crates/clawbox-proxy/README.md) — Network proxy
- [`clawbox-containers`](crates/clawbox-containers/README.md) — Docker management
- [`clawbox-types`](crates/clawbox-types/README.md) — Shared type definitions

## License

By contributing, you agree that your contributions will be licensed under the MIT License.
