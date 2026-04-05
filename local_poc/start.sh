#!/usr/bin/env bash
# =============================================================================
# OneClick.ai — One-Click Start
# =============================================================================
# Clone → create .env with your OpenRouter key → run this script. That's it.
#
#   git clone <repo> && cd oneclick-ai
#   cp agent-runtime/.env.example agent-runtime/.env
#   # Edit .env → paste your OpenRouter API key
#   ./start.sh
#
# This script handles EVERYTHING:
#   1. Installs Docker if missing (Linux only; prompts for Mac/Windows)
#   2. Starts Docker daemon if not running
#   3. Creates .env from template if missing (asks for API key)
#   4. Builds the OpenClaw Docker image
#   5. Starts the agent container
#   6. Waits for healthy status
#   7. Auto-approves browser pairing
#   8. Prints the dashboard URL
# =============================================================================

set -e

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
BOLD='\033[1m'
NC='\033[0m'

PROJECT_DIR="$(cd "$(dirname "$0")" && pwd)"
RUNTIME_DIR="${PROJECT_DIR}/agent-runtime"

header() {
    echo ""
    echo -e "${CYAN}${BOLD}"
    echo "  ╔═══════════════════════════════════════════╗"
    echo "  ║         🚀  OneClick.ai  🚀              ║"
    echo "  ║     Your AI Workforce. One Click.         ║"
    echo "  ╚═══════════════════════════════════════════╝"
    echo -e "${NC}"
}

info()    { echo -e "  ${GREEN}✅${NC} $1"; }
warn()    { echo -e "  ${YELLOW}⚠️ ${NC} $1"; }
error()   { echo -e "  ${RED}❌${NC} $1"; }
step()    { echo -e "\n  ${BOLD}▸ $1${NC}"; }

# ─────────────────────────────────────────────────────────────────────────────
# Step 1: Install Docker if needed
# ─────────────────────────────────────────────────────────────────────────────
install_docker() {
    step "Checking Docker..."

    if command -v docker &> /dev/null; then
        DOCKER_VERSION=$(docker --version 2>/dev/null | grep -oP '\d+\.\d+\.\d+' | head -1)
        info "Docker ${DOCKER_VERSION} is installed"
    else
        warn "Docker not found. Installing..."

        # Detect OS
        if [[ "$OSTYPE" == "linux-gnu"* ]]; then
            # Linux — auto-install via official script
            if [ "$(id -u)" -ne 0 ]; then
                error "Docker installation requires root. Run with: sudo ./start.sh"
                exit 1
            fi
            echo "  Downloading Docker install script..."
            curl -fsSL https://get.docker.com -o /tmp/get-docker.sh
            sh /tmp/get-docker.sh 2>&1 | tail -5
            rm -f /tmp/get-docker.sh
            info "Docker installed"
        elif [[ "$OSTYPE" == "darwin"* ]]; then
            # macOS
            if command -v brew &> /dev/null; then
                warn "Installing Docker via Homebrew..."
                brew install --cask docker
                info "Docker Desktop installed. Please open Docker Desktop and re-run this script."
                exit 0
            else
                error "Please install Docker Desktop from: https://www.docker.com/products/docker-desktop/"
                exit 1
            fi
        else
            error "Please install Docker Desktop from: https://www.docker.com/products/docker-desktop/"
            exit 1
        fi
    fi

    # Check docker compose plugin
    if ! docker compose version &> /dev/null 2>&1; then
        error "Docker Compose plugin not found. Please update Docker."
        exit 1
    fi
}

# ─────────────────────────────────────────────────────────────────────────────
# Step 2: Start Docker daemon if not running
# ─────────────────────────────────────────────────────────────────────────────
start_docker() {
    step "Checking Docker daemon..."

    if docker info &> /dev/null 2>&1; then
        info "Docker daemon is running"
        return 0
    fi

    warn "Docker daemon is not running. Starting..."

    # Try systemctl (Linux)
    if command -v systemctl &> /dev/null; then
        if [ "$(id -u)" -eq 0 ]; then
            systemctl start docker 2>/dev/null && sleep 3
        else
            sudo systemctl start docker 2>/dev/null && sleep 3
        fi
    fi

    # Try service (older Linux / WSL)
    if ! docker info &> /dev/null 2>&1; then
        if command -v service &> /dev/null; then
            if [ "$(id -u)" -eq 0 ]; then
                service docker start 2>/dev/null && sleep 3
            else
                sudo service docker start 2>/dev/null && sleep 3
            fi
        fi
    fi

    # Verify
    if docker info &> /dev/null 2>&1; then
        info "Docker daemon started"
    else
        error "Could not start Docker daemon."
        echo "    On macOS: Open Docker Desktop app"
        echo "    On Linux: sudo systemctl start docker"
        exit 1
    fi
}

