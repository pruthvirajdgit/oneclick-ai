# OneClick.ai

**Your AI Workforce. One Click.**

Deploy 24/7 AI employees in 30 seconds. No coding required.

---

## Project Structure

```
oneclick-ai/
├── agent-runtime/              # Core engine — OpenClaw in Docker
│   ├── Dockerfile              # Custom OpenClaw image
│   ├── docker-compose.yml      # Single agent (local dev)
│   ├── docker-compose.multi.yml # Multi-agent + Traefik (scaling test)
│   ├── entrypoint.sh           # Configures OpenClaw from env vars
│   ├── .env.example            # Environment variable template
│   └── docs/                   # Company docs mounted into agents
├── scripts/
│   ├── setup.sh                # One-command setup
│   └── agent.sh                # Agent lifecycle management
└── docs/                       # Project documentation
```

## Quick Start

```bash
# 1. Run setup (will ask for your OpenRouter API key)
./scripts/setup.sh

# 2. That's it. Gateway runs on ws://localhost:3000
```

## Agent Management

```bash
./scripts/agent.sh start      # Start the agent
./scripts/agent.sh stop       # Stop the agent
./scripts/agent.sh restart    # Restart the agent
./scripts/agent.sh status     # Check if agent is healthy
./scripts/agent.sh logs       # View real-time logs
./scripts/agent.sh prompt     # Change the system prompt
./scripts/agent.sh model      # Switch LLM model
./scripts/agent.sh rebuild    # Rebuild from scratch
```

## Multi-Agent Testing

```bash
./scripts/agent.sh multi-start    # Start 3 agents + Traefik
./scripts/agent.sh multi-stop     # Stop everything
```

## Current Phase: P0 — Core Engine ✅

OpenClaw Gateway + OpenRouter + Docker — working!

- [x] Dockerfile with env-var configuration
- [x] docker-compose for single agent
- [x] docker-compose for multi-agent + Traefik
- [x] Entrypoint script (env vars → OpenClaw config)
- [x] Messaging channel support (Telegram, Slack, Discord)
- [x] Setup and management scripts
- [x] **Tested with real Docker — gateway running and healthy**
- [ ] Test with real OpenRouter API key (send a message)
- [ ] Test Telegram bot connection
- [ ] Validate multi-agent Traefik routing
