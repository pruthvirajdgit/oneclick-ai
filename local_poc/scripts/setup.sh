#!/bin/bash
# =============================================================================
# OneClick.ai — Quick Setup Script
# =============================================================================
# Run this to get your first agent up and running.
# Prerequisites: Docker Desktop must be installed and running.
# =============================================================================

set -e

echo ""
echo "🚀 OneClick.ai — Agent Runtime Setup"
echo "======================================"
echo ""

# Check Docker
if ! command -v docker &> /dev/null; then
    echo "❌ Docker is not installed. Please install Docker Desktop first:"
    echo "   https://www.docker.com/products/docker-desktop/"
    exit 1
fi

if ! docker info &> /dev/null 2>&1; then
    echo "❌ Docker is not running. Please start Docker Desktop and try again."
    exit 1
fi

echo "✅ Docker is running"

# Check for .env file
cd "$(dirname "$0")/../oneclick-runtime"

if [ ! -f .env ]; then
    echo ""
    echo "📝 Creating .env from template..."
    cp .env.example .env
    echo ""
    echo "⚠️  You need to add your OpenRouter API key!"
    echo ""
    read -p "   Paste your OpenRouter API key (sk-or-...): " API_KEY
    
    if [ -z "$API_KEY" ]; then
        echo "❌ No API key provided. Edit .env manually and run this script again."
        exit 1
    fi
    
    # Replace placeholder with actual key
    if [[ "$OSTYPE" == "darwin"* ]]; then
        sed -i '' "s|YOUR_OPENROUTER_API_KEY_HERE|${API_KEY}|" .env
    else
        sed -i "s|YOUR_OPENROUTER_API_KEY_HERE|${API_KEY}|" .env
    fi
    
    echo "   ✅ API key saved to .env"
else
    echo "✅ .env file exists"
fi

# Create docs directory
mkdir -p docs
echo "✅ docs/ directory ready (put your PDFs/TXTs here)"

# Build and start
echo ""
echo "🔨 Building agent image..."
docker compose build --quiet

echo ""
echo "🚀 Starting agent..."
docker compose up -d

echo ""
echo "⏳ Waiting for agent to be ready..."
for i in {1..30}; do
    STATUS=$(docker compose ps --format '{{.Status}}' 2>/dev/null | head -1)
    if echo "$STATUS" | grep -q "healthy"; then
        echo ""
        echo "======================================"
        echo "✅ Your AI agent is LIVE!"
        echo ""
        echo "   🌐 Gateway: ws://localhost:${AGENT_PORT:-3000}"
        echo "   📊 Logs:    docker compose logs -f"
        echo "   🛑 Stop:    docker compose down"
        echo "   🎛️  TUI:    docker exec -it oneclick-agent openclaw tui"
        echo "======================================"
        exit 0
    fi
    printf "."
    sleep 2
done

echo ""
echo "⚠️  Agent is starting but not ready yet. Check logs:"
echo "   docker compose logs -f"
echo ""
echo "   The gateway will be available at ws://localhost:${AGENT_PORT:-3000} once ready."
