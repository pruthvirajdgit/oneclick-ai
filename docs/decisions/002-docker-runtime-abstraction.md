# ADR-002: Docker with Runtime Abstraction

## Status
Accepted

## Context
We need to run one OpenClaw agent instance per user. The runtime must support:
- Creating/starting/stopping containers dynamically
- Resource isolation (memory, CPU limits per agent)
- Persistent state across stop/start cycles
- Future migration to CRIU (Phase 2) and Firecracker (Phase 3)

## Decision
Use **Docker** as the Phase 1 runtime, behind an **`AgentRuntime` trait** abstraction.

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

Phase 1: `DockerRuntime` (bollard crate)
Phase 2: `CRIURuntime` (docker checkpoint/restore)
Phase 3: `FirecrackerRuntime` (native Rust integration)

## Rationale

### Why Docker (Phase 1)
- Already proven in our Phase 0 prototype
- Rich ecosystem: networking, volumes, health checks out of the box
- bollard crate provides full Docker API access from Rust
- Good enough for first 1,000 users

### Why the trait abstraction
- Every other module (orchestrator, scheduler, monitor) talks to `AgentRuntime`, not Docker directly
- Swapping Docker → CRIU → Firecracker requires implementing one new struct, zero changes elsewhere
- Testable: can mock `AgentRuntime` in unit tests

### Why not Firecracker now
- Requires building kernel images, root filesystems, custom networking, custom orchestration
- No Docker/Kubernetes ecosystem support
- Months of infrastructure work for marginal gain at our current scale
- See ADR-007 for detailed analysis

### Why not Kubernetes
- Overkill for Phase 1 (single server, <100 agents)
- Docker Compose + bollard gives us everything we need
- K8s adds operational complexity without proportional benefit at this scale

## Consequences
- Docker daemon required on host
- Backend needs access to Docker socket (/var/run/docker.sock)
- Agent containers are siblings to backend container (not nested)
- One Docker network shared by all services
