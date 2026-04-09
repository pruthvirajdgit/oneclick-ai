//! WebSocket chat handler for real-time agent conversations.
//!
//! Flow: client connects → JWT validated → agent woken → backend POSTs to the
//! in-container chat bridge (port 3001) → bridge connects to OpenClaw gateway
//! via localhost WebSocket → SSE stream is parsed and forwarded token-by-token
//! to the client WebSocket.

use std::time::Duration;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, Query, State};
use axum::response::IntoResponse;
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use oneclick_shared::auth::validate_token;
use oneclick_shared::errors::{AppError, AppResult};

use crate::state::AppState;

// ── Client ↔ Backend message types ─────────────────────────────────────

#[derive(Deserialize)]
pub struct ChatQuery {
    token: String,
}

#[derive(Deserialize)]
struct IncomingMessage {
    #[serde(rename = "type")]
    msg_type: String,
    content: String,
}

#[derive(Serialize)]
struct OutgoingMessage {
    #[serde(rename = "type")]
    msg_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<String>,
}

// ── Public handler ─────────────────────────────────────────────────────

/// `GET /api/agents/{id}/chat?token=<jwt>`
pub async fn ws_handler(
    State(state): State<AppState>,
    Path(agent_id): Path<Uuid>,
    Query(query): Query<ChatQuery>,
    ws: WebSocketUpgrade,
) -> AppResult<impl IntoResponse> {
    let claims = validate_token(&query.token, &state.config.jwt_secret)
        .map_err(|_| AppError::Unauthorized)?;

    let agent = sqlx::query_as::<_, oneclick_shared::models::agent::Agent>(
        "SELECT * FROM agents WHERE id = $1",
    )
    .bind(agent_id)
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| AppError::NotFound(format!("Agent {agent_id} not found")))?;

    if agent.user_id != claims.sub {
        return Err(AppError::NotFound(format!("Agent {agent_id} not found")));
    }

    tracing::info!(agent_id = %agent_id, user_id = %claims.sub, "WebSocket chat connection accepted");

    Ok(ws.on_upgrade(move |socket| handle_socket(socket, state, agent_id, claims.sub)))
}

// ── Client WebSocket loop ──────────────────────────────────────────────

async fn handle_socket(mut socket: WebSocket, state: AppState, agent_id: Uuid, user_id: Uuid) {
    // Wake agent
    if send_status(&mut socket, "Agent waking up...").await.is_err() {
        return;
    }

    let _agent = match state.orchestrator.ensure_ready(agent_id).await {
        Ok(a) => a,
        Err(e) => {
            tracing::error!(agent_id = %agent_id, error = %e, "Failed to wake agent");
            let _ = send_err(&mut socket, "Failed to wake agent").await;
            return;
        }
    };

    if send_status(&mut socket, "Agent ready").await.is_err() {
        return;
    }

    // Get the reachable address for this agent (container IP or VM guest IP)
    let agent_address = match state.orchestrator.get_agent_address(agent_id).await {
        Ok(addr) => addr,
        Err(e) => {
            tracing::error!(agent_id = %agent_id, error = %e, "Failed to get agent address");
            let _ = send_err(&mut socket, "Agent address not available").await;
            return;
        }
    };

    // Message loop
    while let Some(msg_result) = socket.recv().await {
        let msg = match msg_result {
            Ok(m) => m,
            Err(e) => {
                tracing::info!(agent_id = %agent_id, user_id = %user_id, error = %e, "WebSocket closed");
                break;
            }
        };

        let text = match msg {
            Message::Text(t) => t,
            Message::Close(_) => break,
            _ => continue,
        };

        let incoming: IncomingMessage = match serde_json::from_str(&text) {
            Ok(m) => m,
            Err(e) => {
                tracing::warn!(error = %e, "Invalid message format");
                let _ = send_err(&mut socket, "Invalid message format").await;
                continue;
            }
        };

        if incoming.msg_type != "message" {
            continue;
        }

        let _ = send_status(&mut socket, "Thinking...").await;

        match bridge_chat(&agent_address, &incoming.content, &mut socket).await {
            Ok(_) => {}
            Err(e) => {
                tracing::error!(error = %e, "Agent chat failed");
                let _ = send_err(&mut socket, "Agent failed to respond").await;
            }
        }

        if let Err(e) = sqlx::query("UPDATE agents SET last_active = NOW() WHERE id = $1")
            .bind(agent_id)
            .execute(&state.db)
            .await
        {
            tracing::error!(agent_id = %agent_id, error = %e, "Failed to update last_active");
        }
    }

    tracing::info!(agent_id = %agent_id, user_id = %user_id, "WebSocket chat session ended");
}

// ── Chat bridge HTTP call ──────────────────────────────────────────────

