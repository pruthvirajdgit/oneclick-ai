#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")"

echo "🚀 Starting OneClick.ai..."

# 1. Start postgres, redis, frontend in Docker
echo "📦 Starting Docker services (postgres, redis, frontend)..."
sudo docker compose up -d --quiet-pull 2>&1 | tail -n 5

# 2. Wait for postgres to be ready
echo "⏳ Waiting for PostgreSQL..."
until PGPASSWORD=oneclick psql -h 127.0.0.1 -U oneclick -d oneclick -c '\q' 2>/dev/null; do
  sleep 1
done
echo "✅ PostgreSQL is ready"

# 3. Start backend on host
echo "🦀 Starting backend..."
./backend/target/release/oneclick-backend > /dev/null 2>&1 &
BACKEND_PID=$!
echo "✅ Backend started (PID: $BACKEND_PID)"

# 4. Wait for backend to be ready
echo "⏳ Waiting for backend..."
until curl -sf http://localhost:8080/api-docs/openapi.json > /dev/null 2>&1; do
  sleep 1
done
echo "✅ Backend is ready"

echo ""
echo "🎉 All services running!"
echo "   Frontend:  http://localhost:3000"
echo "   Backend:   http://localhost:8080"
echo "   Swagger:   http://localhost:8080/swagger-ui"
