//! The login-attempt state machine (SM1) and the `login` approval arms.
//!
//! States: `open`, `claimed`, `approved`, `consumed`; terminal: `denied`,
//! `reported`, `cancelled`, `expired`. Every transition runs in one
//! transaction (B.7 rule 1, lock order in the module docs). Secrets are
//! 256-bit, stored only as keyed hashes and compared in constant time.

use chrono::{DateTime, Duration, Utc};
use sha2::{Digest, Sha256};
use sqlx::PgConnection;
use uuid::Uuid;

use super::types::{LoginAttempt, LoginQr};
use crate::approvals::context::{CanonicalContext, LoginContext, Variant};
use crate::approvals::core::{
    issue, set_state, verify, ChallengeRow, ChallengeState, NewChallenge, Requester,
};
use crate::approvals::devices::{key_device_session, KeyDeviceSession};
use crate::approvals::error::ApprovalError;
use crate::approvals::secrets::{b64u_decode, ct_eq, keyed_hash, Secret256, SecretPurpose};
use crate::approvals::signing::Decision;
use crate::approvals::types::{AnswerResponse, ApprovalChallenge};
use crate::auth::client_class::ClientClass;
use crate::auth::ctx::AuthCtx;
use crate::auth::error::{AuthError, AuthResult};
use crate::auth::factors::{self, ExitPath, Factor, FactorRef, StateCause};
use crate::auth::middleware::AuthUser;
use crate::auth::session_issue::{
    issue_session_tx, IssuedSession, NewSession, SessionClient, SignInMethod,
};
use crate::auth::strong::AuthenticatorRef;
use crate::security_events::{record, Actor, EventType, NewSecurityEvent};

/// An attempt lives at most 10 minutes (SM1; also a DB CHECK).
pub const ATTEMPT_TTL_SECS: i64 = 600;
/// A QR token is current for 60 s (F2).
pub const QR_TOKEN_SECS: i64 = 60;
/// The `login` challenge lives 2 minutes from the claim (C26).
pub const CLAIM_TTL_SECS: i64 = 120;
/// Collect within 60 s of the approval (C27).
pub const COLLECT_WINDOW_SECS: i64 = 60;
/// The longest long-poll (DC13).
pub const MAX_WAIT_SECS: u8 = 25;
/// Long-poll re-read interval (DC13).
const POLL_STEP: std::time::Duration = std::time::Duration::from_millis(500);
/// Label bound (the column CHECK and Review Focus 5).
const MAX_LABEL_CHARS: usize = 64;

/// The kind of an attempt (and the `login` variant).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AttemptKind {
    Qr,
    Password,
    Oidc,
}

impl AttemptKind {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Qr => "qr",
            Self::Password => "password",
            Self::Oidc => "oidc",
        }
    }

    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "qr" => Some(Self::Qr),
            "password" => Some(Self::Password),
            "oidc" => Some(Self::Oidc),
            _ => None,
        }
    }

    const fn variant(self) -> Variant {
        match self {
            Self::Qr => Variant::Qr,
            Self::Password => Variant::Password,
            Self::Oidc => Variant::Oidc,
        }
    }

    /// The session's sign-in method after a phone confirmation.
    const fn phone_method(self) -> SignInMethod {
        match self {
            Self::Qr => SignInMethod::Qr,
            Self::Password => SignInMethod::PasswordPhone,
            Self::Oidc => SignInMethod::OidcPhone,
        }
    }
}

/// What the PC presents to prove it owns the attempt.
#[derive(Clone)]
pub enum CollectionSecret {
    /// The browser's attempt cookie.
    Cookie(Secret256),
    /// A native client's verifier.
    Verifier(String),
}

impl std::fmt::Debug for CollectionSecret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Cookie(_) => "Cookie([REDACTED])",
            Self::Verifier(_) => "Verifier([REDACTED])",
        })
    }
}

/// How a new attempt is bound to its PC.
#[derive(Debug, Clone)]
pub enum CollectionBinding {
    /// Browser: the server mints a cookie secret.
    Cookie,
    /// Native: the client's S256 challenge (43 b64u characters).
    S256(String),
}

/// A new attempt.
#[derive(Debug, Clone)]
pub struct NewAttempt {
    pub kind: AttemptKind,
    /// The first factor's account (`password`, `oidc`); `None` for QR.
    pub user_id: Option<Uuid>,
    pub first_factor: Option<FactorRef>,
    pub client_class: ClientClass,
    /// Already bounded ([`client_label`]).
    pub client_label: String,
    pub client: SessionClient,
    pub binding: CollectionBinding,
}

