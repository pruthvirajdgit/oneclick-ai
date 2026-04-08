# OneClick.ai Documentation

## Architecture
- [System Overview](architecture/system-overview.md) — Full architecture, components, data flow
- [Technology Choices](architecture/technology-choices.md) — Language, frameworks, infrastructure decisions
- [Firecracker MicroVMs](architecture/firecracker.md) — VM architecture, TAP networking, snapshot lifecycle

## Design Decisions
Architectural Decision Records (ADRs) — why we chose what we chose.

- [ADR-001: Rust for Backend](decisions/001-rust-backend.md)
- [ADR-002: Docker with Runtime Abstraction](decisions/002-docker-runtime-abstraction.md)
- [ADR-003: Scale-to-Zero via Stop/Start](decisions/003-scale-to-zero.md)
- [ADR-004: External Scheduler over In-Agent Cron](decisions/004-external-scheduler.md)
- [ADR-005: LLM Proxy over Direct Provider Access](decisions/005-llm-proxy.md)
- [ADR-006: No LiteLLM](decisions/006-no-litellm.md)
- [ADR-007: No Firecracker in Phase 1](decisions/007-no-firecracker-phase1.md)
- [ADR-008: Free Tier via Groq + OpenRouter](decisions/008-free-tier-providers.md)
- [ADR-009: Testing Strategy](decisions/009-testing-strategy.md)
- [ADR-010: Telemetry](decisions/010-telemetry.md)

## Phase 1
- [Build Specification](phase1/build-spec.md) — What we're building, module by module
- [API Reference](phase1/api-reference.md) — All endpoints
- [Database Schema](phase1/database-schema.md) — PostgreSQL tables
- [Deployment Guide](phase1/deployment.md) — Local dev + production
- [Testing Guide](phase1/testing.md) — Unit + integration tests, CI config
- [Telemetry Guide](phase1/telemetry.md) — Logs, metrics, monitoring

## Current Status
- **Phase 1-2**: Complete (backend + frontend + chat)
- **Phase 3**: Complete (Firecracker microVMs, 116ms snapshot restore)
- **Phase 4**: Next (billing, jailer security, on-disk recovery)
