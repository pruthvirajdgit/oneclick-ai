# OneClick.ai Documentation

## Architecture
- [System Overview](architecture/system-overview.md) — Full architecture, components, data flow
- [Technology Choices](architecture/technology-choices.md) — Language, frameworks, infrastructure decisions

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

## Phase 1
- [Build Specification](phase1/build-spec.md) — What we're building, module by module
- [API Reference](phase1/api-reference.md) — All endpoints
- [Database Schema](phase1/database-schema.md) — PostgreSQL tables
- [Deployment Guide](phase1/deployment.md) — Local dev + Azure production

## Future Phases
- Phase 2: CRIU checkpoint/restore (~1-2s cold starts)
- Phase 3: Firecracker microVMs (<200ms cold starts, multi-region, live migration)