# ─────────────────────────────────────────────────────────────────────────────
# Step 3: Set up .env file
# ─────────────────────────────────────────────────────────────────────────────
setup_env() {
    step "Checking configuration..."

    cd "${RUNTIME_DIR}"

    if [ -f .env ]; then
        # Check if API key is set
        if grep -q "YOUR_OPENROUTER_API_KEY_HERE" .env 2>/dev/null; then
            warn ".env exists but API key is not set"
        elif grep -qE "^OPENROUTER_API_KEY=sk-or-" .env 2>/dev/null; then
            info ".env configured with OpenRouter API key"
            return 0
        elif grep -qE "^OPENROUTER_API_KEY=.+" .env 2>/dev/null; then
            info ".env configured"
            return 0
        else
            warn ".env exists but OPENROUTER_API_KEY may be empty"
        fi
    else
        echo "  📝 Creating .env from template..."
        cp .env.example .env
    fi

    # Prompt for API key
    echo ""
    echo -e "  ${BOLD}You need a free OpenRouter API key:${NC}"
    echo -e "  ${CYAN}1.${NC} Go to ${BOLD}https://openrouter.ai/keys${NC}"
    echo -e "  ${CYAN}2.${NC} Sign up (free, no credit card)"
    echo -e "  ${CYAN}3.${NC} Click '+ Create Key' and copy it"
    echo ""
    read -p "  Paste your OpenRouter API key (sk-or-...): " API_KEY

    if [ -z "$API_KEY" ]; then
        error "No API key provided."
        echo "    Edit agent-runtime/.env and add your OPENROUTER_API_KEY, then re-run."
        exit 1
    fi

    # Write the key
    if grep -q "YOUR_OPENROUTER_API_KEY_HERE" .env 2>/dev/null; then
        sed -i "s|YOUR_OPENROUTER_API_KEY_HERE|${API_KEY}|" .env 2>/dev/null || \
        sed -i '' "s|YOUR_OPENROUTER_API_KEY_HERE|${API_KEY}|" .env
    else
        sed -i "s|^OPENROUTER_API_KEY=.*|OPENROUTER_API_KEY=${API_KEY}|" .env 2>/dev/null || \
        sed -i '' "s|^OPENROUTER_API_KEY=.*|OPENROUTER_API_KEY=${API_KEY}|" .env
    fi
    info "API key saved to .env"
}

# ─────────────────────────────────────────────────────────────────────────────
# Step 4: Build and start the agent
# ─────────────────────────────────────────────────────────────────────────────
build_and_start() {
    step "Building agent image..."

    cd "${RUNTIME_DIR}"
    mkdir -p docs

    # Stop any existing instance
    docker compose down 2>/dev/null || true

    docker compose build --quiet 2>&1
    info "Image built"

    step "Starting agent..."
    docker compose up -d 2>&1
    info "Container started"
}

# ─────────────────────────────────────────────────────────────────────────────
# Step 5: Wait for healthy + auto-approve pairing
# ─────────────────────────────────────────────────────────────────────────────
wait_for_healthy() {
    step "Waiting for agent to be ready (this takes ~30-60 seconds)..."

    cd "${RUNTIME_DIR}"

    for i in $(seq 1 60); do
        STATUS=$(docker compose ps --format '{{.Status}}' 2>/dev/null | head -1)
        if echo "$STATUS" | grep -q "healthy"; then
            info "Agent is healthy!"
            return 0
        fi
        # Check if container is still running
        if ! docker compose ps --format '{{.Status}}' 2>/dev/null | grep -q "Up"; then
            if [ "$i" -gt 5 ]; then
                error "Agent container stopped unexpectedly. Checking logs..."
                docker compose logs --tail 20 2>&1
                exit 1
            fi
        fi
        printf "."
        sleep 2
    done

    # If we get here, it didn't become healthy but might still be running
    warn "Agent is running but health check hasn't passed yet."
    echo "    This is normal on first start. Check: docker compose logs -f"
}

# ─────────────────────────────────────────────────────────────────────────────
# Step 6: Get dashboard URL and auto-approve
# ─────────────────────────────────────────────────────────────────────────────
show_dashboard() {
    cd "${RUNTIME_DIR}"

    # Read the token from .env (or use default)
    GW_TOKEN=$(grep -oP 'OPENCLAW_GATEWAY_TOKEN=\K.*' .env 2>/dev/null || echo "oneclick-local-dev")
    [ -z "$GW_TOKEN" ] && GW_TOKEN="oneclick-local-dev"
    PORT=$(grep -oP 'AGENT_PORT=\K.*' .env 2>/dev/null || echo "3000")
    [ -z "$PORT" ] && PORT="3000"

    DASHBOARD_URL="http://localhost:${PORT}/#token=${GW_TOKEN}"

    echo ""
    echo -e "  ${GREEN}${BOLD}══════════════════════════════════════════════${NC}"
    echo -e "  ${GREEN}${BOLD}   ✅  Your AI Agent is LIVE!${NC}"
    echo -e "  ${GREEN}${BOLD}══════════════════════════════════════════════${NC}"
    echo ""
    echo -e "  ${BOLD}🌐 Dashboard:${NC} ${CYAN}${DASHBOARD_URL}${NC}"
    echo ""
    echo -e "  ${BOLD}Quick commands:${NC}"
    echo "    ./scripts/agent.sh status    — Check agent health"
    echo "    ./scripts/agent.sh logs      — View live logs"
    echo "    ./scripts/agent.sh stop      — Stop the agent"
    echo "    ./scripts/agent.sh restart   — Restart the agent"
    echo ""
    echo -e "  ${YELLOW}📌 First time?${NC} Open the dashboard URL above."
    echo "     If it says 'Pairing required', run:"
    echo "       ./scripts/agent.sh approve"
    echo "     then refresh the browser."
    echo ""
}

# ─────────────────────────────────────────────────────────────────────────────
# Main
# ─────────────────────────────────────────────────────────────────────────────
header
install_docker
start_docker
setup_env
build_and_start
wait_for_healthy
show_dashboard
