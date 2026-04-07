//! Agent CRUD endpoints with orchestrator delegation.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use uuid::Uuid;

use oneclick_shared::errors::{AppError, AppResult};
use oneclick_shared::models::agent::{Agent, AgentResponse, CreateAgentRequest, WakeResponse};

use crate::middleware::auth::AuthUser;
use crate::state::AppState;

/// Mount agent routes under a common prefix.
pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/", get(list_agents).post(create_agent))
        .route("/{id}", get(get_agent).delete(delete_agent))
        .route("/{id}/wake", post(wake_agent))
}

/// `GET /api/agents` — List the authenticated user's agents.
async fn list_agents(
    State(state): State<AppState>,
    auth: AuthUser,
) -> AppResult<impl IntoResponse> {
    tracing::info!(user_id = %auth.0.sub, "Listing agents");

    let agents = sqlx::query_as::<_, Agent>(
        "SELECT * FROM agents WHERE user_id = $1 ORDER BY created_at DESC",
    )
    .bind(auth.0.sub)
    .fetch_all(&state.db)
    .await?;

    let responses: Vec<AgentResponse> = agents.into_iter().map(AgentResponse::from).collect();

    Ok(Json(responses))
}

/// `POST /api/agents` — Create a new agent (delegates to orchestrator).
async fn create_agent(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(req): Json<CreateAgentRequest>,
) -> AppResult<impl IntoResponse> {
    tracing::info!(user_id = %auth.0.sub, model = %req.model, "Creating agent");

    let agent = state
        .orchestrator
        .create_agent(auth.0.sub, &req.model, &state.config)
        .await?;

    tracing::info!(agent_id = %agent.id, "Agent created");

    Ok((StatusCode::CREATED, Json(AgentResponse::from(agent))))
}

/// `GET /api/agents/:id` — Get agent details (ownership enforced).
async fn get_agent(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> AppResult<impl IntoResponse> {
    tracing::info!(user_id = %auth.0.sub, agent_id = %id, "Fetching agent");

    let agent = sqlx::query_as::<_, Agent>("SELECT * FROM agents WHERE id = $1")
        .bind(id)
        .fetch_optional(&state.db)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("Agent {id} not found")))?;

    if agent.user_id != auth.0.sub {
        return Err(AppError::NotFound(format!("Agent {id} not found")));
    }

    Ok(Json(AgentResponse::from(agent)))
}

/// `DELETE /api/agents/:id` — Destroy an agent (delegates to orchestrator, ownership enforced).
async fn delete_agent(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> AppResult<impl IntoResponse> {
    tracing::info!(user_id = %auth.0.sub, agent_id = %id, "Deleting agent");

    // Verify ownership before delegating to orchestrator.
    let agent = sqlx::query_as::<_, Agent>("SELECT * FROM agents WHERE id = $1")
        .bind(id)
        .fetch_optional(&state.db)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("Agent {id} not found")))?;

    if agent.user_id != auth.0.sub {
        return Err(AppError::NotFound(format!("Agent {id} not found")));
    }

    state.orchestrator.destroy_agent(id).await?;

    tracing::info!(agent_id = %id, "Agent destroyed");

    Ok(StatusCode::NO_CONTENT)
}

/// `POST /api/agents/:id/wake` — Wake an agent and return its OpenClaw chat UI URL.
///
/// Blocks until the agent is healthy (up to ~450s). The frontend should show
/// a loading state while this request is in flight, then open the returned
/// `chat_url` in a new browser tab.
async fn wake_agent(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> AppResult<impl IntoResponse> {
    tracing::info!(user_id = %auth.0.sub, agent_id = %id, "Wake agent requested");

    let agent = sqlx::query_as::<_, Agent>("SELECT * FROM agents WHERE id = $1")
        .bind(id)
        .fetch_optional(&state.db)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("Agent {id} not found")))?;

    if agent.user_id != auth.0.sub {
        return Err(AppError::NotFound(format!("Agent {id} not found")));
    }

    // Blocks until healthy or returns error after retries exhausted.
    let agent = state.orchestrator.ensure_ready(agent.id).await?;

    // Get the dynamically assigned host port for the OpenClaw UI
    let host_port = state
        .orchestrator
        .get_host_port(agent.id)
        .await?
        .ok_or_else(|| AppError::Internal("No host port mapped for agent".into()))?;

    // Build the chat URL. In GitHub Codespaces, ports are forwarded via
    // https://{codespace_name}-{port}.{domain}. Outside Codespaces, use localhost.
    // Append the gateway token so the OpenClaw UI auto-authenticates.
    let gw_token = "oneclick-internal";
    let chat_url = match (
        std::env::var("CODESPACE_NAME").ok().filter(|s| !s.is_empty()),
        std::env::var("GITHUB_CODESPACES_PORT_FORWARDING_DOMAIN").ok().filter(|s| !s.is_empty()),
    ) {
        (Some(codespace), Some(domain)) => {
            format!("https://{codespace}-{host_port}.{domain}/?token={gw_token}")
        }
        _ => format!("http://localhost:{host_port}/?token={gw_token}"),
    };
    tracing::info!(agent_id = %id, %chat_url, "Agent woken — chat URL ready");

    Ok(Json(WakeResponse {
        status: agent.status,
        chat_url,
    }))
}
