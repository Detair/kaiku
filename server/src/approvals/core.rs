//! The generic challenge core (spec B.2; v1.1 adds its agent kinds here).
//!
//! A challenge is single use and consumed in the transaction of its effect.
//! The server rebuilds every signed input from the stored row; the device
//! recomputes the context hash from the JSON context it displays.

use chrono::{DateTime, Duration, Utc};
use rand::rngs::OsRng;
use rand::RngCore;
use sqlx::PgConnection;
use uuid::Uuid;

use super::context::CanonicalContext;
use super::error::ApprovalError;
use super::secrets::{b64u_decode, b64u_encode};
use super::signing::{
    message, parse_public_key, signing_prefix, verify as verify_sig, Decision, Kind, SigningInput,
};
use super::types::ApprovalChallenge;
use crate::auth::ctx::AuthCtx;

/// The longest challenge lifetime (B.2; also a DB CHECK).
pub const MAX_TTL_MINUTES: i64 = 10;

/// Who asked for a challenge (and consumes its result).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Requester {
    LoginAttempt(Uuid),
    Enrollment(Uuid),
    Device(Uuid),
    Session(Uuid),
}

impl Requester {
    const fn parts(self) -> (&'static str, Uuid) {
        match self {
            Self::LoginAttempt(id) => ("login_attempt", id),
            Self::Enrollment(id) => ("enrollment", id),
            Self::Device(id) => ("device", id),
            Self::Session(id) => ("session", id),
        }
    }
}

/// Challenge states (B.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChallengeState {
    Issued,
    Approved,
    Consumed,
    Denied,
    Reported,
    Expired,
}

impl ChallengeState {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Issued => "issued",
            Self::Approved => "approved",
            Self::Consumed => "consumed",
            Self::Denied => "denied",
            Self::Reported => "reported",
            Self::Expired => "expired",
        }
    }
}

/// A challenge to issue.
#[derive(Debug, Clone)]
pub struct NewChallenge {
    pub context: CanonicalContext,
    /// Whose device signs (for `enroll_pop`, the subject).
    pub approver_id: Uuid,
    pub subject_id: Uuid,
    /// The answering device's per-install id.
    pub device_id: Uuid,
    pub requester: Requester,
    /// What the answering screen shows besides the context (not signed).
    pub display: serde_json::Value,
    /// Keyed hash of the transferred secret (`login`, `enroll`).
    pub transfer_hash: Option<[u8; 32]>,
    pub ttl: Duration,
}

/// A stored challenge.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct ChallengeRow {
    pub id: Uuid,
    pub kind: String,
    pub variant: Option<String>,
    pub approver_id: Uuid,
    pub subject_id: Uuid,
    pub device_id: Uuid,
    pub requester_kind: String,
    pub requester_id: Uuid,
    pub nonce: Vec<u8>,
    pub context_hash: Vec<u8>,
    pub context: serde_json::Value,
    pub display: serde_json::Value,
    pub transfer_hash: Option<Vec<u8>>,
    pub state: String,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub decided_at: Option<DateTime<Utc>>,
    pub consumed_at: Option<DateTime<Utc>>,
}

impl ChallengeRow {
    /// The kind, if it is one v1.0 knows.
    #[must_use]
    pub fn kind(&self) -> Option<Kind> {
        Kind::parse(&self.kind)
    }

    /// M up to and including `context_hash`, rebuilt from the row.
    pub fn prefix(&self, origin: &str) -> Result<Vec<u8>, ApprovalError> {
        let kind = self.kind().ok_or(ApprovalError::NotFound)?;
        let nonce: [u8; 32] = self
            .nonce
            .as_slice()
            .try_into()
            .map_err(|_| ApprovalError::NotFound)?;
        let context_hash: [u8; 32] = self
            .context_hash
            .as_slice()
            .try_into()
            .map_err(|_| ApprovalError::NotFound)?;
        Ok(signing_prefix(&SigningInput {
            kind,
            challenge_id: self.id,
            nonce,
            expires_at: self.expires_at.timestamp(),
            origin,
            user_id: self.approver_id,
            device_id: self.device_id,
            context_hash,
        }))
    }

    /// The wire form.
    pub fn to_wire(&self, origin: &str) -> Result<ApprovalChallenge, ApprovalError> {
        Ok(ApprovalChallenge {
            challenge_id: self.id,
            kind: self.kind.clone(),
            variant: self.variant.clone(),
            expires_at: self.expires_at,
            signing_prefix: b64u_encode(&self.prefix(origin)?),
            context: self.context.clone(),
        })
    }
}

fn variant_of(ctx: &CanonicalContext) -> Option<String> {
    match ctx {
        CanonicalContext::Login(c) => Some(c.variant.as_str().to_string()),
        CanonicalContext::Enroll(c) => Some(c.variant.as_str().to_string()),
        _ => None,
    }
}

