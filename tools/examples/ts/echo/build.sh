#!/bin/bash
set -e
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
cd "$SCRIPT_DIR"

# Step 1: TypeScript → JavaScript (strip types)
# Requires esbuild: npm install -g esbuild
if command -v esbuild &>/dev/null; then
  esbuild echo.ts --bundle --format=esm --outfile=echo.js --platform=neutral
elif command -v npx &>/dev/null; then
  npx --yes esbuild echo.ts --bundle --format=esm --outfile=echo.js --platform=neutral
else
  echo "ERROR: esbuild not found. Install with: npm install -g esbuild" >&2
  exit 1
fi

# Step 2: JavaScript → WASM
javy build echo.js -o echo-ts.wasm
echo "Built echo-ts.wasm ($(wc -c < echo-ts.wasm) bytes)"
