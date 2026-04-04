# Module: api

**Crate:** `oneclick-api`
**Path:** `backend/crates/api/`
**Role:** HTTP/WebSocket layer. All public and internal endpoints, auth middleware, Swagger UI. Delegates business logic to orchestrator, llm-proxy, notifications.

## Dependencies
`shared`, `orchestrator`, `llm-proxy`, `notifications`

## AppState
```rust
#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    pub db: PgPool,
    pub redis: deadpool_redis::Pool,
    pub orchestrator: Arc<Orchestrator>,
    pub llm_proxy: Arc<LlmProxy>,
    pub metrics_handle: PrometheusHandle,
}
```

## Route Map
| Method | Path | Auth | Handler |
|--------|------|------|---------|
| POST | /api/auth/signup | None | Create account, return JWT |
| POST | /api/auth/login | None | Verify credentials, return JWT |
| POST | /api/auth/refresh | JWT | Issue fresh token |
| GET | /api/agents | JWT | List user's agents |
| POST | /api/agents | JWT | Create agent (→ orchestrator) |
| GET | /api/agents/{id} | JWT | Get agent details (ownership check) |
| DELETE | /api/agents/{id} | JWT | Destroy agent (→ orchestrator) |
| WS | /api/agents/{id}/chat | JWT (query param) | Real-time chat |
| GET | /api/schedules | JWT | List schedules |
| POST | /api/schedules | JWT | Create schedule (cron parse) |
| DELETE | /api/schedules/{id} | JWT | Cancel schedule |
| GET | /api/usage | JWT | Usage stats (today + all-time) |
| GET | /api/notifications | JWT | List notifications |
| POST | /internal/llm/v1/chat/completions | X-Agent-Id/X-User-Id | LLM proxy |
| POST | /internal/schedules | X-Agent-Id/X-User-Id | Agent creates schedule |
| POST | /internal/notifications | X-Agent-Id/X-User-Id | Agent sends notification |
| GET | /health | None | Liveness probe ("ok") |
| GET | /metrics | None | Prometheus metrics |
| GET | /swagger-ui/ | None | Swagger UI (HTML+CDN) |
| GET | /api-docs/openapi.json | None | OpenAPI spec |

## Middleware
- **AuthUser**: Extracts JWT from `Authorization: Bearer` header. Makes `Claims` available.
- **InternalAuth**: Extracts `X-Agent-Id` and `X-User-Id` headers for agent→backend calls.
- **Rate Limit**: Redis INCR on `ratelimit:{user_id}:{date}`. Returns `(count, limit)`.

## WebSocket Chat Flow
1. JWT validated from `?token=` query param (not header — WebSocket limitation)
2. Agent ownership verified via DB query
3. If agent stopped → wake via orchestrator, send status messages to client
4. Message loop: client sends JSON → forward to agent HTTP API → send response to client
5. Update `agents.last_active` after each exchange

## Extension
- New endpoint: add handler in appropriate `routes/*.rs`, register in `routes()` or `create_router()`
- New middleware: add to `middleware/`, apply via `.layer()` on router
