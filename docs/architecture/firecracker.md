# Firecracker MicroVM Architecture

## Overview

OneClick.ai uses Firecracker microVMs as the **primary runtime** for running AI agents (selected via `AGENT_RUNTIME=firecracker`). Docker is available as an alternative runtime. Firecracker provides hardware-level VM isolation with ~400ms snapshot restore, enabling true scale-to-zero with sub-second wake times.

## Why Firecracker?

| Metric | Docker | Firecracker |
|--------|--------|-------------|
| Wake from sleep | 5-10s (start) | **~400ms** (snapshot restore) |
| Cold boot | 5-7 min (gateway JIT) | ~3s (healthy) + ~40s (gateway JIT) |
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
1. Clean up any orphaned Firecracker processes for this agent
2. Start Firecracker process with API socket
3. Configure VM via fctools: kernel, rootfs, network interface
4. Boot VM
5. VM init script reads `/etc/fc-network`, configures networking
6. VM init script reads `/etc/openclaw-env`, starts OpenClaw + chat-bridge
7. Backend polls health check (TCP probe to guest_ip:3001)
8. Total time: ~3s to health check pass, ~40s until gateway ready for chat (JIT)

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
5. **Total time: ~400ms** 🚀

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

Build with: `sudo bash scripts/firecracker/build-rootfs.sh`

Template location: `/opt/firecracker/rootfs-openclaw.ext4`

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

1. **In-memory snapshots lost on backend restart** — need to implement on-disk snapshot recovery. TAP allocations are also in-memory but auto-re-allocated on next `start_agent()`.
2. **No jailer** — VMs run without Firecracker's security jailer (chroot, seccomp, cgroups)
3. **16 VM limit** — TAP pool is fixed at 16 devices (configurable but not dynamically expandable)
4. **4GB per rootfs** — each VM gets a full copy; CoW only helps on btrfs/xfs
5. **1.5GB per snapshot** — disk usage scales linearly with VM count
6. **Conversation memory not persisted** — OpenClaw's in-memory conversation cache is lost on sleep
7. **Stale agent status after restart** — agents may show `running` in DB but have no VM. Status should be reset to `stopped` manually or via startup reconciliation (not yet implemented).

## OpenClaw Configuration

The `contextTokens` setting in OpenClaw controls the maximum context window size. This must be set to at least **65536** (65K) because OpenClaw's system prompt + MCP tools definition exceeds 16K tokens. With the default 16384, every chat fails with "Context limit exceeded".

Set in 4 locations:
- `local_poc/firecracker/scripts/build-rootfs-template.sh` (2 occurrences — build time)
- Rootfs `/usr/sbin/fc-init` (runtime config generation)
- Rootfs `/usr/local/bin/oneclick-entrypoint.sh` (runtime config generation)

## Agent States

```
creating → stopped → running → stopped (sleep) → running (wake) → deleted
```

- `creating`: Rootfs copied, TAP allocated, config injected
- `stopped`: VM not running, rootfs on disk (possibly with snapshot)
- `running`: Firecracker process active, VM booted or restored
- `deleted`: All resources cleaned up (rootfs, snapshots, TAP released)

## Performance (measured on Azure VM, 4 vCPU, 16GB RAM)

| Operation | Duration |
|-----------|----------|
| VM cold boot to health check | ~3s |
| OpenClaw gateway init (cold boot) | ~40s (JIT compile) |
| Snapshot save (sleep) | ~11s |
| Snapshot restore (wake) | ~400ms |
| Gateway ready after snapshot restore | Instant (process state preserved) |
| Chat roundtrip (after gateway ready) | <1s (Groq LLM, 300+ tokens/sec) |
