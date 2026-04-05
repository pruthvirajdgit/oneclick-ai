//! Authentication endpoints: signup, login, and token refresh.

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::post;
use axum::{Json, Router};
use uuid::Uuid;

use oneclick_shared::auth::{create_token, hash_password, verify_password};
use oneclick_shared::errors::{AppError, AppResult};
use oneclick_shared::models::user::{
    AuthResponse, CreateUserRequest, LoginRequest, User, UserResponse,
};

use crate::middleware::auth::AuthUser;
use crate::state::AppState;

/// Mount auth routes under a common prefix.
pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/signup", post(signup))
        .route("/login", post(login))
        .route("/refresh", post(refresh))
}

/// `POST /api/auth/signup` — Create a new user account and return a JWT.
async fn signup(
    State(state): State<AppState>,
    Json(req): Json<CreateUserRequest>,
) -> AppResult<impl IntoResponse> {
    // Validate email format (basic check).
    if !req.email.contains('@') || req.email.len() < 5 {
        return Err(AppError::BadRequest("Invalid email format".into()));
    }

    // Validate password length.
    if req.password.len() < 8 {
        return Err(AppError::BadRequest(
            "Password must be at least 8 characters".into(),
        ));
    }

    tracing::info!(email = %req.email, "Signup attempt");

    // Hash password with Argon2.
    let password_hash =
        hash_password(&req.password).map_err(|e| AppError::Internal(e.to_string()))?;

    let user_id = Uuid::new_v4();

    // Insert user — unique constraint on email will catch duplicates.
    let user = sqlx::query_as::<_, User>(
        "INSERT INTO users (id, email, password, tier) VALUES ($1, $2, $3, $4) RETURNING *",
    )
    .bind(user_id)
    .bind(&req.email)
    .bind(&password_hash)
    .bind("free")
    .fetch_one(&state.db)
    .await
    .map_err(|e| match e {
        sqlx::Error::Database(ref db_err) if db_err.constraint() == Some("users_email_key") => {
            AppError::Conflict("Email already registered".into())
        }
        other => AppError::Database(other),
    })?;

    // Create JWT.
    let token = create_token(
        user.id,
        &user.email,
        &user.tier,
        &state.config.jwt_secret,
        state.config.jwt_expiry_hours,
    )
    .map_err(|e| AppError::Internal(e.to_string()))?;

    tracing::info!(user_id = %user.id, "User signed up");

    Ok((
        StatusCode::CREATED,
        Json(AuthResponse {
            token,
            user: UserResponse::from(user),
        }),
    ))
}

/// `POST /api/auth/login` — Authenticate and return a JWT.
async fn login(
    State(state): State<AppState>,
    Json(req): Json<LoginRequest>,
) -> AppResult<impl IntoResponse> {
    tracing::info!(email = %req.email, "Login attempt");

    let user = sqlx::query_as::<_, User>("SELECT * FROM users WHERE email = $1")
        .bind(&req.email)
        .fetch_optional(&state.db)
        .await?
        .ok_or(AppError::Unauthorized)?;

    let valid =
        verify_password(&req.password, &user.password).map_err(|_| AppError::Unauthorized)?;

    if !valid {
        return Err(AppError::Unauthorized);
    }

    let token = create_token(
        user.id,
        &user.email,
        &user.tier,
        &state.config.jwt_secret,
        state.config.jwt_expiry_hours,
    )
    .map_err(|e| AppError::Internal(e.to_string()))?;

    tracing::info!(user_id = %user.id, "User logged in");

    Ok(Json(AuthResponse {
        token,
        user: UserResponse::from(user),
    }))
}

/// `POST /api/auth/refresh` — Issue a fresh JWT (requires valid current token).
///
/// Re-reads the user from the database to ensure the account still exists
/// and picks up any tier or email changes.
async fn refresh(
    State(state): State<AppState>,
    auth: AuthUser,
) -> AppResult<impl IntoResponse> {
    let claims = auth.0;

    tracing::info!(user_id = %claims.sub, "Token refresh");

    // Re-read user from DB to verify account still exists and get current state.
    let user = sqlx::query_as::<_, User>("SELECT * FROM users WHERE id = $1")
        .bind(claims.sub)
        .fetch_optional(&state.db)
        .await?
        .ok_or(AppError::Unauthorized)?;

    let token = create_token(
        user.id,
        &user.email,
        &user.tier,
        &state.config.jwt_secret,
        state.config.jwt_expiry_hours,
    )
    .map_err(|e| AppError::Internal(e.to_string()))?;

    Ok(Json(AuthResponse {
        token,
        user: UserResponse::from(user),
    }))
}
