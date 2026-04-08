# Module: orchestrator

**Crate:** `oneclick-orchestrator`
**Path:** `backend/crates/orchestrator/`
**Role:** Agent lifecycle management. Creates, wakes, sleeps, destroys agents using pluggable runtimes. Core abstraction for runtime portability (Docker ↔ Firecracker).

## Dependencies
`shared`, `bollard`, `dashmap`, `fctools`

## Key Exports
- `AgentRuntime` — trait (9 methods: 5 async lifecycle + 2 async address + 1 naming + 1 health config)
- `DockerRuntime` — bollard-based container implementation
- `FirecrackerRuntime` — fctools-based microVM implementation
- `TapManager` — TAP network device pool (Firecracker)
- `Orchestrator` — service struct with DB + locking

## AgentRuntime Trait
```rust
#[async_trait]
pub trait AgentRuntime: Send + Sync {
    async fn create_agent(&self, agent: &Agent, config: &Config) -> AppResult<String>;
    async fn start_agent(&self, container_id: &str) -> AppResult<()>;
    async fn stop_agent(&self, container_id: &str) -> AppResult<()>;
    async fn destroy_agent(&self, container_id: &str) -> AppResult<()>;
    async fn health_check(&self, container_id: &str) -> AppResult<bool>;
    async fn get_host_port(&self, container_id: &str) -> AppResult<Option<u16>>;
    async fn get_agent_address(&self, container_id: &str) -> AppResult<String>;
    fn agent_name(&self, user_id: &Uuid, agent_id: &Uuid) -> String;
    fn health_check_budget(&self) -> (u32, Duration);
}
```

## DockerRuntime
- Connects via `Docker::connect_with_local_defaults()` (Docker socket)
- Container name: `agent-{user_id first 8 chars}`
- Network: `config.docker_network`
- Agent address: container bridge IP from `docker inspect` (not DNS name — backend runs on host)
- Host port: random mapped port for gateway
- Health check budget: 100 retries × 3s = 5 min
- Env: `OPENROUTER_BASE_URL`, `DEFAULT_MODEL`, `OPENROUTER_API_KEY` (encodes auth)

## FirecrackerRuntime
- Uses fctools 0.7.0-alpha.1 SDK for VM lifecycle
- VM ID: `fc-{agent_uuid}`
- Rootfs: copy-on-write from template (`cp --reflink=auto`), per-VM config injected via mount
- Networking: TAP device from pool, guest IP on /30 subnet
- Agent address: TAP guest IP (e.g., `172.16.0.2`)
- Host port: None (direct TAP access)
- Health check: TCP probe to guest_ip:3001 (chat bridge)
- Health check budget: 60 retries × 1s = 60s
- Snapshot: in-memory `VmSnapshot` for fast restore + files on disk

### VM Lifecycle
| Operation | What happens |
|-----------|-------------|
| `create_agent` | Copy rootfs → allocate TAP → mount rootfs → inject /etc/fc-network + /etc/openclaw-env → unmount |
| `start_agent` | If snapshot exists → restore (116ms). Else → cold boot via fctools |
| `stop_agent` | Pause VM → create snapshot → shutdown |
| `destroy_agent` | Shutdown VM → release TAP → delete rootfs + snapshots |

### Rootfs Config Injection
Each VM's rootfs gets two config files written at create time:
- `/etc/fc-network`: GUEST_IP, GUEST_CIDR, GATEWAY_IP, NAMESERVER
- `/etc/openclaw-env`: OPENROUTER_API_KEY, OPENROUTER_BASE_URL, DEFAULT_MODEL, OPENCLAW_GATEWAY_TOKEN

The init script inside the rootfs reads these at boot to configure networking and start OpenClaw.

## TapManager
```rust
pub struct TapManager {
    allocations: DashMap<String, TapAllocation>,
    available: Mutex<VecDeque<usize>>,
    config: TapConfig,
}
```
- Pool of tap0-tap15 (configurable via `FC_TAP_COUNT`)
- IP scheme: host=172.16.0.{i*4+1}, guest=172.16.0.{i*4+2}, /30 subnet
- MAC: AA:FC:00:00:00:{index_hex}
- Creates TAP device + assigns IP + iptables masquerade on allocate
- Cleans up on release

## Orchestrator Service
```rust
pub struct Orchestrator {
    runtime: Arc<dyn AgentRuntime>,
    db: PgPool,
    locks: DashMap<Uuid, Arc<tokio::sync::Mutex<()>>>,
}
```

### Methods
| Method | What it does |
|--------|-------------|
| `create_agent(user_id, model, config)` | Atomic capacity check (`INSERT...SELECT WHERE count < max`) → create agent → update DB |
| `wake_agent(agent_id)` | Lock → start agent → health check (budget from runtime) → update status |
| `sleep_agent(agent_id)` | Lock → stop agent (Docker: stop container, FC: snapshot VM) → update status |
| `destroy_agent(agent_id)` | Lock → destroy agent → delete DB record (lock entry intentionally retained) |
| `ensure_ready(agent_id)` | Running→return, Stopped→wake, Creating→error, Error→auto-recover if healthy |
| `get_agent_status(agent_id)` | Query DB |
| `get_host_port(agent_id)` | Delegate to runtime |
| `get_agent_address(agent_id)` | Delegate to runtime — returns container IP (Docker) or TAP IP (FC) |

### Locking
Every mutation acquires `locks.entry(agent_id).or_insert(Mutex)`. Prevents concurrent wake+sleep race conditions. Lock entries are intentionally kept after `destroy_agent`.

## Tests
- `parse_memory_limit` — m/M/g/G/k/K/bytes/invalid (6 tests)
- `cpu_to_nano` — fractional CPU conversion (1 test)

## Extension
- New runtime: implement `AgentRuntime` (9 methods), wire in `main.rs`
- Runtime selection: `AGENT_RUNTIME=docker|firecracker` env var in `main.rs`
- `start_agent` handles Docker 304 (already running) gracefully
- Firecracker `start_agent` checks for in-memory snapshot before attempting cold boot
