#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/../.."

echo "🛑 Stopping OneClick.ai..."

# 1. Stop backend
BACKEND_PIDS=$(pgrep -f 'oneclick-backend' 2>/dev/null || true)
if [ -n "$BACKEND_PIDS" ]; then
  echo "$BACKEND_PIDS" | xargs kill 2>/dev/null || true
  echo "✅ Backend stopped"
else
  echo "⏭️  Backend not running"
fi

# 2. Stop any Firecracker VMs
FC_PIDS=$(pgrep -f 'firecracker' 2>/dev/null || true)
if [ -n "$FC_PIDS" ]; then
  echo "$FC_PIDS" | xargs kill 2>/dev/null || true
  sudo rm -f /tmp/fc-*.socket 2>/dev/null || true
  echo "✅ Firecracker VMs stopped"
else
  echo "⏭️  No Firecracker VMs running"
fi

# 3. Clean up TAP devices
for tap in $(ip -o link show 2>/dev/null | grep tap | awk -F: '{print $2}' | tr -d ' '); do
  sudo ip link delete "$tap" 2>/dev/null || true
done

# 4. Stop Docker services
echo "📦 Stopping Docker services..."
sudo docker compose down 2>&1 | tail -n 5

echo ""
echo "🎉 All services stopped."
