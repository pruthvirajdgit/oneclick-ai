use std::sync::Arc;
use std::time::Duration;

use metrics_exporter_prometheus::PrometheusBuilder;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use oneclick_shared::config::Config;
use oneclick_shared::db;
use oneclick_shared::redis;

use oneclick_api::state::AppState;
use oneclick_llm_proxy::LlmProxy;
use oneclick_monitor::IdleMonitor;
use oneclick_notifications::NotificationService;
use oneclick_orchestrator::{DockerRuntime, Orchestrator};
use oneclick_scheduler::Scheduler;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize structured logging (JSON format for machine consumption)
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "oneclick_backend=debug,tower_http=debug".into()),
        )
        .with(tracing_subscriber::fmt::layer().json())
        .init();

    tracing::info!("Starting OneClick.ai backend");

    // ── Configuration ───────────────────────────────────────────────────
    let config = Config::from_env()?;
    let config = Arc::new(config);
    tracing::info!("Configuration loaded");

    // Warn if using default internal secret
    if config.internal_secret == "oneclick-internal-secret-change-me" {
        tracing::warn!("⚠️  INTERNAL_SECRET is using the default value — set a unique secret for production");
    }

    // ── Database ────────────────────────────────────────────────────────
    let db_pool = db::create_pool(&config.database_url).await?;
    db::run_migrations(&db_pool).await?;

    // ── Redis ───────────────────────────────────────────────────────────
    let redis_pool = redis::create_pool(&config.redis_url)?;

    // ── Prometheus metrics ──────────────────────────────────────────────
    let metrics_handle = PrometheusBuilder::new()
        .install_recorder()
        .expect("Failed to install Prometheus recorder");

    // ── Orchestrator ────────────────────────────────────────────────────
    let runtime = DockerRuntime::new()?;
    let orchestrator = Arc::new(Orchestrator::new(Arc::new(runtime), db_pool.clone()));
    tracing::info!("Orchestrator initialized");

    // ── LLM Proxy ───────────────────────────────────────────────────────
    let llm_proxy = Arc::new(LlmProxy::new(&config, db_pool.clone()));

    // ── Notification Service ────────────────────────────────────────────
    let notification_service = Arc::new(NotificationService::new(db_pool.clone()));

    // ── App state ───────────────────────────────────────────────────────
    let state = AppState {
        config: config.clone(),
        db: db_pool.clone(),
        redis: redis_pool,
        orchestrator: orchestrator.clone(),
        llm_proxy,
        notification_service,
        metrics_handle,
    };

    // ── Axum router ─────────────────────────────────────────────────────
    let router = oneclick_api::create_router(state);

    // ── Background tasks ────────────────────────────────────────────────
    let scheduler = Scheduler::new(
        db_pool.clone(),
        orchestrator.clone(),
        Duration::from_secs(60),
    );
    tokio::spawn(async move {
        if let Err(e) = scheduler.run().await {
            tracing::error!(error = %e, "Scheduler exited with error");
        }
    });

    let monitor = IdleMonitor::new(
        db_pool.clone(),
        orchestrator.clone(),
        config.idle_timeout_minutes,
    );
    tokio::spawn(async move {
        if let Err(e) = monitor.run().await {
            tracing::error!(error = %e, "Idle monitor exited with error");
        }
    });

    // ── Start HTTP server ───────────────────────────────────────────────
    let listener = tokio::net::TcpListener::bind("0.0.0.0:8080").await?;
    tracing::info!("Listening on http://0.0.0.0:8080");
    tracing::info!("Swagger UI at http://localhost:8080/swagger-ui/");

    axum::serve(listener, router).await?;

    Ok(())
}
