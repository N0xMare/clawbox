#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")"
echo "Building clawbox-agent base image..."
docker build -t ghcr.io/n0xmare/clawbox-agent:latest agent-base/
echo "Done. Image: ghcr.io/n0xmare/clawbox-agent:latest"
