//! WebSocket chat handler for real-time agent conversations.
//!
//! Flow: client connects → JWT validated → agent woken → message sent to
//! agent container via `openclaw agent --json` CLI and response streamed back.
//!
//! OpenClaw agents use a complex WebSocket gateway protocol with device pairing.
//! Instead of implementing that protocol, we use the `openclaw agent` CLI
//! command via Docker exec, which handles authentication internally.

use std::time::Duration;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, Query, State};
use axum::response::IntoResponse;
use bollard::exec::{CreateExecOptions, StartExecResults};
use bollard::Docker;
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use oneclick_shared::auth::validate_token;
use oneclick_shared::errors::{AppError, AppResult};
use oneclick_shared::models::agent::Agent;

use crate::state::AppState;

/// Query parameters for the WebSocket chat endpoint.
///
/// # Security Note: WebSocket Authentication via Query Parameters
///
/// The browser `WebSocket` API does not support setting custom HTTP headers on the
/// handshake request, so we pass the JWT as a `?token=` query parameter. This
/// means the token may appear in access logs, proxy logs, and browser history.
/// Mitigations applied:
/// - `TraceLayer` logs only the URI path (query string redacted).
/// - Tokens should be short-lived; consider a one-time WS ticket exchange or
///   `Sec-WebSocket-Protocol` auth in a future iteration.
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

/// Core WebSocket loop: wake agent, relay messages via CLI, stream responses.
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
                    message: Some("Failed to wake agent".into()),
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

    let container_id = match &agent.container_id {
        Some(id) => id.clone(),
        None => {
            tracing::error!(agent_id = %agent_id, "Agent has no container ID");
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

        // Send message to agent via `openclaw agent` CLI (docker exec).
        // This handles the full OpenClaw gateway protocol internally.
        let _ = send_json(
            &mut socket,
            &OutgoingMessage {
                msg_type: "status".into(),
                content: None,
                message: Some("Thinking...".into()),
            },
        )
        .await;

        match exec_agent_message(&state.docker, &container_id, &incoming.content).await {
            Ok(response) => {
                let _ = send_json(
                    &mut socket,
                    &OutgoingMessage {
                        msg_type: "done".into(),
                        content: Some(response),
                        message: None,
                    },
                )
                .await;
            }
            Err(e) => {
                tracing::error!(error = %e, "Agent message failed");
                let _ = send_json(
                    &mut socket,
                    &OutgoingMessage {
                        msg_type: "error".into(),
                        content: None,
                        message: Some("Agent failed to respond".into()),
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

/// Execute a message on the agent container via `openclaw agent --json` CLI.
///
/// Uses Docker exec to run the OpenClaw CLI inside the agent container, which
/// handles the gateway WebSocket protocol, device authentication, and session
/// management internally.
async fn exec_agent_message(docker: &Docker, container_id: &str, message: &str) -> Result<String, String> {
    let exec = docker
        .create_exec(
            container_id,
            CreateExecOptions {
                cmd: Some(vec![
                    "openclaw",
                    "agent",
                    "--agent", "main",
                    "--message", message,
                    "--json",
                    "--timeout", "120",
                ]),
                env: Some(vec!["HOME=/home/node"]),
                attach_stdout: Some(true),
                attach_stderr: Some(true),
                ..Default::default()
            },
        )
        .await
        .map_err(|e| format!("Docker exec create failed: {e}"))?;

    let output = docker
        .start_exec(&exec.id, None)
        .await
        .map_err(|e| format!("Docker exec start failed: {e}"))?;

    let mut stdout = String::new();
    if let StartExecResults::Attached { mut output, .. } = output {
        let stream_result = tokio::time::timeout(Duration::from_secs(130), async {
            while let Some(chunk) = output.next().await {
                match chunk {
                    Ok(bollard::container::LogOutput::StdOut { message }) => {
                        stdout.push_str(&String::from_utf8_lossy(&message));
                    }
                    Ok(bollard::container::LogOutput::StdErr { message }) => {
                        let stderr = String::from_utf8_lossy(&message);
                        tracing::debug!(stderr = %stderr, "Agent stderr");
                    }
                    Err(e) => {
                        return Err(format!("Docker exec stream error: {e}"));
                    }
                    _ => {}
                }
            }
            Ok(())
        })
        .await;

        match stream_result {
            Err(_elapsed) => return Err("Docker exec timed out".into()),
            Ok(Err(e)) => return Err(e),
            Ok(Ok(())) => {}
        }
    }

    // Parse the JSON output from `openclaw agent --json`
    // The response has a `payloads` array with text responses
    tracing::debug!(stdout_len = stdout.len(), stdout_preview = %&stdout[..stdout.len().min(500)], "Agent stdout");
    if let Ok(data) = serde_json::from_str::<serde_json::Value>(&stdout) {
        if let Some(payloads) = data.get("payloads").and_then(|p| p.as_array()) {
            let texts: Vec<&str> = payloads
                .iter()
                .filter_map(|p| p.get("text").and_then(|t| t.as_str()))
                .collect();
            if !texts.is_empty() {
                return Ok(texts.join("\n"));
            }
        }
    }

    // If we couldn't parse JSON, return a generic error to avoid leaking
    // internal agent logs or error details to the client.
    if !stdout.trim().is_empty() {
        tracing::warn!(stdout_preview = %&stdout[..stdout.len().min(500)], "Agent returned non-JSON output");
    }
    tracing::error!("Agent message failed");
    Err("Agent returned an unexpected response".into())
}

/// Serialize and send a JSON message over the WebSocket.
async fn send_json(
    socket: &mut WebSocket,
    msg: &OutgoingMessage,
) -> Result<(), axum::Error> {
    let text = serde_json::to_string(msg).expect("OutgoingMessage is always serializable");
    socket.send(Message::Text(text.into())).await
}
