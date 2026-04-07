#!/bin/bash
# Build a minimal ext4 rootfs for Firecracker with Python HTTP server
# Two-phase: debootstrap into dir, then pack into ext4
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"
RESOURCES_DIR="${PROJECT_DIR}/resources"
ROOTFS_IMG="${RESOURCES_DIR}/rootfs.ext4"
ROOTFS_SIZE_MB=400
BUILD_DIR="/tmp/fc-rootfs-build"
MOUNT_DIR="/tmp/fc-rootfs-mount"

echo "=== Building Firecracker rootfs ==="

# Clean up any previous build
sudo umount "$MOUNT_DIR" 2>/dev/null || true
rm -f "$ROOTFS_IMG"
sudo rm -rf "$BUILD_DIR" "$MOUNT_DIR"

# Phase 1: debootstrap into a directory (avoids ext4 size issues during extraction)
echo "[1/5] Running debootstrap (Debian bookworm, minbase)..."
sudo mkdir -p "$BUILD_DIR"
sudo debootstrap --variant=minbase --include=busybox,iproute2,python3-minimal \
    bookworm "$BUILD_DIR" http://deb.debian.org/debian

# Phase 2: Configure the rootfs
echo "[2/5] Configuring rootfs..."

echo "fc-vm" | sudo tee "$BUILD_DIR/etc/hostname" > /dev/null
echo "nameserver 8.8.8.8" | sudo tee "$BUILD_DIR/etc/resolv.conf" > /dev/null

# Create init script (PID 1 - no systemd)
cat << 'INITEOF' | sudo tee "$BUILD_DIR/sbin/fc-init" > /dev/null
#!/bin/bash
# Minimal init for Firecracker VM (PID 1)
mount -t proc proc /proc
mount -t sysfs sysfs /sys
mount -t devtmpfs devtmpfs /dev

hostname fc-vm

# Configure network
ip addr add 172.16.0.2/30 dev eth0 2>/dev/null
ip link set eth0 up 2>/dev/null
ip route add default via 172.16.0.1 2>/dev/null

# Start HTTP server in background
echo "Starting HTTP server on port 8080..."
cd /var/www && python3 -m http.server 8080 --bind 0.0.0.0 &

echo "=== Firecracker VM boot complete ==="

# Keep init running
while true; do sleep 3600; done
INITEOF
sudo chmod +x "$BUILD_DIR/sbin/fc-init"

# Create web root
sudo mkdir -p "$BUILD_DIR/var/www"
echo '{"status":"ok","vm":"firecracker-poc"}' | sudo tee "$BUILD_DIR/var/www/health" > /dev/null
echo '<h1>Firecracker VM</h1>' | sudo tee "$BUILD_DIR/var/www/index.html" > /dev/null

# Phase 3: Create ext4 image and copy content
echo "[3/5] Creating ${ROOTFS_SIZE_MB}MB ext4 image..."
dd if=/dev/zero of="$ROOTFS_IMG" bs=1M count=$ROOTFS_SIZE_MB status=progress
mkfs.ext4 -F "$ROOTFS_IMG"

echo "[4/5] Copying rootfs into image..."
sudo mkdir -p "$MOUNT_DIR"
sudo mount -o loop "$ROOTFS_IMG" "$MOUNT_DIR"
sudo cp -a "$BUILD_DIR"/* "$MOUNT_DIR"/

echo "[5/5] Cleaning up..."
sudo umount "$MOUNT_DIR"
sudo rm -rf "$BUILD_DIR" "$MOUNT_DIR"

echo ""
echo "=== Rootfs built successfully ==="
echo "Image: $ROOTFS_IMG"
ls -lh "$ROOTFS_IMG"
