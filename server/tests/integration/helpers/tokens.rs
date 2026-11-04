//! Test-only token minting (L1a review m3, moved out of `auth::jwt`).
//!
//! The server mints tokens only through `jwt::generate_session_tokens` (for
//! a committed session row). Tests also need tokens the server no longer
//! issues: an access token without `assurance` and a refresh token without
//! `gen` (the pre-upgrade format), for an arbitrary `sid`. They are built
//! here from the public `Claims`, so no production caller can reach them.

use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use chrono::{Duration, Utc};
use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};
use uuid::Uuid;
use vc_server::auth::jwt::{Claims, TokenPair, TokenType};

/// An access token (`sid`, no `assurance`) and a legacy refresh token (a
/// fresh `jti`, no `gen`) for `session_id`.
pub fn token_pair_for_session(
    user_id: Uuid,
    session_id: Uuid,
    private_key: &str,
    access_expiry_seconds: i64,
    refresh_expiry_seconds: i64,
) -> Result<TokenPair, jsonwebtoken::errors::Error> {
    let now = Utc::now();
    let pem = STANDARD.decode(private_key).expect("base64 PEM key");
    let key = EncodingKey::from_ed_pem(&pem)?;
    let access = Claims {
        sub: user_id.to_string(),
        exp: (now + Duration::seconds(access_expiry_seconds)).timestamp(),
        iat: now.timestamp(),
        typ: TokenType::Access,
        jti: None,
        sid: Some(session_id.to_string()),
        assurance: None,
        gen: None,
    };
    let refresh_jti = Uuid::now_v7();
    let refresh = Claims {
        sub: user_id.to_string(),
        exp: (now + Duration::seconds(refresh_expiry_seconds)).timestamp(),
        iat: now.timestamp(),
        typ: TokenType::Refresh,
        jti: Some(refresh_jti.to_string()),
        sid: None,
        assurance: None,
        gen: None,
    };
    let header = Header::new(Algorithm::EdDSA);
    Ok(TokenPair {
        access_token: encode(&header, &access, &key)?,
        refresh_token: encode(&header, &refresh, &key)?,
        access_expires_in: access_expiry_seconds,
        refresh_token_id: session_id,
        refresh_jti,
        refresh_iat: now.timestamp(),
    })
}
