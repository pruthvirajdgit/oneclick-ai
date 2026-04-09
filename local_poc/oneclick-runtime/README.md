# OneClick.ai — Agent Runtime

This is the core engine of OneClick.ai. It runs OpenClaw inside Docker containers
with pre-configured LLM routing via OpenRouter.

## Quick Start (Local Development)

### Prerequisites
- Docker Desktop installed and running
- OpenRouter API key (get free at https://openrouter.ai)

### Steps

```bash
# 1. Copy env template
cp .env.example .env

# 2. Add your OpenRouter API key to .env
#    Edit .env and replace YOUR_OPENROUTER_API_KEY_HERE

# 3. Start the agent
docker compose up -d

# 4. Open the web UI
open http://localhost:3000

# 5. View logs
docker compose logs -f

# 6. Stop the agent
docker compose down
```

## Project Structure

```
oneclick-runtime/
├── docker-compose.yml          # Main compose file — runs one agent locally
├── docker-compose.multi.yml    # Multi-agent compose (for testing scaling)
├── Dockerfile                  # Custom OpenClaw image with our defaults
├── config/
│   └── openclaw-template.json  # Template config injected at container start
├── entrypoint.sh               # Custom entrypoint that configures OpenClaw from env vars
└── .env.example                # Environment variable template
```

## Environment Variables

| Variable | Required | Default | Description |
|----------|----------|---------|-------------|
| `OPENROUTER_API_KEY` | Yes | — | Your OpenRouter API key (sk-or-...) |
| `AGENT_MODEL` | No | `openrouter/free` | LLM model to use |
| `AGENT_NAME` | No | `Assistant` | Display name for the agent |
| `AGENT_SYSTEM_PROMPT` | No | `You are a helpful AI assistant.` | The agent's identity/instructions |
| `AGENT_DOCS_PATH` | No | `/data/docs` | Path to mounted company documents |
