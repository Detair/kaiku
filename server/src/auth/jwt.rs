//! JWT Token Generation and Validation

use chrono::{Duration, Utc};
use jsonwebtoken::{decode, encode, Algorithm, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::error::{AuthError, AuthResult};

/// JWT claims for access and refresh tokens.
#[derive(Debug, Serialize, Deserialize)]
pub struct Claims {
    /// Subject (user ID as UUID string).
    pub sub: String,
    /// Expiration time (Unix timestamp).
    pub exp: i64,
    /// Issued at (Unix timestamp).
    pub iat: i64,
    /// Token type (access or refresh).
    pub typ: TokenType,
    /// JWT ID for refresh token revocation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub jti: Option<String>,
}

/// Token type discriminator.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum TokenType {
    /// Short-lived access token.
    Access,
    /// Long-lived refresh token.
    Refresh,
}

/// Token pair returned after successful authentication.
#[derive(Debug)]
pub struct TokenPair {
    /// Access token (short-lived).
    pub access_token: String,
    /// Refresh token (long-lived).
    pub refresh_token: String,
    /// Access token expiry in seconds.
    pub access_expires_in: i64,
    /// Refresh token ID for session tracking.
    pub refresh_token_id: Uuid,
}

/// Generate both access and refresh tokens.
///
/// # Arguments
/// * `user_id` - The user's UUID
/// * `secret` - JWT signing secret
/// * `access_expiry_seconds` - Access token validity (typically 900 = 15 min)
/// * `refresh_expiry_seconds` - Refresh token validity (typically 604800 = 7 days)
pub fn generate_token_pair(
    user_id: Uuid,
    secret: &str,
    access_expiry_seconds: i64,
    refresh_expiry_seconds: i64,
) -> AuthResult<TokenPair> {
    let now = Utc::now();
    let refresh_token_id = Uuid::now_v7();

    // Access token
    let access_claims = Claims {
        sub: user_id.to_string(),
        exp: (now + Duration::seconds(access_expiry_seconds)).timestamp(),
        iat: now.timestamp(),
        typ: TokenType::Access,
        jti: None,
    };

    let access_token = encode(
        &Header::new(Algorithm::HS256),
        &access_claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )?;

    // Refresh token (includes jti for revocation tracking)
    let refresh_claims = Claims {
        sub: user_id.to_string(),
        exp: (now + Duration::seconds(refresh_expiry_seconds)).timestamp(),
        iat: now.timestamp(),
        typ: TokenType::Refresh,
        jti: Some(refresh_token_id.to_string()),
    };

    let refresh_token = encode(
        &Header::new(Algorithm::HS256),
        &refresh_claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )?;

    Ok(TokenPair {
        access_token,
        refresh_token,
        access_expires_in: access_expiry_seconds,
        refresh_token_id,
    })
}

/// Validate and decode an access token.
///
/// Returns an error if the token is invalid, expired, or is a refresh token.
pub fn validate_access_token(token: &str, secret: &str) -> AuthResult<Claims> {
    let mut validation = Validation::new(Algorithm::HS256);
    validation.validate_exp = true;
    validation.leeway = 0;

    let token_data = decode::<Claims>(
        token,
        &DecodingKey::from_secret(secret.as_bytes()),
        &validation,
    )
    .map_err(|e| match e.kind() {
        jsonwebtoken::errors::ErrorKind::ExpiredSignature => AuthError::TokenExpired,
        _ => AuthError::InvalidToken,
    })?;

    // Ensure it's an access token
    if token_data.claims.typ != TokenType::Access {
        return Err(AuthError::InvalidToken);
    }

    Ok(token_data.claims)
}

/// Validate and decode a refresh token.
///
/// Returns an error if the token is invalid, expired, or is an access token.
pub fn validate_refresh_token(token: &str, secret: &str) -> AuthResult<Claims> {
    let mut validation = Validation::new(Algorithm::HS256);
    validation.validate_exp = true;
    validation.leeway = 0;

    let token_data = decode::<Claims>(
        token,
        &DecodingKey::from_secret(secret.as_bytes()),
        &validation,
    )
    .map_err(|e| match e.kind() {
        jsonwebtoken::errors::ErrorKind::ExpiredSignature => AuthError::TokenExpired,
        _ => AuthError::InvalidToken,
    })?;

    // Ensure it's a refresh token
    if token_data.claims.typ != TokenType::Refresh {
        return Err(AuthError::InvalidToken);
    }

    // Refresh tokens MUST have a jti
    if token_data.claims.jti.is_none() {
        return Err(AuthError::InvalidToken);
    }

    Ok(token_data.claims)
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_SECRET: &str = "test-secret-at-least-32-bytes-long!!";

    #[test]
    fn test_generate_token_pair() {
        let user_id = Uuid::now_v7();

        let tokens = generate_token_pair(user_id, TEST_SECRET, 900, 604800).unwrap();

        assert!(!tokens.access_token.is_empty());
        assert!(!tokens.refresh_token.is_empty());
        assert_eq!(tokens.access_expires_in, 900);
    }

    #[test]
    fn test_validate_access_token() {
        let user_id = Uuid::now_v7();

        let tokens = generate_token_pair(user_id, TEST_SECRET, 900, 604800).unwrap();
        let claims = validate_access_token(&tokens.access_token, TEST_SECRET).unwrap();

        assert_eq!(claims.sub, user_id.to_string());
        assert_eq!(claims.typ, TokenType::Access);
    }

    #[test]
    fn test_validate_refresh_token() {
        let user_id = Uuid::now_v7();

        let tokens = generate_token_pair(user_id, TEST_SECRET, 900, 604800).unwrap();
        let claims = validate_refresh_token(&tokens.refresh_token, TEST_SECRET).unwrap();

        assert_eq!(claims.sub, user_id.to_string());
        assert_eq!(claims.typ, TokenType::Refresh);
        assert!(claims.jti.is_some());
    }

    #[test]
    fn test_access_token_rejects_refresh_token() {
        let user_id = Uuid::now_v7();

        let tokens = generate_token_pair(user_id, TEST_SECRET, 900, 604800).unwrap();
        let result = validate_access_token(&tokens.refresh_token, TEST_SECRET);

        assert!(result.is_err());
    }

    #[test]
    fn test_refresh_token_rejects_access_token() {
        let user_id = Uuid::now_v7();

        let tokens = generate_token_pair(user_id, TEST_SECRET, 900, 604800).unwrap();
        let result = validate_refresh_token(&tokens.access_token, TEST_SECRET);

        assert!(result.is_err());
    }

    #[test]
    fn test_invalid_secret_fails() {
        let user_id = Uuid::now_v7();

        let tokens = generate_token_pair(user_id, TEST_SECRET, 900, 604800).unwrap();
        let result = validate_access_token(&tokens.access_token, "wrong-secret-key-here!!");

        assert!(result.is_err());
    }
}
