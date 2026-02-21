# clawbox — build, test, run commands

# Default: check everything compiles
default: check

# Fast compile check
check:
    cargo check --workspace

# Build release binary
build:
    cargo build --release

# Run all tests
test:
    cargo test --workspace

# Run clippy lints
lint:
    cargo clippy --workspace --all-targets -- -D warnings

# Format code
fmt:
    cargo fmt --all

# Check formatting
fmt-check:
    cargo fmt --all -- --check

# Run the server (debug mode)
serve:
    cargo run -- serve

# Run the server (release mode)
serve-release:
    cargo run --release -- serve

# Full CI check
ci: fmt-check check lint test

# Clean build artifacts
clean:
    cargo clean
