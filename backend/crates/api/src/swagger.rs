//! OpenAPI / Swagger UI configuration.
//!
//! Generates an OpenAPI spec via [`utoipa`] and serves a Swagger UI page at
//! `/swagger-ui/`. The spec JSON is available at `/api-docs/openapi.json`.

use axum::response::{Html, IntoResponse};
use axum::routing::get;
use axum::{Json, Router};
use utoipa::OpenApi;

use crate::state::AppState;

/// OpenAPI document describing all OneClick.ai API endpoints.
#[derive(OpenApi)]
#[openapi(
    info(
        title = "OneClick.ai API",
        version = "0.1.0",
        description = "Multi-tenant AI agent platform — manage agents, schedules, and usage.",
        license(name = "MIT")
    ),
    tags(
        (name = "auth", description = "Authentication: signup, login, token refresh"),
        (name = "agents", description = "Agent CRUD with orchestrator delegation"),
        (name = "schedules", description = "Cron-based schedule management"),
        (name = "usage", description = "Aggregated token usage statistics"),
        (name = "notifications", description = "User notification listing"),
        (name = "internal", description = "Internal endpoints for agent containers")
    )
)]
pub struct ApiDoc;

/// Return a router serving Swagger UI and the OpenAPI JSON spec.
pub fn swagger_routes() -> Router<AppState> {
    Router::new()
        .route("/api-docs/openapi.json", get(openapi_json))
        .route("/swagger-ui", get(swagger_ui_html))
        .route("/swagger-ui/", get(swagger_ui_html))
}

/// Serve the OpenAPI spec as JSON.
async fn openapi_json() -> impl IntoResponse {
    Json(ApiDoc::openapi())
}

/// Serve a Swagger UI HTML page that loads from the official CDN.
async fn swagger_ui_html() -> impl IntoResponse {
    Html(SWAGGER_UI_HTML)
}

/// Embedded Swagger UI HTML using pinned CDN versions.
///
/// Note: In production, consider serving these assets from the backend image
/// to eliminate CDN dependency. SRI hashes would require updating on each
/// version bump, so pinning to exact versions provides a reasonable balance.
const SWAGGER_UI_HTML: &str = r##"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <title>OneClick.ai — API Documentation</title>
    <link rel="stylesheet" href="https://unpkg.com/swagger-ui-dist@5.18.2/swagger-ui.css">
</head>
<body>
    <div id="swagger-ui"></div>
    <script src="https://unpkg.com/swagger-ui-dist@5.18.2/swagger-ui-bundle.js"></script>
    <script>
        SwaggerUIBundle({
            url: "/api-docs/openapi.json",
            dom_id: "#swagger-ui",
            presets: [SwaggerUIBundle.presets.apis, SwaggerUIBundle.SwaggerUIStandalonePreset],
            layout: "StandaloneLayout"
        });
    </script>
</body>
</html>"##;
