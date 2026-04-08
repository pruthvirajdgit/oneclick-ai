//! End-to-end workflow tests for the OneClick.ai backend.
//!
//! These tests exercise the **complete lifecycle** of the backend system in a
//! single multi-step scenario:
//!
//!   signup → login → refresh → create agent → wake → schedules → internal
//!   endpoints (schedule / notification) → notifications → usage → sleep →
//!   delete → multi-user isolation
//!
//! They require PostgreSQL and Redis to be running. Gated behind `integration`.
//!
//! ```bash
//! docker compose up -d postgres redis
//! DATABASE_URL=postgres://oneclick:devpassword@localhost:5432/oneclick_test \
//!   cargo test --test e2e_workflow --features integration -- --test-threads=1
//! ```

#![cfg(feature = "integration")]

mod common;

use axum::http::StatusCode;
use serde_json::{json, Value};
use sqlx::PgPool;
use uuid::Uuid;

use common::TestApp;

// ===========================================================================
// Database setup (shared with api_integration.rs)
// ===========================================================================

async fn setup_db() -> PgPool {
    let url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://oneclick:devpassword@localhost:5432/oneclick_test".into());

    let pool = PgPool::connect(&url)
        .await
        .expect("Failed to connect to test database. Is PostgreSQL running?");

    oneclick_shared::db::run_migrations(&pool)
        .await
        .expect("Failed to run migrations");

    sqlx::query(
        "TRUNCATE users, agents, scheduled_jobs, usage, message_queue, notifications CASCADE",
    )
    .execute(&pool)
    .await
    .expect("Failed to truncate tables");

    pool
}

// ===========================================================================
// Helpers
// ===========================================================================

/// Sign up a user and return (token, user_id).
async fn signup(app: &TestApp, email: &str) -> (String, Uuid) {
    let (status, body) = app
        .post(
            "/api/auth/signup",
            json!({ "email": email, "password": "password123" }),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "Signup failed: {body}");

    let v: Value = serde_json::from_str(&body).unwrap();
    let token = v["token"].as_str().unwrap().to_string();
    let user_id = Uuid::parse_str(v["user"]["id"].as_str().unwrap()).unwrap();
    (token, user_id)
}

/// Log in and return a fresh token.
async fn login(app: &TestApp, email: &str) -> String {
    let (status, body) = app
        .post(
            "/api/auth/login",
            json!({ "email": email, "password": "password123" }),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK, "Login failed: {body}");

    let v: Value = serde_json::from_str(&body).unwrap();
    v["token"].as_str().unwrap().to_string()
}

/// Create an agent and return the agent ID.
async fn create_agent(app: &TestApp, token: &str) -> Uuid {
    let (status, body) = app
        .post("/api/agents", json!({}), Some(token))
        .await;
    assert_eq!(status, StatusCode::CREATED, "Create agent failed: {body}");

    let v: Value = serde_json::from_str(&body).unwrap();
    Uuid::parse_str(v["id"].as_str().unwrap()).unwrap()
}

// ===========================================================================
// TEST 1: Complete happy-path lifecycle
// ===========================================================================

