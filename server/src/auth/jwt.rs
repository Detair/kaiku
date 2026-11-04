//! JWT Token Generation and Validation
//!
//! Uses `EdDSA` (Ed25519) for asymmetric token signing/verification.
//! `EdDSA` offers better security, smaller keys, and faster operations than RSA.
//! This allows separate signing (private key) and verification (public key),
//! supporting distributed architectures securely.

use base64::engine::general_purpose::STANDARD;
use base64::Engine;
#[cfg(test)]
use chrono::Duration;
use chrono::Utc;
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
    /// Session ID (set on access tokens) linking the token to the session it
    /// was issued with, so logging out or revoking that session can invalidate
    /// the still-valid stateless access token via a short-lived denylist.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sid: Option<String>,
    /// Access tokens only: the session's assurance, for client display. The
    /// server never reads it; it decides from the committed session row
    /// (B.4 rev 5). A token without it is probation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assurance: Option<String>,
    /// Refresh tokens only: the session's refresh generation (DC2). A
    /// refresh token without it is a legacy token.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gen: Option<i64>,
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
    /// The refresh token's `jti` and `iat` (stored on the session row so a
    /// lost response can be answered with the identical successor, DC2).
    pub refresh_jti: Uuid,
    pub refresh_iat: i64,
}

/// Decode a base64-encoded PEM key.
fn decode_pem_key(base64_key: &str) -> AuthResult<Vec<u8>> {
    STANDARD
        .decode(base64_key)
        .map_err(|_| AuthError::Internal("Invalid base64 in JWT key".to_string()))
}

/// Generate both access and refresh tokens.
///
/// # Arguments
/// * `user_id` - The user's UUID
/// * `private_key` - Ed25519 private key (PEM format, base64-encoded)
/// * `access_expiry_seconds` - Access token validity (typically 900 = 15 min)
/// * `refresh_expiry_seconds` - Refresh token validity (typically 604800 = 7 days)
#[cfg(test)]
fn generate_token_pair(
    user_id: Uuid,
    private_key: &str,
    access_expiry_seconds: i64,
    refresh_expiry_seconds: i64,
) -> AuthResult<TokenPair> {
    // A new login: the session id doubles as the first refresh token's `jti`.
    let session_id = Uuid::now_v7();
    build_token_pair(
        user_id,
        session_id,
        session_id,
        None,
        private_key,
        access_expiry_seconds,
        refresh_expiry_seconds,
    )
}

/// Generate a token pair for an existing session (refresh rotation).
///
/// The access token's `sid` stays `session_id` for the whole login, so
/// revoking the session also reaches tokens and sockets from before a refresh;
/// the refresh token gets a fresh `jti` so every rotation yields a distinct
/// refresh token.
#[cfg(test)]
fn generate_token_pair_for_session(
    user_id: Uuid,
    session_id: Uuid,
    private_key: &str,
    access_expiry_seconds: i64,
    refresh_expiry_seconds: i64,
) -> AuthResult<TokenPair> {
    build_token_pair(
        user_id,
        session_id,
        Uuid::now_v7(),
        None,
        private_key,
        access_expiry_seconds,
        refresh_expiry_seconds,
    )
}

/// Tokens for a session created or renewed by `session_issue` / refresh.
///
/// The access token carries `sid` and the session's `assurance` (display
/// only); the refresh token carries `sid` and the refresh generation `gen`
/// (DC2), signed with the server's Ed25519 key, so a token can be
/// recognised as an earlier one of the same session without storing spent
/// hashes, and a forged one cannot end anyone's session.
///
/// # Errors
/// The signing key is invalid.
pub fn generate_session_tokens(
    user_id: Uuid,
    session_id: Uuid,
    assurance: super::assurance::Assurance,
    gen: i64,
    private_key: &str,
    access_expiry_seconds: i64,
    refresh_expiry_seconds: i64,
) -> AuthResult<TokenPair> {
    let now = Utc::now().timestamp();
    let access_token = encode_access(
        user_id,
        session_id,
        assurance,
        now,
        access_expiry_seconds,
        private_key,
    )?;
    let refresh_jti = Uuid::new_v4();
    let refresh_token = encode_refresh(
        user_id,
        session_id,
        gen,
        refresh_jti,
        now,
        now + refresh_expiry_seconds,
        private_key,
    )?;
    Ok(TokenPair {
        access_token,
        refresh_token,
        access_expires_in: access_expiry_seconds,
        refresh_token_id: session_id,
        refresh_jti,
        refresh_iat: now,
    })
}

