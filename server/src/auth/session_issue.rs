//! Creating sessions: the single place every sign-in mints tokens
//! (spec B.4, C27; L1a owns it, LP calls it for passkey sign-in).
//!
//! The session's assurance, client class, sign-in methods and authenticator
//! are recorded at creation and never change. The absolute cap is fixed
//! from `created_at` by client class; the sliding refresh expiry never
//! passes it. IP and User-Agent are stored for display and audit only (no
//! `GeoIP` lookup, DC17).

use std::net::IpAddr;

use axum_extra::extract::cookie::Cookie;
use chrono::{DateTime, Duration, Utc};
use sqlx::PgConnection;
use uuid::Uuid;

use super::assurance::Assurance;
use super::client_class::ClientClass;
use super::ctx::AuthCtx;
use super::error::{AuthError, AuthResult};
use super::jwt::TokenPair;
use super::strong::AuthenticatorRef;
use super::types::AuthResponse;
use crate::config::Config;

/// How a session was created. Decides its assurance and `auth_methods`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SignInMethod {
    /// A passkey assertion with UV (LP).
    Passkey,
    /// A device-key signature of an active key device (Task 12).
    DeviceKey,
    /// QR login confirmed by a key device (L1b).
    Qr,
    /// Password + phone confirmation (L1b).
    PasswordPhone,
    /// OIDC + phone confirmation (L1b).
    OidcPhone,
    /// Password + TOTP fallback (L1b).
    PasswordTotp,
    /// OIDC + TOTP fallback (L1b).
    OidcTotp,
    /// Today's password login (until L1b Task 18 removes it).
    LegacyPassword,
    /// Today's password + TOTP login (until L1b Task 18).
    LegacyPasswordMfa,
    /// Today's OIDC login (until L1b Task 18).
    LegacyOidc,
    /// Today's QR redeem (until L1b Task 17).
    LegacyQrRedeem,
}

impl SignInMethod {
    /// `full` only for a passkey or a device key (CC4).
    #[must_use]
    pub const fn assurance(self) -> Assurance {
        match self {
            Self::Passkey | Self::DeviceKey => Assurance::Full,
            Self::Qr
            | Self::PasswordPhone
            | Self::OidcPhone
            | Self::PasswordTotp
            | Self::OidcTotp
            | Self::LegacyPassword
            | Self::LegacyPasswordMfa
            | Self::LegacyOidc
            | Self::LegacyQrRedeem => Assurance::Probation,
        }
    }

    /// The session's `auth_methods` (C27).
    #[must_use]
    pub const fn auth_methods(self) -> &'static [&'static str] {
        match self {
            Self::Passkey => &["passkey"],
            Self::DeviceKey => &["device_key"],
            Self::Qr | Self::LegacyQrRedeem => &["qr"],
            Self::PasswordPhone => &["password", "phone_confirm"],
            Self::OidcPhone => &["oidc", "phone_confirm"],
            Self::PasswordTotp | Self::LegacyPasswordMfa => &["password", "totp"],
            Self::OidcTotp => &["oidc", "totp"],
            Self::LegacyPassword => &["password"],
            Self::LegacyOidc => &["oidc"],
        }
    }
}

/// Where a sign-in came from (display and audit only).
#[derive(Debug, Default, Clone)]
pub struct SessionClient {
    /// The client IP after the trusted-proxy rule (`reauth::client_ip`).
    pub ip: Option<IpAddr>,
    /// The User-Agent as sent (bounded by the column, 512 characters).
    pub user_agent: Option<String>,
}

/// Maximum stored User-Agent length (the column is `VARCHAR(512)`).
const MAX_USER_AGENT_CHARS: usize = 512;

impl SessionClient {
    /// From a request: the client IP as the rate limiter sees it and the
    /// User-Agent, truncated on a character boundary.
    #[must_use]
    pub fn from_request(
        state: &crate::api::AppState,
        headers: &axum::http::HeaderMap,
        addr: std::net::SocketAddr,
    ) -> Self {
        let user_agent = super::helpers::extract_user_agent(headers)
            .map(|ua| ua.chars().take(MAX_USER_AGENT_CHARS).collect());
        Self {
            ip: Some(super::reauth::client_ip(state, headers, addr)),
            user_agent,
        }
    }
}

