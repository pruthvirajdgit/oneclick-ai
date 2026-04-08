//! Shared test utilities for integration tests.
//!
//! Provides a [`MockRuntime`] that fakes Docker operations and a
//! [`TestApp`] builder that wires up the full Axum router against
//! a real PostgreSQL database (started via `docker compose up postgres`).

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};

use async_trait::async_trait;

use oneclick_orchestrator::AgentRuntime;
use oneclick_shared::config::Config;
use oneclick_shared::errors::AppResult;
use oneclick_shared::models::agent::Agent;

// ---------------------------------------------------------------------------
// MockRuntime
// ---------------------------------------------------------------------------

/// A fake agent runtime that records calls without touching Docker.
///
/// Every `create_agent` call returns a unique deterministic container ID.
/// `start_agent`, `stop_agent`, and `destroy_agent` are all no-ops.
/// `health_check` always returns `true`.
pub struct MockRuntime {
    counter: AtomicU64,
}

impl MockRuntime {
    pub fn new() -> Self {
        Self {
            counter: AtomicU64::new(0),
        }
    }
}

#[async_trait]
impl AgentRuntime for MockRuntime {
    async fn create_agent(&self, _agent: &Agent, _config: &Config) -> AppResult<String> {
        let id = self.counter.fetch_add(1, Ordering::SeqCst);
        Ok(format!("mock-container-{id}"))
    }

    async fn start_agent(&self, _container_id: &str) -> AppResult<()> {
        Ok(())
    }

    async fn stop_agent(&self, _container_id: &str) -> AppResult<()> {
        Ok(())
    }

    async fn destroy_agent(&self, _container_id: &str) -> AppResult<()> {
        Ok(())
    }

    async fn health_check(&self, _container_id: &str) -> AppResult<bool> {
        Ok(true)
    }

    async fn get_host_port(&self, _container_id: &str) -> AppResult<Option<u16>> {
        Ok(Some(39999))
    }

    async fn get_agent_address(&self, _container_id: &str) -> AppResult<String> {
        Ok("127.0.0.1".to_string())
    }

    fn agent_name(&self, user_id: &uuid::Uuid, agent_id: &uuid::Uuid) -> String {
        format!("mock-agent-{}-{}", &user_id.to_string()[..8], &agent_id.to_string()[..8])
    }

    fn health_check_budget(&self) -> (u32, std::time::Duration) {
        (1, std::time::Duration::from_millis(10))
    }
}

// ---------------------------------------------------------------------------
// TestApp
// ---------------------------------------------------------------------------

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::Router;
use metrics_exporter_prometheus::{PrometheusBuilder, PrometheusHandle};
use sqlx::PgPool;
use tower::ServiceExt;
use uuid::Uuid;

use oneclick_api::state::AppState;
use oneclick_llm_proxy::LlmProxy;
use oneclick_notifications::NotificationService;
use oneclick_orchestrator::Orchestrator;

/// Global Prometheus recorder — installed exactly once across all tests.
static METRICS_HANDLE: OnceLock<PrometheusHandle> = OnceLock::new();

/// A test harness wrapping the full Axum router.
///
/// Build with [`TestApp::new`], then use [`TestApp::request`] or the
/// convenience methods ([`get`], [`post`], [`delete`]) to exercise endpoints.
/// Requires an externally running PostgreSQL instance (e.g., via `docker compose up postgres`).
#[allow(dead_code)]
pub struct TestApp {
    pub router: Router,
    pub db: PgPool,
}

impl TestApp {
    /// Create a test app backed by the given database pool.
    ///
    /// Uses [`MockRuntime`] so no Docker daemon is needed.
    pub fn new(db: PgPool) -> Self {
        let config = Arc::new(Config {
            database_url: String::new(),
            redis_url: "redis://127.0.0.1:6379".into(),
            jwt_secret: "test-secret-key-for-integration-tests".into(),
            jwt_expiry_hours: 24,
            groq_api_key: "test-groq-key".into(),
            openrouter_api_key: "test-openrouter-key".into(),
            agent_image: "oneclick-agent:latest".into(),
            agent_memory_limit: "512m".into(),
            agent_cpu_limit: 0.5,
            max_agents: 100,
            free_tier_daily_limit: 50,
            idle_timeout_minutes: 15,
            docker_network: "test-net".into(),
            internal_secret: "test-internal-secret".into(),
            cors_allowed_origins: "*".into(),
            agent_runtime: "docker".into(),
            fc_kernel_path: String::new(),
            fc_rootfs_template: String::new(),
            fc_snapshot_dir: String::new(),
            fc_vm_dir: String::new(),
            fc_vcpu_count: 2,
            fc_mem_size_mib: 1536,
            fc_tap_prefix: "tap".into(),
            fc_tap_count: 16,
            fc_subnet_prefix: "172.16".into(),
        });

        let runtime = MockRuntime::new();
        let orchestrator = Arc::new(Orchestrator::new(Arc::new(runtime), db.clone()));
        let llm_proxy = Arc::new(LlmProxy::new(&config, db.clone()));
        let notification_service = Arc::new(NotificationService::new(db.clone()));

        // Redis pool — will fail if actually used. Basic CRUD tests skip Redis.
        let redis_cfg = deadpool_redis::Config::from_url("redis://127.0.0.1:6379");
        let redis_pool = redis_cfg
            .create_pool(Some(deadpool_redis::Runtime::Tokio1))
            .expect("Redis pool config");

        let metrics_handle = METRICS_HANDLE
            .get_or_init(|| {
                PrometheusBuilder::new()
                    .install_recorder()
                    .expect("Prometheus recorder")
            })
            .clone();

        // Docker client — not used by any route handler but required by AppState.
        // connect_with_local_defaults() always succeeds (connection is lazy).
        let docker = bollard::Docker::connect_with_local_defaults()
            .expect("Docker client creation should not fail");

        let state = AppState {
            config,
            db: db.clone(),
            redis: redis_pool,
            docker,
            orchestrator,
            llm_proxy,
            notification_service,
            metrics_handle,
        };

        let router = oneclick_api::create_router(state);

        Self { router, db }
    }

