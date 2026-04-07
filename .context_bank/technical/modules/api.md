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
    pub docker: Arc<bollard::Docker>,
    pub metrics_handle: PrometheusHandle,
}
```
Docker client is shared via `AppState` (not created per-message) and used for `docker exec` into agent containers.

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
| POST | /api/agents/{id}/wake | JWT | Wake a sleeping agent (~450s budget) |
| WS | /api/agents/{id}/chat | JWT (query param) | Real-time chat with token streaming |
| GET | /api/schedules | JWT | List schedules |
| POST | /api/schedules | JWT | Create schedule (cron parse) |
| DELETE | /api/schedules/{id} | JWT | Cancel schedule |
| GET | /api/usage | JWT | Usage stats (today + all-time) |
| GET | /api/notifications | JWT | List notifications |
| POST | /api/notifications/{id}/read | JWT | Mark notification as read |
| POST | /internal/llm/v1/chat/completions | Bearer token OR X-Agent-Id/X-User-Id | LLM proxy (SSE streaming supported) |
| POST | /internal/schedules | X-Agent-Id/X-User-Id | Agent creates schedule |
| POST | /internal/notifications | X-Agent-Id/X-User-Id | Agent sends notification |
| GET | /health | None | Liveness probe ("ok") |
| GET | /metrics | None | Prometheus metrics |
| GET | /swagger-ui/ | None | Swagger UI (HTML+CDN v5.18.2, pinned) |
| GET | /api-docs/openapi.json | None | OpenAPI spec |

## Middleware
- **AuthUser**: Extracts JWT from `Authorization: Bearer` header (case-insensitive per RFC 7235). Makes `Claims` available.
- **InternalAuth**: Extracts auth from `Authorization: Bearer` token (format: `secret|agent_id|user_id` encoded in `OPENROUTER_API_KEY`) OR legacy `X-Agent-Id`/`X-User-Id` headers. Confirms user owns agent via `SELECT EXISTS` DB query.
- **Rate Limit**: Split into two operations — `check_rate_limit` (read-only Redis GET pre-check before request) and `increment_rate_limit` (Redis INCR after successful LLM call only). Prevents counting failed requests toward limit.

## WebSocket Chat Flow
1. JWT validated from `?token=` query param (not header — WebSocket limitation)
2. Agent ownership verified via DB query
3. If agent stopped → wake via orchestrator, send status messages to client
4. Chat handler sends HTTP POST to chat-bridge.js (port 3001) inside the agent container. The bridge handles WebSocket authentication with the OpenClaw gateway via Ed25519 keypair and device pairing.
5. chat-bridge.js returns an SSE stream. Backend parses SSE `data:` events and forwards tokens to the client WebSocket as `{type: "chunk"}` messages in real-time.
6. If bridge returns 503 (gateway not ready), backend retries up to 10 times with 5s delay between attempts.
7. Status messages sent to client: "Waking up agent..." → "Agent ready" → "Thinking..." → streaming tokens → done
8. Update `agents.last_active` after each exchange
9. Error responses return generic messages — internal details are never leaked to the client

## Extension
- New endpoint: add handler in appropriate `routes/*.rs`, register in `routes()` or `create_router()`
- New middleware: add to `middleware/`, apply via `.layer()` on router
