# ADR-002: Docker with Runtime Abstraction

## Status
Accepted — **Firecracker runtime now implemented** (Phase 3 complete)

## Context
We need to run one OpenClaw agent instance per user. The runtime must support:
- Creating/starting/stopping agents dynamically
- Resource isolation (memory, CPU limits per agent)
- Persistent state across stop/start cycles
- Future migration to Firecracker microVMs

## Decision
Use an **`AgentRuntime` trait** abstraction with swappable implementations.

```rust
#[async_trait]
trait AgentRuntime: Send + Sync {
    async fn create(&self, agent: &Agent, config: &AgentConfig) -> Result<String>;
    async fn start(&self, container_id: &str) -> Result<()>;
    async fn stop(&self, container_id: &str) -> Result<()>;
    async fn destroy(&self, container_id: &str) -> Result<()>;
    async fn status(&self, container_id: &str) -> Result<ContainerStatus>;
    async fn health_check(&self, container_id: &str) -> Result<bool>;
}
```

Current implementations:
- `FirecrackerRuntime` — **primary** (fctools SDK, KVM microVMs, TAP networking)
- `DockerRuntime` — alternative fallback (bollard crate)
- `MockRuntime` — for E2E tests without real infrastructure

Selected via `AGENT_RUNTIME` environment variable (`firecracker` or `docker`).

## Rationale

### Why the trait abstraction
- Every other module (orchestrator, scheduler, monitor) talks to `AgentRuntime`, not Docker/Firecracker directly
- Swapping runtimes requires zero changes to business logic
- Testable: MockRuntime enables fast E2E tests without real VMs

### Current architecture
- Backend runs on the **host** (bare metal), not in Docker — required for KVM access
- Frontend, PostgreSQL, Redis run in Docker via docker-compose.yml
- Firecracker VMs use TAP networking (172.16.0.x/30 subnets)

## Consequences
- Backend needs direct access to `/dev/kvm` for Firecracker
- Backend Dockerfile has been removed (backend is never containerized)
- KVM permissions managed via udev rule
- TAP devices managed by TapManager in Rust
