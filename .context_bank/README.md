# OneClick.ai — Context Bank

Machine-readable project context for AI agents. Three sections:

## Structure

```
.context_bank/
├── README.md                          ← You are here
├── ../new_machine_setup/              # Machine setup guides (Linux + macOS)
├── product/
│   ├── vision.md                      # What we're building, for whom, and why
│   ├── decisions.md                   # Product decisions with rationale
│   └── roadmap.md                     # Phased delivery plan (Phase 0-3)
└── technical/
    ├── architecture.md                # System design, data flows, crate map
    ├── decisions.md                   # Technical ADRs (condensed)
    ├── guidelines.md                  # Coding standards, Rust idiomatics, PR rules
    ├── infrastructure.md              # Docker, Postgres, Redis, Firecracker, networking
    └── modules/
        ├── shared.md                  # Foundation crate
        ├── api.md                     # HTTP/WS routes, middleware, SSE bridge
        ├── orchestrator.md            # Agent lifecycle, DockerRuntime + FirecrackerRuntime
        ├── llm-proxy.md               # Multi-provider LLM routing with SSE streaming
        ├── scheduler.md               # Cron runner
        ├── monitor.md                 # Idle detection
        ├── notifications.md           # Alerts + broadcast
        ├── message-queue.md           # Buffer for sleeping agents
        ├── agent-tools.md             # OpenClaw JS plugin
        └── webhook-receiver.md        # Inbound integrations (stub)
```

## Current State (as of Phase 3 — Firecracker integration)

- **Backend**: Rust monolith (10 crates) on port 8080, runs on host (not in Docker)
- **Frontend**: React 19 + Vite + Tailwind + shadcn/ui, served by nginx on port 80/3000
- **Chat**: In-app WebSocket → SSE bridge pipeline with real-time token streaming
- **Agent Runtime**: Dual — `DockerRuntime` (containers) or `FirecrackerRuntime` (microVMs), selected via `AGENT_RUNTIME` env var
- **Firecracker**: 116ms snapshot wake, TAP networking, fctools SDK, VM-level isolation
- **Infrastructure**: Docker Compose (frontend + postgres + redis). Backend on host.
- **Next**: Production hardening — jailer security, on-disk snapshot recovery, billing

## Usage

- **Product context**: Read `product/` to act as a product manager. Understand vision, user needs, competitive positioning, and what's been decided vs. deferred.
- **Technical context**: Read `technical/` to contribute code. Understand architecture, module boundaries, coding standards, and design invariants.
- **Module context**: Read `technical/modules/<crate>.md` before modifying that crate. Contains API surface, key types, invariants, and extension points.

## Rules

1. These files are the source of truth. If `docs/` conflicts with `.context_bank/`, the context bank is authoritative.
2. Update the relevant context bank file when making architectural or product decisions.
3. Keep entries compact — these are for AI agents, not human prose.
