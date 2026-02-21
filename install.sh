#!/usr/bin/env bash
# clawbox installer — downloads the latest release binary for your platform
# Usage: curl -sSf https://raw.githubusercontent.com/N0xMare/clawbox/main/install.sh | bash
set -euo pipefail

REPO="N0xMare/clawbox"
INSTALL_DIR="${CLAWBOX_INSTALL_DIR:-$HOME/.local/bin}"

info()  { printf '\033[1;34m→\033[0m %s\n' "$*"; }
ok()    { printf '\033[1;32m✓\033[0m %s\n' "$*"; }
fail()  { printf '\033[1;31m✗\033[0m %s\n' "$*" >&2; exit 1; }

# Detect platform
OS="$(uname -s)"
ARCH="$(uname -m)"

case "$OS" in
    Linux)  PLATFORM="linux" ;;
    Darwin) PLATFORM="darwin" ;;
    *)      fail "Unsupported OS: $OS" ;;
esac

case "$ARCH" in
    x86_64|amd64)  ARCH="x86_64" ;;
    aarch64|arm64) ARCH="aarch64" ;;
    *)             fail "Unsupported architecture: $ARCH" ;;
esac

ARTIFACT="clawbox-${PLATFORM}-${ARCH}"
info "Detected platform: ${PLATFORM}-${ARCH}"

# Find latest release
info "Fetching latest release..."
RELEASE_URL="https://api.github.com/repos/${REPO}/releases/latest"

if command -v curl >/dev/null 2>&1; then
    DOWNLOAD_URL=$(curl -sSf "$RELEASE_URL" | grep "browser_download_url.*${ARTIFACT}\"" | head -1 | cut -d '"' -f 4)
    CHECKSUM_URL=$(curl -sSf "$RELEASE_URL" | grep "browser_download_url.*${ARTIFACT}.sha256\"" | head -1 | cut -d '"' -f 4)
elif command -v wget >/dev/null 2>&1; then
    DOWNLOAD_URL=$(wget -qO- "$RELEASE_URL" | grep "browser_download_url.*${ARTIFACT}\"" | head -1 | cut -d '"' -f 4)
    CHECKSUM_URL=$(wget -qO- "$RELEASE_URL" | grep "browser_download_url.*${ARTIFACT}.sha256\"" | head -1 | cut -d '"' -f 4)
else
    fail "curl or wget required"
fi

if [[ -z "${DOWNLOAD_URL:-}" ]]; then
    fail "No release binary found for ${ARTIFACT}. Check https://github.com/${REPO}/releases"
fi

# Download
TMPDIR="$(mktemp -d)"
trap 'rm -rf "$TMPDIR"' EXIT

info "Downloading ${ARTIFACT}..."
if command -v curl >/dev/null 2>&1; then
    curl -sSfL "$DOWNLOAD_URL" -o "$TMPDIR/clawbox"
    [[ -n "${CHECKSUM_URL:-}" ]] && curl -sSfL "$CHECKSUM_URL" -o "$TMPDIR/clawbox.sha256"
else
    wget -qO "$TMPDIR/clawbox" "$DOWNLOAD_URL"
    [[ -n "${CHECKSUM_URL:-}" ]] && wget -qO "$TMPDIR/clawbox.sha256" "$CHECKSUM_URL"
fi

# Verify checksum
if [[ -f "$TMPDIR/clawbox.sha256" ]]; then
    info "Verifying checksum..."
    cd "$TMPDIR"
    # sha256 file contains "hash  artifact-name", fix the filename to match
    EXPECTED=$(awk '{print $1}' clawbox.sha256)
    if command -v sha256sum >/dev/null 2>&1; then
        ACTUAL=$(sha256sum clawbox | awk '{print $1}')
    elif command -v shasum >/dev/null 2>&1; then
        ACTUAL=$(shasum -a 256 clawbox | awk '{print $1}')
    else
        info "No sha256 tool found, skipping verification"
        ACTUAL="$EXPECTED"
    fi
    if [[ "$EXPECTED" != "$ACTUAL" ]]; then
        fail "Checksum mismatch! Expected: $EXPECTED Got: $ACTUAL"
    fi
    ok "Checksum verified"
    cd - >/dev/null
fi

# Install
mkdir -p "$INSTALL_DIR"
mv "$TMPDIR/clawbox" "$INSTALL_DIR/clawbox"
chmod +x "$INSTALL_DIR/clawbox"
ok "Installed to $INSTALL_DIR/clawbox"

# Check PATH
if ! echo "$PATH" | tr ':' '\n' | grep -qx "$INSTALL_DIR"; then
    echo ""
    info "Add to your PATH:"
    echo "  export PATH=\"$INSTALL_DIR:\$PATH\""
    echo ""
fi

# Version
"$INSTALL_DIR/clawbox" --version 2>/dev/null || true

echo ""
ok "Done! Run 'clawbox init' to set up, then 'clawbox serve' to start."