/// A stored attempt.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct AttemptRow {
    pub id: Uuid,
    pub kind: String,
    pub collection_hash: Vec<u8>,
    pub collection_kind: String,
    pub collection_digest: Vec<u8>,
    pub qr_token_hash: Option<Vec<u8>>,
    pub qr_token_at: Option<DateTime<Utc>>,
    pub qr_token_used: bool,
    pub user_id: Option<Uuid>,
    pub first_factor: Option<String>,
    pub first_factor_generation: Option<i64>,
    pub client_class: String,
    pub client_label: String,
    pub user_agent: Option<String>,
    pub place: String,
    pub state: String,
    pub claimed_by_device: Option<Uuid>,
    pub challenge_id: Option<Uuid>,
    pub approved_at: Option<DateTime<Utc>>,
    pub session_id: Option<Uuid>,
    pub unfinished_recorded: bool,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub consumed_at: Option<DateTime<Utc>>,
    pub ended_at: Option<DateTime<Utc>>,
}

/// Every column but `ip` (read separately where needed).
macro_rules! columns {
    () => {
        "id, kind, collection_hash, collection_kind, collection_digest, qr_token_hash,
         qr_token_at, qr_token_used, user_id, first_factor, first_factor_generation,
         client_class, client_label, user_agent, place, state, claimed_by_device, challenge_id,
         approved_at, session_id, unfinished_recorded, created_at, expires_at, consumed_at,
         ended_at"
    };
}

impl AttemptRow {
    #[must_use]
    pub fn kind(&self) -> AttemptKind {
        AttemptKind::parse(&self.kind).unwrap_or(AttemptKind::Qr)
    }

    /// The first factor as used (password or OIDC attempts).
    #[must_use]
    pub fn first_factor_ref(&self) -> Option<FactorRef> {
        let factor = match self.first_factor.as_deref()? {
            "password" => Factor::Password,
            // OIDC has no local generation; only a password is ever marked.
            _ => return None,
        };
        Some(FactorRef {
            factor,
            generation: self.first_factor_generation?,
        })
    }

    fn is_terminal(&self) -> bool {
        matches!(
            self.state.as_str(),
            "consumed" | "denied" | "reported" | "cancelled" | "expired"
        )
    }
}

/// A created attempt: the wire form (with the first QR) and, for a browser,
/// the cookie secret to set.
#[derive(Debug)]
pub struct CreatedAttempt {
    pub attempt: LoginAttempt,
    pub row: AttemptRow,
    pub cookie: Option<Secret256>,
}

/// `kaiku://login/v1?o=<percent-encoded origin>&t=<b64u token>` (C27).
#[must_use]
pub fn qr_payload(origin: &str, token: &Secret256) -> String {
    let o: String = url::form_urlencoded::byte_serialize(origin.as_bytes()).collect();
    format!("kaiku://login/v1?o={o}&t={}", token.to_b64u())
}

/// The PC's display label from its class and User-Agent, cleaned and
/// bounded once at intake (Review Focus 5): control characters and bidi
/// overrides removed, ≤ 64 characters.
#[must_use]
pub fn client_label(class: ClientClass, user_agent: Option<&str>) -> String {
    let parsed = user_agent.map(crate::auth::ua_parser::parse_device_name);
    let os = parsed
        .as_deref()
        .and_then(|p| p.split_once(" on ").map(|(_, os)| os.to_string()));
    let raw = match class {
        ClientClass::Desktop => os.map_or_else(
            || "Kaiku desktop".to_string(),
            |os| format!("Kaiku desktop on {os}"),
        ),
        ClientClass::Android => os.map_or_else(
            || "Kaiku on Android".to_string(),
            |os| format!("Kaiku on {os}"),
        ),
        ClientClass::Web => parsed.unwrap_or_else(|| "Web browser".to_string()),
    };
    let cleaned: String = raw
        .chars()
        .filter(|c| {
            !c.is_control() && !matches!(*c, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
        })
        .take(MAX_LABEL_CHARS)
        .collect();
    let trimmed = cleaned.trim();
    if trimmed.is_empty() {
        "Unknown device".to_string()
    } else {
        trimmed.to_string()
    }
}

fn token_hash(ctx: &AuthCtx<'_>, token: &[u8]) -> [u8; 32] {
    keyed_hash(ctx.hmac_key(), SecretPurpose::LoginToken, token)
}

/// Whether the account has an active TOTP (`totp_available`).
async fn totp_available(conn: &mut PgConnection, row: &AttemptRow) -> sqlx::Result<bool> {
    if row.kind() == AttemptKind::Qr {
        return Ok(false);
    }
    let Some(user) = row.user_id else {
        return Ok(false);
    };
    sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM users
                         WHERE id = $1 AND mfa_secret IS NOT NULL AND totp_state = 'active')",
    )
    .bind(user)
    .fetch_one(&mut *conn)
    .await
}

