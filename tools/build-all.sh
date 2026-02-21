#!/bin/bash
set -e
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"

echo "==> Building Rust tools..."
for tool in echo http-request; do
  echo "  Building $tool..."
  (cd "$SCRIPT_DIR/examples/rust/$tool" && cargo build --target wasm32-wasip1 --release)
  cp "$SCRIPT_DIR/examples/rust/$tool/target/wasm32-wasip1/release/"*.wasm "$SCRIPT_DIR/wasm/" 2>/dev/null || true
done

echo "==> Building JS tools..."
(cd "$SCRIPT_DIR/examples/js/echo" && javy build echo.js -o echo-js.wasm)
cp "$SCRIPT_DIR/examples/js/echo/echo-js.wasm" "$SCRIPT_DIR/wasm/"

echo "==> Building TS tools..."
if [ -x "$SCRIPT_DIR/examples/ts/echo/build.sh" ]; then
  (cd "$SCRIPT_DIR/examples/ts/echo" && ./build.sh)
  cp "$SCRIPT_DIR/examples/ts/echo/echo-ts.wasm" "$SCRIPT_DIR/wasm/" 2>/dev/null || true
fi

echo "Done! Built tools:"
ls -lh "$SCRIPT_DIR/wasm/"*.wasm 2>/dev/null