    /// Send an arbitrary request and return `(StatusCode, body string)`.
    pub async fn request(&self, req: Request<Body>) -> (StatusCode, String) {
        let response = self
            .router
            .clone()
            .oneshot(req)
            .await
            .expect("Request failed");

        let status = response.status();
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("Failed to read body");
        let body_str = String::from_utf8(body.to_vec()).expect("Non-UTF8 body");

        (status, body_str)
    }

    /// `GET` a path with optional auth token.
    pub async fn get(&self, path: &str, token: Option<&str>) -> (StatusCode, String) {
        let mut builder = Request::builder().method("GET").uri(path);
        if let Some(t) = token {
            builder = builder.header("Authorization", format!("Bearer {t}"));
        }
        let req = builder.body(Body::empty()).unwrap();
        self.request(req).await
    }

    /// `POST` JSON to a path with optional auth token.
    pub async fn post(
        &self,
        path: &str,
        body: serde_json::Value,
        token: Option<&str>,
    ) -> (StatusCode, String) {
        let mut builder = Request::builder()
            .method("POST")
            .uri(path)
            .header("Content-Type", "application/json");
        if let Some(t) = token {
            builder = builder.header("Authorization", format!("Bearer {t}"));
        }
        let req = builder
            .body(Body::from(serde_json::to_string(&body).unwrap()))
            .unwrap();
        self.request(req).await
    }

    /// `DELETE` a path with auth token.
    pub async fn delete(&self, path: &str, token: &str) -> (StatusCode, String) {
        let req = Request::builder()
            .method("DELETE")
            .uri(path)
            .header("Authorization", format!("Bearer {token}"))
            .body(Body::empty())
            .unwrap();
        self.request(req).await
    }

    // -----------------------------------------------------------------------
    // Internal endpoint helpers (header-based auth)
    // -----------------------------------------------------------------------

    /// `POST` JSON to an internal endpoint with X-Agent-Id/X-User-Id/X-Internal-Secret headers.
    pub async fn post_internal(
        &self,
        path: &str,
        body: serde_json::Value,
        agent_id: Uuid,
        user_id: Uuid,
        secret: &str,
    ) -> (StatusCode, String) {
        let req = Request::builder()
            .method("POST")
            .uri(path)
            .header("Content-Type", "application/json")
            .header("X-Agent-Id", agent_id.to_string())
            .header("X-User-Id", user_id.to_string())
            .header("X-Internal-Secret", secret)
            .body(Body::from(serde_json::to_string(&body).unwrap()))
            .unwrap();
        self.request(req).await
    }

    /// `GET` an internal endpoint with X-Agent-Id/X-User-Id/X-Internal-Secret headers.
    pub async fn get_internal(
        &self,
        path: &str,
        agent_id: Uuid,
        user_id: Uuid,
        secret: &str,
    ) -> (StatusCode, String) {
        let req = Request::builder()
            .method("GET")
            .uri(path)
            .header("X-Agent-Id", agent_id.to_string())
            .header("X-User-Id", user_id.to_string())
            .header("X-Internal-Secret", secret)
            .body(Body::empty())
            .unwrap();
        self.request(req).await
    }

    /// `DELETE` an internal endpoint with X-Agent-Id/X-User-Id/X-Internal-Secret headers.
    pub async fn delete_internal(
        &self,
        path: &str,
        agent_id: Uuid,
        user_id: Uuid,
        secret: &str,
    ) -> (StatusCode, String) {
        let req = Request::builder()
            .method("DELETE")
            .uri(path)
            .header("X-Agent-Id", agent_id.to_string())
            .header("X-User-Id", user_id.to_string())
            .header("X-Internal-Secret", secret)
            .body(Body::empty())
            .unwrap();
        self.request(req).await
    }

    /// `POST` JSON with a composite Bearer token (`secret|agent_id|user_id`).
    pub async fn post_with_bearer(
        &self,
        path: &str,
        body: serde_json::Value,
        bearer: &str,
    ) -> (StatusCode, String) {
        let req = Request::builder()
            .method("POST")
            .uri(path)
            .header("Content-Type", "application/json")
            .header("Authorization", format!("Bearer {bearer}"))
            .body(Body::from(serde_json::to_string(&body).unwrap()))
            .unwrap();
        self.request(req).await
    }
}
