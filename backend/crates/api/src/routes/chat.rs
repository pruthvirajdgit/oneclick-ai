//! WebSocket chat handler for real-time agent conversations.
//!
//! Flow: client connects → JWT validated → agent woken → messages forwarded
//! to agent container and responses streamed back to the client.

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, Query, State};
use axum::response::IntoResponse;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use oneclick_shared::auth::validate_token;
use oneclick_shared::errors::{AppError, AppResult};
use oneclick_shared::models::agent::Agent;

use crate::state::AppState;

/// Query parameters for the WebSocket chat endpoint.
#[derive(Deserialize)]
pub struct ChatQuery {
    /// JWT token used for authentication (WebSocket cannot send headers).
    token: String,
}

/// Message sent from the client over the WebSocket.
#[derive(Deserialize)]
struct IncomingMessage {
    #[serde(rename = "type")]
    msg_type: String,
    content: String,
}

/// Message sent from the server to the client over the WebSocket.
#[derive(Serialize)]
struct OutgoingMessage {
    #[serde(rename = "type")]
    msg_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<String>,
}

/// Response payload from the agent container's HTTP chat API.
#[derive(Deserialize)]
struct AgentChatResponse {
    response: String,
}

/// WebSocket chat endpoint handler.
///
/// Authenticates via `?token=<jwt>` query parameter, verifies agent ownership,
/// then upgrades the connection to a WebSocket for real-time messaging.
///
/// # Route
///
/// `GET /api/agents/{id}/chat?token=<jwt>`
pub async fn ws_handler(
    State(state): State<AppState>,
    Path(agent_id): Path<Uuid>,
    Query(query): Query<ChatQuery>,
    ws: WebSocketUpgrade,
) -> AppResult<impl IntoResponse> {
    // Validate JWT from query parameter.
    let claims = validate_token(&query.token, &state.config.jwt_secret)
        .map_err(|_| AppError::Unauthorized)?;

    // Verify agent exists and belongs to the authenticated user.
    let agent = sqlx::query_as::<_, Agent>("SELECT * FROM agents WHERE id = $1")
        .bind(agent_id)
        .fetch_optional(&state.db)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("Agent {agent_id} not found")))?;

    if agent.user_id != claims.sub {
        return Err(AppError::NotFound(format!("Agent {agent_id} not found")));
    }

    tracing::info!(
        agent_id = %agent_id,
        user_id = %claims.sub,
        "WebSocket chat connection accepted"
    );

    Ok(ws.on_upgrade(move |socket| handle_socket(socket, state, agent_id, claims.sub)))
}

