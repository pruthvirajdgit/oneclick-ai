# Firecracker MicroVM PoC

Standalone proof-of-concept for OneClick.ai: boot, snapshot-sleep, snapshot-wake Firecracker microVMs with bidirectional networking and OpenClaw AI gateway.

**Built with [fctools](https://crates.io/crates/fctools)** — the official Firecracker Rust SDK from the rust-firecracker org.

## Results

### Stage 1 — Basic VM (busybox httpd)

| Operation | Time |
|-----------|------|
| Cold boot (kernel + init + httpd) | ~1.0s |
| Snapshot restore | **~105ms** |
| Health check after restore | ~25ms |
| 5-cycle stress test | All pass, max 106ms |

### Stage 2 — OpenClaw in Firecracker

| Operation | Time |
|-----------|------|
| Cold boot (kernel + OpenClaw gateway + bridge) | ~30s |
| Snapshot restore | **~10-12ms** |
| Chat after restore | Immediate |
| 5-cycle stress test | 5/5 restores pass, 10-12ms each |

Target was <500ms restore — achieved **10-12ms** for OpenClaw VMs.

## Prerequisites

- Linux host with KVM support (`/dev/kvm`)
- Firecracker v1.12+ installed at `/usr/local/bin/firecracker`
- Rust toolchain
- `sudo` access (for TAP networking)
- `debootstrap` (for Stage 1 rootfs)
- Docker (for Stage 2 rootfs — exports `oneclick-agent:latest` image)
- Groq API key (for Stage 2 LLM chat)

## Quick Start — Stage 1 (Basic)

```bash
# 1. Build rootfs (one-time, requires sudo + debootstrap)
sudo bash scripts/build-rootfs.sh

# 2. Download kernel (one-time)
curl -sL "https://s3.amazonaws.com/spec.ccfc.min/firecracker-ci/v1.12/x86_64/vmlinux-6.1.128" \
  -o resources/vmlinux-6.1

# 3. Set up TAP networking (requires sudo)
bash scripts/setup-network.sh tap0

# 4. Run the full lifecycle
cargo run --release -- start    # Create + boot + wait for health
cargo run --release -- check    # Verify HTTP health
cargo run --release -- stop     # Snapshot to disk + kill
cargo run --release -- wake     # Restore from snapshot (fctools)
cargo run --release -- check    # Verify still healthy
cargo run --release -- destroy  # Clean up

# 5. Run full lifecycle in one process (100% fctools)
cargo run --release -- lifecycle

# 6. Run 5-cycle stress test in one process (100% fctools)
cargo run --release -- stress

# 7. Tear down networking
bash scripts/teardown-network.sh tap0
```

## Quick Start — Stage 2 (OpenClaw)

```bash
# 1. Build the Docker agent image first (from repo root)
cd /path/to/oneclick-ai && docker compose build oneclick-runtime

# 2. Build OpenClaw rootfs (exports Docker image → ext4)
source .env  # needs GROQ_API_KEY
cd local_poc/firecracker
bash scripts/build-rootfs-openclaw.sh oneclick-agent:latest "$GROQ_API_KEY"

# 3. Set up networking
sudo bash scripts/setup-network.sh tap0

# 4. Run with openclaw profile
cargo run --release -- --profile openclaw create
cargo run --release -- --profile openclaw start
cargo run --release -- --profile openclaw check

# 5. Test chat
cargo run --release -- --profile openclaw chat "Hello, what is 2+2?"

# 6. Snapshot sleep/wake cycle
cargo run --release -- --profile openclaw stop    # Snapshot + kill
cargo run --release -- --profile openclaw wake    # Restore in ~12ms
cargo run --release -- --profile openclaw chat "Are you still there?"

# 7. Stress test
cargo run --release -- --profile openclaw stress
cargo run --release -- --profile openclaw destroy
```

## Architecture

### Stage 1

```text
Host (172.16.0.1)          Firecracker VM (172.16.0.2)
┌──────────────┐           ┌──────────────────────┐
│ Rust CLI     │           │ Linux 6.1 kernel     │
│              │  TAP/tap0 │                      │
│ fctools::Vm  ├───────────┤ eth0 (virtio-net)    │
│   Vm::prepare│           │                      │
│   vm.start() │  HTTP     │ busybox httpd :8080   │
│ health_check ├───────────┤  /health → JSON      │
└──────┬───────┘           └──────────────────────┘
       │
       ▼
  Firecracker API (Unix socket, via fctools)
```

### Stage 2

```text
Host (172.16.0.1)          Firecracker VM (172.16.0.2)
┌──────────────┐           ┌──────────────────────────┐
│ Rust CLI     │           │ Linux 6.1 kernel         │
│              │  TAP/tap0 │                          │
│ fctools::Vm  ├───────────┤ eth0 (virtio-net)        │
│   Vm::prepare│           │                          │
│   vm.start() │  :3001    │ chat-bridge.js (:3001)   │
│ health_check ├───────────┤   ↕ WebSocket            │
│   (TCP)      │           │ OpenClaw gateway (:3000) │
│              │  :3000    │   ↕ HTTPS                │
│ cmd_chat()   ├───────────┤ Groq/OpenRouter API      │
│   (SSE)      │           │                          │
│              │           │ pair-device.js (auto)     │
└──────┬───────┘           └──────────────────────────┘
       │
       ▼
  Firecracker API (Unix socket, via fctools)
```

## CLI Commands

| Command | Description |
|---------|-------------|
| `create`/`start` | Create + start VM via fctools `Vm::prepare` + `vm.start()` |
| `check` | Verify health endpoint responds (TCP) |
| `stop` | Pause VM → full snapshot → kill (HTTP fallback for cross-process) |
| `wake` | Restore from snapshot via fctools `VmConfiguration::RestoredFromSnapshot` |
| `destroy` | Kill Firecracker, delete snapshots |
| `lifecycle` | Full lifecycle in one process (100% fctools) |
| `stress` | Run 5 consecutive stop/wake cycles in one process (100% fctools) |
| `chat <msg>` | Send chat message via bridge SSE endpoint (openclaw profile) |

Use `--profile openclaw` for Stage 2 (default is `basic` for Stage 1).

## Networking

TAP device `tap0` bridges host and VM:
- Host: 172.16.0.1/30
- VM: 172.16.0.2/30 (configured by init script)
- NAT via iptables MASQUERADE for internet access

## Rootfs

### Stage 1 — Minimal (400MB)
Built with debootstrap (Debian bookworm): busybox httpd, iproute2, python3-minimal.

### Stage 2 — OpenClaw (4GB)
Exported from `oneclick-agent:latest` Docker image: Node.js 24, OpenClaw 2026.4.5, chat-bridge.js, pair-device.js.

Key rootfs fixes applied by build script:
- `/bin/bash` → `/usr/bin/bash` symlink (Docker image doesn't have `/bin/bash`)
- Static busybox binary for `ip` command (iproute2 not in Docker image)
- Custom `/sbin/fc-init` as PID 1 (no systemd, writes OpenClaw config directly)

## Snapshot Format

- `snapshots/vm.snap` — VM state (~29KB)
- `snapshots/vm.mem` — Full memory dump (256MB basic, 1.5GB openclaw)

## Troubleshooting

### Kernel panic "init failed (error -2)"
The shebang `#!/bin/bash` fails because `/bin/bash` doesn't exist. Create symlink: `ln -sf /usr/bin/bash /bin/bash`

### Network: ARP "(incomplete)" for 172.16.0.2
The `ip` command is missing from rootfs. The build script installs static busybox and creates `/sbin/ip` symlink.

### OpenClaw: Rate limit errors
Groq free tier has low TPM limits. Use `meta-llama/llama-4-scout-17b-16e-instruct` (30K TPM) with `native: false` to minimize system prompt tokens.

### Chat bridge: "Gateway not connected"
Wait ~30s after cold boot for gateway to finish starting. After snapshot restore, the bridge reconnects instantly.

## Files

```
local_poc/firecracker/
├── Cargo.toml
├── README.md
├── src/
│   └── main.rs                    # Rust CLI using fctools SDK
├── scripts/
│   ├── build-rootfs.sh            # Stage 1: minimal Debian rootfs
│   ├── build-rootfs-openclaw.sh   # Stage 2: OpenClaw rootfs from Docker
│   ├── setup-network.sh           # Create TAP device + NAT
│   └── teardown-network.sh
├── resources/
│   ├── vmlinux-6.1                # Firecracker CI kernel (download)
│   ├── rootfs.ext4                # Stage 1 rootfs (build-rootfs.sh)
│   └── rootfs-openclaw.ext4       # Stage 2 rootfs (build-rootfs-openclaw.sh)
├── snapshots/                     # Stage 1 VM snapshots
└── snapshots-openclaw/            # Stage 2 VM snapshots
```
