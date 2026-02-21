# clawbox agent base image — minimal runtime for sandboxed sub-agents
FROM python:3.12-slim AS base

# Security: run as non-root user
RUN groupadd -g 1000 agent && useradd -u 1000 -g agent -m agent

# Install system tools
RUN apt-get update && apt-get install -y --no-install-recommends \
    curl \
    ca-certificates \
    jq \
    git \
    && rm -rf /var/lib/apt/lists/*

# Install Node.js (for JS/TS tasks)
RUN curl -fsSL https://deb.nodesource.com/setup_22.x | bash - \
    && apt-get install -y --no-install-recommends nodejs \
    && rm -rf /var/lib/apt/lists/*

# Install common Python packages for agent tasks
RUN pip install --no-cache-dir \
    requests \
    httpx \
    pydantic \
    beautifulsoup4 \
    markdownify

# Create writable directories (the rest of rootfs will be read-only)
RUN mkdir -p /tmp/agent /home/agent/.cache && \
    chown -R agent:agent /tmp/agent /home/agent

# Default environment — proxy will be injected at container creation
ENV PYTHONUNBUFFERED=1 \
    HOME=/home/agent

WORKDIR /home/agent
USER agent

# Default entrypoint: run a Python script passed as argument
ENTRYPOINT ["python3"]
CMD ["--help"]
