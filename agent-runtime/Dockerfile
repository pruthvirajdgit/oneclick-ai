# =============================================================================
# OneClick.ai — Custom OpenClaw Agent Image
# =============================================================================
# Extends the official OpenClaw image with env-var-based configuration.
# The entrypoint translates environment variables into OpenClaw CLI config
# commands, then starts the gateway.
# =============================================================================

FROM ghcr.io/openclaw/openclaw:latest

USER root

# Create directories for docs and workspace
RUN mkdir -p /data/docs /home/node/workspace \
    && chown -R node:node /data/docs /home/node/workspace

# Copy entrypoint with correct permissions
COPY --chmod=0755 entrypoint.sh /usr/local/bin/oneclick-entrypoint.sh

# Stay as root — entrypoint will fix volume perms then exec as node
WORKDIR /home/node

# Default environment variables
ENV AGENT_NAME="Assistant" \
    AGENT_MODEL="openrouter/auto" \
    AGENT_SYSTEM_PROMPT="You are a helpful AI assistant." \
    AGENT_DOCS_PATH="/data/docs" \
    AGENT_PORT=3000 \
    TZ=Asia/Kolkata

EXPOSE 3000

# Health check via OpenClaw CLI (WebSocket-based)
HEALTHCHECK --interval=60s --timeout=10s --start-period=45s --retries=3 \
  CMD openclaw health --timeout 8000 || exit 1

ENTRYPOINT ["/usr/local/bin/oneclick-entrypoint.sh"]
