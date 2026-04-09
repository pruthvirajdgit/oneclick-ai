//! Live Firecracker E2E tests.
//!
//! Exercises the full VM lifecycle against a **running** backend with real
//! Firecracker microVMs:
//!
//!   signup → create agent → wake (cold boot) → gateway ready →
//!   sleep (snapshot) → wake (restore) → gateway ready → sleep → delete
//!
//! Prerequisites:
//!   - Backend running with `AGENT_RUNTIME=firecracker`
//!   - PostgreSQL + Redis running
//!   - `/dev/kvm` accessible
//!   - Firecracker kernel + rootfs configured in `.env`
//!
//! ```bash
//! cargo test --test e2e_firecracker --features firecracker -- --test-threads=1 --nocapture
//! ```

#![cfg(feature = "firecracker")]

use reqwest::{Client, StatusCode};
use serde_json::{json, Value};
use std::time::{Duration, Instant};

const API_BASE: &str = "http://127.0.0.1:8080/api";
const GATEWAY_TIMEOUT: Duration = Duration::from_secs(120);
const GATEWAY_POLL_INTERVAL: Duration = Duration::from_secs(3);

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

struct TestContext {
    client: Client,
    token: String,
    agent_id: Option<String>,
}

impl TestContext {
    async fn new() -> Self {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);

        let client = Client::builder()
            .timeout(Duration::from_secs(60))
            .build()
            .expect("HTTP client");

        // Sign up with a unique email
        let email = format!("fc-e2e-{}-{n}@test.com", std::process::id());
        let resp = client
            .post(format!("{API_BASE}/auth/signup"))
            .json(&json!({ "email": email, "password": "password123" }))
            .send()
            .await
            .expect("Signup request failed — is the backend running?");

        let status = resp.status();
        let body: Value = resp.json().await.expect("Signup response not JSON");

        // If user already exists, login instead
        let token = if status == StatusCode::CREATED {
            body["token"].as_str().unwrap().to_string()
        } else {
            let resp = client
                .post(format!("{API_BASE}/auth/login"))
                .json(&json!({ "email": email, "password": "password123" }))
                .send()
                .await
                .expect("Login request failed");
            let body: Value = resp.json().await.unwrap();
            body["token"].as_str().unwrap().to_string()
        };

        Self {
            client,
            token,
            agent_id: None,
        }
    }

    fn auth_header(&self) -> String {
        format!("Bearer {}", self.token)
    }

    async fn get(&self, path: &str) -> (StatusCode, Value) {
        let resp = self
            .client
            .get(format!("{API_BASE}{path}"))
            .header("Authorization", self.auth_header())
            .send()
            .await
            .unwrap_or_else(|e| panic!("GET {path} failed: {e}"));
        let status = resp.status();
        let body: Value = resp.json().await.unwrap_or(json!(null));
        (status, body)
    }

    async fn post(&self, path: &str) -> (StatusCode, Value) {
        let resp = self
            .client
            .post(format!("{API_BASE}{path}"))
            .header("Authorization", self.auth_header())
            .send()
            .await
            .unwrap_or_else(|e| panic!("POST {path} failed: {e}"));
        let status = resp.status();
        let body: Value = resp.json().await.unwrap_or(json!(null));
        (status, body)
    }

    async fn post_json(&self, path: &str, body: Value) -> (StatusCode, Value) {
        let resp = self
            .client
            .post(format!("{API_BASE}{path}"))
            .header("Authorization", self.auth_header())
            .json(&body)
            .send()
            .await
            .unwrap_or_else(|e| panic!("POST {path} failed: {e}"));
        let status = resp.status();
        let body: Value = resp.json().await.unwrap_or(json!(null));
        (status, body)
    }

    async fn delete(&self, path: &str) -> StatusCode {
        let resp = self
            .client
            .delete(format!("{API_BASE}{path}"))
            .header("Authorization", self.auth_header())
            .send()
            .await
            .unwrap_or_else(|e| panic!("DELETE {path} failed: {e}"));
        resp.status()
    }

    /// Poll gateway-status until ready or timeout.
    async fn wait_gateway(&self, agent_id: &str, timeout: Duration) -> bool {
        let start = Instant::now();
        loop {
            if start.elapsed() >= timeout {
                return false;
            }
            let (status, body) = self.get(&format!("/agents/{agent_id}/gateway-status")).await;
            if status == StatusCode::OK {
                if body["ready"].as_bool().unwrap_or(false) {
                    return true;
                }
            }
            tokio::time::sleep(GATEWAY_POLL_INTERVAL).await;
        }
    }

    /// Cleanup: delete agent if it exists.
    async fn cleanup(&mut self) {
        if let Some(id) = self.agent_id.take() {
            // Try sleep first (in case it's running)
            let _ = self.post(&format!("/agents/{id}/sleep")).await;
            tokio::time::sleep(Duration::from_secs(2)).await;
            let _ = self.delete(&format!("/agents/{id}")).await;
        }
    }
}

