# Stage 3: Firecracker Production Integration Plan

## Problem
Port the proven Firecracker PoC (10-12ms snapshot restore, OpenClaw in microVM) into the production codebase by implementing `FirecrackerRuntime` behind the existing `AgentRuntime` trait. Docker remains as fallback via `AGENT_RUNTIME=docker|firecracker` flag.

## Approach
The `AgentRuntime` trait is already defined with 6 methods (`create_agent`, `start_agent`, `stop_agent`, `destroy_agent`, `health_check`, `get_host_port`). We implement a new `FirecrackerRuntime` struct that maps each method to Firecracker operations from the PoC, add a TAP network manager for multi-VM support, and wire everything up with a runtime selector in `main.rs`.

## Key Design Decisions
- **Both runtimes coexist** — `AGENT_RUNTIME=docker|firecracker` env var selects which runtime is used
- **Fixed TAP pool** — simple pool of pre-numbered TAP devices (tap0-tap15) with IP allocation, expand later
- **Snapshot on stop, restore on wake** — matches PoC's 10-12ms wake time from day one
- **No DB schema changes** — `container_id` field stores VM ID (e.g., `fc-{uuid}`), existing fields suffice
- **Rootfs is copy-on-write** — each VM gets a `cp --reflink=auto` copy of the template rootfs

---

## Todos

### 1. `config-additions` — Add Firecracker config fields to `Config`
**File**: `backend/crates/shared/src/config.rs`

Add new fields with sensible defaults:
```rust
pub agent_runtime: String,          // "docker" | "firecracker", default "docker"
pub fc_kernel_path: String,         // default "/opt/firecracker/vmlinux-6.1"
pub fc_rootfs_template: String,     // default "/opt/firecracker/rootfs-openclaw.ext4"
pub fc_snapshot_dir: String,        // default "/var/lib/oneclick/snapshots"
pub fc_vm_dir: String,              // default "/var/lib/oneclick/vms"
pub fc_vcpu_count: u32,             // default 2
pub fc_mem_size_mib: u32,           // default 1536
pub fc_tap_prefix: String,          // default "tap"
pub fc_tap_count: u32,              // default 16
pub fc_subnet_prefix: String,       // default "172.16"
```

Load from env: `AGENT_RUNTIME`, `FC_KERNEL_PATH`, `FC_ROOTFS_TEMPLATE`, etc.

### 2. `tap-manager` — TAP Network Manager Module
**File**: `backend/crates/orchestrator/src/tap_manager.rs`

Manages a fixed pool of TAP devices and IP assignments:
```rust
pub struct TapManager {
    pool_size: u32,
    prefix: String,           // "tap"
    subnet_prefix: String,    // "172.16"
    assignments: DashMap<String, TapSlot>,  // vm_id → slot
}

pub struct TapSlot {
    pub index: u32,           // 0-15
    pub tap_name: String,     // "tap0"
    pub host_ip: String,      // "172.16.0.1"
    pub guest_ip: String,     // "172.16.0.2"
    pub guest_mac: String,    // derived from index
}
```

Methods:
- `new(prefix, subnet_prefix, pool_size)` — initialize pool
- `allocate(vm_id) -> Result<TapSlot>` — claim next free slot
- `release(vm_id)` — return slot to pool
- `setup_tap(slot) -> Result<()>` — create TAP device, assign IP, add iptables NAT (like PoC script)
- `teardown_tap(slot) -> Result<()>` — reverse of setup

IP scheme: Each slot `i` gets a `/30` subnet:
- Host: `172.16.{i*4/256}.{(i*4)%256 + 1}`
- Guest: `172.16.{i*4/256}.{(i*4)%256 + 2}`

Simplified for 16 slots: `172.16.0.{i*4+1}` / `172.16.0.{i*4+2}`, tap names `tap0`-`tap15`.

### 3. `firecracker-runtime` — Implement `AgentRuntime` for Firecracker
**File**: `backend/crates/orchestrator/src/firecracker_runtime.rs`

Core struct:
```rust
pub struct FirecrackerRuntime {
    config: FirecrackerConfig,     // kernel, rootfs, mem, vcpu paths
    tap_manager: Arc<TapManager>,
    vms: DashMap<String, VmState>, // container_id → state
}

struct VmState {
    pid: u32,
    socket_path: PathBuf,
    tap_slot: TapSlot,
    snapshot_dir: PathBuf,
    rootfs_path: PathBuf,         // per-VM copy
    has_snapshot: bool,
}
```

Trait method mapping (from PoC):
| Trait Method | Firecracker Operation |
|---|---|
| `create_agent` | Copy rootfs template → write env to `/etc/openclaw-env` → allocate TAP → return vm_id |
| `start_agent` | If snapshot exists: `snapshot_wake()`. Else: `start_firecracker_process()` + `configure_vm()` + `instance_start()` |
| `stop_agent` | `snapshot_sleep()` — pause → snapshot → kill FC process, release nothing (TAP stays allocated) |
| `destroy_agent` | Kill FC process → release TAP → delete rootfs copy → delete snapshots |
| `health_check` | HTTP GET `http://{guest_ip}:3001/health` (bridge port, like PoC) |
| `get_host_port` | Return `None` — Firecracker VMs are accessed by IP, not host port mapping |

