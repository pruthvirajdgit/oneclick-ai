use std::sync::Arc;
use std::time::Duration;

use metrics_exporter_prometheus::PrometheusBuilder;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
use tracing_appender::rolling;

use oneclick_shared::config::Config;
use oneclick_shared::db;
use oneclick_shared::redis;

use oneclick_api::state::AppState;
use oneclick_llm_proxy::LlmProxy;
use oneclick_monitor::IdleMonitor;
use oneclick_notifications::NotificationService;
use bollard::Docker;
use oneclick_orchestrator::{DockerRuntime, FirecrackerRuntime, Orchestrator, TapManager};
use oneclick_scheduler::Scheduler;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize structured logging (JSON to both stdout and daily-rotating file)
    let log_dir = std::env::var("LOG_DIR").unwrap_or_else(|_| "logs".to_string());
    std::fs::create_dir_all(&log_dir).expect("Failed to create log directory");
    let file_appender = rolling::daily(&log_dir, "backend.log");
    let (non_blocking_file, _guard) = tracing_appender::non_blocking(file_appender);

    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| {
            "oneclick_backend=debug,\
             oneclick_api=info,\
             oneclick_orchestrator=info,\
             oneclick_shared=warn,\
             oneclick_monitor=info,\
             oneclick_scheduler=info,\
             oneclick_llm_proxy=info,\
             oneclick_notifications=info,\
             tower_http=debug"
                .into()
        });

    tracing_subscriber::registry()
        .with(env_filter)
        .with(tracing_subscriber::fmt::layer().json())
        .with(
            tracing_subscriber::fmt::layer()
                .json()
                .with_writer(non_blocking_file),
        )
        .init();

    tracing::info!("Starting OneClick.ai backend");

    // ── Configuration ───────────────────────────────────────────────────
    let config = Config::from_env()?;
    let config = Arc::new(config);
    tracing::info!("Configuration loaded");

    // ── Database ────────────────────────────────────────────────────────
    let db_pool = db::create_pool(&config.database_url).await?;
    db::run_migrations(&db_pool).await?;

    // ── Redis ───────────────────────────────────────────────────────────
    let redis_pool = redis::create_pool(&config.redis_url)?;

    // ── Prometheus metrics ──────────────────────────────────────────────
    let metrics_handle = PrometheusBuilder::new()
        .install_recorder()
        .expect("Failed to install Prometheus recorder");

    // ── Docker client (shared for exec operations) ────────────────────
    let docker = Docker::connect_with_local_defaults()?;
    tracing::info!("Docker client connected");

    // ── Orchestrator ────────────────────────────────────────────────────
    let runtime: Arc<dyn oneclick_orchestrator::AgentRuntime> = match config.agent_runtime.as_str() {
        "firecracker" => {
            tracing::info!("Using Firecracker runtime");
            let tap_manager = Arc::new(TapManager::new(&config));
            Arc::new(FirecrackerRuntime::new(config.clone(), tap_manager))
        }
        _ => {
            tracing::info!("Using Docker runtime");
            Arc::new(DockerRuntime::new()?)
        }
    };
    let orchestrator = Arc::new(Orchestrator::new(runtime, db_pool.clone()));
    tracing::info!("Orchestrator initialized (runtime: {})", config.agent_runtime);

    // ── LLM Proxy ───────────────────────────────────────────────────────
    let llm_proxy = Arc::new(LlmProxy::new(&config, db_pool.clone()));

    // ── Notification Service ────────────────────────────────────────────
    let notification_service = Arc::new(NotificationService::new(db_pool.clone()));

    // ── App state ───────────────────────────────────────────────────────
    let state = AppState {
        config: config.clone(),
        db: db_pool.clone(),
        redis: redis_pool,
        docker,
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
