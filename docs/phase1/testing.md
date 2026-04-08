# Testing Guide

## Test Levels

```
┌─────────────────────┐
│    E2E Tests         │  12 tests, real infra + mock/real runtime
│    bash tests/       │  Run: on VM with Firecracker
│    run_e2e.sh        │  (~5 min with real FC VMs)
└──────────┬──────────┘
           │
┌──────────┴──────────┐
│  Integration Tests   │  ~30 tests, real DB, mock runtime
│  cargo test          │  Run: CI on every PR (~30s)
│  --features integ    │
└──────────┬──────────┘
           │
┌──────────┴──────────┐
│    Unit Tests        │  ~100 tests, everything mocked
│    cargo test        │  Run: locally + CI (~5s)
└─────────────────────┘
```

## E2E Tests

Full end-to-end tests covering the complete agent lifecycle via HTTP API.

**Location**: `backend/tests/e2e_workflow.rs` + `backend/tests/run_e2e.sh`

**What they test** (12 test functions):
1. Health check endpoint
2. User signup
3. User login
4. Agent creation (with rootfs copy)
5. Agent wake (cold boot + health check)
6. Chat roundtrip (user → bridge → LLM → response)
7. Agent sleep (snapshot save)
8. Agent wake from snapshot (restore)
9. Chat after snapshot wake
10. Agent destroy
11. Duplicate signup rejection
12. Unauthorized access prevention

**Running**:
```bash
# Mock runtime (no Firecracker/Docker needed)
cd backend && cargo test --test e2e_workflow

# Full Firecracker runtime (requires KVM + TAP + rootfs)
cd backend && bash tests/run_e2e.sh
```

**Prerequisites for Firecracker E2E**:
- KVM enabled (`/dev/kvm` exists)
- Firecracker + jailer binaries in `/usr/local/bin/`
- Kernel at `/opt/firecracker/vmlinux-6.1`
- Rootfs template at `/opt/firecracker/rootfs-openclaw.ext4`
- TAP devices configured
- PostgreSQL and Redis running

## Unit Tests

Pure logic, no external dependencies. Run in seconds.

```bash
cargo test
```

### Examples

**Orchestrator — concurrent wake protection:**
```rust
#[tokio::test]
async fn test_double_wake_only_starts_once() {
    let runtime = MockRuntime::new();
    let orch = Orchestrator::new(Box::new(runtime.clone()), test_db().await);

    // Simulate two concurrent wake requests
    let (r1, r2) = tokio::join!(
        orch.wake("agent-1"),
        orch.wake("agent-1"),
    );

    // Both succeed, but runtime.start() called only once
    assert!(r1.is_ok());
    assert!(r2.is_ok());
    assert_eq!(runtime.start_count("agent-1"), 1);
}
```

**LLM Proxy — fallback on rate limit:**
```rust
#[tokio::test]
async fn test_fallback_on_rate_limit() {
    // Mock Groq returning 429, OpenRouter returning 200
    let groq = wiremock::MockServer::start().await;
    Mock::given(any()).respond_with(ResponseTemplate::new(429))
        .mount(&groq).await;

    let openrouter = wiremock::MockServer::start().await;
    Mock::given(any()).respond_with(
        ResponseTemplate::new(200).set_body_json(mock_llm_response())
    ).mount(&openrouter).await;

    let proxy = LlmProxy::new(vec![
        Provider::new("groq", &groq.uri()),
        Provider::new("openrouter", &openrouter.uri()),
    ]);

    let resp = proxy.complete(mock_request()).await.unwrap();
    assert_eq!(resp.provider, "openrouter"); // fell through to second
}
```

**Scheduler — cron next_run_at calculation:**
```rust
#[test]
fn test_next_run_every_3_hours() {
    let now = Utc.with_ymd_and_hms(2026, 4, 3, 10, 0, 0).unwrap();
    let next = calculate_next_run("0 */3 * * *", now);
    assert_eq!(next.hour(), 12); // next is noon
}
```

