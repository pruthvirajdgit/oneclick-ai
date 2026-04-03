use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize logging
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "oneclick_backend=debug,tower_http=debug".into()),
        )
        .with(tracing_subscriber::fmt::layer().json())
        .init();

    tracing::info!("Starting OneClick.ai backend");

    // TODO: Load config
    // TODO: Connect to PostgreSQL
    // TODO: Connect to Redis
    // TODO: Initialize orchestrator
    // TODO: Build axum router
    // TODO: Start scheduler
    // TODO: Start idle monitor
    // TODO: Start HTTP server

    tracing::info!("OneClick.ai backend started");

    Ok(())
}
