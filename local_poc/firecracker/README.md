# Firecracker MicroVM PoC

Standalone proof-of-concept: boot, snapshot-sleep, snapshot-wake a Firecracker microVM with bidirectional networking and HTTP health check.

## Results

| Operation | Time |
|-----------|------|
| Cold boot (kernel + init + httpd) | ~1.0s |
| Snapshot restore | **~105ms** |
| Health check after restore | ~25ms |
| 5-cycle stress test | All pass, max 106ms |

Target was <500ms restore — achieved **~105ms** consistently.

## Prerequisites

- Linux host with KVM support (`/dev/kvm`)
- Firecracker v1.12+ installed at `/usr/local/bin/firecracker`
- Rust toolchain
- `sudo` access (for TAP networking)
- `debootstrap` (for building rootfs)

## Quick Start

```bash
# 1. Build rootfs (one-time, requires sudo + debootstrap)
sudo bash scripts/build-rootfs.sh

# 2. Download kernel (one-time)
curl -sL "https://s3.amazonaws.com/spec.ccfc.min/firecracker-ci/v1.12/x86_64/vmlinux-6.1.128" \
  -o resources/vmlinux-6.1

# 3. Set up TAP networking (requires sudo)
bash scripts/setup-network.sh tap0

# 4. Run the full lifecycle
cargo run --release -- create   # Configure VM
cargo run --release -- start    # Boot + wait for health
cargo run --release -- check    # Verify HTTP health
cargo run --release -- stop     # Snapshot to disk + kill
cargo run --release -- wake     # Restore from snapshot
cargo run --release -- check    # Verify still healthy
cargo run --release -- destroy  # Clean up

# 5. Run 5-cycle stress test
cargo run --release -- create
cargo run --release -- start
cargo run --release -- stress
cargo run --release -- destroy

# 6. Tear down networking
bash scripts/teardown-network.sh tap0
```

## Architecture

```
Host (172.16.0.1)          Firecracker VM (172.16.0.2)
┌──────────────┐           ┌──────────────────────┐
│ Rust CLI     │           │ Linux 6.1 kernel     │
│              │  TAP/tap0 │                      │
│ fc_request() ├───────────┤ eth0 (virtio-net)    │
│   (Unix sock)│           │                      │
│              │  HTTP     │ busybox httpd :8080   │
│ health_check ├───────────┤  /health → JSON      │
│   (TCP)      │           │  /index.html         │
└──────┬───────┘           └──────────────────────┘
       │
       ▼
  Firecracker API
  (Unix socket)
  /tmp/fc-poc.socket
```

## CLI Commands

| Command | Description |
|---------|-------------|
| `create` | Start Firecracker process, configure VM (kernel, rootfs, network) |
| `start` | Send InstanceStart, wait for HTTP health check |
| `check` | Verify HTTP server responds at 172.16.0.2:8080 |
| `stop` | Pause VM → full snapshot → kill Firecracker |
| `wake` | New Firecracker process → load snapshot → resume |
| `destroy` | Kill Firecracker, delete snapshots |
| `stress` | Run 5 consecutive stop/wake cycles, report times |

## Networking

TAP device `tap0` bridges host and VM:
- Host: 172.16.0.1/30
- VM: 172.16.0.2/30 (configured by init script)
- NAT via iptables MASQUERADE for internet access

## Rootfs

Minimal Debian bookworm (400MB ext4) built with debootstrap:
- busybox (httpd for health check)
- iproute2 (network config)
- python3-minimal (for future use)
- Custom `/sbin/fc-init` as PID 1 (no systemd)

## Snapshot Format

- `snapshots/vm.snap` — VM state (29KB)
- `snapshots/vm.mem` — Full memory dump (256MB = mem_size_mib)

## Manual Boot (curl commands)

```bash
# Start Firecracker
touch /tmp/fc.log
firecracker --api-sock /tmp/fc.socket --log-path /tmp/fc.log --level Info &

# Configure
RESOURCES=$(pwd)/resources
curl --unix-socket /tmp/fc.socket -X PUT http://localhost/boot-source \
  -H "Content-Type: application/json" \
  -d "{\"kernel_image_path\":\"$RESOURCES/vmlinux-6.1\",\"boot_args\":\"console=ttyS0 reboot=k panic=1 pci=off init=/sbin/fc-init\"}"

curl --unix-socket /tmp/fc.socket -X PUT http://localhost/drives/rootfs \
  -H "Content-Type: application/json" \
  -d "{\"drive_id\":\"rootfs\",\"path_on_host\":\"$RESOURCES/rootfs.ext4\",\"is_root_device\":true,\"is_read_only\":false}"

curl --unix-socket /tmp/fc.socket -X PUT http://localhost/machine-config \
  -H "Content-Type: application/json" \
  -d '{"vcpu_count":2,"mem_size_mib":256}'

curl --unix-socket /tmp/fc.socket -X PUT http://localhost/network-interfaces/eth0 \
  -H "Content-Type: application/json" \
  -d '{"iface_id":"eth0","guest_mac":"AA:FC:00:00:00:01","host_dev_name":"tap0"}'

# Boot
curl --unix-socket /tmp/fc.socket -X PUT http://localhost/actions \
  -H "Content-Type: application/json" -d '{"action_type":"InstanceStart"}'

# Test
sleep 2
curl http://172.16.0.2:8080/health
```

## Files

```
local_poc/firecracker/
├── Cargo.toml
├── README.md
├── src/
│   └── main.rs          # Rust CLI (create/start/stop/wake/check/destroy/stress)
├── scripts/
│   ├── build-rootfs.sh  # Build minimal Debian ext4 rootfs
│   ├── setup-network.sh # Create TAP device + NAT
│   └── teardown-network.sh
├── resources/
│   ├── vmlinux-6.1      # Firecracker CI kernel (download)
│   └── rootfs.ext4      # Built by build-rootfs.sh
└── snapshots/           # VM state + memory snapshots
```
