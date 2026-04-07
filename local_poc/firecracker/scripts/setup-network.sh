#!/bin/bash
# Set up TAP networking for Firecracker VM
# Host: 172.16.0.1, VM: 172.16.0.2, /30 subnet
set -euo pipefail

TAP_DEV="${1:-tap0}"
HOST_IP="172.16.0.1"
VM_IP="172.16.0.2"
SUBNET_MASK="255.255.255.252"

echo "=== Setting up TAP network ==="
echo "TAP device: $TAP_DEV"
echo "Host IP: $HOST_IP"
echo "VM IP: $VM_IP"

# Create TAP device
sudo ip tuntap add dev "$TAP_DEV" mode tap user "$(whoami)"
sudo ip addr add "${HOST_IP}/30" dev "$TAP_DEV"
sudo ip link set "$TAP_DEV" up

# Enable IP forwarding
sudo sysctl -w net.ipv4.ip_forward=1 > /dev/null

# Set up NAT so VM can reach the internet
# Find the default route interface
DEFAULT_IF=$(ip route | grep default | awk '{print $5}' | head -1)
echo "Default interface: $DEFAULT_IF"

sudo iptables -t nat -A POSTROUTING -o "$DEFAULT_IF" -j MASQUERADE
sudo iptables -A FORWARD -m conntrack --ctstate RELATED,ESTABLISHED -j ACCEPT
sudo iptables -A FORWARD -i "$TAP_DEV" -o "$DEFAULT_IF" -j ACCEPT

echo ""
echo "=== Network setup complete ==="
echo "Host can reach VM at $VM_IP"
echo "VM should configure eth0 as $VM_IP with gateway $HOST_IP"
ip addr show "$TAP_DEV"
