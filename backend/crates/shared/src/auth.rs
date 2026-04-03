use argon2::password_hash::rand_core::OsRng;
use argon2::password_hash::SaltString;
use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier};
use chrono::Utc;
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// JWT claims stored in the token payload.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Claims {
    /// User ID
    pub sub: Uuid,
    /// User email
    pub email: String,
    /// User tier (free, pro)
    pub tier: String,
    /// Expiry timestamp (seconds since epoch)
    pub exp: usize,
    /// Issued at timestamp
    pub iat: usize,
}

/// Hash a plaintext password using Argon2.
pub fn hash_password(password: &str) -> anyhow::Result<String> {
    let salt = SaltString::generate(&mut OsRng);
    let hash = Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map_err(|e| anyhow::anyhow!("Password hashing failed: {}", e))?
        .to_string();
    Ok(hash)
}

/// Verify a plaintext password against an Argon2 hash.
pub fn verify_password(password: &str, hash: &str) -> anyhow::Result<bool> {
    let parsed = PasswordHash::new(hash)
        .map_err(|e| anyhow::anyhow!("Invalid password hash: {}", e))?;
    Ok(Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .is_ok())
}

/// Create a signed JWT token for a user.
pub fn create_token(
    user_id: Uuid,
    email: &str,
    tier: &str,
    secret: &str,
    expiry_hours: u64,
) -> anyhow::Result<String> {
    let now = Utc::now();
    let claims = Claims {
        sub: user_id,
        email: email.to_string(),
        tier: tier.to_string(),
        exp: (now + chrono::Duration::hours(expiry_hours as i64)).timestamp() as usize,
        iat: now.timestamp() as usize,
    };
    let token = encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )?;
    Ok(token)
}

/// Validate a JWT token and extract claims.
pub fn validate_token(token: &str, secret: &str) -> anyhow::Result<Claims> {
    let data = decode::<Claims>(
        token,
        &DecodingKey::from_secret(secret.as_bytes()),
        &Validation::default(),
    )?;
    Ok(data.claims)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_password_hash_and_verify() {
        let password = "secure-password-123";
        let hash = hash_password(password).unwrap();

        assert!(verify_password(password, &hash).unwrap());
        assert!(!verify_password("wrong-password", &hash).unwrap());
    }

    #[test]
    fn test_jwt_create_and_validate() {
        let user_id = Uuid::new_v4();
        let secret = "test-secret-key";

        let token = create_token(user_id, "test@example.com", "free", secret, 24).unwrap();
        let claims = validate_token(&token, secret).unwrap();

        assert_eq!(claims.sub, user_id);
        assert_eq!(claims.email, "test@example.com");
        assert_eq!(claims.tier, "free");
    }

    #[test]
    fn test_jwt_invalid_secret_fails() {
        let user_id = Uuid::new_v4();
        let token = create_token(user_id, "test@example.com", "free", "secret-1", 24).unwrap();
        let result = validate_token(&token, "secret-2");

        assert!(result.is_err());
    }
}
