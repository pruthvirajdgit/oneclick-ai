# Module: shared

**Crate:** `oneclick-shared`
**Path:** `backend/crates/shared/`
**Role:** Foundation crate. Types, config, database, auth utilities. No business logic.

## Dependency Rule
Every other crate depends on `shared`. `shared` depends on NO workspace crate.

## Files
| File | Exports |
|------|---------|
| `config.rs` | `Config` — all env vars with defaults, loaded via `Config::from_env()` |
| `db.rs` | `create_pool(url)`, `run_migrations(pool)` — PgPool + sqlx migrations |
| `redis.rs` | `create_pool(url)` — deadpool-redis Pool |
| `errors.rs` | `AppError` enum (8 variants), `AppResult<T>` type alias |
| `auth.rs` | `hash_password`, `verify_password`, `create_token`, `validate_token`, `Claims` struct |
| `models/user.rs` | `User`, `CreateUserRequest`, `LoginRequest`, `AuthResponse`, `UserResponse` |
| `models/agent.rs` | `Agent`, `AgentStatus` enum (Creating/Running/Stopped/Error), `CreateAgentRequest`, `AgentResponse` |
| `models/schedule.rs` | `ScheduledJob`, `ScheduleStatus` enum, `CreateScheduleRequest`, `ScheduleResponse` |
| `models/usage.rs` | `Usage`, `UsageStats`, `DailyUsage`, `TotalUsage` |
| `models/message.rs` | `QueuedMessage`, `MessageStatus` enum |
| `models/notification.rs` | `Notification`, `CreateNotificationRequest` |

## Key Types

### AppError
Maps to HTTP status codes via `IntoResponse`:
- `NotFound(String)` → 404
- `BadRequest(String)` → 400
- `Unauthorized` → 401
- `RateLimited { limit, resets_at }` → 429
- `Conflict(String)` → 409
- `CapacityReached` → 403
- `AgentUnavailable(String)` → 503
- `Internal(String)` → 500
- `Database(sqlx::Error)` → 500
- `Redis(PoolError)` → 500

### Config
Required env vars (startup fails if missing): `DATABASE_URL`, `JWT_SECRET`, `INTERNAL_SECRET`.
Optional with defaults: all others (see `Config::from_env()`).
- `cors_allowed_origins: Vec<String>` — from `CORS_ALLOWED_ORIGINS` (comma-separated), defaults to `*`
- Validates on startup that at least one LLM provider key (`GROQ_API_KEY` or `OPENROUTER_API_KEY`) is set.

## Tests
- `auth::tests` — password hash/verify, JWT create/validate, invalid secret rejection (3 tests)

## Extension
- Add new model: create file in `models/`, add `pub mod` in `models/mod.rs`, re-export in `lib.rs`
- Add new error variant: add to `AppError` enum + `IntoResponse` match arm