**Rate limiter:**
```rust
#[tokio::test]
async fn test_rate_limit_blocks_at_50() {
    let redis = test_redis().await;
    let limiter = RateLimiter::new(redis, 50);

    for i in 1..=50 {
        assert!(limiter.check("user-1").await.is_ok());
    }
    assert!(limiter.check("user-1").await.is_err()); // 51st blocked
}
```

## Integration Tests

Real PostgreSQL + Redis via testcontainers. Mock only the agent runtime.

```bash
cargo test --features integration
```

### Setup

```rust
// Shared test context — spins up real DB + Redis
struct TestContext {
    db: PgPool,
    redis: RedisPool,
    app: axum::Router,
}

impl TestContext {
    async fn new() -> Self {
        // Start real PostgreSQL container
        let pg = testcontainers::PostgresImage::default().start().await;
        let db = PgPool::connect(&pg.connection_string()).await.unwrap();
        sqlx::migrate!().run(&db).await.unwrap();

        // Start real Redis container
        let redis = testcontainers::RedisImage::default().start().await;
        let redis_pool = connect_redis(&redis.connection_string()).await;

        // Build app with MockRuntime
        let runtime = MockRuntime::new();
        let app = build_app(db.clone(), redis_pool.clone(), Box::new(runtime));

        Self { db, redis: redis_pool, app }
    }
}
```

### Examples

**Full auth flow:**
```rust
#[tokio::test]
async fn test_signup_login_flow() {
    let ctx = TestContext::new().await;

    // Signup
    let res = ctx.post("/api/auth/signup", json!({
        "email": "test@test.com", "password": "password123"
    })).await;
    assert_eq!(res.status(), 201);
    let token = res.json()["token"].as_str().unwrap();

    // Use token to create agent
    let res = ctx.authed(token).post("/api/agents", json!({})).await;
    assert_eq!(res.status(), 201);

    // Login with same credentials
    let res = ctx.post("/api/auth/login", json!({
        "email": "test@test.com", "password": "password123"
    })).await;
    assert_eq!(res.status(), 200);
}
```

**Rate limiting with real Redis:**
```rust
#[tokio::test]
async fn test_rate_limit_enforced_on_chat() {
    let ctx = TestContext::new().await;
    let token = ctx.create_user_and_agent().await;

    // Send 50 messages (limit)
    for _ in 0..50 {
        let res = ctx.authed(token).post("/api/agents/1/message", json!({"content": "hi"})).await;
        assert_eq!(res.status(), 200);
    }

    // 51st should be rate limited
    let res = ctx.authed(token).post("/api/agents/1/message", json!({"content": "hi"})).await;
    assert_eq!(res.status(), 429);
}
```

**Schedule creation and retrieval:**
```rust
#[tokio::test]
async fn test_create_and_list_schedules() {
    let ctx = TestContext::new().await;
    let token = ctx.create_user_and_agent().await;

    let res = ctx.authed(token).post("/api/schedules", json!({
        "agent_id": "uuid",
        "cron_expr": "0 */3 * * *",
        "task_message": "Check flights"
    })).await;
    assert_eq!(res.status(), 201);
    assert!(res.json()["next_run_at"].is_string());

    let res = ctx.authed(token).get("/api/schedules").await;
    assert_eq!(res.json()["schedules"].as_array().unwrap().len(), 1);
}
```

## Running Tests

```bash
# Unit tests only (fast, no Docker needed)
cargo test

# Integration tests (needs Docker for testcontainers)
cargo test --features integration

# All tests
cargo test --all-features

# With output
cargo test -- --nocapture

# Specific crate
cargo test -p oneclick-orchestrator
cargo test -p oneclick-llm-proxy
```

## CI Configuration

```yaml
# .github/workflows/test.yml
name: Test
on: [pull_request]

jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2

      # Unit tests
      - run: cargo test

      # Integration tests (GitHub Actions has Docker)
      - run: cargo test --features integration
```