// ---------------------------------------------------------------------------
// Preflight check
// ---------------------------------------------------------------------------

#[tokio::test]
#[cfg(feature = "firecracker")]
async fn preflight_backend_reachable() {
    let client = Client::new();
    let resp = client
        .get("http://127.0.0.1:8080/api-docs/openapi.json")
        .send()
        .await
        .expect("Backend not reachable at :8080 — start it first");
    assert_eq!(resp.status(), StatusCode::OK, "Backend not returning 200");
}

// ---------------------------------------------------------------------------
// Full lifecycle test
// ---------------------------------------------------------------------------

#[tokio::test]
#[cfg(feature = "firecracker")]
async fn full_lifecycle() {
    let mut ctx = TestContext::new().await;

    // ── Create Agent ────────────────────────────────────────────────────
    let (status, body) = ctx
        .post_json(
            "/agents",
            json!({ "model": "groq/meta-llama/llama-4-scout-17b-16e-instruct" }),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "Create agent failed: {body}");

    let agent_id = body["id"].as_str().expect("No agent ID").to_string();
    let agent_status = body["status"].as_str().unwrap_or("");
    assert_eq!(agent_status, "stopped", "Initial status should be stopped");
    ctx.agent_id = Some(agent_id.clone());
    eprintln!("  ✓ Agent created: {agent_id}");

    // ── Wake (Cold Boot) ────────────────────────────────────────────────
    let start = Instant::now();
    let (status, body) = ctx.post(&format!("/agents/{agent_id}/wake")).await;
    let cold_boot_ms = start.elapsed().as_millis();
    assert_eq!(status, StatusCode::OK, "Wake failed: {body}");
    assert_eq!(
        body["status"].as_str().unwrap_or(""),
        "running",
        "Agent not running after wake"
    );
    eprintln!("  ✓ Cold boot: {cold_boot_ms}ms");

    // ── Verify Running ──────────────────────────────────────────────────
    let (status, body) = ctx.get(&format!("/agents/{agent_id}")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["status"].as_str().unwrap_or(""), "running");
    eprintln!("  ✓ Agent confirmed running");

    // ── Wait for Gateway ────────────────────────────────────────────────
    let start = Instant::now();
    let gw_ready = ctx.wait_gateway(&agent_id, GATEWAY_TIMEOUT).await;
    let gw_elapsed = start.elapsed().as_secs();
    assert!(gw_ready, "Gateway not ready after {gw_elapsed}s");
    eprintln!("  ✓ Gateway ready in {gw_elapsed}s");

    // ── Sleep (Snapshot) ────────────────────────────────────────────────
    let start = Instant::now();
    let (status, body) = ctx.post(&format!("/agents/{agent_id}/sleep")).await;
    let sleep_ms = start.elapsed().as_millis();
    assert_eq!(status, StatusCode::OK, "Sleep failed: {body}");
    assert_eq!(
        body["status"].as_str().unwrap_or(""),
        "stopped",
        "Agent not stopped after sleep"
    );
    eprintln!("  ✓ Snapshot save: {sleep_ms}ms");

    // Wait for FC process to exit
    tokio::time::sleep(Duration::from_secs(2)).await;

    // ── Wake (Snapshot Restore) ─────────────────────────────────────────
    let start = Instant::now();
    let (status, body) = ctx.post(&format!("/agents/{agent_id}/wake")).await;
    let restore_ms = start.elapsed().as_millis();
    assert_eq!(status, StatusCode::OK, "Wake (restore) failed: {body}");
    assert_eq!(
        body["status"].as_str().unwrap_or(""),
        "running",
        "Agent not running after restore"
    );
    eprintln!("  ✓ Snapshot restore: {restore_ms}ms");

    // ── Gateway Ready After Restore ─────────────────────────────────────
    let start = Instant::now();
    let gw_ready = ctx.wait_gateway(&agent_id, Duration::from_secs(30)).await;
    let gw2_elapsed = start.elapsed().as_secs();
    assert!(
        gw_ready,
        "Gateway not ready after restore (waited {gw2_elapsed}s)"
    );
    eprintln!("  ✓ Gateway ready after restore in {gw2_elapsed}s");

    // ── Sleep Again ─────────────────────────────────────────────────────
    let (status, _) = ctx.post(&format!("/agents/{agent_id}/sleep")).await;
    assert_eq!(status, StatusCode::OK, "Second sleep failed");
    eprintln!("  ✓ Second sleep OK");
    tokio::time::sleep(Duration::from_secs(2)).await;

    // ── Delete ──────────────────────────────────────────────────────────
    let status = ctx.delete(&format!("/agents/{agent_id}")).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "Delete failed");
    ctx.agent_id = None; // Don't double-delete in cleanup
    eprintln!("  ✓ Agent deleted");

    // ── Verify Gone ─────────────────────────────────────────────────────
    let (status, _) = ctx.get(&format!("/agents/{agent_id}")).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "Deleted agent still exists");
    eprintln!("  ✓ Agent confirmed deleted (404)");
}