/// The wire form; `qr` only with a freshly minted token.
pub async fn to_wire(
    conn: &mut PgConnection,
    ctx: &AuthCtx<'_>,
    row: &AttemptRow,
    minted: Option<&Secret256>,
) -> sqlx::Result<LoginAttempt> {
    let qr = minted.map(|t| LoginQr {
        payload: qr_payload(ctx.origin(), t),
        expires_at: row.qr_token_at.unwrap_or_else(Utc::now) + Duration::seconds(QR_TOKEN_SECS),
    });
    Ok(LoginAttempt {
        attempt_id: row.id,
        kind: row.kind.clone(),
        state: row.state.clone(),
        expires_at: row.expires_at,
        qr,
        totp_available: totp_available(conn, row).await?,
    })
}

/// Create an attempt in `open` with its first QR token.
#[tracing::instrument(skip(conn, ctx, new), fields(kind = new.kind.as_str()))]
pub async fn create_tx(
    conn: &mut PgConnection,
    ctx: &AuthCtx<'_>,
    new: NewAttempt,
) -> AuthResult<CreatedAttempt> {
    let (collection_kind, collection_hash, digest, cookie) = match &new.binding {
        CollectionBinding::Cookie => {
            let secret = Secret256::random();
            let hash = keyed_hash(
                ctx.hmac_key(),
                SecretPurpose::CollectionSecret,
                secret.as_bytes(),
            );
            let digest: [u8; 32] = Sha256::digest(secret.as_bytes()).into();
            ("cookie", hash.to_vec(), digest.to_vec(), Some(secret))
        }
        CollectionBinding::S256(challenge) => {
            let bytes = b64u_decode(challenge)
                .filter(|b| b.len() == 32 && challenge.len() == 43)
                .ok_or_else(|| AuthError::Validation("client_challenge".into()))?;
            ("s256", bytes.clone(), bytes, None)
        }
    };
    let token = Secret256::random();
    let now = Utc::now();
    let expires_at = now + Duration::seconds(ATTEMPT_TTL_SECS);
    let id = Uuid::new_v4();
    let row: AttemptRow = sqlx::query_as(concat!(
        "INSERT INTO login_attempts
            (id, kind, collection_hash, collection_kind, collection_digest, qr_token_hash,
             qr_token_at, user_id, first_factor, first_factor_generation, client_class,
             client_label, ip, user_agent, state, created_at, expires_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13::inet, $14, 'open', $7, $15)
         RETURNING ",
        columns!()
    ))
    .bind(id)
    .bind(new.kind.as_str())
    .bind(&collection_hash)
    .bind(collection_kind)
    .bind(&digest)
    .bind(token_hash(ctx, token.as_bytes()).to_vec())
    .bind(now)
    .bind(new.user_id)
    .bind(
        new.first_factor
            .map(|f| f.factor.as_str())
            .or(match new.kind {
                AttemptKind::Oidc => Some("oidc"),
                _ => None,
            }),
    )
    .bind(match new.kind {
        AttemptKind::Qr => None,
        AttemptKind::Password => new.first_factor.map(|f| f.generation),
        AttemptKind::Oidc => Some(0),
    })
    .bind(new.client_class.as_str())
    .bind(&new.client_label)
    .bind(new.client.ip.map(|ip| ip.to_string()))
    .bind(new.client.user_agent.as_deref())
    .bind(expires_at)
    .fetch_one(&mut *conn)
    .await?;
    let attempt = to_wire(conn, ctx, &row, Some(&token)).await?;
    Ok(CreatedAttempt {
        attempt,
        row,
        cookie,
    })
}

/// Cancel the open attempt bound to a browser's cookie (one live attempt
/// per screen). Touches only the attempt row.
pub async fn cancel_by_cookie(
    conn: &mut PgConnection,
    ctx: &AuthCtx<'_>,
    cookie: &Secret256,
) -> sqlx::Result<u64> {
    let hash = keyed_hash(
        ctx.hmac_key(),
        SecretPurpose::CollectionSecret,
        cookie.as_bytes(),
    );
    Ok(sqlx::query(
        "UPDATE login_attempts SET state = 'cancelled', ended_at = NOW()
          WHERE collection_kind = 'cookie' AND collection_hash = $1
            AND state IN ('open', 'claimed')",
    )
    .bind(hash.to_vec())
    .execute(&mut *conn)
    .await?
    .rows_affected())
}

async fn read(conn: &mut PgConnection, id: Uuid, lock: bool) -> sqlx::Result<Option<AttemptRow>> {
    let sql = if lock {
        concat!(
            "SELECT ",
            columns!(),
            " FROM login_attempts WHERE id = $1 FOR UPDATE"
        )
    } else {
        concat!("SELECT ", columns!(), " FROM login_attempts WHERE id = $1")
    };
    sqlx::query_as(sql)
        .bind(id)
        .fetch_optional(&mut *conn)
        .await
}

