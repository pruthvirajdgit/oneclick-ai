# ADR-007: No Firecracker in Phase 1

## Status
Accepted

## Context
Firecracker microVMs offer sub-200ms cold starts, native snapshots, and hardware-level isolation. We evaluated it as the agent runtime for Phase 1.

## Decision
**Use Docker for Phase 1. Design for Firecracker in Phase 3.**

The `AgentRuntime` trait abstraction (ADR-002) ensures the swap is a matter of implementing a new struct, not rewriting the system.

## Analysis

### What Firecracker provides
- KVM-based microVM in ~125ms boot
- ~5MB overhead per VM
- Hardware isolation (own kernel per VM)
- Native snapshot/restore
- REST API for VM lifecycle

### What Firecracker does NOT provide
- No Docker images — must build Linux kernel images + root filesystems
- No Kubernetes/orchestration — build your own
- No networking stack — manage TAP devices, IP allocation, firewalls manually
- No storage management — manage block devices, mount points
- No ecosystem — no pre-built images, monitoring tools, etc.

### Security argument
Firecracker's main advantage is hardware isolation (each VM runs its own kernel). This matters when running **untrusted user code** (like AWS Lambda).

For OneClick.ai Phase 1:
- Agents run trusted OpenClaw code, not arbitrary user code
- Cloud provider is responsible for kernel security
- Docker's namespace/cgroup isolation is sufficient

### Performance argument
- Firecracker snapshot restore: ~100-200ms
- Docker + CRIU restore: ~1-2s
- Docker cold start: ~5-10s

For AI agents, even 5-10s is acceptable — users expect a brief pause before an AI responds.

### Portability argument (Phase 3 value)
Firecracker snapshots are portable — store in S3, restore on any machine. This enables:
- Multi-region deployment
- Live migration between servers
- Cold storage of idle agents to S3 ($0.023/GB/month vs RAM costs)

This is valuable at 10K+ users but not needed at 100.

### Effort comparison
- Docker Phase 1: days of work (already have a working prototype)
- Firecracker Phase 1: months of work (kernel images, networking, orchestrator, storage)

## Consequences
- 5-10s cold starts (acceptable for Phase 1)
- No hardware isolation (acceptable — trusted code only)
- No snapshot portability (single server is fine for Phase 1)
- Clean migration path via AgentRuntime trait when ready for Phase 3
