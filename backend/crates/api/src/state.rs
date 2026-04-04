//! Shared application state available to all handlers via Axum's `State` extractor.

use std::sync::Arc;

use metrics_exporter_prometheus::PrometheusHandle;
use sqlx::PgPool;

use oneclick_llm_proxy::LlmProxy;
use oneclick_notifications::NotificationService;
use oneclick_orchestrator::Orchestrator;
use oneclick_shared::config::Config;

/// Application-wide state shared across all request handlers.
///
/// Clone is cheap — every field is either `Arc`-wrapped or internally
/// reference-counted (e.g. `PgPool`, `deadpool_redis::Pool`, `PrometheusHandle`).
#[derive(Clone)]
pub struct AppState {
    /// Application configuration.
    pub config: Arc<Config>,
    /// PostgreSQL connection pool.
    pub db: PgPool,
    /// Redis connection pool (rate limiting, caching).
    pub redis: deadpool_redis::Pool,
    /// Agent lifecycle orchestrator.
    pub orchestrator: Arc<Orchestrator>,
    /// LLM provider proxy with fallback chain.
    pub llm_proxy: Arc<LlmProxy>,
    /// Notification service with real-time broadcast.
    pub notification_service: Arc<NotificationService>,
    /// Handle for rendering Prometheus metrics.
    pub metrics_handle: PrometheusHandle,
}