/// A session to create.
#[derive(Debug)]
pub struct NewSession<'a> {
    pub user_id: Uuid,
    pub method: SignInMethod,
    pub client: &'a SessionClient,
    pub client_class: ClientClass,
    /// The passkey or key device that created a `full` session; `Some` iff
    /// the method's assurance is `full`.
    pub authenticator: Option<AuthenticatorRef>,
}

/// A created session and its tokens.
#[derive(Debug)]
pub struct IssuedSession {
    pub tokens: TokenPair,
    pub session_id: Uuid,
    pub assurance: Assurance,
    pub absolute_expires_at: DateTime<Utc>,
    pub setup_complete: bool,
    /// Security events recorded in the issuing transaction (the session
    /// limit's `session_ended_limit`); deliver them after commit
    /// (`security_events::spawn_delivery`), else the sweep does.
    pub event_ids: Vec<Uuid>,
}

impl IssuedSession {
    /// The `AuthResponse` (C27 fields). `refresh_in_body` for native
    /// clients; browsers get the refresh token only as the cookie.
    #[must_use]
    pub fn into_response(
        self,
        password_change_required: bool,
        refresh_in_body: bool,
    ) -> AuthResponse {
        AuthResponse {
            access_token: self.tokens.access_token,
            refresh_token: refresh_in_body.then_some(self.tokens.refresh_token),
            expires_in: self.tokens.access_expires_in,
            token_type: "Bearer".to_string(),
            setup_required: !self.setup_complete,
            password_change_required,
            session_id: self.session_id,
            assurance: self.assurance,
            absolute_expires_at: self.absolute_expires_at,
        }
    }

    /// The browser refresh cookie for this session.
    #[must_use]
    pub fn refresh_cookie(&self, config: &Config) -> Cookie<'static> {
        super::cookies::build_refresh_cookie(
            &self.tokens.refresh_token,
            config.jwt_refresh_expiry,
            config,
        )
    }
}