/// Whether `secret` is this attempt's collection secret (constant time).
fn secret_matches(ctx: &AuthCtx<'_>, row: &AttemptRow, secret: &CollectionSecret) -> bool {
    match (row.collection_kind.as_str(), secret) {
        ("cookie", CollectionSecret::Cookie(s)) => ct_eq(
            &keyed_hash(
                ctx.hmac_key(),
                SecretPurpose::CollectionSecret,
                s.as_bytes(),
            ),
            &row.collection_hash,
        ),
        ("s256", CollectionSecret::Verifier(v)) if !v.is_empty() && v.len() <= 128 => {
            ct_eq(&Sha256::digest(v.as_bytes()), &row.collection_hash)
        }
        _ => false,
    }
}

/// The attempt `id` if `secret` is its collection secret, else 404
/// `LOGIN_ATTEMPT_UNKNOWN` (unknown id and wrong secret alike).
pub async fn authenticate(
    conn: &mut PgConnection,
    ctx: &AuthCtx<'_>,
    id: Uuid,
    secret: &CollectionSecret,
) -> AuthResult<AttemptRow> {
    let row = read(conn, id, false)
        .await?
        .ok_or(AuthError::LoginAttemptUnknown)?;
    if secret_matches(ctx, &row, secret) {
        Ok(row)
    } else {
        Err(AuthError::LoginAttemptUnknown)
    }
}

/// Apply the lazy transitions of one attempt (attempt row only): a claim
/// whose challenge is gone returns to `open` (SM1 release/expiry); a live
/// attempt past its deadline, or approved and not collected within 60 s,
/// becomes `expired`.
async fn refresh(conn: &mut PgConnection, id: Uuid) -> sqlx::Result<()> {
    sqlx::query(
        "UPDATE login_attempts a
            SET state = 'open', claimed_by_device = NULL, challenge_id = NULL,
                user_id = CASE WHEN a.kind = 'qr' THEN NULL ELSE a.user_id END
          WHERE a.id = $1 AND a.state = 'claimed' AND a.expires_at > NOW()
            AND NOT EXISTS (SELECT 1 FROM approvals c
                             WHERE c.id = a.challenge_id AND c.state = 'issued'
                               AND c.expires_at > NOW())",
    )
    .bind(id)
    .execute(&mut *conn)
    .await?;
    sqlx::query(
        "UPDATE login_attempts
            SET state = 'expired', ended_at = NOW()
          WHERE id = $1 AND state IN ('open', 'claimed', 'approved')
            AND (expires_at <= NOW()
                 OR (state = 'approved' AND approved_at <= NOW() - make_interval(secs => $2)))",
    )
    .bind(id)
    .bind(COLLECT_WINDOW_SECS as f64)
    .execute(&mut *conn)
    .await?;
    Ok(())
}

/// Mint a new QR token if the attempt is `open` and its token is ≥ 60 s old
/// or was used by a claim (a rotation resets no counter).
async fn rotate(
    conn: &mut PgConnection,
    ctx: &AuthCtx<'_>,
    id: Uuid,
) -> sqlx::Result<Option<(AttemptRow, Secret256)>> {
    let token = Secret256::random();
    let row: Option<AttemptRow> = sqlx::query_as(concat!(
        "UPDATE login_attempts
            SET qr_token_hash = $2, qr_token_at = NOW(), qr_token_used = false
          WHERE id = $1 AND state = 'open' AND expires_at > NOW()
            AND (qr_token_used OR qr_token_at IS NULL
                 OR qr_token_at <= NOW() - make_interval(secs => $3))
          RETURNING ",
        columns!()
    ))
    .bind(id)
    .bind(token_hash(ctx, token.as_bytes()).to_vec())
    .bind(QR_TOKEN_SECS as f64)
    .fetch_optional(&mut *conn)
    .await?;
    Ok(row.map(|r| (r, token)))
}

/// `poll`: the attempt's state; a new QR token when the old one is due; a
/// long-poll of up to `wait` seconds returns early on a state change or a
/// new token (DC13: re-read every 500 ms).
#[tracing::instrument(skip(ctx, secret))]
pub async fn poll(
    ctx: &AuthCtx<'_>,
    id: Uuid,
    secret: &CollectionSecret,
    wait: u8,
) -> AuthResult<LoginAttempt> {
    let mut conn = ctx.db.acquire().await?;
    let first = authenticate(&mut conn, ctx, id, secret).await?;
    let deadline = tokio::time::Instant::now()
        + std::time::Duration::from_secs(u64::from(wait.min(MAX_WAIT_SECS)));
    loop {
        refresh(&mut conn, id).await?;
        if let Some((row, token)) = rotate(&mut conn, ctx, id).await? {
            return Ok(to_wire(&mut conn, ctx, &row, Some(&token)).await?);
        }
        let row = read(&mut conn, id, false)
            .await?
            .ok_or(AuthError::LoginAttemptUnknown)?;
        if row.state != first.state || tokio::time::Instant::now() >= deadline {
            return Ok(to_wire(&mut conn, ctx, &row, None).await?);
        }
        tokio::time::sleep(POLL_STEP).await;
    }
}

