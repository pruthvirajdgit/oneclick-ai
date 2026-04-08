#!/usr/bin/env bash
# ===========================================================================
# OneClick.ai — E2E Backend Test Runner
#
# Starts the required infrastructure (PostgreSQL + Redis), runs the E2E
# integration tests, and reports results. Supports both Docker-managed
# and externally-running databases.
#
# Usage:
#   ./tests/run_e2e.sh              # Uses docker compose for infra
#   ./tests/run_e2e.sh --no-infra   # Assumes PG + Redis already running
#
# Environment variables:
#   DATABASE_URL  — Override the test database connection string.
#   REDIS_URL     — Override the Redis connection string.
# ===========================================================================

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
BACKEND_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
ROOT_DIR="$(cd "$BACKEND_DIR/.." && pwd)"

# Defaults
DB_URL="${DATABASE_URL:-postgres://oneclick:devpassword@localhost:5432/oneclick_test}"
REDIS_URL_VAR="${REDIS_URL:-redis://127.0.0.1:6379}"
MANAGE_INFRA=true

# Parse arguments
for arg in "$@"; do
    case "$arg" in
        --no-infra) MANAGE_INFRA=false ;;
        --help|-h)
            echo "Usage: $0 [--no-infra]"
            echo ""
            echo "Options:"
            echo "  --no-infra   Skip starting/stopping PostgreSQL and Redis."
            echo "               Assumes they are already running."
            echo ""
            echo "Environment:"
            echo "  DATABASE_URL  Test database URL (default: postgres://oneclick:devpassword@localhost:5432/oneclick_test)"
            echo "  REDIS_URL     Redis URL (default: redis://127.0.0.1:6379)"
            exit 0
            ;;
    esac
done

# ── Colors ──────────────────────────────────────────────────────────────
GREEN='\033[0;32m'
RED='\033[0;31m'
YELLOW='\033[0;33m'
CYAN='\033[0;36m'
NC='\033[0m'

info()  { echo -e "${CYAN}[INFO]${NC}  $*"; }
ok()    { echo -e "${GREEN}[OK]${NC}    $*"; }
warn()  { echo -e "${YELLOW}[WARN]${NC}  $*"; }
fail()  { echo -e "${RED}[FAIL]${NC}  $*"; }

# ── Infrastructure ──────────────────────────────────────────────────────

start_infra() {
    if [ "$MANAGE_INFRA" = true ]; then
        info "Starting PostgreSQL and Redis via docker compose..."
        cd "$ROOT_DIR"
        docker compose up -d postgres redis 2>&1 | tail -3

        info "Waiting for PostgreSQL to be ready..."
        local retries=30
        until docker compose exec -T postgres pg_isready -U oneclick -d oneclick_test >/dev/null 2>&1 || [ $retries -eq 0 ]; do
            retries=$((retries - 1))
            sleep 1
        done

        if [ $retries -eq 0 ]; then
            fail "PostgreSQL did not become ready in 30 seconds"
            exit 1
        fi

        # Create the test database if it doesn't exist.
        docker compose exec -T postgres psql -U oneclick -d postgres \
            -c "SELECT 1 FROM pg_database WHERE datname = 'oneclick_test'" | grep -q 1 || \
        docker compose exec -T postgres psql -U oneclick -d postgres \
            -c "CREATE DATABASE oneclick_test OWNER oneclick;" 2>/dev/null || true

        info "Waiting for Redis to be ready..."
        retries=15
        until docker compose exec -T redis redis-cli ping 2>/dev/null | grep -q PONG || [ $retries -eq 0 ]; do
            retries=$((retries - 1))
            sleep 1
        done

        if [ $retries -eq 0 ]; then
            fail "Redis did not become ready in 15 seconds"
            exit 1
        fi

        ok "Infrastructure ready"
    else
        info "Skipping infrastructure setup (--no-infra)"
    fi
}

# ── Run Tests ───────────────────────────────────────────────────────────

run_tests() {
    cd "$BACKEND_DIR"

    info "Running E2E workflow tests..."
    echo ""

    export DATABASE_URL="$DB_URL"
    export REDIS_URL="$REDIS_URL_VAR"

    if cargo test --test e2e_workflow --features integration -- --test-threads=1 2>&1; then
        echo ""
        ok "All E2E tests passed! ✅"
    else
        echo ""
        fail "Some E2E tests failed ❌"
        exit 1
    fi
}

# ── Main ────────────────────────────────────────────────────────────────

echo ""
echo "════════════════════════════════════════════════════════════"
echo "  OneClick.ai — E2E Backend Test Runner"
echo "════════════════════════════════════════════════════════════"
echo ""

start_infra
run_tests

echo ""
echo "════════════════════════════════════════════════════════════"
echo "  E2E tests completed successfully"
echo "════════════════════════════════════════════════════════════"
echo ""