#[tokio::test]
async fn test_e2e_full_lifecycle() {
    let db = setup_db().await;
    let app = TestApp::new(db.clone());

    // ── 1. Auth: Signup ─────────────────────────────────────────────────
    let (token, user_id) = signup(&app, "e2e@test.com").await;
    assert!(!token.is_empty());

    // ── 2. Auth: Login ──────────────────────────────────────────────────
    let login_token = login(&app, "e2e@test.com").await;
    assert!(!login_token.is_empty());

    // ── 3. Auth: Refresh ────────────────────────────────────────────────
    let (status, body) = app
        .post("/api/auth/refresh", json!({}), Some(&token))
        .await;
    assert_eq!(status, StatusCode::OK, "Refresh failed: {body}");
    let refreshed_token = serde_json::from_str::<Value>(&body).unwrap()["token"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(!refreshed_token.is_empty());

    // ── 4. Agent: Create ────────────────────────────────────────────────
    let agent_id = create_agent(&app, &token).await;

    // ── 5. Agent: List ──────────────────────────────────────────────────
    let (status, body) = app.get("/api/agents", Some(&token)).await;
    assert_eq!(status, StatusCode::OK);
    let agents: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(agents.as_array().unwrap().len(), 1);
    assert_eq!(agents[0]["id"].as_str().unwrap(), agent_id.to_string());

    // ── 6. Agent: Get by ID ─────────────────────────────────────────────
    let (status, body) = app
        .get(&format!("/api/agents/{agent_id}"), Some(&token))
        .await;
    assert_eq!(status, StatusCode::OK);
    let agent: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(agent["model"], "llama-3.3-70b-versatile");

    // ── 7. Agent: Wake (MockRuntime → immediate) ────────────────────────
    let (status, body) = app
        .post(
            &format!("/api/agents/{agent_id}/wake"),
            json!({}),
            Some(&token),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "Wake failed: {body}");
    let wake: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(wake["status"], "running");
    assert!(wake["chat_url"].as_str().unwrap().contains("token="));

    // ── 8. Schedule: Create via public API ──────────────────────────────
    let (status, body) = app
        .post(
            "/api/schedules",
            json!({
                "agent_id": agent_id.to_string(),
                "cron_expr": "0 */3 * * *",
                "task_message": "Check flights under ₹3500"
            }),
            Some(&token),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "Create schedule failed: {body}");
    let schedule: Value = serde_json::from_str(&body).unwrap();
    let schedule_id = schedule["id"].as_str().unwrap().to_string();
    assert_eq!(schedule["cron_expr"], "0 */3 * * *");
    assert_eq!(schedule["status"], "active");
    assert!(schedule["next_run_at"].is_string());

    // ── 9. Schedule: List ───────────────────────────────────────────────
    let (status, body) = app.get("/api/schedules", Some(&token)).await;
    assert_eq!(status, StatusCode::OK);
    let schedules: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(schedules.as_array().unwrap().len(), 1);

    // ── 10. Schedule: Delete ────────────────────────────────────────────
    let (status, _) = app
        .delete(&format!("/api/schedules/{schedule_id}"), &token)
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    // Verify deleted
    let (status, body) = app.get("/api/schedules", Some(&token)).await;
    assert_eq!(status, StatusCode::OK);
    let schedules: Value = serde_json::from_str(&body).unwrap();
    assert!(schedules.as_array().unwrap().is_empty());

    // ── 11. Usage: Initial (should be zero) ─────────────────────────────
    let (status, body) = app.get("/api/usage", Some(&token)).await;
    assert_eq!(status, StatusCode::OK);
    let usage: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(usage["today"]["requests"], 0);
    assert_eq!(usage["all_time"]["requests"], 0);

    // ── 12. Notifications: Initial (empty) ──────────────────────────────
    let (status, body) = app.get("/api/notifications", Some(&token)).await;
    assert_eq!(status, StatusCode::OK);
    let notifs: Value = serde_json::from_str(&body).unwrap();
    assert!(notifs.as_array().unwrap().is_empty());

    // ── 13. Agent: Sleep ────────────────────────────────────────────────
    let (status, body) = app
        .post(
            &format!("/api/agents/{agent_id}/sleep"),
            json!({}),
            Some(&token),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "Sleep failed: {body}");
    let slept: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(slept["status"], "stopped");

    // Verify agent is stopped
    let (status, body) = app
        .get(&format!("/api/agents/{agent_id}"), Some(&token))
        .await;
    assert_eq!(status, StatusCode::OK);
    let agent: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(agent["status"], "stopped");

    // ── 14. Agent: Delete ───────────────────────────────────────────────
    let (status, _) = app
        .delete(&format!("/api/agents/{agent_id}"), &token)
        .await;
    assert!(
        status == StatusCode::NO_CONTENT || status == StatusCode::OK,
        "Delete agent failed with status {status}"
    );

    // Verify gone
    let (status, _) = app
        .get(&format!("/api/agents/{agent_id}"), Some(&token))
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // ── 15. Health + Metrics still OK ───────────────────────────────────
    let (status, body) = app.get("/health", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, "ok");

    let (status, _) = app.get("/metrics", None).await;
    assert_eq!(status, StatusCode::OK);
}

// ===========================================================================
// TEST 2: Internal endpoints (agent→backend)
// ===========================================================================

#[tokio::test]
async fn test_e2e_internal_endpoints() {
    let db = setup_db().await;
    let app = TestApp::new(db.clone());
    let internal_secret = "test-internal-secret";

    let (token, user_id) = signup(&app, "internal@test.com").await;
    let agent_id = create_agent(&app, &token).await;

    // ── Internal Schedule: Create ───────────────────────────────────────
    let (status, body) = app
        .post_internal(
            "/internal/schedules",
            json!({
                "cron_expr": "*/30 * * * *",
                "task_message": "Check weather forecast"
            }),
            agent_id,
            user_id,
            internal_secret,
        )
        .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "Internal schedule create failed: {body}"
    );
    let schedule: Value = serde_json::from_str(&body).unwrap();
    let int_schedule_id = schedule["id"].as_str().unwrap().to_string();
    assert_eq!(schedule["task_message"], "Check weather forecast");
    assert_eq!(schedule["status"], "active");

    // ── Internal Schedule: List ─────────────────────────────────────────
    let (status, body) = app
        .get_internal(
            "/internal/schedules",
            agent_id,
            user_id,
            internal_secret,
        )
        .await;
    assert_eq!(status, StatusCode::OK, "Internal schedule list failed: {body}");
    let schedules: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(schedules.as_array().unwrap().len(), 1);

    // ── Internal Schedule: Delete ───────────────────────────────────────
    let (status, _) = app
        .delete_internal(
            &format!("/internal/schedules/{int_schedule_id}"),
            agent_id,
            user_id,
            internal_secret,
        )
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    // Verify deleted
    let (status, body) = app
        .get_internal(
            "/internal/schedules",
            agent_id,
            user_id,
            internal_secret,
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let schedules: Value = serde_json::from_str(&body).unwrap();
    assert!(schedules.as_array().unwrap().is_empty());

    // ── Internal Notification: Create ───────────────────────────────────
    let (status, body) = app
        .post_internal(
            "/internal/notifications",
            json!({
                "title": "Flight found!",
                "body": "Mumbai → BLR for ₹2,999 on Apr 15"
            }),
            agent_id,
            user_id,
            internal_secret,
        )
        .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "Internal notification create failed: {body}"
    );
    let notif: Value = serde_json::from_str(&body).unwrap();
    let notif_id = notif["id"].as_i64().unwrap();
    assert_eq!(notif["title"], "Flight found!");
    assert!(!notif["read"].as_bool().unwrap());

    // ── Verify notification visible via public API ──────────────────────
    let (status, body) = app.get("/api/notifications", Some(&token)).await;
    assert_eq!(status, StatusCode::OK);
    let notifs: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(notifs.as_array().unwrap().len(), 1);
    assert_eq!(notifs[0]["title"], "Flight found!");

    // ── Mark notification as read ───────────────────────────────────────
    let (status, _) = app
        .post(
            &format!("/api/notifications/{notif_id}/read"),
            json!({}),
            Some(&token),
        )
        .await;
    assert_eq!(status, StatusCode::OK);

    // Verify it's now read
    let (status, body) = app.get("/api/notifications", Some(&token)).await;
    assert_eq!(status, StatusCode::OK);
    let notifs: Value = serde_json::from_str(&body).unwrap();
    assert!(notifs[0]["read"].as_bool().unwrap());
}