/// `cancel`: `open` or `claimed` → `cancelled` (attempt row only; a claimed
/// challenge dies at its answer). Terminal attempts are left as they are.
#[tracing::instrument(skip(conn, ctx, secret))]
pub async fn cancel_tx(
    conn: &mut PgConnection,
    ctx: &AuthCtx<'_>,
    id: Uuid,
    secret: &CollectionSecret,
) -> AuthResult<AttemptRow> {
    authenticate(conn, ctx, id, secret).await?;
    sqlx::query(
        "UPDATE login_attempts SET state = 'cancelled', ended_at = NOW()
          WHERE id = $1 AND state IN ('open', 'claimed')",
    )
    .bind(id)
    .execute(&mut *conn)
    .await?;
    read(conn, id, false)
        .await?
        .ok_or(AuthError::LoginAttemptUnknown)
}

/// Lock the attempt for a PC-side transition that mints a session: the
/// account first (read without a lock), then the attempt; `None` if its
/// account changed in between (the caller retries in a new transaction).
async fn lock_for_pc(
    conn: &mut PgConnection,
    ctx: &AuthCtx<'_>,
    id: Uuid,
    secret: &CollectionSecret,
) -> AuthResult<Option<AttemptRow>> {
    let seen = authenticate(conn, ctx, id, secret).await?;
    if let Some(user) = seen.user_id {
        crate::approvals::transition::lock_accounts(conn, &[user]).await?;
    }
    let row = read(conn, id, true)
        .await?
        .ok_or(AuthError::LoginAttemptUnknown)?;
    if row.user_id != seen.user_id {
        return Ok(None);
    }
    Ok(Some(row))
}

/// The terminal answer for an attempt that cannot be collected.
fn ended(row: &AttemptRow) -> AuthError {
    if row.is_terminal() {
        AuthError::LoginAttemptEnded {
            state: row.state.clone(),
        }
    } else {
        AuthError::LoginAttemptNotReady
    }
}

/// Mark an attempt `expired` inside the transaction (rule 4 failures).
async fn expire_row(conn: &mut PgConnection, id: Uuid) -> sqlx::Result<()> {
    sqlx::query(
        "UPDATE login_attempts SET state = 'expired', ended_at = NOW()
          WHERE id = $1 AND state IN ('open', 'claimed', 'approved')",
    )
    .bind(id)
    .execute(&mut *conn)
    .await?;
    Ok(())
}

/// A collected attempt: the new session and the events to deliver.
#[derive(Debug)]
pub struct Collected {
    pub issued: IssuedSession,
    pub password_change_required: bool,
    pub event_ids: Vec<Uuid>,
}

/// Outcome of one locked try (the account changed: retry).
pub enum Locked<T> {
    Done(T),
    Retry,
}

/// `collect`: `approved` → `consumed` within 60 s of the approval, while the
/// approving device is active and the first-factor generation unchanged
/// (B.7 rule 4); mints a probation session.
#[tracing::instrument(skip(conn, ctx, secret, client))]
pub async fn collect_tx(
    conn: &mut PgConnection,
    ctx: &AuthCtx<'_>,
    id: Uuid,
    secret: &CollectionSecret,
    client: &SessionClient,
) -> AuthResult<Locked<Collected>> {
    let Some(row) = lock_for_pc(conn, ctx, id, secret).await? else {
        return Ok(Locked::Retry);
    };
    if row.state != "approved" {
        return Err(ended(&row));
    }
    let user = row.user_id.ok_or(AuthError::LoginAttemptNotReady)?;
    let in_window = row
        .approved_at
        .is_some_and(|t| t + Duration::seconds(COLLECT_WINDOW_SECS) > Utc::now())
        && row.expires_at > Utc::now();
    let approver_active = match row.claimed_by_device {
        Some(dev) => {
            crate::auth::strong::is_active(conn, AuthenticatorRef::key_device(dev)).await?
        }
        None => false,
    };
    let generation_ok = match row.first_factor_ref() {
        Some(f) => factors::states(conn, user).await?.password.generation == f.generation,
        None => true,
    };
    if !(in_window && approver_active && generation_ok) {
        expire_row(conn, id).await?;
        return Err(AuthError::LoginAttemptEnded {
            state: "expired".into(),
        });
    }
    let class = ClientClass::parse(&row.client_class).unwrap_or(ClientClass::Web);
    let collected = mint(
        conn,
        ctx,
        &row,
        user,
        row.kind().phone_method(),
        class,
        client,
    )
    .await?;
    Ok(Locked::Done(collected))
}

