# clawbox Agent Base Image

Python 3.12 + Node.js 22 runtime for sandboxed AI agent sub-agents.

## Security
- Runs as non-root user (UID 1000)
- Designed for read-only rootfs (writable: /tmp, /home/agent)
- No package manager access at runtime
- All network traffic routed through clawbox proxy via Unix domain socket at /run/clawbox/proxy.sock

## Building
docker build -t ghcr.io/n0xmare/clawbox-agent:latest images/agent-base/

## Usage
This image is automatically used by clawbox when spawning containers with the default image.
