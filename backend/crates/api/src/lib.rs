//! OneClick.ai — HTTP API layer.
//!
//! This crate provides all public and internal HTTP endpoints, middleware, and
//! Swagger UI. It delegates business logic to the orchestrator, llm-proxy, and
//! shared crates.

pub mod middleware;
pub mod routes;
pub mod state;
pub mod swagger;

use axum::routing::get;
use axum::Router;
use axum::http::{HeaderValue, Method, Request};
use tower_http::cors::{AllowOrigin, CorsLayer};
use tower_http::trace::TraceLayer;

use crate::state::AppState;

/// Build the top-level Axum [`Router`] with all routes, middleware, and state.
pub fn create_router(state: AppState) -> Router {
    let api_routes = Router::new()
        .nest("/api/auth", routes::auth::routes())
        .nest("/api/agents", routes::agents::routes())
        .route("/api/agents/{id}/chat", get(routes::chat::ws_handler))
        .nest("/api/schedules", routes::schedules::routes())
        .route("/api/usage", get(routes::usage::get_usage))
        .nest("/api/notifications", routes::notifications::routes())
        .route("/agent-ui/{id}", get(routes::agent_ui::proxy_agent_ui))
        .route("/agent-ui/{id}/{*rest}", get(routes::agent_ui::proxy_agent_ui));

    let internal_routes = Router::new()
        .route(
            "/internal/llm/v1/chat/completions",
            axum::routing::post(routes::internal::llm_proxy),
        )
        .route(
            "/internal/schedules",
            axum::routing::get(routes::internal::list_internal_schedules)
                .post(routes::internal::create_internal_schedule),
        )
        .route(
            "/internal/schedules/{id}",
            axum::routing::delete(routes::internal::delete_internal_schedule),
        )
        .route(
            "/internal/notifications",
            axum::routing::post(routes::internal::create_internal_notification),
        );

    // Configure CORS policy from config.cors_allowed_origins.
    let allow_origin = if state.config.cors_allowed_origins == "*" {
        AllowOrigin::any()
    } else {
        let origins: Vec<HeaderValue> = state
            .config
            .cors_allowed_origins
            .split(',')
            .filter_map(|o| o.trim().parse().ok())
            .collect();
        AllowOrigin::list(origins)
    };
    let cors = CorsLayer::new()
        .allow_origin(allow_origin)
        .allow_methods([Method::GET, Method::POST, Method::PUT, Method::DELETE, Method::OPTIONS])
        .allow_headers(tower_http::cors::Any);

    Router::new()
        .merge(api_routes)
        .merge(internal_routes)
        .merge(swagger::swagger_routes())
        .route("/health", get(health_check))
        .route("/metrics", get(metrics_endpoint))
        .layer(cors)
        // Redact query strings to avoid leaking JWT tokens from WebSocket URLs.
        .layer(
            TraceLayer::new_for_http().make_span_with(|request: &Request<_>| {
                tracing::info_span!(
                    "http_request",
                    method = %request.method(),
                    uri_path = %request.uri().path(),
                )
            }),
        )
        .with_state(state)
}

/// Simple liveness probe — returns `"ok"` with 200.
async fn health_check() -> &'static str {
    "ok"
}

/// Render Prometheus metrics collected by the global recorder.
async fn metrics_endpoint(
    axum::extract::State(state): axum::extract::State<AppState>,
) -> String {
    state.metrics_handle.render()
}