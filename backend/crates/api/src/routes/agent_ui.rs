//! Reverse proxy to agent container's OpenClaw web UI.
//!
//! Requests to `/agent-ui/{agent_id}/...` are proxied to the agent's
//! OpenClaw gateway on port 3000. No authentication is required here
//! because the OpenClaw UI is a standalone SPA that communicates with
//! its own gateway — auth is handled by the gateway's device pairing.

use axum::body::Body;
use axum::extract::{Path, Request, State};
use axum::response::{IntoResponse, Response};
use reqwest::Client;
use uuid::Uuid;

use oneclick_shared::errors::{AppError, AppResult};
use oneclick_shared::models::agent::Agent;

use crate::state::AppState;

/// Proxy handler for `/agent-ui/{id}` and `/agent-ui/{id}/*rest`.
///
/// Looks up the agent's container name, then forwards the request
/// (path, query, headers) to `http://{container_name}:3000/`.
pub async fn proxy_agent_ui(
    State(state): State<AppState>,
    Path(agent_id): Path<Uuid>,
    req: Request,
) -> AppResult<impl IntoResponse> {
    let agent = sqlx::query_as::<_, Agent>("SELECT * FROM agents WHERE id = $1")
        .bind(agent_id)
        .fetch_optional(&state.db)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("Agent {agent_id} not found")))?;

    let container_name = agent
        .container_name
        .ok_or_else(|| AppError::Internal("Agent has no container name".into()))?;

    // Strip the /agent-ui/{id} prefix to get the path the agent should see
    let uri = req.uri();
    let prefix = format!("/agent-ui/{agent_id}");
    let downstream_path = uri
        .path()
        .strip_prefix(&prefix)
        .unwrap_or("/");
    let downstream_path = if downstream_path.is_empty() { "/" } else { downstream_path };

    let query = uri.query().map(|q| format!("?{q}")).unwrap_or_default();
    let target_url = format!("http://{container_name}:3000{downstream_path}{query}");

    let client = Client::new();
    let proxy_resp = client
        .get(&target_url)
        .send()
        .await
        .map_err(|e| {
            tracing::error!(agent_id = %agent_id, url = %target_url, error = %e, "Agent UI proxy failed");
            AppError::AgentUnavailable(format!("Agent UI not reachable: {e}"))
        })?;

    let status = proxy_resp.status();
    let headers = proxy_resp.headers().clone();
    let bytes = proxy_resp.bytes().await.map_err(|e| {
        AppError::Internal(format!("Failed to read agent UI response: {e}"))
    })?;

    let mut response = Response::builder().status(status.as_u16());
    for (key, value) in headers.iter() {
        // Forward content-type and other relevant headers
        if let Ok(name) = axum::http::HeaderName::from_bytes(key.as_str().as_bytes()) {
            response = response.header(name, value.clone());
        }
    }

    Ok(response.body(Body::from(bytes)).unwrap())
}