Key implementation details:
- Port PoC's `fc_request()` (hyper over Unix socket) into the module
- Port `start_firecracker_process()`, `configure_vm()`, `snapshot_sleep/wake()`
- Write agent-specific env vars into rootfs `/etc/openclaw-env` before boot
- FC socket path: `/tmp/fc-{vm_id}.socket`
- FC runs under `sudo` (for TAP access) — same pattern as PoC
- Handle "already running" / "already stopped" idempotently (like Docker impl)

### 4. `runtime-selector` — Wire Up Runtime Selection in `main.rs`
**Files**: `backend/src/main.rs`, `backend/crates/orchestrator/src/lib.rs`

In `lib.rs` — add exports:
```rust
pub mod firecracker_runtime;
pub mod tap_manager;
pub use firecracker_runtime::FirecrackerRuntime;
```

In `main.rs` — select runtime based on config:
```rust
let runtime: Arc<dyn AgentRuntime> = match config.agent_runtime.as_str() {
    "firecracker" => {
        let tap_mgr = Arc::new(TapManager::new(&config));
        Arc::new(FirecrackerRuntime::new(&config, tap_mgr)?)
    }
    _ => Arc::new(DockerRuntime::new()?),
};
let orchestrator = Arc::new(Orchestrator::new(runtime, db_pool.clone()));
```

Keep `Docker` client for cases where it's still needed (e.g., AppState has `docker` field).

### 5. `health-check-tuning` — Adjust Health Check Timeouts
**File**: `backend/crates/orchestrator/src/service.rs`

The orchestrator's `poll_health` and `ensure_ready` use hardcoded retry loops. These need to be runtime-aware:
- **Docker cold boot**: 100 retries × 3s = 5 min (current)
- **Firecracker cold boot**: 60 retries × 1s = 60s
- **Firecracker snapshot wake**: 10 retries × 500ms = 5s

Options:
1. Add retry config to `AgentRuntime` trait (e.g., `fn health_check_config() -> HealthCheckConfig`)
2. Or read from `Config` based on `agent_runtime` setting

### 6. `wake-endpoint-update` — Handle Firecracker in Wake Endpoint
**File**: `backend/crates/api/src/routes/agents.rs`

The wake endpoint currently returns a `chat_url` with `http://localhost:{host_port}`. For Firecracker:
- `get_host_port()` returns `None` (VMs use direct IP, not host port mapping)
- Chat goes through the backend's existing SSE bridge, not direct browser access
- May need to skip the `chat_url` in response for Firecracker agents, or provide the VM IP

This is mostly a no-op since Phase 2 chat already works through the backend bridge, not direct port access.

### 7. `cargo-deps` — Add Dependencies
**File**: `backend/crates/orchestrator/Cargo.toml`

Add:
- `hyper` (for Unix socket HTTP to FC API — already in workspace)
- `hyper-util` (TokioIo adapter)
- `http-body-util` (body handling)
- `bytes` (request bodies)
- `nix` or just `tokio::process` (for process management — already available)

No new external crates needed — the PoC already uses hyper which is in the workspace.

### 8. `e2e-test` — End-to-End Verification
Manual test sequence (matches PoC validation):
1. Set `AGENT_RUNTIME=firecracker` in `.env`
2. `docker compose up -d` (backend + frontend + postgres + redis)
3. Sign up / log in via frontend
4. Create agent → verify FC VM boots, gateway healthy
5. Chat via frontend → verify LLM response
6. Wait for idle timeout → verify snapshot sleep
7. Chat again → verify snapshot wake (~12ms) + chat works
8. Delete agent → verify VM destroyed, TAP released
9. Switch back to `AGENT_RUNTIME=docker` → verify Docker still works

### 9. `docs-update` — Update Documentation
- `docs/context_bank/` — add Firecracker architecture docs
- `README.md` — mention Firecracker runtime option
- `local_poc/firecracker/README.md` — add Stage 3 reference

---

## File Change Summary

| File | Change Type | Description |
|------|-------------|-------------|
| `backend/crates/shared/src/config.rs` | Modify | Add `fc_*` config fields |
| `backend/crates/orchestrator/src/tap_manager.rs` | **New** | TAP network pool manager |
| `backend/crates/orchestrator/src/firecracker_runtime.rs` | **New** | FirecrackerRuntime impl |
| `backend/crates/orchestrator/src/lib.rs` | Modify | Export new modules |
| `backend/crates/orchestrator/Cargo.toml` | Modify | Add hyper/bytes deps |
| `backend/src/main.rs` | Modify | Runtime selector logic |
| `backend/crates/orchestrator/src/service.rs` | Modify | Health check tuning |
| `backend/crates/api/src/routes/agents.rs` | Modify | Minor: handle None host_port |

