//! Integration tests for the OneClick.ai API.
//!
//! These tests require a running PostgreSQL database and are gated behind
//! the `integration` feature so `cargo test` (unit tests) always passes.
//!
//! ```bash
//! # Start infra, then run integration tests:
//! docker compose up -d postgres redis
//! DATABASE_URL=postgres://oneclick:password@localhost:5432/oneclick \
//!   cargo test --test api_integration --features integration
//! ```

#![cfg(feature = "integration")]

mod common;

use axum::http::StatusCode;
use serde_json::json;
use sqlx::PgPool;

use common::TestApp;

/// Connect to the test database, run migrations, and return the pool.
///
/// Uses `DATABASE_URL` from the environment. Falls back to a default
/// that matches docker-compose.
async fn setup_db() -> PgPool {
    let url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://oneclick:password@localhost:5432/oneclick_test".into());

    let pool = PgPool::connect(&url)
        .await
        .expect("Failed to connect to test database. Is PostgreSQL running?");

    oneclick_shared::db::run_migrations(&pool)
        .await
        .expect("Failed to run migrations");

    // Clean all tables for a fresh test run.
    sqlx::query("TRUNCATE users, agents, scheduled_jobs, usage, message_queue, notifications CASCADE")
        .execute(&pool)
        .await
        .expect("Failed to truncate tables");

    pool
}

// ===========================================================================
// Health check
// ===========================================================================

#[tokio::test]
async fn test_health_check() {
    let db = setup_db().await;
    let app = TestApp::new(db);

    let (status, body) = app.get("/health", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, "ok");
}

// ===========================================================================
// Auth: signup
// ===========================================================================

#[tokio::test]
async fn test_signup_success() {
    let db = setup_db().await;
    let app = TestApp::new(db);

    let (status, body) = app
        .post(
            "/api/auth/signup",
            json!({ "email": "alice@test.com", "password": "password123" }),
            None,
        )
        .await;

    assert_eq!(status, StatusCode::CREATED);

    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert!(v["token"].is_string());
    assert_eq!(v["user"]["email"], "alice@test.com");
    assert_eq!(v["user"]["tier"], "free");
}

