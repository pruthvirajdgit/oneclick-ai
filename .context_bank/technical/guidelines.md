# Coding Guidelines & Design Rules

These are mandatory for all contributions to the OneClick.ai backend. AI agents must follow these when generating or modifying code.

## Rust Idiomatics

### Error Handling
- **Never panic in library code.** All fallible functions return `Result<T, AppError>` (aliased as `AppResult<T>`).
- Use `?` for propagation. Use `.map_err()` to convert external errors to `AppError` variants.
- `AppError` implements `IntoResponse` — handlers return `AppResult<impl IntoResponse>`.
- Log errors at the point of handling, not at the point of creation. Use `tracing::error!` with structured fields.

### Async
- All I/O is async via tokio. No blocking calls on the async runtime.
- Use `tokio::spawn` for background tasks. Use `Arc` for shared state across tasks.
- Per-agent locking uses `DashMap<Uuid, Arc<tokio::sync::Mutex<()>>>` — not std Mutex.

### Types
- Use newtypes and enums over raw strings. `AgentStatus::Running` not `"running"`.
- Derive `Serialize`, `Deserialize`, `FromRow` where appropriate.
- Enum variants stored as lowercase text in PostgreSQL via `#[sqlx(type_name = "text", rename_all = "lowercase")]`.
- Use `Uuid` for entity IDs, `DateTime<Utc>` for timestamps. Never `String` for either.

### Traits
- Use `#[async_trait]` for async trait methods.
- Trait objects use `Arc<dyn Trait>` for shared ownership.
- Design traits for testability: `AgentRuntime` can be mocked without Docker.

### Modules
- One responsibility per file. If a file exceeds ~300 lines, split it.
- `lib.rs` contains only `pub mod` declarations and `pub use` re-exports.
- Internal helpers are `pub(crate)` or private. Only types needed by other crates are `pub`.

## Code Style

### Documentation
- All `pub` items get doc comments (`///`).
- Crate-level docs (`//!`) explain what the crate does in 2-3 sentences.
- No comments that restate the code. Comment *why*, not *what*.

### Naming
- Crates: `oneclick-{name}` (kebab-case)
- Modules: `snake_case`
- Types: `PascalCase`
- Functions: `snake_case`
- Constants: `SCREAMING_SNAKE_CASE`

### Formatting
- `rustfmt` defaults. No custom formatting rules.
- `clippy` clean. Address all warnings.
- Group imports: std → external crates → workspace crates → crate-local.

## Architecture Rules

### Dependency Direction
```
shared ← everything (foundation, no business logic)
orchestrator ← scheduler, monitor, api (agent lifecycle)
llm-proxy ← api (LLM routing)
notifications ← api (alerts)
```
- **shared** never depends on any other workspace crate.
- **api** depends on orchestrator, llm-proxy, notifications. It is the "wiring" crate.
- **scheduler** and **monitor** depend on orchestrator only.
- No circular dependencies. The dependency graph is a DAG.

### Database Access
- Raw `sqlx` queries. No ORM, no query builder.
- Queries use `$1, $2` parameter binding — never string interpolation.
- All queries are in the crate that owns the domain (e.g., agent queries in orchestrator, usage queries in llm-proxy).
- Migrations in `backend/migrations/` using sqlx naming: `YYYYMMDDHHMMSS_description.sql`.
- Foreign keys on usage tables use `ON DELETE CASCADE` for referential integrity.
- All time comparisons use UTC. Day boundaries: `date_trunc('day', NOW() AT TIME ZONE 'UTC') AT TIME ZONE 'UTC'`.

### State Management
- `AppState` (in api crate) is the single shared state struct passed to all handlers.
- `AppState` is `Clone` — all fields are `Arc`-wrapped or internally reference-counted.
- Background tasks (scheduler, monitor) receive cloned `Arc<Orchestrator>` and `PgPool`.

### Testing
- Unit tests in the same file, inside `#[cfg(test)] mod tests {}`.
- Integration tests in `backend/tests/` with `#[cfg(feature = "integration")]`.
- `MockRuntime` (implements `AgentRuntime`) for testing without Docker.
- `TestApp` helper builds the full router with mock runtime.

## PR & Git Rules
- **Never push directly to main.** Always create a branch and open a PR.
- Branch naming: `feat/`, `fix/`, `docs/`, `refactor/` prefixes.
- Commit messages: conventional commits (`feat(crate): description`).
- All commits include: `Co-authored-by: Copilot <223556219+Copilot@users.noreply.github.com>` trailer.
- `cargo check` and `cargo test --workspace` must pass before committing.

## Extension Points
When adding new features, follow these patterns:

### Adding a new API endpoint
1. Add route function in `crates/api/src/routes/{module}.rs`
2. Register in `routes()` function or in `create_router()` in `lib.rs`
3. Use `AuthUser` extractor for protected routes
4. Return `AppResult<impl IntoResponse>`

### Adding a new agent runtime
1. Implement `AgentRuntime` trait (5 async methods)
2. Wire into `main.rs` based on config flag
3. No changes needed to scheduler, monitor, api — they use `Arc<dyn AgentRuntime>`

### Adding a new LLM provider
1. Add `ProviderConfig` entry in `LlmProxy::new()`
2. Same OpenAI-compatible API format — no new code needed if provider is compatible
3. Add provider name to usage logging

### Adding a new database table
1. Create migration: `backend/migrations/YYYYMMDDHHMMSS_description.sql`
2. Add model struct in `crates/shared/src/models/`
3. Add queries in the owning crate