fn encoding_key(private_key: &str) -> AuthResult<EncodingKey> {
    let key_bytes = decode_pem_key(private_key)?;
    EncodingKey::from_ed_pem(&key_bytes)
        .map_err(|e| AuthError::Internal(format!("Invalid Ed25519 private key: {e}")))
}

/// A fresh access token for a session.
///
/// # Errors
/// The signing key is invalid.
pub fn encode_access(
    user_id: Uuid,
    session_id: Uuid,
    assurance: super::assurance::Assurance,
    iat: i64,
    access_expiry_seconds: i64,
    private_key: &str,
) -> AuthResult<String> {
    let claims = Claims {
        sub: user_id.to_string(),
        exp: iat + access_expiry_seconds,
        iat,
        typ: TokenType::Access,
        jti: None,
        sid: Some(session_id.to_string()),
        assurance: Some(assurance.as_str().to_string()),
        gen: None,
    };
    Ok(encode(
        &Header::new(Algorithm::EdDSA),
        &claims,
        &encoding_key(private_key)?,
    )?)
}

/// A refresh token of the authenticated format (DC2). Deterministic: the
/// same inputs give the identical token (Ed25519), which answers a lost
/// response with the same successor.
///
/// # Errors
/// The signing key is invalid.
pub fn encode_refresh(
    user_id: Uuid,
    sid: Uuid,
    gen: i64,
    jti: Uuid,
    iat: i64,
    exp: i64,
    private_key: &str,
) -> AuthResult<String> {
    let claims = Claims {
        sub: user_id.to_string(),
        exp,
        iat,
        typ: TokenType::Refresh,
        jti: Some(jti.to_string()),
        sid: Some(sid.to_string()),
        assurance: None,
        gen: Some(gen),
    };
    Ok(encode(
        &Header::new(Algorithm::EdDSA),
        &claims,
        &encoding_key(private_key)?,
    )?)
}

/// Verify a refresh token's signature and type, **not** its expiry: the
/// session row decides whether the session has lapsed (answered as
/// `SESSION_EXPIRED`, C27).
///
/// # Errors
/// Bad signature, wrong type or a missing `jti`.
pub fn verify_refresh_signature(token: &str, public_key: &str) -> AuthResult<Claims> {
    let mut validation = Validation::new(Algorithm::EdDSA);
    validation.validate_exp = false;
    validation.required_spec_claims.remove("exp");
    let key_bytes = decode_pem_key(public_key)?;
    let decoding_key = DecodingKey::from_ed_pem(&key_bytes)
        .map_err(|e| AuthError::Internal(format!("Invalid Ed25519 public key: {e}")))?;
    let data =
        decode::<Claims>(token, &decoding_key, &validation).map_err(|_| AuthError::InvalidToken)?;
    if data.claims.typ != TokenType::Refresh || data.claims.jti.is_none() {
        return Err(AuthError::InvalidToken);
    }
    Ok(data.claims)
}

