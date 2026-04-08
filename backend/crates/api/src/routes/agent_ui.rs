//! Reverse proxy to agent container's OpenClaw web UI.
//!
//! Requests to `/agent-ui/{agent_id}/...` are proxied to the agent's
//! OpenClaw gateway on port 3000. No authentication is required here
//! because the OpenClaw UI is a standalone SPA that communicates with
//! its own gateway — auth is handled by the gateway's device pairing.

use axum::body::Body;
use axum::extract::{Request, State};
use axum::response::{IntoResponse, Response};
use reqwest::Client;

use oneclick_shared::errors::{AppError, AppResult};
use oneclick_shared::models::agent::Agent;

use crate::state::AppState;

/// Proxy handler for `/agent-ui/{id}` and `/agent-ui/{id}/*rest`.
///
/// Parses the agent ID from the URI path directly (to handle both the
/// base and wildcard routes with a single handler), looks up the
/// container name, then reverse-proxies to `http://{container}:3000/`.
pub async fn proxy_agent_ui(
    State(state): State<AppState>,
    req: Request,
) -> AppResult<impl IntoResponse> {
    let path = req.uri().path();

    // Extract agent ID from /agent-ui/{uuid}/...
    let after_prefix = path.strip_prefix("/agent-ui/").ok_or_else(|| {
        AppError::BadRequest("Invalid agent-ui path".into())
    })?;
    let id_str = after_prefix.split('/').next().unwrap_or("");
    let agent_id: uuid::Uuid = id_str.parse().map_err(|_| {
        AppError::BadRequest(format!("Invalid agent ID: {id_str}"))
    })?;

    let agent = sqlx::query_as::<_, Agent>("SELECT * FROM agents WHERE id = $1")
        .bind(agent_id)
        .fetch_optional(&state.db)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("Agent {agent_id} not found")))?;

    let _container_name = agent
        .container_name
        .ok_or_else(|| AppError::Internal("Agent has no container name".into()))?;

    // Get reachable address for this agent
    let agent_address = state
        .orchestrator
        .get_agent_address(agent_id)
        .await
        .map_err(|e| {
            tracing::warn!(agent_id = %agent_id, error = %e, "Failed to resolve agent address");
            AppError::AgentUnavailable(format!("Failed to resolve agent address: {e}"))
        })?;

    // Strip the /agent-ui/{id} prefix to get the downstream path
    let prefix = format!("/agent-ui/{agent_id}");
    let downstream_path = path.strip_prefix(&prefix).unwrap_or("/");
    let downstream_path = if downstream_path.is_empty() { "/" } else { downstream_path };

    let query = req.uri().query().map(|q| format!("?{q}")).unwrap_or_default();
    let target_url = format!("http://{agent_address}:3000{downstream_path}{query}");

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
        if let Ok(name) = axum::http::HeaderName::from_bytes(key.as_str().as_bytes()) {
            response = response.header(name, value.clone());
        }
    }

    Ok(response.body(Body::from(bytes)).unwrap())
}