/// POST to the in-container chat bridge and stream the SSE response back
/// to the client WebSocket token-by-token.
///
/// Retries up to 25 times with 3s delay if the bridge returns 503
/// (gateway not connected yet — common right after agent wake).
async fn bridge_chat(
    agent_address: &str,
    message: &str,
    client_ws: &mut WebSocket,
) -> Result<String, String> {
    let chat_url = format!("http://{}:3001/chat", agent_address);
    let client = reqwest::Client::new();

    let max_attempts = 25;
    let mut last_err = String::new();
    for attempt in 1..=max_attempts {
        let result = client
            .post(&chat_url)
            .json(&serde_json::json!({ "message": message }))
            .timeout(Duration::from_secs(180))
            .send()
            .await;

        match result {
            Ok(resp) if resp.status().is_success() => {
                // Success — proceed to stream SSE below
                return stream_bridge_response(resp, client_ws).await;
            }
            Ok(resp) if resp.status().as_u16() == 503 && attempt < max_attempts => {
                let body = resp.text().await.unwrap_or_default();
                tracing::info!(
                    attempt,
                    agent_address,
                    "Bridge not ready (503: {body}), retrying in 3s..."
                );
                let _ =
                    send_status(client_ws, "Connecting to agent...").await;
                tokio::time::sleep(Duration::from_secs(3)).await;
                last_err = format!("Bridge 503: {body}");
            }
            Ok(resp) => {
                let body = resp.text().await.unwrap_or_default();
                return Err(format!("Bridge error: {body}"));
            }
            Err(e) if attempt < max_attempts => {
                tracing::info!(
                    attempt,
                    agent_address,
                    error = %e,
                    "Bridge request failed, retrying in 3s..."
                );
                tokio::time::sleep(Duration::from_secs(3)).await;
                last_err = format!("Bridge request failed: {e}");
            }
            Err(e) => {
                return Err(format!("Bridge request failed after retries: {e}"));
            }
        }
    }

    Err(format!("Bridge not ready after {max_attempts} attempts: {last_err}"))
}

/// Stream SSE response from bridge and forward tokens to client WebSocket.
async fn stream_bridge_response(
    response: reqwest::Response,
    client_ws: &mut WebSocket,
) -> Result<String, String> {

    // Stream SSE events from the response body
    let mut stream = response.bytes_stream();
    let mut buffer = String::new();
    let mut final_text = String::new();

    while let Some(chunk) = stream.next().await {
        let bytes = chunk.map_err(|e| format!("Stream error: {e}"))?;
        buffer.push_str(&String::from_utf8_lossy(&bytes));

        // Process complete SSE events (delimited by \n\n)
        while let Some(pos) = buffer.find("\n\n") {
            let event = buffer[..pos].to_string();
            buffer = buffer[pos + 2..].to_string();

            for line in event.lines() {
                if let Some(data) = line.strip_prefix("data: ") {
                    if data == "[DONE]" {
                        return Ok(final_text);
                    }

                    if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(data) {
                        let event_type = parsed
                            .get("type")
                            .and_then(|t| t.as_str())
                            .unwrap_or("");
                        let content = parsed
                            .get("content")
                            .and_then(|c| c.as_str())
                            .unwrap_or("");
                        let msg_text = parsed
                            .get("message")
                            .and_then(|m| m.as_str())
                            .unwrap_or("");

                        match event_type {
                            "stream" => {
                                if !content.is_empty() {
                                    let _ = send_json(
                                        client_ws,
                                        &OutgoingMessage {
                                            msg_type: "stream".into(),
                                            content: Some(content.to_string()),
                                            message: None,
                                        },
                                    )
                                    .await;
                                }
                            }
                            "done" => {
                                final_text = content.to_string();
                                let _ = send_json(
                                    client_ws,
                                    &OutgoingMessage {
                                        msg_type: "done".into(),
                                        content: Some(final_text.clone()),
                                        message: None,
                                    },
                                )
                                .await;
                            }
                            "error" => {
                                let err = if !msg_text.is_empty() {
                                    msg_text
                                } else {
                                    content
                                };
                                return Err(format!("Agent error: {err}"));
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
    }

    if final_text.is_empty() {
        Err("Bridge stream ended without final response".to_string())
    } else {
        Ok(final_text)
    }
}

// ── Helpers ────────────────────────────────────────────────────────────

async fn send_json(socket: &mut WebSocket, msg: &OutgoingMessage) -> Result<(), axum::Error> {
    let text = serde_json::to_string(msg).expect("OutgoingMessage is always serializable");
    socket.send(Message::Text(text.into())).await
}

async fn send_status(socket: &mut WebSocket, msg: &str) -> Result<(), axum::Error> {
    send_json(
        socket,
        &OutgoingMessage {
            msg_type: "status".into(),
            content: None,
            message: Some(msg.into()),
        },
    )
    .await
}

async fn send_err(socket: &mut WebSocket, msg: &str) -> Result<(), axum::Error> {
    send_json(
        socket,
        &OutgoingMessage {
            msg_type: "error".into(),
            content: None,
            message: Some(msg.into()),
        },
    )
    .await
}