#[cfg(test)]
fn build_token_pair(
    user_id: Uuid,
    session_id: Uuid,
    refresh_jti: Uuid,
    assurance: Option<super::assurance::Assurance>,
    private_key: &str,
    access_expiry_seconds: i64,
    refresh_expiry_seconds: i64,
) -> AuthResult<TokenPair> {
    let now = Utc::now();
    let refresh_token_id = session_id;

    // Decode the private key from base64-encoded PEM
    let key_bytes = decode_pem_key(private_key)?;
    let encoding_key = EncodingKey::from_ed_pem(&key_bytes)
        .map_err(|e| AuthError::Internal(format!("Invalid Ed25519 private key: {e}")))?;

    // Access token. Carries `sid` = the refresh session's id so the token can
    // be revoked when that session is logged out or revoked. The matching
    // session row is stored under the same id (see `create_session`).
    let access_claims = Claims {
        sub: user_id.to_string(),
        exp: (now + Duration::seconds(access_expiry_seconds)).timestamp(),
        iat: now.timestamp(),
        typ: TokenType::Access,
        jti: None,
        sid: Some(refresh_token_id.to_string()),
        assurance: assurance.map(|a| a.as_str().to_string()),
        gen: None,
    };

    let access_token = encode(
        &Header::new(Algorithm::EdDSA),
        &access_claims,
        &encoding_key,
    )?;

    // Refresh token (includes jti for revocation tracking)
    let refresh_claims = Claims {
        sub: user_id.to_string(),
        exp: (now + Duration::seconds(refresh_expiry_seconds)).timestamp(),
        iat: now.timestamp(),
        typ: TokenType::Refresh,
        jti: Some(refresh_jti.to_string()),
        sid: None,
        assurance: None,
        gen: None,
    };

    let refresh_token = encode(
        &Header::new(Algorithm::EdDSA),
        &refresh_claims,
        &encoding_key,
    )?;

    Ok(TokenPair {
        access_token,
        refresh_token,
        access_expires_in: access_expiry_seconds,
        refresh_token_id,
        refresh_jti,
        refresh_iat: now.timestamp(),
    })
}