// ===========================================================================
// TEST 3: Internal auth via Bearer token (secret|agent_id|user_id)
// ===========================================================================

#[tokio::test]
async fn test_e2e_internal_bearer_auth() {
    let db = setup_db().await;
    let app = TestApp::new(db.clone());
    let internal_secret = "test-internal-secret";

    let (token, user_id) = signup(&app, "bearer@test.com").await;
    let agent_id = create_agent(&app, &token).await;

    // Use the composite Bearer token format: "secret|agent_id|user_id"
    let bearer = format!("{internal_secret}|{agent_id}|{user_id}");

    let (status, body) = app
        .post_with_bearer(
            "/internal/notifications",
            json!({
                "title": "Bearer test",
                "body": "Created via bearer auth"
            }),
            &bearer,
        )
        .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "Bearer auth notification failed: {body}"
    );

    // Verify via public API
    let (status, body) = app.get("/api/notifications", Some(&token)).await;
    assert_eq!(status, StatusCode::OK);
    let notifs: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(notifs.as_array().unwrap().len(), 1);
    assert_eq!(notifs[0]["title"], "Bearer test");
}

// ===========================================================================
// TEST 4: Multi-user isolation
// ===========================================================================

#[tokio::test]
async fn test_e2e_multi_user_isolation() {
    let db = setup_db().await;
    let app = TestApp::new(db.clone());

    let (token_a, user_a) = signup(&app, "alice@test.com").await;
    let (token_b, _user_b) = signup(&app, "bob@test.com").await;

    // Alice creates an agent and a schedule.
    let agent_a = create_agent(&app, &token_a).await;

    let (status, body) = app
        .post(
            "/api/schedules",
            json!({
                "agent_id": agent_a.to_string(),
                "cron_expr": "0 9 * * *",
                "task_message": "Alice's morning task"
            }),
            Some(&token_a),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED);
    let schedule_a = serde_json::from_str::<Value>(&body).unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();

    // Create a notification for Alice via internal endpoint.
    let internal_secret = "test-internal-secret";
    let (status, _) = app
        .post_internal(
            "/internal/notifications",
            json!({ "title": "Alice's alert", "body": "Only for Alice" }),
            agent_a,
            user_a,
            internal_secret,
        )
        .await;
    assert_eq!(status, StatusCode::CREATED);

    // ── Bob cannot see Alice's agents ───────────────────────────────────
    let (status, _) = app
        .get(&format!("/api/agents/{agent_a}"), Some(&token_b))
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // ── Bob cannot see Alice's schedules ────────────────────────────────
    let (status, body) = app.get("/api/schedules", Some(&token_b)).await;
    assert_eq!(status, StatusCode::OK);
    let bob_schedules: Value = serde_json::from_str(&body).unwrap();
    assert!(bob_schedules.as_array().unwrap().is_empty());

    // ── Bob cannot delete Alice's schedule ──────────────────────────────
    let (status, _) = app
        .delete(&format!("/api/schedules/{schedule_a}"), &token_b)
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // ── Bob cannot see Alice's notifications ────────────────────────────
    let (status, body) = app.get("/api/notifications", Some(&token_b)).await;
    assert_eq!(status, StatusCode::OK);
    let bob_notifs: Value = serde_json::from_str(&body).unwrap();
    assert!(bob_notifs.as_array().unwrap().is_empty());

    // ── Bob cannot delete Alice's agent ─────────────────────────────────
    let (status, _) = app
        .delete(&format!("/api/agents/{agent_a}"), &token_b)
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // ── Alice can still see everything ──────────────────────────────────
    let (status, body) = app.get("/api/agents", Some(&token_a)).await;
    assert_eq!(status, StatusCode::OK);
    let alice_agents: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(alice_agents.as_array().unwrap().len(), 1);

    let (status, body) = app.get("/api/schedules", Some(&token_a)).await;
    assert_eq!(status, StatusCode::OK);
    let alice_schedules: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(alice_schedules.as_array().unwrap().len(), 1);

    let (status, body) = app.get("/api/notifications", Some(&token_a)).await;
    assert_eq!(status, StatusCode::OK);
    let alice_notifs: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(alice_notifs.as_array().unwrap().len(), 1);
}

