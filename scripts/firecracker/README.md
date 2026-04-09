# Firecracker Infrastructure

Scripts for building and managing Firecracker VM images.

## Scripts

### `scripts/build-rootfs.sh [docker-image]`
Builds the rootfs template used by the backend to create agent VMs.
Exports the `oneclick-agent:latest` Docker image into an ext4 filesystem
with a parameterized init script (`fc-init`).

**Prerequisites:**
- Docker image `oneclick-agent:latest` must be built first
- Must run as root (or with sudo)

**Output:** `$FC_ROOTFS_TEMPLATE` (default: `/opt/firecracker/rootfs-openclaw.ext4`)

### `scripts/setup-network.sh [tap-device]`
Sets up TAP networking for a single Firecracker VM (manual/debug use).
The backend's `TapManager` handles this automatically in production.

### `scripts/teardown-network.sh [tap-device]`
Tears down TAP networking for a single VM.