/// Core WebSocket loop: wake agent, relay messages, stream responses.
async fn handle_socket(mut socket: WebSocket, state: AppState, agent_id: Uuid, user_id: Uuid) {
    // --- Wake agent if needed ---
    if let Err(e) = send_json(
        &mut socket,
        &OutgoingMessage {
            msg_type: "status".into(),
            content: None,
            message: Some("Agent waking up...".into()),
        },
    )
    .await
    {
        tracing::error!(error = %e, "Failed to send wake status");
        return;
    }

    let agent = match state.orchestrator.ensure_ready(agent_id).await {
        Ok(agent) => agent,
        Err(e) => {
            tracing::error!(agent_id = %agent_id, error = %e, "Failed to wake agent");
            let _ = send_json(
                &mut socket,
                &OutgoingMessage {
                    msg_type: "error".into(),
                    content: None,
                    message: Some(format!("Failed to wake agent: {e}")),
                },
            )
            .await;
            return;
        }
    };

    if let Err(e) = send_json(
        &mut socket,
        &OutgoingMessage {
            msg_type: "status".into(),
            content: None,
            message: Some("Agent ready".into()),
        },
    )
    .await
    {
        tracing::error!(error = %e, "Failed to send ready status");
        return;
    }

    // Build base URL for the agent container's HTTP API.
    let container_name = match &agent.container_name {
        Some(name) => name.clone(),
        None => {
            tracing::error!(agent_id = %agent_id, "Agent has no container name");
            let _ = send_json(
                &mut socket,
                &OutgoingMessage {
                    msg_type: "error".into(),
                    content: None,
                    message: Some("Agent container not available".into()),
                },
            )
            .await;
            return;
        }
    };

    let http_client = reqwest::Client::new();
    let agent_url = format!("http://{container_name}:3000/api/chat");

    // --- Message loop ---
    while let Some(msg_result) = socket.recv().await {
        let msg = match msg_result {
            Ok(msg) => msg,
            Err(e) => {
                tracing::info!(
                    agent_id = %agent_id,
                    user_id = %user_id,
                    error = %e,
                    "WebSocket connection closed"
                );
                break;
            }
        };

        let text = match msg {
            Message::Text(t) => t,
            Message::Close(_) => {
                tracing::info!(agent_id = %agent_id, user_id = %user_id, "Client sent close frame");
                break;
            }
            // Ignore binary, ping, pong.
            _ => continue,
        };

        let incoming: IncomingMessage = match serde_json::from_str(&text) {
            Ok(m) => m,
            Err(e) => {
                tracing::warn!(error = %e, "Invalid message format from client");
                let _ = send_json(
                    &mut socket,
                    &OutgoingMessage {
                        msg_type: "error".into(),
                        content: None,
                        message: Some("Invalid message format".into()),
                    },
                )
                .await;
                continue;
            }
        };

        if incoming.msg_type != "message" {
            continue;
        }

        // Forward message to agent container.
        let agent_resp = http_client
            .post(&agent_url)
            .json(&serde_json::json!({ "message": incoming.content }))
            .send()
            .await;

        match agent_resp {
            Ok(resp) if resp.status().is_success() => {
                match resp.json::<AgentChatResponse>().await {
                    Ok(body) => {
                        let _ = send_json(
                            &mut socket,
                            &OutgoingMessage {
                                msg_type: "done".into(),
                                content: Some(body.response),
                                message: None,
                            },
                        )
                        .await;
                    }
                    Err(e) => {
                        tracing::error!(error = %e, "Failed to parse agent response");
                        let _ = send_json(
                            &mut socket,
                            &OutgoingMessage {
                                msg_type: "error".into(),
                                content: None,
                                message: Some("Failed to parse agent response".into()),
                            },
                        )
                        .await;
                    }
                }
            }
            Ok(resp) => {
                let status = resp.status();
                tracing::error!(status = %status, "Agent returned error status");
                let _ = send_json(
                    &mut socket,
                    &OutgoingMessage {
                        msg_type: "error".into(),
                        content: None,
                        message: Some(format!("Agent error (HTTP {status})")),
                    },
                )
                .await;
            }
            Err(e) => {
                tracing::error!(error = %e, "Failed to contact agent container");
                let _ = send_json(
                    &mut socket,
                    &OutgoingMessage {
                        msg_type: "error".into(),
                        content: None,
                        message: Some("Failed to contact agent".into()),
                    },
                )
                .await;
            }
        }

        // Update last_active timestamp after each exchange.
        if let Err(e) = sqlx::query("UPDATE agents SET last_active = NOW() WHERE id = $1")
            .bind(agent_id)
            .execute(&state.db)
            .await
        {
            tracing::error!(agent_id = %agent_id, error = %e, "Failed to update last_active");
        }
    }

    tracing::info!(
        agent_id = %agent_id,
        user_id = %user_id,
        "WebSocket chat session ended"
    );
}

/// Serialize and send a JSON message over the WebSocket.
async fn send_json(
    socket: &mut WebSocket,
    msg: &OutgoingMessage,
) -> Result<(), axum::Error> {
    let text = serde_json::to_string(msg).expect("OutgoingMessage is always serializable");
    socket.send(Message::Text(text.into())).await
}