/// Issue a challenge: a random v4 id (not a secret), a 32-byte CSPRNG
/// nonce, `expires_at` = now + `ttl` (whole seconds, as signed).
#[tracing::instrument(skip(conn, ctx, new), fields(kind = new.context.kind().as_str()))]
pub async fn issue(
    conn: &mut PgConnection,
    ctx: &AuthCtx<'_>,
    new: NewChallenge,
) -> Result<ApprovalChallenge, ApprovalError> {
    if new.ttl > Duration::minutes(MAX_TTL_MINUTES) || new.ttl <= Duration::zero() {
        return Err(ApprovalError::Validation(
            "challenge lifetime out of range".into(),
        ));
    }
    let mut nonce = [0u8; 32];
    OsRng.fill_bytes(&mut nonce);
    let now = Utc::now();
    // Whole seconds: the signed `expires_at` is an i64 of Unix seconds.
    let expires_at =
        DateTime::from_timestamp((now + new.ttl).timestamp(), 0).unwrap_or(now + new.ttl);
    let (requester_kind, requester_id) = new.requester.parts();
    let row: ChallengeRow = sqlx::query_as(
        "INSERT INTO approvals (id, kind, variant, approver_id, subject_id, device_id,
                                requester_kind, requester_id, nonce, context_hash, context,
                                display, transfer_hash, state, created_at, expires_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, 'issued', $14, $15)
         RETURNING *",
    )
    .bind(Uuid::new_v4())
    .bind(new.context.kind().as_str())
    .bind(variant_of(&new.context))
    .bind(new.approver_id)
    .bind(new.subject_id)
    .bind(new.device_id)
    .bind(requester_kind)
    .bind(requester_id)
    .bind(nonce.to_vec())
    .bind(new.context.hash().to_vec())
    .bind(new.context.to_json())
    .bind(&new.display)
    .bind(new.transfer_hash.map(|h| h.to_vec()))
    .bind(now)
    .bind(expires_at)
    .fetch_one(&mut *conn)
    .await?;
    row.to_wire(ctx.origin())
}

/// Lock a challenge for answering by `device_id`.
///
/// Unknown or another device's → 404 `APPROVAL_NOT_FOUND`; past
/// `expires_at` → marked expired, 410 `APPROVAL_EXPIRED` (the caller commits
/// that mark); not `issued` → 409 `APPROVAL_STATE`.
pub async fn lock_for_answer(
    conn: &mut PgConnection,
    id: Uuid,
    device_id: Uuid,
) -> Result<ChallengeRow, ApprovalError> {
    let row: ChallengeRow = sqlx::query_as("SELECT * FROM approvals WHERE id = $1 FOR UPDATE")
        .bind(id)
        .fetch_optional(&mut *conn)
        .await?
        .filter(|r: &ChallengeRow| r.device_id == device_id)
        .ok_or(ApprovalError::NotFound)?;
    if row.state == "expired" {
        return Err(ApprovalError::Expired);
    }
    if row.state == "issued" && row.expires_at <= Utc::now() {
        sqlx::query("UPDATE approvals SET state = 'expired' WHERE id = $1")
            .bind(id)
            .execute(&mut *conn)
            .await?;
        return Err(ApprovalError::Expired);
    }
    if row.state != "issued" {
        return Err(ApprovalError::State);
    }
    Ok(row)
}

/// Verify `signature_b64u` over the row's M with `decision` and `response`.
pub fn verify(
    row: &ChallengeRow,
    origin: &str,
    public_key: &[u8],
    decision: Decision,
    response: &[u8],
    signature_b64u: &str,
) -> Result<(), ApprovalError> {
    let key = parse_public_key(public_key).map_err(|_| ApprovalError::SignatureInvalid)?;
    let sig = b64u_decode(signature_b64u).ok_or(ApprovalError::SignatureInvalid)?;
    let m = message(&row.prefix(origin)?, decision, response);
    verify_sig(&key, &m, &sig).map_err(|_| ApprovalError::SignatureInvalid)
}

/// Move a challenge from one of `from` to `to` (decided/consumed stamps).
pub async fn set_state(
    conn: &mut PgConnection,
    id: Uuid,
    from: &[ChallengeState],
    to: ChallengeState,
) -> Result<(), ApprovalError> {
    let from: Vec<&str> = from.iter().map(|s| s.as_str()).collect();
    let n = sqlx::query(
        "UPDATE approvals
            SET state = $3,
                decided_at = CASE WHEN $3 IN ('approved', 'denied', 'reported', 'consumed')
                                  THEN COALESCE(decided_at, NOW()) ELSE decided_at END,
                consumed_at = CASE WHEN $3 = 'consumed' THEN NOW() ELSE consumed_at END
          WHERE id = $1 AND state = ANY($2)",
    )
    .bind(id)
    .bind(&from)
    .bind(to.as_str())
    .execute(&mut *conn)
    .await?
    .rows_affected();
    if n == 1 {
        Ok(())
    } else {
        Err(ApprovalError::State)
    }
}

/// Expire every open challenge past its deadline (timers step).
pub async fn expire_due(conn: &mut PgConnection) -> sqlx::Result<u64> {
    Ok(sqlx::query(
        "UPDATE approvals SET state = 'expired'
          WHERE state IN ('issued', 'approved') AND expires_at <= NOW()",
    )
    .execute(&mut *conn)
    .await?
    .rows_affected())
}

/// Void the open challenges answered by a device (B.7 rule 2).
pub async fn void_for_device(
    conn: &mut PgConnection,
    user_id: Uuid,
    device_id: Uuid,
) -> sqlx::Result<u64> {
    Ok(sqlx::query(
        "UPDATE approvals SET state = 'expired'
          WHERE approver_id = $1 AND device_id = $2 AND state IN ('issued', 'approved')",
    )
    .bind(user_id)
    .bind(device_id)
    .execute(&mut *conn)
    .await?
    .rows_affected())
}
