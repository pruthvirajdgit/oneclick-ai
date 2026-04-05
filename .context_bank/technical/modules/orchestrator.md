# Module: orchestrator

**Crate:** `oneclick-orchestrator`
**Path:** `backend/crates/orchestrator/`
**Role:** Agent container lifecycle management. Creates, wakes, sleeps, destroys agent containers. Core abstraction for runtime portability.

## Dependencies
`shared`, `bollard`, `dashmap`

## Key Exports
- `AgentRuntime` — trait (5 async methods)
- `DockerRuntime` — bollard-based implementation
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
}
```

## DockerRuntime
- Connects via `Docker::connect_with_local_defaults()` (Docker socket)
- Container name: `agent-{user_id first 8 chars}`
- Memory: parsed from string ("512m" → bytes via `parse_memory_limit`)
- CPU: converted to nano-CPUs (0.5 → 500,000,000)
- Network: `config.docker_network`
- Env: `OPENROUTER_BASE_URL=http://backend:8080/internal/llm/v1`, `DEFAULT_MODEL`
- Labels: `oneclick.agent_id`, `oneclick.user_id`
- Health check: inspect container state, check Docker health status if configured

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
| `create_agent(user_id, model, config)` | Atomic capacity check (`INSERT...SELECT WHERE count < max`) → create container → update DB |
| `wake_agent(agent_id)` | Lock → start container → health check (5 retries, 2s) → update status |
| `sleep_agent(agent_id)` | Lock → stop container → update status |
| `destroy_agent(agent_id)` | Lock → remove container → delete DB record (lock entry intentionally retained) |
| `purge_stale_locks()` | Periodic cleanup: removes DashMap entries for agents no longer in DB |
| `ensure_ready(agent_id)` | Running→return, Stopped→wake, Creating→error, Error→error |
| `get_agent_status(agent_id)` | Query DB |

### Locking
Every mutation acquires `locks.entry(agent_id).or_insert(Mutex)`. Prevents concurrent wake+sleep race conditions. Lock entries are intentionally kept after `destroy_agent` (removing while other tasks hold Arc clones would break serialization). Call `purge_stale_locks()` periodically to bound DashMap growth.

## Tests
- `parse_memory_limit` — m/M/g/G/k/K/bytes/invalid (6 tests)
- `cpu_to_nano` — fractional CPU conversion (1 test)

## Extension
- New runtime: implement `AgentRuntime`, wire in `main.rs`
- Phase 2 (CRIU): `CriuRuntime` — checkpoint via `criu dump`, restore via `criu restore`
- Phase 3 (Firecracker): `FirecrackerRuntime` — snapshot to S3, restore from snapshot