// ---------------------------------------------------------------------------
// Idempotency tests
// ---------------------------------------------------------------------------

#[tokio::test]
#[cfg(feature = "firecracker")]
async fn wake_already_running_is_idempotent() {
    let mut ctx = TestContext::new().await;

    let (_, body) = ctx
        .post_json(
            "/agents",
            json!({ "model": "groq/meta-llama/llama-4-scout-17b-16e-instruct" }),
        )
        .await;
    let agent_id = body["id"].as_str().unwrap().to_string();
    ctx.agent_id = Some(agent_id.clone());

    // First wake
    let (status, _) = ctx.post(&format!("/agents/{agent_id}/wake")).await;
    assert_eq!(status, StatusCode::OK);
    eprintln!("  ✓ First wake OK");

    // Wait for health
    let ready = ctx.wait_gateway(&agent_id, GATEWAY_TIMEOUT).await;
    assert!(ready, "Gateway not ready");

    // Second wake while running — should be idempotent
    let (status, body) = ctx.post(&format!("/agents/{agent_id}/wake")).await;
    assert_eq!(status, StatusCode::OK, "Idempotent wake failed: {body}");
    assert_eq!(body["status"].as_str().unwrap_or(""), "running");
    eprintln!("  ✓ Idempotent wake OK");

    ctx.cleanup().await;
}

#[tokio::test]
#[cfg(feature = "firecracker")]
async fn sleep_already_stopped_is_idempotent() {
    let mut ctx = TestContext::new().await;

    let (_, body) = ctx
        .post_json(
            "/agents",
            json!({ "model": "groq/meta-llama/llama-4-scout-17b-16e-instruct" }),
        )
        .await;
    let agent_id = body["id"].as_str().unwrap().to_string();
    ctx.agent_id = Some(agent_id.clone());

    // Agent is stopped by default — sleep should be idempotent
    let (status, body) = ctx.post(&format!("/agents/{agent_id}/sleep")).await;
    assert_eq!(status, StatusCode::OK, "Idempotent sleep failed: {body}");
    assert_eq!(body["status"].as_str().unwrap_or(""), "stopped");
    eprintln!("  ✓ Idempotent sleep OK");

    ctx.cleanup().await;
}

// ---------------------------------------------------------------------------
// Multi-agent isolation
// ---------------------------------------------------------------------------

#[tokio::test]
#[cfg(feature = "firecracker")]
async fn multi_user_isolation() {
    let ctx1 = TestContext::new().await;
    let ctx2 = TestContext::new().await;

    // User 1 creates an agent
    let (status, body) = ctx1
        .post_json(
            "/agents",
            json!({ "model": "groq/meta-llama/llama-4-scout-17b-16e-instruct" }),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED);
    let agent_id = body["id"].as_str().unwrap().to_string();

    // User 2 cannot see it
    let (status, _) = ctx2.get(&format!("/agents/{agent_id}")).await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "User 2 should not see User 1's agent"
    );
    eprintln!("  ✓ Multi-user isolation verified");

    // User 2 cannot wake it
    let (status, _) = ctx2.post(&format!("/agents/{agent_id}/wake")).await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "User 2 should not wake User 1's agent"
    );
    eprintln!("  ✓ Cross-user wake blocked");

    // User 2 cannot delete it
    let status = ctx2.delete(&format!("/agents/{agent_id}")).await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "User 2 should not delete User 1's agent"
    );
    eprintln!("  ✓ Cross-user delete blocked");

    // Cleanup
    let _ = ctx1.delete(&format!("/agents/{agent_id}")).await;
}