// ===========================================================================
// TEST 5: Error paths and edge cases
// ===========================================================================

#[tokio::test]
async fn test_e2e_error_cases() {
    let db = setup_db().await;
    let app = TestApp::new(db.clone());

    let (token, user_id) = signup(&app, "errors@test.com").await;
    let agent_id = create_agent(&app, &token).await;

    // ── Invalid cron expression ─────────────────────────────────────────
    let (status, body) = app
        .post(
            "/api/schedules",
            json!({
                "agent_id": agent_id.to_string(),
                "cron_expr": "not-a-cron",
                "task_message": "bad schedule"
            }),
            Some(&token),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "Invalid cron should 400: {body}");

    // ── Schedule for non-existent agent ─────────────────────────────────
    let fake_agent = Uuid::new_v4();
    let (status, _) = app
        .post(
            "/api/schedules",
            json!({
                "agent_id": fake_agent.to_string(),
                "cron_expr": "0 * * * *",
                "task_message": "ghost agent"
            }),
            Some(&token),
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // ── Delete non-existent schedule ────────────────────────────────────
    let fake_schedule = Uuid::new_v4();
    let (status, _) = app
        .delete(&format!("/api/schedules/{fake_schedule}"), &token)
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // ── Delete non-existent agent ───────────────────────────────────────
    let fake_agent_2 = Uuid::new_v4();
    let (status, _) = app
        .delete(&format!("/api/agents/{fake_agent_2}"), &token)
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // ── Wake non-existent agent ─────────────────────────────────────────
    let (status, _) = app
        .post(
            &format!("/api/agents/{fake_agent_2}/wake"),
            json!({}),
            Some(&token),
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // ── Internal auth with wrong secret ─────────────────────────────────
    let (status, _) = app
        .post_internal(
            "/internal/notifications",
            json!({ "title": "hack", "body": "attempt" }),
            agent_id,
            user_id,
            "wrong-secret",
        )
        .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    // ── Internal auth with mismatched user ──────────────────────────────
    let wrong_user = Uuid::new_v4();
    let (status, _) = app
        .post_internal(
            "/internal/notifications",
            json!({ "title": "hack", "body": "attempt" }),
            agent_id,
            wrong_user,
            "test-internal-secret",
        )
        .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    // ── Mark non-existent notification as read ──────────────────────────
    let (status, _) = app
        .post(
            "/api/notifications/999999/read",
            json!({}),
            Some(&token),
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // ── Auth: Missing token ─────────────────────────────────────────────
    let (status, _) = app.get("/api/agents", None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    let (status, _) = app.get("/api/schedules", None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    let (status, _) = app.get("/api/usage", None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    let (status, _) = app.get("/api/notifications", None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

// ===========================================================================
// TEST 6: Schedule for another user's agent
// ===========================================================================

#[tokio::test]
async fn test_e2e_schedule_agent_ownership() {
    let db = setup_db().await;
    let app = TestApp::new(db.clone());

    let (token_a, _) = signup(&app, "owner@test.com").await;
    let (token_b, _) = signup(&app, "thief@test.com").await;

    let agent_a = create_agent(&app, &token_a).await;

    // Bob tries to create a schedule for Alice's agent → should fail.
    let (status, _) = app
        .post(
            "/api/schedules",
            json!({
                "agent_id": agent_a.to_string(),
                "cron_expr": "0 * * * *",
                "task_message": "steal Alice's agent"
            }),
            Some(&token_b),
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

// ===========================================================================
// TEST 7: Multiple agents for same user
// ===========================================================================

#[tokio::test]
async fn test_e2e_multiple_agents() {
    let db = setup_db().await;
    let app = TestApp::new(db.clone());

    let (token, _) = signup(&app, "multi@test.com").await;

    let agent_1 = create_agent(&app, &token).await;
    let agent_2 = create_agent(&app, &token).await;
    assert_ne!(agent_1, agent_2);

    let (status, body) = app.get("/api/agents", Some(&token)).await;
    assert_eq!(status, StatusCode::OK);
    let agents: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(agents.as_array().unwrap().len(), 2);

    // Create schedules for different agents.
    let (s1, _) = app
        .post(
            "/api/schedules",
            json!({
                "agent_id": agent_1.to_string(),
                "cron_expr": "0 8 * * *",
                "task_message": "Agent 1 morning"
            }),
            Some(&token),
        )
        .await;
    assert_eq!(s1, StatusCode::CREATED);

    let (s2, _) = app
        .post(
            "/api/schedules",
            json!({
                "agent_id": agent_2.to_string(),
                "cron_expr": "0 20 * * *",
                "task_message": "Agent 2 evening"
            }),
            Some(&token),
        )
        .await;
    assert_eq!(s2, StatusCode::CREATED);

    let (status, body) = app.get("/api/schedules", Some(&token)).await;
    assert_eq!(status, StatusCode::OK);
    let schedules: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(schedules.as_array().unwrap().len(), 2);

    // Delete one agent — its schedule should be cascade-deleted.
    let (status, _) = app
        .delete(&format!("/api/agents/{agent_1}"), &token)
        .await;
    assert!(status == StatusCode::NO_CONTENT || status == StatusCode::OK);

    let (status, body) = app.get("/api/schedules", Some(&token)).await;
    assert_eq!(status, StatusCode::OK);
    let schedules: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(
        schedules.as_array().unwrap().len(),
        1,
        "Cascade delete should remove agent_1's schedule"
    );
}

// ===========================================================================
// TEST 8: Notification pagination
// ===========================================================================

#[tokio::test]
async fn test_e2e_notification_pagination() {
    let db = setup_db().await;
    let app = TestApp::new(db.clone());
    let internal_secret = "test-internal-secret";

    let (token, user_id) = signup(&app, "paginate@test.com").await;
    let agent_id = create_agent(&app, &token).await;

    // Create 5 notifications.
    for i in 1..=5 {
        let (status, _) = app
            .post_internal(
                "/internal/notifications",
                json!({
                    "title": format!("Notif #{i}"),
                    "body": format!("Body for notification {i}")
                }),
                agent_id,
                user_id,
                internal_secret,
            )
            .await;
        assert_eq!(status, StatusCode::CREATED);
    }

    // Page 1, per_page=2
    let (status, body) = app
        .get("/api/notifications?page=1&per_page=2", Some(&token))
        .await;
    assert_eq!(status, StatusCode::OK);
    let page1: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(page1.as_array().unwrap().len(), 2);
    // Newest first — #5 should be first.
    assert_eq!(page1[0]["title"], "Notif #5");

    // Page 3, per_page=2 → only 1 result.
    let (status, body) = app
        .get("/api/notifications?page=3&per_page=2", Some(&token))
        .await;
    assert_eq!(status, StatusCode::OK);
    let page3: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(page3.as_array().unwrap().len(), 1);
}

// ===========================================================================
// TEST 9: Duplicate wake is idempotent
// ===========================================================================

#[tokio::test]
async fn test_e2e_idempotent_wake() {
    let db = setup_db().await;
    let app = TestApp::new(db.clone());

    let (token, _) = signup(&app, "idempotent@test.com").await;
    let agent_id = create_agent(&app, &token).await;

    // Wake twice — both should succeed.
    let (s1, _) = app
        .post(
            &format!("/api/agents/{agent_id}/wake"),
            json!({}),
            Some(&token),
        )
        .await;
    assert_eq!(s1, StatusCode::OK);

    let (s2, body) = app
        .post(
            &format!("/api/agents/{agent_id}/wake"),
            json!({}),
            Some(&token),
        )
        .await;
    assert_eq!(s2, StatusCode::OK);
    let wake: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(wake["status"], "running");
}

// ===========================================================================
// TEST 10: Sleep idempotent
// ===========================================================================

#[tokio::test]
async fn test_e2e_idempotent_sleep() {
    let db = setup_db().await;
    let app = TestApp::new(db.clone());

    let (token, _) = signup(&app, "sleepy@test.com").await;
    let agent_id = create_agent(&app, &token).await;

    // Agent starts as Stopped. Sleep should be idempotent.
    let (status, body) = app
        .post(
            &format!("/api/agents/{agent_id}/sleep"),
            json!({}),
            Some(&token),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "Sleep of stopped agent should be OK: {body}");
    let slept: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(slept["status"], "stopped");
}

// ===========================================================================
// TEST 11: Swagger and OpenAPI remain accessible
// ===========================================================================

#[tokio::test]
async fn test_e2e_swagger_openapi() {
    let db = setup_db().await;
    let app = TestApp::new(db.clone());

    let (status, body) = app.get("/swagger-ui/", None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        body.contains("swagger") || body.contains("Swagger") || body.contains("openapi"),
        "Swagger UI page not found"
    );

    let (status, body) = app.get("/api-docs/openapi.json", None).await;
    assert_eq!(status, StatusCode::OK);
    let spec: Value = serde_json::from_str(&body).unwrap();
    assert!(spec["openapi"].is_string() || spec["info"].is_object());
}

// ===========================================================================
// TEST 12: Various cron expressions
// ===========================================================================

#[tokio::test]
async fn test_e2e_cron_expressions() {
    let db = setup_db().await;
    let app = TestApp::new(db.clone());

    let (token, _) = signup(&app, "cron@test.com").await;
    let agent_id = create_agent(&app, &token).await;

    // Valid 5-field cron expressions.
    let valid_crons = vec![
        "0 * * * *",      // every hour
        "*/15 * * * *",   // every 15 min
        "0 9 * * 1-5",    // weekdays at 9am
        "30 2 1 * *",     // 1st of month at 2:30am
    ];

    for cron in &valid_crons {
        let (status, body) = app
            .post(
                "/api/schedules",
                json!({
                    "agent_id": agent_id.to_string(),
                    "cron_expr": cron,
                    "task_message": format!("test cron: {cron}")
                }),
                Some(&token),
            )
            .await;
        assert_eq!(
            status,
            StatusCode::CREATED,
            "Valid cron '{cron}' should succeed: {body}"
        );
    }

    // Invalid cron expressions.
    let invalid_crons = vec!["bad", "* *", "60 * * * *"];

    for cron in &invalid_crons {
        let (status, _) = app
            .post(
                "/api/schedules",
                json!({
                    "agent_id": agent_id.to_string(),
                    "cron_expr": cron,
                    "task_message": "should fail"
                }),
                Some(&token),
            )
            .await;
        assert_eq!(
            status,
            StatusCode::BAD_REQUEST,
            "Invalid cron '{cron}' should return 400"
        );
    }
}
