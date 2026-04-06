//! WebSocket chat handler for real-time agent conversations.
//!
//! Flow: client connects → JWT validated → agent woken → backend opens a
//! WebSocket to the agent's OpenClaw gateway → Ed25519 device handshake →
//! chat messages are relayed with real token-by-token streaming.

use std::time::Duration;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, Query, State};
use axum::response::IntoResponse;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use ed25519_dalek::{Signer, SigningKey};
use futures_util::{SinkExt, StreamExt};
use rand::rngs::OsRng;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio_tungstenite::tungstenite;
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

    let agent = match state.orchestrator.ensure_ready(agent_id).await {
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

    let container_name = match &agent.container_name {
        Some(n) => n.clone(),
        None => {
            tracing::error!(agent_id = %agent_id, "Agent has no container name");
            let _ = send_err(&mut socket, "Agent container not available").await;
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

        match agent_ws_chat(&container_name, &incoming.content, &mut socket).await {
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

// ── Agent gateway WebSocket chat ───────────────────────────────────────

/// Connect to the agent's OpenClaw gateway, perform the Ed25519 device
/// handshake, send a chat message, and stream delta tokens back to the client.
async fn agent_ws_chat(
    container_name: &str,
    message: &str,
    client_ws: &mut WebSocket,
) -> Result<String, String> {
    // 1. Generate Ed25519 identity
    let signing_key = SigningKey::generate(&mut OsRng);
    let verifying_key = signing_key.verifying_key();
    let raw_pub = verifying_key.as_bytes();
    let device_id = hex::encode(Sha256::digest(raw_pub));
    let pub_key_b64 = URL_SAFE_NO_PAD.encode(raw_pub);

    // 2. Connect to agent gateway
    let uri = format!("ws://{}:3000/?token=oneclick-internal", container_name);
    let request = tungstenite::http::Request::builder()
        .uri(&uri)
        .header("Origin", "http://127.0.0.1:3000")
        .header(
            "Sec-WebSocket-Key",
            tungstenite::handshake::client::generate_key(),
        )
        .header("Upgrade", "websocket")
        .header("Connection", "Upgrade")
        .header("Sec-WebSocket-Version", "13")
        .header("Host", format!("{}:3000", container_name))
        .body(())
        .map_err(|e| format!("Failed to build WS request: {e}"))?;

    let (ws_stream, _) = tokio_tungstenite::connect_async(request)
        .await
        .map_err(|e| format!("Gateway connect failed: {e}"))?;

    let (mut gw_tx, mut gw_rx) = ws_stream.split();

    // 3. Wait for connect.challenge
    let nonce = recv_challenge(&mut gw_rx).await?;

    // 4. Sign and send connect request
    let scopes = "operator.admin,operator.read,operator.write,operator.approvals,operator.pairing";
    let signed_at_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    let gw_token = "oneclick-internal";

    let sign_payload = format!(
        "v2|{}|openclaw-control-ui|webchat|operator|{}|{}|{}|{}",
        device_id, scopes, signed_at_ms, gw_token, nonce
    );
    let signature = signing_key.sign(sign_payload.as_bytes());
    let sig_b64 = URL_SAFE_NO_PAD.encode(signature.to_bytes());

    let connect_msg = serde_json::json!({
        "type": "req",
        "id": "c1",
        "method": "connect",
        "params": {
            "minProtocol": 3,
            "maxProtocol": 3,
            "client": {
                "id": "openclaw-control-ui",
                "platform": "linux",
                "mode": "webchat",
                "version": "2026.3.13",
                "instanceId": "oneclick-backend"
            },
            "role": "operator",
            "scopes": ["operator.admin", "operator.read", "operator.write", "operator.approvals", "operator.pairing"],
            "device": {
                "id": device_id,
                "publicKey": pub_key_b64,
                "signature": sig_b64,
                "signedAt": signed_at_ms,
                "nonce": nonce
            },
            "auth": { "token": gw_token },
            "caps": ["tool-events"]
        }
    });

    gw_tx
        .send(tungstenite::Message::Text(connect_msg.to_string().into()))
        .await
        .map_err(|e| format!("Failed to send connect: {e}"))?;

    // 5. Wait for hello-ok
    wait_hello_ok(&mut gw_rx).await?;

    // 6. Send chat message
    let idempotency_key = Uuid::new_v4().to_string();
    let chat_msg = serde_json::json!({
        "type": "req",
        "id": "ch1",
        "method": "chat.send",
        "params": {
            "message": message,
            "sessionKey": "main",
            "idempotencyKey": idempotency_key
        }
    });

    gw_tx
        .send(tungstenite::Message::Text(chat_msg.to_string().into()))
        .await
        .map_err(|e| format!("Failed to send chat: {e}"))?;

    // 7. Stream response tokens to client
    let final_text = stream_chat_response(&mut gw_rx, client_ws).await?;

    // Close gateway connection
    let _ = gw_tx.send(tungstenite::Message::Close(None)).await;

    Ok(final_text)
}

/// Wait for the `connect.challenge` event and extract the nonce.
async fn recv_challenge<S>(gw_rx: &mut S) -> Result<String, String>
where
    S: StreamExt<Item = Result<tungstenite::Message, tungstenite::Error>> + Unpin,
{
    let timeout = Duration::from_secs(10);
    let msg = tokio::time::timeout(timeout, async {
        while let Some(frame) = gw_rx.next().await {
            match frame {
                Ok(tungstenite::Message::Text(t)) => return Ok(t),
                Ok(tungstenite::Message::Ping(_)) => continue,
                Ok(tungstenite::Message::Close(_)) => {
                    return Err("Gateway closed before challenge".to_string())
                }
                Err(e) => return Err(format!("Gateway recv error: {e}")),
                _ => continue,
            }
        }
        Err("Gateway stream ended before challenge".to_string())
    })
    .await
    .map_err(|_| "Timeout waiting for challenge".to_string())??;

    let v: serde_json::Value =
        serde_json::from_str(&msg).map_err(|e| format!("Bad challenge JSON: {e}"))?;

    if v.get("event").and_then(|e| e.as_str()) != Some("connect.challenge") {
        return Err(format!("Expected connect.challenge, got: {msg}"));
    }

    v.pointer("/payload/nonce")
        .and_then(|n| n.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| "No nonce in challenge".to_string())
}

/// Wait for the `hello-ok` response to the connect request.
async fn wait_hello_ok<S>(gw_rx: &mut S) -> Result<(), String>
where
    S: StreamExt<Item = Result<tungstenite::Message, tungstenite::Error>> + Unpin,
{
    let timeout = Duration::from_secs(10);
    let msg = tokio::time::timeout(timeout, async {
        while let Some(frame) = gw_rx.next().await {
            match frame {
                Ok(tungstenite::Message::Text(t)) => return Ok(t),
                Ok(tungstenite::Message::Ping(_)) => continue,
                Ok(tungstenite::Message::Close(_)) => {
                    return Err("Gateway closed before hello-ok".to_string())
                }
                Err(e) => return Err(format!("Gateway recv error: {e}")),
                _ => continue,
            }
        }
        Err("Gateway stream ended before hello-ok".to_string())
    })
    .await
    .map_err(|_| "Timeout waiting for hello-ok".to_string())??;

    let v: serde_json::Value =
        serde_json::from_str(&msg).map_err(|e| format!("Bad hello-ok JSON: {e}"))?;

    // Accept: {"type":"res","id":"c1","ok":true,...}
    if v.get("ok").and_then(|o| o.as_bool()) == Some(true) {
        tracing::debug!("Gateway handshake complete (hello-ok)");
        return Ok(());
    }

    Err(format!("Gateway handshake failed: {msg}"))
}

/// Listen for `chat` events from the gateway, forwarding deltas to the client
/// WebSocket. Returns the full final text.
async fn stream_chat_response<S>(
    gw_rx: &mut S,
    client_ws: &mut WebSocket,
) -> Result<String, String>
where
    S: StreamExt<Item = Result<tungstenite::Message, tungstenite::Error>> + Unpin,
{
    let timeout = Duration::from_secs(180);
    let result = tokio::time::timeout(timeout, async {
        while let Some(frame) = gw_rx.next().await {
            let text = match frame {
                Ok(tungstenite::Message::Text(t)) => t,
                Ok(tungstenite::Message::Ping(_)) => continue,
                Ok(tungstenite::Message::Close(_)) => {
                    return Err("Gateway closed during chat".to_string())
                }
                Err(e) => return Err(format!("Gateway recv error: {e}")),
                _ => continue,
            };

            let v: serde_json::Value = match serde_json::from_str(&text) {
                Ok(v) => v,
                Err(_) => continue,
            };

            // Handle chat.send response (ack)
            if v.get("type").and_then(|t| t.as_str()) == Some("res") {
                if v.get("ok").and_then(|o| o.as_bool()) != Some(true) {
                    let err_msg = v
                        .pointer("/error/message")
                        .and_then(|m| m.as_str())
                        .unwrap_or("Unknown gateway error");
                    return Err(format!("chat.send rejected: {err_msg}"));
                }
                continue;
            }

            // Handle chat events
            let event = v.get("event").and_then(|e| e.as_str()).unwrap_or("");
            if event != "chat" {
                // Ignore non-chat events (e.g. tool events)
                continue;
            }

            let payload = match v.get("payload") {
                Some(p) => p,
                None => continue,
            };

            let state = payload
                .get("state")
                .and_then(|s| s.as_str())
                .unwrap_or("");

            match state {
                "delta" => {
                    // Extract text from content array
                    if let Some(text_content) = extract_text_from_content(payload) {
                        if !text_content.is_empty() {
                            let _ = send_json(
                                client_ws,
                                &OutgoingMessage {
                                    msg_type: "stream".into(),
                                    content: Some(text_content),
                                    message: None,
                                },
                            )
                            .await;
                        }
                    }
                }
                "final" => {
                    let full_text =
                        extract_text_from_content(payload).unwrap_or_default();
                    let _ = send_json(
                        client_ws,
                        &OutgoingMessage {
                            msg_type: "done".into(),
                            content: Some(full_text.clone()),
                            message: None,
                        },
                    )
                    .await;
                    return Ok(full_text);
                }
                "error" => {
                    let err_text =
                        extract_text_from_content(payload).unwrap_or_default();
                    return Err(format!("Agent error: {err_text}"));
                }
                _ => {
                    // Other states (thinking, tool_use, etc.) — ignore or log
                    tracing::debug!(state = %state, "Ignoring chat event state");
                }
            }
        }
        Err("Gateway stream ended without final response".to_string())
    })
    .await
    .map_err(|_| "Timeout waiting for agent response".to_string())?;

    result
}

/// Extract concatenated text from the `message.content` array.
fn extract_text_from_content(payload: &serde_json::Value) -> Option<String> {
    let content = payload.pointer("/message/content")?.as_array()?;
    let mut text = String::new();
    for item in content {
        if item.get("type").and_then(|t| t.as_str()) == Some("text") {
            if let Some(t) = item.get("text").and_then(|t| t.as_str()) {
                text.push_str(t);
            }
        }
    }
    Some(text)
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