#[tokio::test]
async fn test_signup_duplicate_email() {
    let db = setup_db().await;
    let app = TestApp::new(db);

    // First signup succeeds.
    let (status, _) = app
        .post(
            "/api/auth/signup",
            json!({ "email": "dup@test.com", "password": "password123" }),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::CREATED);

    // Second with same email conflicts.
    let (status, body) = app
        .post(
            "/api/auth/signup",
            json!({ "email": "dup@test.com", "password": "password123" }),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert!(body.contains("already registered"));
}

#[tokio::test]
async fn test_signup_invalid_email() {
    let db = setup_db().await;
    let app = TestApp::new(db);

    let (status, _) = app
        .post(
            "/api/auth/signup",
            json!({ "email": "bad", "password": "password123" }),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_signup_short_password() {
    let db = setup_db().await;
    let app = TestApp::new(db);

    let (status, _) = app
        .post(
            "/api/auth/signup",
            json!({ "email": "short@test.com", "password": "1234567" }),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

// ===========================================================================
// Auth: login
// ===========================================================================

#[tokio::test]
async fn test_login_success() {
    let db = setup_db().await;
    let app = TestApp::new(db);

    // Signup first.
    app.post(
        "/api/auth/signup",
        json!({ "email": "login@test.com", "password": "password123" }),
        None,
    )
    .await;

    // Login.
    let (status, body) = app
        .post(
            "/api/auth/login",
            json!({ "email": "login@test.com", "password": "password123" }),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK);

    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert!(v["token"].is_string());
}

#[tokio::test]
async fn test_login_wrong_password() {
    let db = setup_db().await;
    let app = TestApp::new(db);

    app.post(
        "/api/auth/signup",
        json!({ "email": "wrong@test.com", "password": "password123" }),
        None,
    )
    .await;

    let (status, _) = app
        .post(
            "/api/auth/login",
            json!({ "email": "wrong@test.com", "password": "wrongpassword" }),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn test_login_nonexistent_user() {
    let db = setup_db().await;
    let app = TestApp::new(db);

    let (status, _) = app
        .post(
            "/api/auth/login",
            json!({ "email": "ghost@test.com", "password": "password123" }),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

// ===========================================================================
// Auth: refresh
// ===========================================================================

#[tokio::test]
async fn test_refresh_token() {
    let db = setup_db().await;
    let app = TestApp::new(db);

    let (_, body) = app
        .post(
            "/api/auth/signup",
            json!({ "email": "refresh@test.com", "password": "password123" }),
            None,
        )
        .await;
    let token = serde_json::from_str::<serde_json::Value>(&body).unwrap()["token"]
        .as_str()
        .unwrap()
        .to_string();

    let (status, body) = app
        .post("/api/auth/refresh", json!({}), Some(&token))
        .await;
    assert_eq!(status, StatusCode::OK);

    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert!(v["token"].is_string());
}

#[tokio::test]
async fn test_refresh_without_token() {
    let db = setup_db().await;
    let app = TestApp::new(db);

    let (status, _) = app.post("/api/auth/refresh", json!({}), None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

// ===========================================================================
// Agents: CRUD
// ===========================================================================

/// Helper: sign up and return the JWT token.
async fn signup_and_get_token(app: &TestApp, email: &str) -> String {
    let (_, body) = app
        .post(
            "/api/auth/signup",
            json!({ "email": email, "password": "password123" }),
            None,
        )
        .await;
    serde_json::from_str::<serde_json::Value>(&body).unwrap()["token"]
        .as_str()
        .unwrap()
        .to_string()
}

#[tokio::test]
async fn test_create_and_list_agents() {
    let db = setup_db().await;
    let app = TestApp::new(db);
    let token = signup_and_get_token(&app, "agents@test.com").await;

    // Create agent.
    let (status, body) = app
        .post("/api/agents", json!({}), Some(&token))
        .await;
    assert_eq!(status, StatusCode::CREATED);

    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert!(v["id"].is_string());

    // List agents.
    let (status, body) = app.get("/api/agents", Some(&token)).await;
    assert_eq!(status, StatusCode::OK);

    let agents: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(agents.as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn test_get_agent_by_id() {
    let db = setup_db().await;
    let app = TestApp::new(db);
    let token = signup_and_get_token(&app, "getagt@test.com").await;

    let (_, body) = app
        .post("/api/agents", json!({}), Some(&token))
        .await;
    let agent_id = serde_json::from_str::<serde_json::Value>(&body).unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();

    let (status, body) = app
        .get(&format!("/api/agents/{agent_id}"), Some(&token))
        .await;
    assert_eq!(status, StatusCode::OK);

    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["id"].as_str().unwrap(), agent_id);
}

#[tokio::test]
async fn test_delete_agent() {
    let db = setup_db().await;
    let app = TestApp::new(db);
    let token = signup_and_get_token(&app, "delagt@test.com").await;

    let (_, body) = app
        .post("/api/agents", json!({}), Some(&token))
        .await;
    let agent_id = serde_json::from_str::<serde_json::Value>(&body).unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();

    let (status, _) = app
        .delete(&format!("/api/agents/{agent_id}"), &token)
        .await;
    assert!(status == StatusCode::NO_CONTENT || status == StatusCode::OK);

    // Verify it's gone.
    let (status, _) = app
        .get(&format!("/api/agents/{agent_id}"), Some(&token))
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_agent_ownership_enforced() {
    let db = setup_db().await;
    let app = TestApp::new(db);

    let token_a = signup_and_get_token(&app, "usera@test.com").await;
    let token_b = signup_and_get_token(&app, "userb@test.com").await;

    // User A creates an agent.
    let (_, body) = app
        .post("/api/agents", json!({}), Some(&token_a))
        .await;
    let agent_id = serde_json::from_str::<serde_json::Value>(&body).unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();

    // User B cannot see it.
    let (status, _) = app
        .get(&format!("/api/agents/{agent_id}"), Some(&token_b))
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_agents_require_auth() {
    let db = setup_db().await;
    let app = TestApp::new(db);

    let (status, _) = app.get("/api/agents", None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    let (status, _) = app.post("/api/agents", json!({}), None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

// ===========================================================================
// Usage
// ===========================================================================

#[tokio::test]
async fn test_usage_returns_stats() {
    let db = setup_db().await;
    let app = TestApp::new(db);
    let token = signup_and_get_token(&app, "usage@test.com").await;

    let (status, body) = app.get("/api/usage", Some(&token)).await;
    assert_eq!(status, StatusCode::OK);

    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert!(v["today"].is_object());
    assert!(v["all_time"].is_object());
    assert_eq!(v["today"]["requests"], 0);
}

// ===========================================================================
// Notifications
// ===========================================================================

#[tokio::test]
async fn test_notifications_empty() {
    let db = setup_db().await;
    let app = TestApp::new(db);
    let token = signup_and_get_token(&app, "notif@test.com").await;

    let (status, body) = app.get("/api/notifications", Some(&token)).await;
    assert_eq!(status, StatusCode::OK);

    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert!(v.as_array().unwrap().is_empty());
}

// ===========================================================================
// Swagger / OpenAPI
// ===========================================================================

#[tokio::test]
async fn test_swagger_ui() {
    let db = setup_db().await;
    let app = TestApp::new(db);

    let (status, body) = app.get("/swagger-ui/", None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("swagger") || body.contains("Swagger") || body.contains("openapi"));
}

#[tokio::test]
async fn test_openapi_spec() {
    let db = setup_db().await;
    let app = TestApp::new(db);

    let (status, body) = app.get("/api-docs/openapi.json", None).await;
    assert_eq!(status, StatusCode::OK);

    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert!(v["openapi"].is_string() || v["info"].is_object());
}

// ===========================================================================
// Metrics
// ===========================================================================

#[tokio::test]
async fn test_metrics_endpoint() {
    let db = setup_db().await;
    let app = TestApp::new(db);

    let (status, _) = app.get("/metrics", None).await;
    assert_eq!(status, StatusCode::OK);
}
