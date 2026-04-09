#!/bin/bash
# Tear down TAP networking for Firecracker VM
set -euo pipefail

TAP_DEV="${1:-tap0}"

echo "=== Tearing down TAP network ==="

# Remove iptables rules (best effort)
DEFAULT_IF=$(ip route | grep default | awk '{print $5}' | head -1)
sudo iptables -t nat -D POSTROUTING -o "$DEFAULT_IF" -j MASQUERADE 2>/dev/null || true
sudo iptables -D FORWARD -m conntrack --ctstate RELATED,ESTABLISHED -j ACCEPT 2>/dev/null || true
sudo iptables -D FORWARD -i "$TAP_DEV" -o "$DEFAULT_IF" -j ACCEPT 2>/dev/null || true

# Remove TAP device
sudo ip link set "$TAP_DEV" down 2>/dev/null || true
sudo ip tuntap del dev "$TAP_DEV" mode tap 2>/dev/null || true

echo "TAP network torn down"
