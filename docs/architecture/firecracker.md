# Firecracker MicroVM Architecture

## Overview

OneClick.ai uses Firecracker microVMs as an alternative to Docker containers for running AI agents. Firecracker provides hardware-level VM isolation with near-instant (~116ms) snapshot restore, enabling true scale-to-zero with sub-second wake times.

## Why Firecracker?

| Metric | Docker | Firecracker |
|--------|--------|-------------|
| Wake from sleep | 5-10s (start) | **116ms** (snapshot restore) |
| Cold boot | 5-7 min (gateway JIT) | 1.1s (VM) + 26s (gateway) |
| Isolation | Container (cgroups/namespaces) | Hardware VM (KVM) |
| Snapshot | CRIU (complex, fragile) | Native VM snapshots |
| Memory overhead | Shared kernel | Dedicated kernel (~20MB) |

## Architecture

```
Host Machine
├── Backend (Rust binary, port 8080)
│   ├── FirecrackerRuntime (fctools SDK)
│   └── TapManager (tap0-tap15 pool)
│
├── TAP Network
│   ├── tap0 ── 172.16.0.1/30 ──→ VM-1 (172.16.0.2)
│   ├── tap1 ── 172.16.0.5/30 ──→ VM-2 (172.16.0.6)
│   └── tapN ── 172.16.0.{4N+1}/30 ──→ VM-N (172.16.0.{4N+2})
│
├── /var/lib/oneclick/vms/
│   ├── fc-{uuid-1}.ext4    (per-VM rootfs copy, 4GB)
│   └── fc-{uuid-2}.ext4
│
├── /var/lib/oneclick/snapshots/
│   ├── fc-{uuid-1}/
│   │   ├── mem_file         (VM memory, ~1.5GB)
│   │   └── snapshot_file    (VM state)
│   └── fc-{uuid-2}/
│
└── Firecracker processes (one per running VM)
    ├── firecracker --api-sock /tmp/fc-{uuid-1}.socket
    └── firecracker --api-sock /tmp/fc-{uuid-2}.socket
```

## VM Lifecycle

### Create
1. Copy rootfs template (`cp --reflink=auto` for CoW on supported FS)
2. Allocate TAP device from pool
3. Mount rootfs, write per-VM config:
   - `/etc/fc-network`: guest IP, gateway, nameserver
   - `/etc/openclaw-env`: OpenClaw config (API keys, model, gateway token)
4. Unmount rootfs
5. Return VM ID: `fc-{agent_uuid}`

### Cold Boot (first start)
1. Start Firecracker process with API socket
2. Configure VM via fctools: kernel, rootfs, network interface
3. Boot VM
4. VM init script reads `/etc/fc-network`, configures networking
5. VM init script reads `/etc/openclaw-env`, starts OpenClaw + chat-bridge
6. Backend polls health check (TCP probe to guest_ip:3001)
7. Total time: ~1.1s to health check pass, ~26s until gateway ready for chat

### Snapshot Sleep
1. Pause VM (PATCH /vm → Paused)
2. Create snapshot: save memory + VM state to disk
3. Store `VmSnapshot` in memory (for fast restore)
4. Shutdown VM process
5. Total time: ~12s

### Snapshot Wake
1. Start new Firecracker process
2. Load snapshot from in-memory `VmSnapshot` (or from disk)
3. Resume VM
4. Health check passes immediately (processes were frozen mid-execution)
5. **Total time: ~116ms** 🚀

### Destroy
1. Shutdown VM process (if running)
2. Release TAP device back to pool
3. Delete rootfs copy and snapshot files

## Rootfs Template

The rootfs template is a 4GB ext4 filesystem containing:
- Debian base system
- Node.js 22 LTS
- OpenClaw (from `oneclick-agent:latest` Docker image)
- chat-bridge.js + pair-device.js
- Parameterized init script

The init script (`/sbin/init`) at boot:
1. Mounts procfs, sysfs, devpts
2. Reads `/etc/fc-network` → configures eth0 IP, gateway, DNS
3. Reads `/etc/openclaw-env` → exports environment variables
4. Generates OpenClaw config (`~/.openclaw/openclaw.json`)
5. Starts OpenClaw gateway (port 3000)
6. Starts chat-bridge.js (port 3001)
7. Starts pair-device.js (auto-approves device pairing)

Build with: `sudo bash local_poc/firecracker/scripts/build-rootfs-template.sh`

## TAP Networking

Each VM gets a /30 subnet on a dedicated TAP interface:

| Index | TAP | Host IP | Guest IP | MAC |
|-------|-----|---------|----------|-----|
| 0 | tap0 | 172.16.0.1 | 172.16.0.2 | AA:FC:00:00:00:00 |
| 1 | tap1 | 172.16.0.5 | 172.16.0.6 | AA:FC:00:00:00:01 |
| N | tapN | 172.16.0.{4N+1} | 172.16.0.{4N+2} | AA:FC:00:00:00:{hex(N)} |

IP forwarding and iptables MASQUERADE provide outbound NAT for VMs.

The backend communicates directly with VMs via their TAP IP addresses.

## fctools SDK Usage

The production backend uses fctools 0.7.0-alpha.1 (Rust crate) for all Firecracker operations:

```rust
// Cold boot
let vm = Vm::prepare(config, resource_system, executor).await?;
let vm = vm.start(boot_source, machine_config, network_interfaces).await?;

// Snapshot
vm.pause().await?;
vm.create_snapshot(mem_path, snapshot_path).await?;
vm.shutdown(actions).await;

// Restore
let vm = Vm::restore_from_snapshot(config, snapshot, resource_system, executor).await?;
vm.resume().await?;
```

Key details:
- `MachineConfiguration.vcpu_count` is `u8` (not `u32`)
- VM socket created by Firecracker as root — needs `chmod 666` for fctools access
- `VmSnapshot` is not Clone — stored as `Option<VmSnapshot>` and taken on restore

## Known Limitations

1. **In-memory snapshots lost on backend restart** — need to implement on-disk snapshot recovery
2. **No jailer** — VMs run without Firecracker's security jailer (chroot, seccomp, cgroups)
3. **16 VM limit** — TAP pool is fixed at 16 devices (configurable but not dynamically expandable)
4. **4GB per rootfs** — each VM gets a full copy; CoW only helps on btrfs/xfs
5. **1.5GB per snapshot** — disk usage scales linearly with VM count
6. **Conversation memory not persisted** — OpenClaw's in-memory conversation cache is lost on sleep
