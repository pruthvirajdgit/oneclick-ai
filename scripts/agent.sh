#!/bin/bash
# =============================================================================
# OneClick.ai — Agent Lifecycle Management
# =============================================================================
# Commands for managing agent containers.
# Usage: ./agent.sh <command> [options]
# =============================================================================

set -e

RUNTIME_DIR="$(dirname "$0")/../agent-runtime"
cd "$RUNTIME_DIR"

usage() {
    echo "Usage: ./agent.sh <command>"
    echo ""
    echo "Commands:"
    echo "  start       Start the agent"
    echo "  stop        Stop the agent"
    echo "  restart     Restart the agent"
    echo "  status      Show agent status"
    echo "  logs        Show agent logs (follow)"
    echo "  shell       Open a shell inside the agent container"
    echo "  rebuild     Rebuild and restart the agent"
    echo "  prompt      Update the agent's system prompt"
    echo "  model       Update the agent's LLM model"
    echo ""
    echo "Multi-agent commands:"
    echo "  multi-start   Start 3 agents with Traefik (scaling test)"
    echo "  multi-stop    Stop all agents and Traefik"
    echo ""
}

case "${1:-}" in
    start)
        echo "🚀 Starting agent..."
        docker compose up -d
        echo "✅ Agent started. Gateway: ws://localhost:${AGENT_PORT:-3000}"
        ;;
    stop)
        echo "🛑 Stopping agent..."
        docker compose down
        echo "✅ Agent stopped."
        ;;
    restart)
        echo "🔄 Restarting agent..."
        docker compose restart
        echo "✅ Agent restarted."
        ;;
    status)
        echo "📊 Agent status:"
        docker compose ps
        echo ""
        STATUS=$(docker compose ps --format '{{.Status}}' 2>/dev/null | head -1)
        if echo "$STATUS" | grep -q "healthy"; then
            echo "🟢 Agent is healthy and running"
        elif echo "$STATUS" | grep -q "Up"; then
            echo "🟡 Agent is starting..."
        else
            echo "🔴 Agent is not running"
        fi
        ;;
    logs)
        docker compose logs -f
        ;;
    shell)
        echo "🐚 Opening shell in agent container..."
        docker compose exec agent /bin/bash
        ;;
    rebuild)
        echo "🔨 Rebuilding and restarting agent..."
        docker compose down
        docker compose build --no-cache
        docker compose up -d
        echo "✅ Agent rebuilt and started."
        ;;
    prompt)
        if [ -z "${2:-}" ]; then
            echo "Current prompt:"
            grep AGENT_SYSTEM_PROMPT .env | cut -d= -f2-
            echo ""
            read -p "Enter new system prompt: " NEW_PROMPT
        else
            NEW_PROMPT="$2"
        fi
        # Update .env
        if [[ "$OSTYPE" == "darwin"* ]]; then
            sed -i '' "s|^AGENT_SYSTEM_PROMPT=.*|AGENT_SYSTEM_PROMPT=${NEW_PROMPT}|" .env
        else
            sed -i "s|^AGENT_SYSTEM_PROMPT=.*|AGENT_SYSTEM_PROMPT=${NEW_PROMPT}|" .env
        fi
        echo "✅ Prompt updated. Restarting agent..."
        docker compose restart
        echo "✅ Agent restarted with new prompt."
        ;;
    model)
        echo "Available models on OpenRouter:"
        echo "  1. openrouter/auto (auto-route to best available model)"
        echo "  2. openrouter/meta-llama/llama-3.3-70b-instruct"
        echo "  3. openrouter/google/gemma-3-27b-it"
        echo "  4. openrouter/qwen/qwen3-next-80b"
        echo "  5. openrouter/mistralai/mistral-small-3.1-24b-instruct"
        echo ""
        read -p "Enter model name (or number 1-5): " MODEL_CHOICE
        case "$MODEL_CHOICE" in
            1) MODEL="openrouter/auto" ;;
            2) MODEL="openrouter/meta-llama/llama-3.3-70b-instruct" ;;
            3) MODEL="openrouter/google/gemma-3-27b-it" ;;
            4) MODEL="openrouter/qwen/qwen3-next-80b" ;;
            5) MODEL="openrouter/mistralai/mistral-small-3.1-24b-instruct" ;;
            *) MODEL="$MODEL_CHOICE" ;;
        esac
        if [[ "$OSTYPE" == "darwin"* ]]; then
            sed -i '' "s|^AGENT_MODEL=.*|AGENT_MODEL=${MODEL}|" .env
        else
            sed -i "s|^AGENT_MODEL=.*|AGENT_MODEL=${MODEL}|" .env
        fi
        echo "✅ Model set to: ${MODEL}. Restarting..."
        docker compose restart
        echo "✅ Agent restarted with new model."
        ;;
    multi-start)
        echo "🚀 Starting multi-agent setup (3 agents + Traefik)..."
        docker compose -f docker-compose.multi.yml up -d --build
        echo "✅ Multi-agent setup running:"
        echo "   Agent 1 (Support):    http://localhost/agent/agent-1/"
        echo "   Agent 2 (Sales):      http://localhost/agent/agent-2/"
        echo "   Agent 3 (Assistant):  http://localhost/agent/agent-3/"
        echo "   Traefik Dashboard:    http://localhost:8080"
        ;;
    multi-stop)
        echo "🛑 Stopping multi-agent setup..."
        docker compose -f docker-compose.multi.yml down
        echo "✅ All agents stopped."
        ;;
    *)
        usage
        ;;
esac
