//! JWT authentication extractor.
//!
//! [`AuthUser`] extracts and validates a `Bearer` token from the `Authorization`
//! header, making the decoded [`Claims`] available to any handler that includes
//! it as a parameter.

use axum::extract::{FromRef, FromRequestParts};
use axum::http::request::Parts;

use oneclick_shared::auth::{validate_token, Claims};
use oneclick_shared::errors::AppError;

use crate::state::AppState;

/// Authenticated user extractor.
///
/// # Usage
///
/// ```ignore
/// async fn my_handler(auth: AuthUser) -> impl IntoResponse {
///     let user_id = auth.0.sub;
///     // ...
/// }
/// ```
pub struct AuthUser(pub Claims);

impl<S> FromRequestParts<S> for AuthUser
where
    S: Send + Sync,
    AppState: FromRef<S>,
{
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let state = AppState::from_ref(state);

        let auth_header = parts
            .headers
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| {
                // HTTP auth scheme is case-insensitive (RFC 7235).
                let lower = v.to_lowercase();
                if lower.starts_with("bearer ") {
                    Some(v[7..].to_string())
                } else {
                    None
                }
            })
            .ok_or(AppError::Unauthorized)?;

        let claims = validate_token(&auth_header, &state.config.jwt_secret)
            .map_err(|_| AppError::Unauthorized)?;

        Ok(AuthUser(claims))
    }
}