/// Validate and decode an access token.
///
/// Returns an error if the token is invalid, expired, or is a refresh token.
pub fn validate_access_token(token: &str, public_key: &str) -> AuthResult<Claims> {
    let mut validation = Validation::new(Algorithm::EdDSA);
    validation.validate_exp = true;
    validation.leeway = 0;

    // Decode the public key from base64-encoded PEM
    let key_bytes = decode_pem_key(public_key)?;
    let decoding_key = DecodingKey::from_ed_pem(&key_bytes)
        .map_err(|e| AuthError::Internal(format!("Invalid Ed25519 public key: {e}")))?;

    let token_data =
        decode::<Claims>(token, &decoding_key, &validation).map_err(|e| match e.kind() {
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
#[cfg(test)]
fn validate_refresh_token(token: &str, public_key: &str) -> AuthResult<Claims> {
    let mut validation = Validation::new(Algorithm::EdDSA);
    validation.validate_exp = true;
    validation.leeway = 0;

    // Decode the public key from base64-encoded PEM
    let key_bytes = decode_pem_key(public_key)?;
    let decoding_key = DecodingKey::from_ed_pem(&key_bytes)
        .map_err(|e| AuthError::Internal(format!("Invalid Ed25519 public key: {e}")))?;

    let token_data =
        decode::<Claims>(token, &decoding_key, &validation).map_err(|e| match e.kind() {
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

    // Test Ed25519 key pair - generated with:
    // openssl genpkey -algorithm Ed25519 -out ed25519_private.pem
    // openssl pkey -in ed25519_private.pem -pubout -out ed25519_public.pem
    const TEST_PRIVATE_KEY: &str = "LS0tLS1CRUdJTiBQUklWQVRFIEtFWS0tLS0tCk1DNENBUUF3QlFZREsyVndCQ0lFSUZuUDFodDNNcjlkOGJyYW4zV2IyTGFxSStqd2NnY0V4YXp2V0pQNWUrSG8KLS0tLS1FTkQgUFJJVkFURSBLRVktLS0tLQo=";
    const TEST_PUBLIC_KEY: &str = "LS0tLS1CRUdJTiBQVUJMSUMgS0VZLS0tLS0KTUNvd0JRWURLMlZ3QXlFQW80TlJjVnQ2ajF3OHRCWUtxUEJzS0krNUZVREkwVGtJaHF4WWlud05TRlU9Ci0tLS0tRU5EIFBVQkxJQyBLRVktLS0tLQo=";

    // A different Ed25519 public key for testing validation failure
    const WRONG_PUBLIC_KEY: &str = "LS0tLS1CRUdJTiBQVUJMSUMgS0VZLS0tLS0KTUNvd0JRWURLMlZ3QXlFQU5xRlcrTXJIWHUrKzhYS0hKam96Nnc1WXhIYXA5VjNqdDYrN0VKOWZ2ZGc9Ci0tLS0tRU5EIFBVQkxJQyBLRVktLS0tLQo=";

    #[test]
    fn test_generate_token_pair() {
        let user_id = Uuid::now_v7();

        let tokens = generate_token_pair(user_id, TEST_PRIVATE_KEY, 900, 604800).unwrap();

        assert!(!tokens.access_token.is_empty());
        assert!(!tokens.refresh_token.is_empty());
        assert_eq!(tokens.access_expires_in, 900);
    }

    #[test]
    fn test_validate_access_token() {
        let user_id = Uuid::now_v7();

        let tokens = generate_token_pair(user_id, TEST_PRIVATE_KEY, 900, 604800).unwrap();
        let claims = validate_access_token(&tokens.access_token, TEST_PUBLIC_KEY).unwrap();

        assert_eq!(claims.sub, user_id.to_string());
        assert_eq!(claims.typ, TokenType::Access);
    }

    #[test]
    fn test_validate_refresh_token() {
        let user_id = Uuid::now_v7();

        let tokens = generate_token_pair(user_id, TEST_PRIVATE_KEY, 900, 604800).unwrap();
        let claims = validate_refresh_token(&tokens.refresh_token, TEST_PUBLIC_KEY).unwrap();

        assert_eq!(claims.sub, user_id.to_string());
        assert_eq!(claims.typ, TokenType::Refresh);
        assert!(claims.jti.is_some());
    }

    #[test]
    fn test_access_token_rejects_refresh_token() {
        let user_id = Uuid::now_v7();

        let tokens = generate_token_pair(user_id, TEST_PRIVATE_KEY, 900, 604800).unwrap();
        let result = validate_access_token(&tokens.refresh_token, TEST_PUBLIC_KEY);

        assert!(result.is_err());
    }

    #[test]
    fn test_refresh_token_rejects_access_token() {
        let user_id = Uuid::now_v7();

        let tokens = generate_token_pair(user_id, TEST_PRIVATE_KEY, 900, 604800).unwrap();
        let result = validate_refresh_token(&tokens.access_token, TEST_PUBLIC_KEY);

        assert!(result.is_err());
    }

    #[test]
    fn test_access_token_sid_matches_refresh_jti() {
        // The access token's `sid` must equal the refresh token's `jti` so that
        // logging out / revoking the session (keyed on that id) invalidates the
        // paired access token via the denylist.
        let user_id = Uuid::now_v7();

        let tokens = generate_token_pair(user_id, TEST_PRIVATE_KEY, 900, 604800).unwrap();
        let access = validate_access_token(&tokens.access_token, TEST_PUBLIC_KEY).unwrap();
        let refresh = validate_refresh_token(&tokens.refresh_token, TEST_PUBLIC_KEY).unwrap();

        assert_eq!(
            access.sid.as_deref(),
            Some(tokens.refresh_token_id.to_string().as_str())
        );
        assert_eq!(access.sid, refresh.jti);
        assert!(access.jti.is_none(), "access token must not carry a jti");
    }

    #[test]
    fn test_rotated_pair_keeps_sid_with_fresh_jti() {
        let user_id = Uuid::now_v7();
        let first = generate_token_pair(user_id, TEST_PRIVATE_KEY, 900, 604800).unwrap();
        let rotated = generate_token_pair_for_session(
            user_id,
            first.refresh_token_id,
            TEST_PRIVATE_KEY,
            900,
            604800,
        )
        .unwrap();

        let access = validate_access_token(&rotated.access_token, TEST_PUBLIC_KEY).unwrap();
        let refresh = validate_refresh_token(&rotated.refresh_token, TEST_PUBLIC_KEY).unwrap();
        assert_eq!(rotated.refresh_token_id, first.refresh_token_id);
        assert_eq!(
            access.sid.as_deref(),
            Some(first.refresh_token_id.to_string().as_str())
        );
        assert_ne!(refresh.jti, access.sid);
        assert_ne!(rotated.refresh_token, first.refresh_token);
    }

    #[test]
    fn test_invalid_secret_fails() {
        let user_id = Uuid::now_v7();

        let tokens = generate_token_pair(user_id, TEST_PRIVATE_KEY, 900, 604800).unwrap();
        let result = validate_access_token(&tokens.access_token, WRONG_PUBLIC_KEY);

        assert!(result.is_err());
    }
}
