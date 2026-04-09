# ADR-007: Firecracker Implementation

## Status
**Superseded** — Originally deferred Firecracker to Phase 3. **Now fully implemented and the primary runtime.**

## Context
Firecracker microVMs offer sub-second snapshot restores, native snapshots, and hardware-level isolation. Originally evaluated and deferred for Phase 1 due to infrastructure complexity.

## Original Decision (Phase 1)
Use Docker for Phase 1. Design for Firecracker in Phase 3.

## Current State (Implemented)
Firecracker is now the **primary runtime** (`AGENT_RUNTIME=firecracker`). Docker remains as a fallback.

### What was built
- `FirecrackerRuntime` struct implementing `AgentRuntime` trait
- `TapManager` for TAP device allocation (172.16.0.x/30 subnets)
- Custom rootfs template with OpenClaw, chat-bridge, pair-device
- Parameterized init script (`/sbin/init`) with network + env config injection
- Native VM snapshots via fctools SDK (snapshot save/restore)
- Orphaned Firecracker process cleanup on cold boot
- KVM permissions via udev rule

### Measured performance
| Operation | Duration |
|-----------|----------|
| Cold boot to healthy | ~3s |
| OpenClaw gateway ready (cold) | ~40s (JIT) |
| Snapshot save (sleep) | ~11s |
| Snapshot restore (wake) | ~400ms |
| Gateway after restore | Instant |

### Key configuration
```bash
AGENT_RUNTIME=firecracker
FC_KERNEL_PATH=/opt/firecracker/vmlinux-6.1
FC_ROOTFS_TEMPLATE=/opt/firecracker/rootfs-openclaw.ext4
FC_VCPU_COUNT=2
FC_MEM_SIZE_MIB=1536
FC_TAP_COUNT=4
```

## Analysis (original, still relevant)

### What Firecracker provides
- KVM-based microVM in ~125ms boot
- ~5MB overhead per VM
- Hardware isolation (own kernel per VM)
- Native snapshot/restore
- REST API for VM lifecycle

### What Firecracker does NOT provide (we built these)
- No Docker images — we built Linux rootfs templates with OpenClaw
- No orchestration — TapManager + FirecrackerRuntime handle lifecycle
- No networking stack — TAP devices + iptables MASQUERADE for NAT
- No storage management — rootfs copies with `cp --reflink=auto`

### Infrastructure effort (actual)
Building the Firecracker integration required:
- Custom rootfs build script (`scripts/firecracker/build-rootfs.sh`)
- TAP networking setup + IP allocation
- Init script for VM boot configuration
- fctools SDK integration for VM lifecycle + snapshots
- Orphaned process cleanup logic
- 5 live E2E tests + 12 mock E2E tests

## Consequences
- Backend must run on the host (not in Docker) for KVM access
- Backend Dockerfile was removed
- 4GB per rootfs copy, 1.5GB per snapshot on disk
- 16 VM limit (TAP pool, configurable)
- In-memory snapshots lost on backend restart (on-disk recovery planned)
- No jailer security yet (planned for Phase 4)