## Dependencies Between Todos

```
config-additions ─┬─→ tap-manager ───→ firecracker-runtime ─┬─→ runtime-selector → e2e-test → docs-update
                  │                                          │
                  └──→ cargo-deps ──────────────────────────┘
                                                             │
                               health-check-tuning ──────────┘
                               wake-endpoint-update ─────────┘
```

## Critical Architecture Decision: Backend ↔ Firecracker Deployment

**The core problem:** The backend currently runs inside a Docker container (`docker-compose`). Firecracker needs `/dev/kvm`, root/sudo access, and the ability to create TAP devices — all of which are difficult or dangerous to do from inside Docker.

In the PoC, the Rust binary ran directly on the host, alongside Firecracker. In production, we need to decide how the backend process communicates with Firecracker.

### Option 1: Backend Runs on Host (not in Docker) for Firecracker Mode
- **How:** When `AGENT_RUNTIME=firecracker`, deploy the backend as a systemd service on bare metal, not via `docker-compose`
- **Pros:** Simplest, direct KVM/TAP access, identical to PoC model, no abstraction layers
- **Cons:** Two deployment models (Docker for dev, host for prod), can't use `docker-compose` for everything
- **Best for:** Single-server or dedicated host deployments

### Option 2: Privileged Docker Container with KVM Passthrough
- **How:** Run backend container with `--privileged`, mount `/dev/kvm`, install Firecracker inside the container
- **Pros:** Keeps `docker-compose` workflow, single deployment model
- **Cons:** `--privileged` defeats Docker isolation, complex networking (TAP inside container), nesting concerns, security risk
- **Best for:** Dev/testing where convenience matters more than security

### Option 3: Sidecar Daemon (Firecracker Manager) on Host
- **How:** Backend stays in Docker. A separate `fc-manager` daemon runs on the host with KVM access. Backend communicates via gRPC/HTTP over a mounted Unix socket
- **Pros:** Clean separation of concerns, backend stays containerized, fc-manager handles all privileged ops
- **Cons:** New service to build/maintain, extra hop for VM operations, more moving parts
- **Best for:** Production-grade, multi-tenant deployments

### Option 4: Hybrid — Docker for Dev, Host for Prod
- **How:** `AGENT_RUNTIME=docker` uses docker-compose as-is. `AGENT_RUNTIME=firecracker` requires the backend binary to run on the host. Same binary, different deployment
- **Pros:** No compromise on either path, dev stays simple, prod gets full performance
- **Cons:** Need separate deployment scripts/docs for each mode
- **Best for:** Pragmatic approach that works now, evolve to Option 3 later

### ✅ DECIDED: Backend Always on Host + Deployment Refactor

**Decision:** Backend runs on the host **regardless of runtime** (Docker or Firecracker). This is a deployment refactor that happens BEFORE the Firecracker runtime work.

**Rationale:** Mimics production where backend, database, redis, and frontend are all on separate servers. Clean separation of concerns.

**New deployment model:**
- `docker-compose.yml` runs: frontend (nginx), postgres, redis
- Backend runs on host via `cargo run` (dev) or systemd service (prod)
- Backend connects to postgres at `localhost:5432`, redis at `localhost:6379`
- Frontend nginx proxies `/api/*` to `host.docker.internal:8080`
- Agent containers (Docker runtime) reached by container IP from Docker inspect
- Agent containers reach backend at `http://host.docker.internal:8080`

**Backend→Agent communication change (critical):**
Today, backend uses Docker DNS names (`http://{container_name}:3000`). With backend on host, Docker DNS doesn't resolve. Fix: use container IP from `docker inspect` instead.

All 4 places that use container_name as hostname:
1. `runtime.rs:373` — health check probe
2. `chat.rs:168` — chat bridge POST
3. `agent_ui.rs:54` — UI reverse proxy
4. `scheduler/service.rs:142` — scheduled chat

**Solution:** Add `get_agent_address()` to `AgentRuntime` trait (or resolve container IP in a shared helper). DockerRuntime returns container bridge IP. FirecrackerRuntime returns TAP IP. Both work identically from the host.

---

## Other Risks & Notes
- **sudo requirement**: FC needs root for TAP devices. In production, use jailer with proper capabilities. For now, backend runs as root or has sudo.
- **Rootfs size**: 4GB per VM. With `cp --reflink=auto` on supported filesystems (btrfs, xfs), copies are instant and share blocks. On ext4, it's a full 4GB copy.
- **Snapshot mem files**: 1.5GB per VM. 16 VMs = 24GB disk for snapshots alone. Plan storage accordingly.
- **No jailer yet**: Security hardening (chroot, seccomp, cgroups) is deferred. Add as a follow-up after basic integration works.
- **Docker still needed**: Even with `AGENT_RUNTIME=firecracker`, Docker is still in AppState for the chat exec endpoint. Can be made optional later.