/// Mint the session of a consumed attempt, record `sign_in`, end a password
/// throttle for a QR sign-in (B.1 exit: another method).
pub(crate) async fn mint(
    conn: &mut PgConnection,
    ctx: &AuthCtx<'_>,
    row: &AttemptRow,
    user: Uuid,
    method: SignInMethod,
    class: ClientClass,
    client: &SessionClient,
) -> AuthResult<Collected> {
    let issued = issue_session_tx(
        conn,
        ctx,
        &NewSession {
            user_id: user,
            method,
            client,
            client_class: class,
            authenticator: None,
        },
    )
    .await?;
    sqlx::query(
        "UPDATE login_attempts
            SET state = 'consumed', consumed_at = NOW(), ended_at = NOW(), session_id = $2
          WHERE id = $1",
    )
    .bind(row.id)
    .bind(issued.session_id)
    .execute(&mut *conn)
    .await?;
    if method == SignInMethod::Qr {
        factors::clear_throttle_tx(conn, user, Factor::Password, ExitPath::OtherMethodSignIn)
            .await?;
    }
    let mut ev = NewSecurityEvent::new(
        user,
        EventType::SignIn,
        Actor::User(user),
        serde_json::json!({
            "auth_methods": method.auth_methods(),
            "assurance": method.assurance().as_str(),
        }),
    );
    ev.client = Some((class, row.client_label.clone()));
    ev.ip = client.ip;
    ev.session_id = Some(issued.session_id);
    let mut event_ids = issued.event_ids.clone();
    event_ids.push(record(conn, ev).await?);
    let password_change_required: bool =
        sqlx::query_scalar("SELECT must_change_password FROM users WHERE id = $1")
            .bind(user)
            .fetch_one(&mut *conn)
            .await?;
    Ok(Collected {
        issued,
        password_change_required,
        event_ids,
    })
}

// ============================================================================
// Device side: claim, answer, deny, release (C26)
// ============================================================================

/// `POST /api/approvals/claim`: the attempt by the token's keyed hash; a
/// `login` challenge (2 min, at most the attempt's deadline) for the
/// claiming key device.
#[tracing::instrument(skip(conn, ctx, auth, token), fields(user_id = %auth.id))]
pub async fn claim_tx(
    conn: &mut PgConnection,
    ctx: &AuthCtx<'_>,
    auth: &AuthUser,
    token: &[u8; 32],
) -> Result<ApprovalChallenge, ApprovalError> {
    crate::approvals::devices::require_key_device_snapshot(auth)?;
    let hash = token_hash(ctx, token);
    let seen: Option<(Uuid, Option<Uuid>)> =
        sqlx::query_as("SELECT id, user_id FROM login_attempts WHERE qr_token_hash = $1")
            .bind(hash.to_vec())
            .fetch_optional(&mut *conn)
            .await?;
    let Some((id, attempt_user)) = seen else {
        return Err(ApprovalError::LoginCodeStale);
    };
    // Accounts (claimer, the attempt's user) → device → session → attempt.
    let mut accounts = vec![auth.id];
    accounts.extend(attempt_user);
    let locked = crate::approvals::transition::lock_accounts(conn, &accounts).await?;
    let dev = key_device_session(conn, auth).await?;
    refresh(conn, id).await?;
    let row = read(conn, id, true)
        .await?
        .ok_or(ApprovalError::LoginCodeStale)?;
    // The token must still be this attempt's current, unused one.
    let current = row
        .qr_token_hash
        .as_deref()
        .is_some_and(|h| ct_eq(h, &hash));
    if row.state == "claimed" && current {
        return Err(ApprovalError::LoginAttemptClaimed);
    }
    let fresh = row
        .qr_token_at
        .is_some_and(|t| t + Duration::seconds(QR_TOKEN_SECS) > Utc::now());
    if row.state != "open" || !current || row.qr_token_used || !fresh {
        return Err(ApprovalError::LoginCodeStale);
    }
    if !locked.get(&auth.id).is_some_and(|a| a.may_approve) {
        return Err(ApprovalError::NotAllowed);
    }
    let kind = row.kind();
    if kind != AttemptKind::Qr && row.user_id != Some(auth.id) {
        return Err(ApprovalError::LoginAttemptOtherAccount);
    }
    if row.user_id.is_some() && row.user_id != attempt_user {
        // The attempt's account changed after the unlocked read.
        return Err(ApprovalError::LoginCodeStale);
    }
    let ttl_end = (Utc::now() + Duration::seconds(CLAIM_TTL_SECS)).min(row.expires_at);
    let username: String = sqlx::query_scalar("SELECT username FROM users WHERE id = $1")
        .bind(auth.id)
        .fetch_one(&mut *conn)
        .await?;
    let collection_hash: [u8; 32] = row
        .collection_digest
        .as_slice()
        .try_into()
        .map_err(|_| ApprovalError::LoginCodeStale)?;
    let context = CanonicalContext::Login(LoginContext {
        attempt_id: row.id,
        collection_hash,
        variant: kind.variant(),
        client_class: ClientClass::parse(&row.client_class).unwrap_or(ClientClass::Web),
        client_label: row.client_label.clone(),
        created_at: row.created_at.timestamp(),
        server_name: crate::approvals::server_name(conn).await?,
        place: row.place.clone(),
        username,
    });
    let challenge = issue(
        conn,
        ctx,
        NewChallenge {
            context,
            approver_id: auth.id,
            subject_id: auth.id,
            device_id: dev.device.device_id,
            requester: Requester::LoginAttempt(row.id),
            display: serde_json::json!({}),
            transfer_hash: Some(hash),
            ttl: ttl_end - Utc::now(),
        },
    )
    .await
    .map_err(|e| match e {
        ApprovalError::Expired => ApprovalError::LoginCodeStale,
        other => other,
    })?;
    sqlx::query(
        "UPDATE login_attempts
            SET state = 'claimed', qr_token_used = true, user_id = $2, claimed_by_device = $3,
                challenge_id = $4
          WHERE id = $1",
    )
    .bind(row.id)
    .bind(auth.id)
    .bind(dev.device.id)
    .bind(challenge.challenge_id)
    .execute(&mut *conn)
    .await?;
    Ok(challenge)
}

