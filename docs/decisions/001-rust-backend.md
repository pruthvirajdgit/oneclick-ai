# ADR-001: Rust for Backend

## Status
Accepted

## Context
We need a backend language for a multi-tenant AI agent platform that will:
- Manage thousands of agent container lifecycles concurrently
- Proxy LLM requests with sub-millisecond overhead
- Eventually integrate with Firecracker microVMs (written in Rust) — **now done**
- Run as a single efficient binary on modest hardware

Options considered: Rust, Go, TypeScript (Node.js), Python (FastAPI)

## Decision
**Rust** (axum + tokio + sqlx + bollard)

## Rationale

### Why Rust over Go
- Team has Rust experience, no Go experience
- Firecracker is written in Rust — integration is native (fctools crate, not SDK wrappers) — **now proven**
- Stronger type system enforces clean architecture at compile time
- The borrow checker is an architectural tool, not a burden, when the design is right

### Why Rust over TypeScript
- Concurrency model: tokio handles millions of concurrent tasks vs Node's single-threaded event loop
- Memory: 5-15MB binary vs 50-100MB Node.js process
- Single binary deployment vs shipping node_modules

### Why Rust over Python
- An orchestrator is I/O-bound infrastructure work, not ML/AI — Python's async story is messier
- Performance: no GIL bottleneck for concurrent container management
- No dependency conflicts or virtual environment management

### Why not "too complex for a startup"
- axum + sqlx ecosystem is mature — productivity comparable to Express/FastAPI for API work
- Compile-time guarantees reduce runtime debugging significantly
- The complexity is front-loaded (compiler catches errors) rather than back-loaded (production bugs)

## Consequences
- Slower initial development compared to Python/TypeScript
- Smaller hiring pool if team grows
- All backend engineers need Rust proficiency
- Excellent performance and reliability characteristics from day 1