/// Create a session in the caller's transaction.
///
/// Refuses the system account, takes the account's row lock, applies the
/// session limit, fixes the absolute cap by client class, and marks the
/// account's first activity (B.0).
#[tracing::instrument(skip(conn, ctx, new), fields(user_id = %new.user_id, method = ?new.method))]
pub async fn issue_session_tx(
    conn: &mut PgConnection,
    ctx: &AuthCtx<'_>,
    new: &NewSession<'_>,
) -> AuthResult<IssuedSession> {
    let assurance = new.method.assurance();
    if (assurance == Assurance::Full) != new.authenticator.is_some() {
        return Err(AuthError::Internal(
            "a full session needs exactly its authenticator".to_string(),
        ));
    }
    if crate::system_account::is_system_user(new.user_id) {
        return Err(AuthError::InvalidCredentials);
    }
    // Account lock (B.7 rule 1): serialises sign-ins of one account, so the
    // session limit holds under concurrency.
    let account: Option<(bool,)> =
        sqlx::query_as("SELECT is_system FROM users WHERE id = $1 FOR UPDATE")
            .bind(new.user_id)
            .fetch_optional(&mut *conn)
            .await?;
    match account {
        None => return Err(AuthError::UserNotFound),
        Some((true,)) => return Err(AuthError::InvalidCredentials),
        Some((false,)) => {}
    }

    // Session limit (B.4): at the limit, end the least recently used
    // sessions with a notice, under the account lock above.
    let mut event_ids = Vec::new();
    let live: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM sessions
          WHERE user_id = $1 AND ended_at IS NULL
            AND expires_at > NOW() AND absolute_expires_at > NOW()",
    )
    .bind(new.user_id)
    .fetch_one(&mut *conn)
    .await?;
    let excess = live - ctx.config.max_sessions_per_user + 1;
    if excess > 0 {
        let oldest: Vec<Uuid> = sqlx::query_scalar(
            "SELECT id FROM sessions
              WHERE user_id = $1 AND ended_at IS NULL
                AND expires_at > NOW() AND absolute_expires_at > NOW()
              ORDER BY last_used_at, created_at
              LIMIT $2",
        )
        .bind(new.user_id)
        .bind(excess)
        .fetch_all(&mut *conn)
        .await?;
        let ended = super::revocation::end_sessions_tx(
            conn,
            ctx,
            new.user_id,
            &oldest,
            super::revocation::EndReason::Limit,
        )
        .await?;
        for sid in ended {
            let mut ev = crate::security_events::NewSecurityEvent::new(
                new.user_id,
                crate::security_events::EventType::SessionEndedLimit,
                crate::security_events::Actor::System,
                serde_json::json!({ "session_id": sid }),
            );
            ev.session_id = Some(sid);
            event_ids.push(crate::security_events::record(conn, ev).await?);
        }
    }

    let session_id = Uuid::now_v7();
    let tokens = super::jwt::generate_session_tokens(
        new.user_id,
        session_id,
        assurance,
        0,
        &ctx.config.jwt_private_key,
        ctx.config.jwt_access_expiry,
        ctx.config.jwt_refresh_expiry,
    )?;
    let now = Utc::now();
    let absolute = now + new.client_class.absolute_cap();
    let expires_at = (now + Duration::seconds(ctx.config.jwt_refresh_expiry)).min(absolute);
    let methods: Vec<String> = new
        .method
        .auth_methods()
        .iter()
        .map(|m| (*m).to_string())
        .collect();
    sqlx::query(
        "INSERT INTO sessions (id, user_id, token_hash, expires_at, ip_address, user_agent,
                               assurance, auth_methods, auth_time, client_class,
                               authenticator_type, authenticator_id, absolute_expires_at,
                               last_used_at, created_at, refresh_generation, refresh_jti,
                               refresh_issued_at)
         VALUES ($1, $2, $3, $4, $5::inet, $6, $7, $8, $9, $10, $11, $12, $13, $9, $9, 0, $14,
                 to_timestamp($15))",
    )
    .bind(session_id)
    .bind(new.user_id)
    .bind(super::hash_token(&tokens.refresh_token))
    .bind(expires_at)
    .bind(new.client.ip.map(|ip| ip.to_string()))
    .bind(new.client.user_agent.as_deref())
    .bind(assurance.as_str())
    .bind(&methods)
    .bind(now)
    .bind(new.client_class.as_str())
    .bind(new.authenticator.map(|a| a.as_str()))
    .bind(new.authenticator.map(|a| a.id))
    .bind(absolute)
    .bind(tokens.refresh_jti)
    .bind(tokens.refresh_iat as f64)
    .execute(&mut *conn)
    .await?;

    sqlx::query(
        "UPDATE users SET first_activity_at = COALESCE(first_activity_at, NOW()) WHERE id = $1",
    )
    .bind(new.user_id)
    .execute(&mut *conn)
    .await?;

    let setup_complete = sqlx::query_scalar::<_, serde_json::Value>(
        "SELECT value FROM server_config WHERE key = 'setup_complete'",
    )
    .fetch_optional(&mut *conn)
    .await?
    .and_then(|v| v.as_bool())
    .unwrap_or(false);

    Ok(IssuedSession {
        tokens,
        session_id,
        assurance,
        absolute_expires_at: absolute,
        setup_complete,
        event_ids,
    })
}

/// [`issue_session_tx`] in its own transaction.
pub async fn issue_session(ctx: &AuthCtx<'_>, new: &NewSession<'_>) -> AuthResult<IssuedSession> {
    let mut tx = ctx.db.begin().await?;
    let issued = issue_session_tx(&mut tx, ctx, new).await?;
    tx.commit().await?;
    Ok(issued)
}