/// The claimed attempt of a `login` challenge, locked (after the challenge,
/// per the device-side lock order). An attempt that has ended or moved on
/// voids the challenge and answers 409 `APPROVAL_STATE`.
async fn attempt_of(
    conn: &mut PgConnection,
    row: &ChallengeRow,
) -> Result<AttemptRow, ApprovalError> {
    let attempt = read(conn, row.requester_id, true).await?;
    match attempt {
        Some(a)
            if a.state == "claimed"
                && a.challenge_id == Some(row.id)
                && a.expires_at > Utc::now() =>
        {
            Ok(a)
        }
        _ => {
            set_state(
                conn,
                row.id,
                &[ChallengeState::Issued],
                ChallengeState::Expired,
            )
            .await?;
            Err(ApprovalError::State)
        }
    }
}

/// Answer a `login` challenge (C26).
///
/// The response is compared with the scanned token first (400
/// `APPROVAL_RESPONSE_MISMATCH`, nothing changes), then the signature;
/// approve re-checks `may_approve` under the lock; report ends the attempt
/// and, for a password attempt, marks that password generation compromised.
/// Returns the answer and events to deliver.
#[tracing::instrument(skip_all, fields(challenge_id = %row.id))]
pub async fn answer_tx(
    conn: &mut PgConnection,
    ctx: &AuthCtx<'_>,
    dev: &KeyDeviceSession,
    row: &ChallengeRow,
    decision: Decision,
    response: &[u8],
    signature: &str,
) -> Result<(AnswerResponse, Vec<Uuid>), ApprovalError> {
    let transfer = row
        .transfer_hash
        .as_deref()
        .ok_or(ApprovalError::NotFound)?;
    if !ct_eq(&token_hash(ctx, response), transfer) {
        return Err(ApprovalError::ResponseMismatch);
    }
    verify(
        row,
        ctx.origin(),
        &dev.device.public_key,
        decision,
        response,
        signature,
    )?;
    let attempt = attempt_of(conn, row).await?;
    let user = dev.auth.id;
    let mut events = Vec::new();
    let state = match decision {
        Decision::Approve => {
            let may: bool = sqlx::query_scalar("SELECT may_approve FROM users WHERE id = $1")
                .bind(user)
                .fetch_one(&mut *conn)
                .await?;
            if !may {
                return Err(ApprovalError::NotAllowed);
            }
            set_state(
                conn,
                row.id,
                &[ChallengeState::Issued],
                ChallengeState::Consumed,
            )
            .await?;
            sqlx::query(
                "UPDATE login_attempts SET state = 'approved', approved_at = NOW() WHERE id = $1",
            )
            .bind(attempt.id)
            .execute(&mut *conn)
            .await?;
            "approved"
        }
        Decision::Report => {
            set_state(
                conn,
                row.id,
                &[ChallengeState::Issued],
                ChallengeState::Reported,
            )
            .await?;
            sqlx::query(
                "UPDATE login_attempts SET state = 'reported', ended_at = NOW() WHERE id = $1",
            )
            .bind(attempt.id)
            .execute(&mut *conn)
            .await?;
            let mut ev = NewSecurityEvent::new(
                user,
                EventType::ApprovalReported,
                Actor::User(user),
                serde_json::json!({ "attempt_kind": attempt.kind }),
            );
            ev.authenticator = Some(AuthenticatorRef::key_device(dev.device.id));
            ev.session_id = dev.auth.session_id;
            events.push(record(conn, ev).await?);
            if let Some(f) = attempt.first_factor_ref() {
                factors::mark_compromised_tx(
                    conn,
                    ctx,
                    user,
                    f,
                    StateCause::Report,
                    Actor::User(user),
                )
                .await?;
            }
            "reported"
        }
        Decision::Deny => return Err(ApprovalError::Validation("decision".into())),
    };
    sqlx::query("UPDATE approval_devices SET last_used_at = NOW() WHERE id = $1")
        .bind(dev.device.id)
        .execute(&mut *conn)
        .await?;
    Ok((
        AnswerResponse {
            state: state.into(),
            grant: None,
            enrollment: None,
        },
        events,
    ))
}

/// The unsigned "No": the challenge and the attempt are `denied`.
pub async fn deny_tx(conn: &mut PgConnection, row: &ChallengeRow) -> Result<(), ApprovalError> {
    let attempt = attempt_of(conn, row).await?;
    set_state(
        conn,
        row.id,
        &[ChallengeState::Issued],
        ChallengeState::Denied,
    )
    .await?;
    sqlx::query("UPDATE login_attempts SET state = 'denied', ended_at = NOW() WHERE id = $1")
        .bind(attempt.id)
        .execute(&mut *conn)
        .await?;
    Ok(())
}

/// Back / dismissed: the challenge is void and the attempt `open` again (a
/// QR attempt forgets its claimer).
pub async fn release_tx(conn: &mut PgConnection, row: &ChallengeRow) -> Result<(), ApprovalError> {
    let attempt = attempt_of(conn, row).await?;
    set_state(
        conn,
        row.id,
        &[ChallengeState::Issued],
        ChallengeState::Expired,
    )
    .await?;
    sqlx::query(
        "UPDATE login_attempts
            SET state = 'open', claimed_by_device = NULL, challenge_id = NULL,
                user_id = CASE WHEN kind = 'qr' THEN NULL ELSE user_id END
          WHERE id = $1",
    )
    .bind(attempt.id)
    .execute(&mut *conn)
    .await?;
    Ok(())
}

// ============================================================================
// Timers and cascades
// ============================================================================

/// The minute sweep (B.7 rule 3) of the attempts.
///
/// Claims whose challenge is gone return to `open`; attempts past their
/// deadline (or approved and not collected in time) expire. Rows another
/// transaction holds are skipped (next minute). Ended attempts are deleted
/// a day after they ended.
pub async fn expire_due(conn: &mut PgConnection) -> sqlx::Result<u64> {
    let ids: Vec<Uuid> = sqlx::query_scalar(
        "SELECT id FROM login_attempts
          WHERE state IN ('open', 'claimed', 'approved')
          ORDER BY id
          FOR UPDATE SKIP LOCKED",
    )
    .fetch_all(&mut *conn)
    .await?;
    for id in &ids {
        refresh(conn, *id).await?;
    }
    let n = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM login_attempts WHERE id = ANY($1) AND state = 'expired'",
    )
    .bind(&ids)
    .fetch_one(&mut *conn)
    .await?;
    sqlx::query(
        "DELETE FROM login_attempts WHERE ended_at IS NOT NULL AND ended_at < NOW() - INTERVAL '1 day'",
    )
    .execute(&mut *conn)
    .await?;
    Ok(u64::try_from(n).unwrap_or(0))
}

/// B.7 rule 2: a first-factor generation ended; every non-terminal attempt
/// that used it expires (its challenge dies at its answer).
pub async fn on_generation_ended(
    conn: &mut PgConnection,
    user_id: Uuid,
    used: FactorRef,
) -> sqlx::Result<u64> {
    if used.factor != Factor::Password {
        return Ok(0);
    }
    Ok(sqlx::query(
        "UPDATE login_attempts SET state = 'expired', ended_at = NOW()
          WHERE user_id = $1 AND first_factor = 'password' AND first_factor_generation = $2
            AND state IN ('open', 'claimed', 'approved')",
    )
    .bind(user_id)
    .bind(used.generation)
    .execute(&mut *conn)
    .await?
    .rows_affected())
}

/// Decode a b64u QR token (32 bytes).
#[must_use]
pub fn decode_token(raw: &str) -> Option<[u8; 32]> {
    b64u_decode(raw).and_then(|b| b.try_into().ok())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::approvals::secrets::b64u_encode;

    #[test]
    fn qr_payload_percent_encodes_the_origin() {
        let t = Secret256::from_b64u(&b64u_encode(&[0xa0; 32])).unwrap();
        let p = qr_payload("https://chat.example.com", &t);
        assert_eq!(
            p,
            format!(
                "kaiku://login/v1?o=https%3A%2F%2Fchat.example.com&t={}",
                b64u_encode(&[0xa0; 32])
            )
        );
    }

    #[test]
    fn client_labels_by_class() {
        let win = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) Kaiku/1.0";
        assert_eq!(
            client_label(ClientClass::Desktop, Some(win)),
            "Kaiku desktop on Windows 10"
        );
        assert_eq!(client_label(ClientClass::Desktop, None), "Kaiku desktop");
        let long = format!("{}\u{202e}\u{7}", "x".repeat(500));
        let l = client_label(ClientClass::Web, Some(&long));
        assert!(l.chars().count() <= 64 && !l.contains('\u{202e}'));
    }
}
