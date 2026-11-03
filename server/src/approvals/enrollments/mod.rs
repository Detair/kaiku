//! Enrolment of one key device (spec SM2, F5; contract C26 "Enrolment
//! core").
//!
//! `issued` → (open, `enroll_pop` challenge: no change) → `claimed` (the new
//! device proved its key and passed the attestation minimum; a 6-digit code
//! goes to it only) → `done` (the issuer typed the right code: on a key
//! device as a signed `enroll` approval, the signature checked first) or
//! `cancelled` / `expired` / `failed` (third wrong code). At `done` the
//! `approval_devices` row is created in the same transaction.

pub mod handlers;

use chrono::{DateTime, Duration, Utc};
use rand::rngs::OsRng;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::PgConnection;
use uuid::Uuid;

use super::context::{
    AuthenticatorType, CanonicalContext, ClaimantCheck, EnrollContext, EnrollPopContext,
    EnrollVariant,
};
use super::core::{
    issue, lock_for_answer, set_state, verify, ChallengeRow, ChallengeState, NewChallenge,
    Requester,
};
use super::devices::attestation::{check as check_attestation, clean_label};
use super::devices::{DeviceState, KeyDeviceSession, UvClass};
use super::error::ApprovalError;
use super::secrets::{
    b64u_decode, b64u_encode, ct_eq, keyed_hash, random_digits, Secret256, SecretPurpose,
};
use super::signing::{parse_public_key, Decision, Kind};
use super::types::ApprovalChallenge;
use crate::api::AppState;
use crate::auth::ctx::AuthCtx;
use crate::auth::strong::AuthenticatorRef;
use crate::security_events::{record, Actor, EventType, NewSecurityEvent};

/// Setup token lifetime (SM2).
pub const SETUP_TOKEN_MINUTES: i64 = 10;
/// Code lifetime from the claim (SM2).
pub const CODE_MINUTES: i64 = 5;
/// Wrong codes allowed.
pub const MAX_TRIES: i16 = 3;
/// Open enrolments per issuer.
pub const MAX_OPEN_PER_ISSUER: i64 = 5;
/// Long-poll bound of `status` (DC13).
pub const MAX_WAIT_SECS: u8 = 25;
/// Attestation chain bounds checked before decoding (the reader keeps its
/// own `MAX_CHAIN` too): at most 8 certificates of at most 8 KiB DER each.
const MAX_CHAIN_CERTS: usize = 8;
const MAX_CERT_B64U_LEN: usize = 8 * 1024 * 4 / 3 + 4;

/// SM2 variants.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnrollmentVariant {
    Own,
    Admin,
    Request,
    Registration,
    Cli,
}

impl EnrollmentVariant {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Own => "own",
            Self::Admin => "admin",
            Self::Request => "request",
            Self::Registration => "registration",
            Self::Cli => "cli",
        }
    }

    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "own" => Some(Self::Own),
            "admin" => Some(Self::Admin),
            "request" => Some(Self::Request),
            "registration" => Some(Self::Registration),
            "cli" => Some(Self::Cli),
            _ => None,
        }
    }
}

/// SM2 states.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnrollmentState {
    Issued,
    Claimed,
    Done,
    Cancelled,
    Expired,
    Failed,
}

/// Who issued an enrolment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Issuer {
    KeyDevice {
        session_id: Uuid,
        device_id: Uuid,
    },
    PcSession {
        session_id: Uuid,
        passkey_id: Uuid,
        grant_id: Uuid,
    },
    Request {
        request_id: Uuid,
    },
    Ticket {
        ticket_id: Uuid,
    },
    Cli,
}

/// What to issue.
#[derive(Debug, Clone)]
pub struct IssueParams {
    pub variant: EnrollmentVariant,
    pub subject_id: Uuid,
    pub issuer: Issuer,
    pub claimant: ClaimantCheck,
    pub claimant_note: Option<String>,
    pub lost_device_ids: Vec<Uuid>,
}

/// Every `enrollments` column.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct EnrollmentRow {
    pub id: Uuid,
    pub variant: String,
    pub subject_id: Uuid,
    pub issuer_kind: String,
    pub issuer_session_id: Option<Uuid>,
    pub issuer_device_id: Option<Uuid>,
    pub issuer_passkey_id: Option<Uuid>,
    pub issuer_grant_id: Option<Uuid>,
    pub issuer_request_id: Option<Uuid>,
    pub issuer_ticket_id: Option<Uuid>,
    pub setup_token_hash: Vec<u8>,
    pub nonce: Vec<u8>,
    pub code_hash: Option<Vec<u8>>,
    pub code_expires_at: Option<DateTime<Utc>>,
    pub tries: i16,
    pub claim_secret_hash: Option<Vec<u8>>,
    pub authenticator_type: Option<String>,
    pub claimed_device_id: Option<Uuid>,
    pub claimed_public_key: Option<Vec<u8>>,
    pub claimed_key_hash: Option<Vec<u8>>,
    pub claimed_label: Option<String>,
    pub claimed_attestation: Option<Vec<Vec<u8>>>,
    pub claimed_properties: Option<serde_json::Value>,
    pub claimed_uv_class: Option<String>,
    pub claimed_api_level: Option<i32>,
    pub claimed_passkey: Option<serde_json::Value>,
    pub claimant_check: String,
    pub claimant_note: Option<String>,
    pub lost_device_ids: Vec<Uuid>,
    pub request_id: Option<Uuid>,
    pub state: String,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub claimed_at: Option<DateTime<Utc>>,
    pub done_at: Option<DateTime<Utc>>,
    pub ended_at: Option<DateTime<Utc>>,
}

/// `{user_id, username, display_name}`.
#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub struct EnrollmentSubject {
    pub user_id: Uuid,
    pub username: String,
    pub display_name: String,
}

/// `Enrollment.claimed` (C26).
#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub struct ClaimedAuthenticator {
    pub authenticator_type: String,
    /// Shown as "a device calling itself …".
    pub label: String,
    /// b64u SHA-256 of the new key (SPKI DER).
    pub key_hash: String,
}

/// `Enrollment` (C26).
#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub struct Enrollment {
    pub id: Uuid,
    pub variant: String,
    pub state: String,
    pub subject: EnrollmentSubject,
    pub claimed: Option<ClaimedAuthenticator>,
    pub tries_left: i16,
    pub code_expires_at: Option<DateTime<Utc>>,
    pub expires_at: DateTime<Utc>,
    pub request_id: Option<Uuid>,
}

/// `EnrollmentIssued` (C26). The setup token is a secret: only the issuer
/// gets it, once (its `Debug` redacts it and the link).
#[derive(Clone, Serialize, utoipa::ToSchema)]
pub struct EnrollmentIssued {
    pub enrollment_id: Uuid,
    pub setup_token: String,
    /// `{PUBLIC_URL}/setup#v=1&t=<b64u>` (the token in the fragment).
    pub setup_url: String,
    pub expires_at: DateTime<Utc>,
}

impl std::fmt::Debug for EnrollmentIssued {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EnrollmentIssued")
            .field("enrollment_id", &self.enrollment_id)
            .field("expires_at", &self.expires_at)
            .finish_non_exhaustive()
    }
}

impl std::fmt::Debug for ClaimResponse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ClaimResponse")
            .field("enrollment_id", &self.enrollment_id)
            .finish_non_exhaustive()
    }
}

/// `POST /auth/enrollments/open` answer.
#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub struct OpenResponse {
    pub server_name: String,
    /// b64u of the 32-byte enrolment nonce (the attestation challenge).
    pub enrollment_nonce: String,
    pub expires_at: DateTime<Utc>,
}

/// `POST /auth/enrollments/pop-challenge` body.
#[derive(Deserialize, utoipa::ToSchema)]
pub struct PopChallengeRequest {
    pub setup_token: String,
    pub device_id: Uuid,
    /// b64u SPKI DER (uncompressed P-256).
    pub public_key: String,
}

/// `POST /auth/enrollments/pop-challenge` answer.
#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub struct PopChallengeResponse {
    pub challenge: ApprovalChallenge,
    pub subject: EnrollmentSubject,
}

/// `POST /auth/enrollments/claim` body.
#[derive(Deserialize, utoipa::ToSchema)]
pub struct ClaimRequest {
    pub setup_token: String,
    pub challenge_id: Uuid,
    pub signature: String,
    pub public_key: String,
    /// b64u DER certificates, leaf first.
    #[serde(default)]
    pub attestation_chain: Vec<String>,
    pub label: String,
    /// `per_use` or `window_10s`.
    pub uv_class: String,
    #[serde(default)]
    pub api_level: Option<i32>,
}

/// `POST /auth/enrollments/claim` answer: the code goes only here.
#[derive(Clone, Serialize, utoipa::ToSchema)]
pub struct ClaimResponse {
    pub enrollment_id: Uuid,
    pub state: String,
    pub code: String,
    pub code_expires_at: DateTime<Utc>,
    pub claim_secret: String,
}

/// `POST /auth/enrollments/status` body.
#[derive(Deserialize, utoipa::ToSchema)]
pub struct StatusRequest {
    pub enrollment_id: Uuid,
    pub claim_secret: String,
    #[serde(default)]
    pub wait: Option<u8>,
}

/// `POST /auth/enrollments/status` answer.
#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub struct StatusResponse {
    pub state: String,
    /// `active` or `pending` once done.
    pub authenticator_state: Option<String>,
    pub ready_at: Option<DateTime<Utc>>,
}

/// A claim that carries the issuing client's own request secret or ticket
/// (the Android app enrolling itself; L1b Tasks 19 and 22).
pub enum OwnClaim {
    Request(Secret256),
    Ticket(Secret256),
}

/// Who confirms the code.
pub enum Confirmer<'a> {
    /// The issuing key device's signed `enroll` answer (signature checked),
    /// from its key-device session re-checked under the account lock.
    KeyDeviceAnswer(&'a KeyDeviceSession),
    /// A full PC session with its recorded passkey (L1b/LP).
    Pc(&'a crate::auth::AuthUser),
    Request(Uuid),
    Ticket(Uuid),
    Cli,
}

/// Who cancels.
pub enum EnrollmentCanceller {
    Issuer,
    Admin(Uuid),
    Request,
}

/// What to do after a `done` commit.
pub struct AfterDone {
    pub subject_id: Uuid,
    pub issuer_user: Option<Uuid>,
    pub enrollment_id: Uuid,
    pub event_ids: Vec<Uuid>,
}

/// SM2: `own`, `cli`, `registration` and an `admin` enrolment of a new
/// account become active at once; `request` children and `admin`
/// enrolments of existing accounts stay pending.
#[must_use]
pub const fn activation_state(variant: EnrollmentVariant, subject_is_new: bool) -> DeviceState {
    match variant {
        EnrollmentVariant::Own | EnrollmentVariant::Cli | EnrollmentVariant::Registration => {
            DeviceState::Active
        }
        EnrollmentVariant::Admin if subject_is_new => DeviceState::Active,
        EnrollmentVariant::Admin | EnrollmentVariant::Request => DeviceState::Pending,
    }
}

/// `revoke_reason` of a live key that a `done` enrolment replaces on the
/// same install (C26 `KeyDevice.revoke_reason`).
const fn replaced_reason(variant: EnrollmentVariant) -> &'static str {
    match variant {
        EnrollmentVariant::Own | EnrollmentVariant::Registration => "owner",
        EnrollmentVariant::Admin => "admin",
        EnrollmentVariant::Request => "request",
        EnrollmentVariant::Cli => "cli",
    }
}

/// Whether the account has never had any activity (B.0 "new account").
async fn subject_is_new(conn: &mut PgConnection, user_id: Uuid) -> Result<bool, ApprovalError> {
    let first_activity: Option<DateTime<Utc>> =
        sqlx::query_scalar("SELECT first_activity_at FROM users WHERE id = $1")
            .bind(user_id)
            .fetch_one(&mut *conn)
            .await?;
    Ok(first_activity.is_none())
}

/// The live key device that a `done` of `row` would replace on install
/// `device_id` (`approval_devices_live` allows one per account and install),
/// or the refusal (SM4: `active` → `revoked` only by the owner, an admin's
/// "Remove device now", the CLI, a completed account request, …):
///
/// - an enrolment never replaces the key device that issued it (400 `VALIDATION_ERROR`: the issuing
///   app never claims its own link);
/// - a key that will be `pending` (an `admin` enrolment of an existing account, a `request` child)
///   never replaces a live one: that would skip the account request's wait and its cancellers and,
///   on another admin's account, `ADMIN_TARGET_IS_ADMIN`. Its replacement belongs to the request's
///   completion (L1b). 400 `ENROLLMENT_DEVICE_IN_USE`: the phone offers to sign in with that key.
///
/// `lock` takes the row `FOR UPDATE` (confirm, under the account lock).
async fn replaceable_on_install(
    conn: &mut PgConnection,
    row: &EnrollmentRow,
    device_id: Uuid,
    will_be: DeviceState,
    lock: bool,
) -> Result<Option<Uuid>, ApprovalError> {
    let sql = if lock {
        "SELECT id FROM approval_devices
          WHERE user_id = $1 AND device_id = $2 AND state <> 'revoked' FOR UPDATE"
    } else {
        "SELECT id FROM approval_devices
          WHERE user_id = $1 AND device_id = $2 AND state <> 'revoked'"
    };
    let live: Option<Uuid> = sqlx::query_scalar(sql)
        .bind(row.subject_id)
        .bind(device_id)
        .fetch_optional(&mut *conn)
        .await?;
    let Some(live) = live else {
        return Ok(None);
    };
    // C26 amendment: the issuer's own device and a live device a pending
    // enrolment would replace are both "this phone is already a key".
    if row.issuer_device_id == Some(live) || will_be != DeviceState::Active {
        return Err(ApprovalError::EnrollmentDeviceInUse);
    }
    Ok(Some(live))
}

fn code_hash(key: &[u8; 32], enrollment_id: Uuid, code: &[u8]) -> [u8; 32] {
    let mut input = enrollment_id.as_bytes().to_vec();
    input.extend_from_slice(code);
    keyed_hash(key, SecretPurpose::EnrollCode, &input)
}

async fn subject(
    conn: &mut PgConnection,
    user_id: Uuid,
) -> Result<EnrollmentSubject, ApprovalError> {
    let row: Option<(String, String)> =
        sqlx::query_as("SELECT username, display_name FROM users WHERE id = $1 AND NOT is_system")
            .bind(user_id)
            .fetch_optional(&mut *conn)
            .await?;
    let (username, display_name) = row.ok_or(ApprovalError::EnrollmentTokenInvalid)?;
    Ok(EnrollmentSubject {
        user_id,
        username,
        display_name,
    })
}

/// The C26 wire form.
pub async fn to_wire(
    conn: &mut PgConnection,
    row: &EnrollmentRow,
) -> Result<Enrollment, ApprovalError> {
    let claimed = match (&row.claimed_label, &row.claimed_key_hash) {
        (Some(label), Some(hash)) => Some(ClaimedAuthenticator {
            authenticator_type: row
                .authenticator_type
                .clone()
                .unwrap_or_else(|| "key_device".into()),
            label: label.clone(),
            key_hash: b64u_encode(hash),
        }),
        _ => None,
    };
    Ok(Enrollment {
        id: row.id,
        variant: row.variant.clone(),
        state: row.state.clone(),
        subject: subject(conn, row.subject_id).await?,
        claimed,
        tries_left: (MAX_TRIES - row.tries).max(0),
        code_expires_at: row.code_expires_at,
        expires_at: row.expires_at,
        request_id: row.request_id,
    })
}

/// Issue an enrolment (SM2 "issue"): ≤ 5 open per issuer, a 256-bit setup
/// token valid 10 minutes, a 32-byte nonce.
#[tracing::instrument(skip(conn, ctx, p), fields(variant = p.variant.as_str(), subject_id = %p.subject_id))]
pub async fn issue_tx(
    conn: &mut PgConnection,
    ctx: &AuthCtx<'_>,
    p: IssueParams,
) -> Result<EnrollmentIssued, ApprovalError> {
    let locked = super::transition::lock_accounts(conn, &[p.subject_id]).await?;
    let acct = locked
        .get(&p.subject_id)
        .ok_or(ApprovalError::EnrollmentTokenInvalid)?;
    if !acct.exists || acct.is_system {
        return Err(ApprovalError::Validation("unknown account".into()));
    }
    if acct.banned {
        return Err(ApprovalError::AccountBanned);
    }
    let (kind, session, device, passkey, grant, request, ticket) = match p.issuer {
        Issuer::KeyDevice {
            session_id,
            device_id,
        } => (
            "key_device",
            Some(session_id),
            Some(device_id),
            None,
            None,
            None,
            None,
        ),
        Issuer::PcSession {
            session_id,
            passkey_id,
            grant_id,
        } => (
            "session",
            Some(session_id),
            None,
            Some(passkey_id),
            Some(grant_id),
            None,
            None,
        ),
        Issuer::Request { request_id } => {
            ("request", None, None, None, None, Some(request_id), None)
        }
        Issuer::Ticket { ticket_id } => ("ticket", None, None, None, None, None, Some(ticket_id)),
        Issuer::Cli => ("cli", None, None, None, None, None, None),
    };
    let open: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM enrollments
          WHERE issuer_kind = $1
            AND ((state = 'issued' AND expires_at > NOW())
                 OR (state = 'claimed' AND code_expires_at > NOW()))
            AND issuer_session_id IS NOT DISTINCT FROM $2
            AND issuer_device_id IS NOT DISTINCT FROM $3
            AND issuer_request_id IS NOT DISTINCT FROM $4
            AND issuer_ticket_id IS NOT DISTINCT FROM $5
            AND (issuer_kind <> 'cli' OR subject_id = $6)",
    )
    .bind(kind)
    .bind(session)
    .bind(device)
    .bind(request)
    .bind(ticket)
    .bind(p.subject_id)
    .fetch_one(&mut *conn)
    .await?;
    if open >= MAX_OPEN_PER_ISSUER {
        return Err(ApprovalError::EnrollmentLimit);
    }
    if let Some(request_id) = request {
        // A request has at most one live child: a new one voids the old.
        sqlx::query(
            "UPDATE enrollments SET state = 'cancelled', ended_at = NOW()
              WHERE issuer_request_id = $1 AND state IN ('issued', 'claimed')",
        )
        .bind(request_id)
        .execute(&mut *conn)
        .await?;
    }
    let token = Secret256::random();
    let mut nonce = [0u8; 32];
    OsRng.fill_bytes(&mut nonce);
    let id = Uuid::new_v4();
    let now = Utc::now();
    let expires_at = now + Duration::minutes(SETUP_TOKEN_MINUTES);
    sqlx::query(
        "INSERT INTO enrollments (id, variant, subject_id, issuer_kind, issuer_session_id,
             issuer_device_id, issuer_passkey_id, issuer_grant_id, issuer_request_id,
             issuer_ticket_id, setup_token_hash, nonce, claimant_check, claimant_note,
             lost_device_ids, request_id, state, created_at, expires_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $9, 'issued', $16, $17)",
    )
    .bind(id)
    .bind(p.variant.as_str())
    .bind(p.subject_id)
    .bind(kind)
    .bind(session)
    .bind(device)
    .bind(passkey)
    .bind(grant)
    .bind(request)
    .bind(ticket)
    .bind(keyed_hash(ctx.hmac_key(), SecretPurpose::SetupToken, token.as_bytes()).to_vec())
    .bind(nonce.to_vec())
    .bind(p.claimant.as_str())
    .bind(p.claimant_note)
    .bind(&p.lost_device_ids)
    .bind(now)
    .bind(expires_at)
    .execute(&mut *conn)
    .await?;
    let encoded = token.to_b64u();
    Ok(EnrollmentIssued {
        enrollment_id: id,
        setup_url: format!("{}/setup#v=1&t={encoded}", ctx.origin()),
        setup_token: encoded,
        expires_at,
    })
}

async fn by_token(
    conn: &mut PgConnection,
    ctx: &AuthCtx<'_>,
    setup_token: &str,
    lock: bool,
) -> Result<EnrollmentRow, ApprovalError> {
    let token = Secret256::from_b64u(setup_token).ok_or(ApprovalError::EnrollmentTokenInvalid)?;
    let hash = keyed_hash(ctx.hmac_key(), SecretPurpose::SetupToken, token.as_bytes());
    let sql = if lock {
        "SELECT * FROM enrollments WHERE setup_token_hash = $1 FOR UPDATE"
    } else {
        "SELECT * FROM enrollments WHERE setup_token_hash = $1"
    };
    let row: EnrollmentRow = sqlx::query_as(sql)
        .bind(hash.to_vec())
        .fetch_optional(&mut *conn)
        .await?
        .ok_or(ApprovalError::EnrollmentTokenInvalid)?;
    match row.state.as_str() {
        "issued" if row.expires_at > Utc::now() => Ok(row),
        "claimed" | "done" => Err(ApprovalError::EnrollmentAlreadyClaimed),
        _ => Err(ApprovalError::EnrollmentTokenInvalid),
    }
}

/// SM2 "open": the server name and the enrolment nonce; consumes nothing,
/// takes no lock.
pub async fn open(ctx: &AuthCtx<'_>, setup_token: &str) -> Result<OpenResponse, ApprovalError> {
    let mut conn = ctx.db.acquire().await?;
    let row = by_token(&mut conn, ctx, setup_token, false).await?;
    Ok(OpenResponse {
        server_name: crate::approvals::server_name(&mut conn).await?,
        enrollment_nonce: b64u_encode(&row.nonce),
        expires_at: row.expires_at,
    })
}

/// SM2 `enroll_pop` challenge: one live challenge per enrolment (a new one
/// voids the open one); consumes nothing.
pub async fn pop_challenge(
    ctx: &AuthCtx<'_>,
    req: PopChallengeRequest,
) -> Result<PopChallengeResponse, ApprovalError> {
    let der = b64u_decode(&req.public_key)
        .ok_or_else(|| ApprovalError::Validation("public_key".into()))?;
    parse_public_key(&der).map_err(|_| {
        ApprovalError::Validation("public_key must be an uncompressed P-256 key".into())
    })?;
    let mut tx = ctx.db.begin().await?;
    let row = by_token(&mut tx, ctx, &req.setup_token, true).await?;
    let subject = subject(&mut tx, row.subject_id).await?;
    // Refused before any code exists; `confirm_tx` checks again under the lock.
    let variant = EnrollmentVariant::parse(&row.variant).ok_or(ApprovalError::EnrollmentState)?;
    let will_be = activation_state(variant, subject_is_new(&mut tx, row.subject_id).await?);
    replaceable_on_install(&mut tx, &row, req.device_id, will_be, false).await?;
    sqlx::query(
        "UPDATE approvals SET state = 'expired'
          WHERE requester_kind = 'enrollment' AND requester_id = $1 AND kind = 'enroll_pop'
            AND state = 'issued'",
    )
    .bind(row.id)
    .execute(&mut *tx)
    .await?;
    let nonce: [u8; 32] = row
        .nonce
        .as_slice()
        .try_into()
        .map_err(|_| ApprovalError::EnrollmentState)?;
    let ttl = (row.expires_at - Utc::now()).min(Duration::minutes(super::core::MAX_TTL_MINUTES));
    let challenge = issue(
        &mut tx,
        ctx,
        NewChallenge {
            context: CanonicalContext::EnrollPop(EnrollPopContext {
                enrollment_id: row.id,
                new_key_hash: Sha256::digest(&der).into(),
                attestation_challenge: nonce,
                subject_username: subject.username.clone(),
            }),
            approver_id: row.subject_id,
            subject_id: row.subject_id,
            device_id: req.device_id,
            requester: Requester::Enrollment(row.id),
            display: serde_json::json!({}),
            transfer_hash: None,
            ttl,
        },
    )
    .await?;
    tx.commit().await?;
    Ok(PopChallengeResponse { challenge, subject })
}

/// SM2 "claim": the `enroll_pop` signature over the challenge, the
/// attestation minimum, the cleaned label; the code and the claim secret
/// go only to this response.
#[tracing::instrument(skip_all, fields(challenge_id = %req.challenge_id))]
pub async fn claim(
    ctx: &AuthCtx<'_>,
    req: ClaimRequest,
    own: Option<OwnClaim>,
) -> Result<ClaimResponse, ApprovalError> {
    if own.is_some() {
        // L1b Tasks 19 and 22: the issuing client's own claim.
        return Err(ApprovalError::Validation("not supported".into()));
    }
    let uv_class = UvClass::parse(&req.uv_class)
        .ok_or_else(|| ApprovalError::Validation("uv_class".into()))?;
    let der = b64u_decode(&req.public_key)
        .ok_or_else(|| ApprovalError::Validation("public_key".into()))?;
    // Bounded before anything is decoded (a public route).
    if req.attestation_chain.len() > MAX_CHAIN_CERTS
        || req
            .attestation_chain
            .iter()
            .any(|c| c.len() > MAX_CERT_B64U_LEN)
    {
        return Err(ApprovalError::Validation("attestation_chain".into()));
    }
    let mut chain = Vec::with_capacity(req.attestation_chain.len());
    for c in &req.attestation_chain {
        chain.push(
            b64u_decode(c).ok_or_else(|| ApprovalError::Validation("attestation_chain".into()))?,
        );
    }
    let mut tx = ctx.db.begin().await?;
    let row = by_token(&mut tx, ctx, &req.setup_token, true).await?;
    // The pop challenge of this enrolment, answered by the claiming install.
    let challenge: Option<ChallengeRow> = sqlx::query_as("SELECT * FROM approvals WHERE id = $1")
        .bind(req.challenge_id)
        .fetch_optional(&mut *tx)
        .await?;
    let challenge = challenge
        .filter(|c| {
            c.kind == "enroll_pop" && c.requester_kind == "enrollment" && c.requester_id == row.id
        })
        .ok_or(ApprovalError::NotFound)?;
    let challenge =
        match lock_for_answer(&mut tx, challenge.id, row.subject_id, challenge.device_id).await {
            Ok(c) => c,
            Err(ApprovalError::Expired) => {
                tx.commit().await?;
                return Err(ApprovalError::Expired);
            }
            Err(e) => return Err(e),
        };
    let new_key_hash: [u8; 32] = Sha256::digest(&der).into();
    let ctx_json = CanonicalContext::from_json(Kind::EnrollPop, &challenge.context)
        .map_err(|_| ApprovalError::NotFound)?;
    let CanonicalContext::EnrollPop(pop) = ctx_json else {
        return Err(ApprovalError::NotFound);
    };
    if !ct_eq(&pop.new_key_hash, &new_key_hash) {
        return Err(ApprovalError::SignatureInvalid);
    }
    verify(
        &challenge,
        ctx.origin(),
        &der,
        Decision::Approve,
        b"",
        &req.signature,
    )?;
    let nonce: [u8; 32] = row
        .nonce
        .as_slice()
        .try_into()
        .map_err(|_| ApprovalError::EnrollmentState)?;
    let props = check_attestation(&chain, &nonce, uv_class, &der)
        .map_err(|u| ApprovalError::DeviceUnsuitable { reason: u.reason() })?;
    set_state(
        &mut tx,
        challenge.id,
        &[ChallengeState::Issued],
        ChallengeState::Consumed,
    )
    .await?;
    let label = clean_label(&req.label);
    let code = random_digits(6);
    let claim_secret = Secret256::random();
    let now = Utc::now();
    let code_expires_at = now + Duration::minutes(CODE_MINUTES);
    sqlx::query(
        "UPDATE enrollments
            SET state = 'claimed', claimed_at = $2, code_hash = $3, code_expires_at = $4,
                claim_secret_hash = $5, authenticator_type = 'key_device', claimed_device_id = $6,
                claimed_public_key = $7, claimed_key_hash = $8, claimed_label = $9,
                claimed_attestation = $10, claimed_properties = $11, claimed_uv_class = $12,
                claimed_api_level = $13
          WHERE id = $1",
    )
    .bind(row.id)
    .bind(now)
    .bind(code_hash(ctx.hmac_key(), row.id, code.as_bytes()).to_vec())
    .bind(code_expires_at)
    .bind(
        keyed_hash(
            ctx.hmac_key(),
            SecretPurpose::ClaimSecret,
            claim_secret.as_bytes(),
        )
        .to_vec(),
    )
    .bind(challenge.device_id)
    .bind(&der)
    .bind(new_key_hash.to_vec())
    .bind(&label)
    .bind(&chain)
    .bind(serde_json::to_value(&props).unwrap_or_default())
    .bind(uv_class.as_str())
    .bind(req.api_level)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(ClaimResponse {
        enrollment_id: row.id,
        state: "claimed".into(),
        code,
        code_expires_at,
        claim_secret: claim_secret.to_b64u(),
    })
}

async fn lock_by_id(conn: &mut PgConnection, id: Uuid) -> Result<EnrollmentRow, ApprovalError> {
    sqlx::query_as("SELECT * FROM enrollments WHERE id = $1 FOR UPDATE")
        .bind(id)
        .fetch_optional(&mut *conn)
        .await?
        .ok_or(ApprovalError::EnrollmentTokenInvalid)
}

/// Lock an enrolment that `dev` issued (the issuing key device is the only
/// one that confirms or denies it); another issuer's → 404.
pub async fn lock_issued_by(
    conn: &mut PgConnection,
    id: Uuid,
    dev: &KeyDeviceSession,
) -> Result<EnrollmentRow, ApprovalError> {
    let row = lock_by_id(conn, id).await?;
    if row.issuer_kind != "key_device" || row.issuer_device_id != Some(dev.device.id) {
        return Err(ApprovalError::NotFound);
    }
    Ok(row)
}

/// SM2 status for the claiming device (long-poll ≤ 25 s, re-read every 500 ms, DC13).
///
/// Unknown id or wrong claim secret → 404 `ENROLLMENT_TOKEN_INVALID`; a known enrolment
/// answers 200 with its state, also `cancelled`, `expired` or `failed`.
pub async fn status(
    ctx: &AuthCtx<'_>,
    enrollment_id: Uuid,
    claim_secret: &Secret256,
    wait: u8,
) -> Result<StatusResponse, ApprovalError> {
    let wait = std::time::Duration::from_secs(u64::from(wait.min(MAX_WAIT_SECS)));
    let started = tokio::time::Instant::now();
    let want = keyed_hash(
        ctx.hmac_key(),
        SecretPurpose::ClaimSecret,
        claim_secret.as_bytes(),
    );
    let mut first_state: Option<String> = None;
    loop {
        // A code past due reads `expired` before the sweep marks it.
        let row: Option<(String, Option<Vec<u8>>, Option<String>)> = sqlx::query_as(
            "SELECT CASE WHEN e.state = 'claimed' AND e.code_expires_at <= NOW()
                         THEN 'expired' ELSE e.state END,
                    e.claim_secret_hash,
                    (SELECT d.state FROM approval_devices d WHERE d.enrollment_id = e.id LIMIT 1)
               FROM enrollments e WHERE e.id = $1",
        )
        .bind(enrollment_id)
        .fetch_optional(ctx.db)
        .await?;
        let Some((state, hash, device_state)) = row else {
            return Err(ApprovalError::EnrollmentTokenInvalid);
        };
        if !hash.is_some_and(|h| ct_eq(&h, &want)) {
            return Err(ApprovalError::EnrollmentTokenInvalid);
        }
        let changed = first_state.as_ref().is_some_and(|s| *s != state);
        if changed || state != "claimed" || started.elapsed() >= wait {
            return Ok(StatusResponse {
                state,
                authenticator_state: device_state,
                ready_at: None,
            });
        }
        first_state.get_or_insert(state);
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }
}

/// The issuing key device asks for the `enroll` approval of the enrolment
/// open on its screen; a new one voids the open one. TTL: until the code
/// expires.
pub async fn issuer_challenge_tx(
    conn: &mut PgConnection,
    ctx: &AuthCtx<'_>,
    dev: &KeyDeviceSession,
    id: Uuid,
) -> Result<ApprovalChallenge, ApprovalError> {
    let row = lock_issued_by(conn, id, dev).await?;
    if row.state != "claimed" {
        return Err(ApprovalError::EnrollmentState);
    }
    let code_expires_at = row.code_expires_at.ok_or(ApprovalError::EnrollmentState)?;
    if code_expires_at <= Utc::now() {
        expire(conn, row.id).await?;
        return Err(ApprovalError::EnrollmentState);
    }
    let variant = match EnrollmentVariant::parse(&row.variant) {
        Some(EnrollmentVariant::Own) => EnrollVariant::Own,
        Some(EnrollmentVariant::Admin) => EnrollVariant::Admin,
        _ => return Err(ApprovalError::NotFound),
    };
    let claimant = ClaimantCheck::parse(&row.claimant_check).unwrap_or(ClaimantCheck::None);
    let new_key_hash: [u8; 32] = row
        .claimed_key_hash
        .as_deref()
        .and_then(|h| h.try_into().ok())
        .ok_or(ApprovalError::EnrollmentState)?;
    let subject = subject(conn, row.subject_id).await?;
    sqlx::query(
        "UPDATE approvals SET state = 'expired'
          WHERE requester_kind = 'enrollment' AND requester_id = $1 AND kind = 'enroll'
            AND state = 'issued'",
    )
    .bind(row.id)
    .execute(&mut *conn)
    .await?;
    let ttl = (code_expires_at - Utc::now()).min(Duration::minutes(super::core::MAX_TTL_MINUTES));
    issue(
        conn,
        ctx,
        NewChallenge {
            context: CanonicalContext::Enroll(EnrollContext {
                enrollment_id: row.id,
                subject_id: row.subject_id,
                subject_username: subject.username,
                variant,
                authenticator_type: AuthenticatorType::KeyDevice,
                new_key_hash,
                label: row.claimed_label.clone().unwrap_or_default(),
                claimant_check: claimant,
            }),
            approver_id: dev.auth.id,
            subject_id: row.subject_id,
            device_id: dev.device.device_id,
            requester: Requester::Enrollment(row.id),
            display: serde_json::json!({}),
            transfer_hash: row.code_hash.as_deref().and_then(|h| h.try_into().ok()),
            ttl,
        },
    )
    .await
}

async fn expire(conn: &mut PgConnection, id: Uuid) -> sqlx::Result<()> {
    sqlx::query(
        "UPDATE enrollments SET state = 'expired', ended_at = NOW()
          WHERE id = $1 AND state IN ('issued', 'claimed')",
    )
    .bind(id)
    .execute(&mut *conn)
    .await?;
    Ok(())
}

/// `authenticator_removed.details.by` for a key replaced by an enrolment (C28).
const fn removed_by(actor: Actor) -> &'static str {
    match actor {
        Actor::Cli => "cli",
        Actor::Admin(_) => "admin",
        Actor::System => "request",
        _ => "self",
    }
}

/// Confirm with `code` (SM2 "confirm").
///
/// B.7 rule 1: the subject's account (and the answering device's) is locked first, in one
/// statement, then the enrolment row. The issuer is re-checked (B.7 rule 4; a key-device answer
/// must come from the enrolment's issuing device), then the code in constant time. A wrong code
/// counts a try (400 `ENROLL_CODE_WRONG {tries_left}`, the third → 410 `ENROLLMENT_FAILED`) —
/// the caller commits that. At `done` the device row is created in this transaction.
pub async fn confirm_tx(
    conn: &mut PgConnection,
    ctx: &AuthCtx<'_>,
    id: Uuid,
    by: Confirmer<'_>,
    code: &[u8],
) -> Result<(Enrollment, AfterDone), ApprovalError> {
    // Read without a lock to learn whose account to lock first.
    let subject_id: Uuid = sqlx::query_scalar("SELECT subject_id FROM enrollments WHERE id = $1")
        .bind(id)
        .fetch_optional(&mut *conn)
        .await?
        .ok_or(ApprovalError::EnrollmentTokenInvalid)?;
    let mut accounts = vec![subject_id];
    if let Confirmer::KeyDeviceAnswer(dev) = &by {
        accounts.push(dev.auth.id);
    }
    let locked = super::transition::lock_accounts(conn, &accounts).await?;
    let row = lock_by_id(conn, id).await?;
    match row.state.as_str() {
        "claimed" => {}
        _ => return Err(ApprovalError::EnrollmentState),
    }
    if row.code_expires_at.is_none_or(|t| t <= Utc::now()) {
        expire(conn, row.id).await?;
        return Err(ApprovalError::EnrollmentState);
    }
    let issuer_user = match &by {
        Confirmer::KeyDeviceAnswer(dev) => {
            // The answering device is the issuer, active under the lock.
            if row.issuer_kind != "key_device" || row.issuer_device_id != Some(dev.device.id) {
                return Err(ApprovalError::NotFound);
            }
            if !dev.device.is_active() {
                return Err(ApprovalError::KeyDeviceRequired);
            }
            Some(dev.auth.id)
        }
        Confirmer::Cli => {
            if row.issuer_kind != "cli" {
                return Err(ApprovalError::NotFound);
            }
            None
        }
        // L1b: PC sessions, requests and tickets confirm through their routes.
        Confirmer::Pc(_) | Confirmer::Request(_) | Confirmer::Ticket(_) => {
            return Err(ApprovalError::NotFound);
        }
    };
    if locked.get(&row.subject_id).is_some_and(|a| a.banned) {
        return Err(ApprovalError::AccountBanned);
    }
    let want = row
        .code_hash
        .clone()
        .ok_or(ApprovalError::EnrollmentState)?;
    if !ct_eq(&code_hash(ctx.hmac_key(), row.id, code), &want) {
        let tries = row.tries + 1;
        if tries >= MAX_TRIES {
            sqlx::query("UPDATE enrollments SET tries = $2, state = 'failed', ended_at = NOW() WHERE id = $1")
                .bind(row.id)
                .bind(tries)
                .execute(&mut *conn)
                .await?;
            return Err(ApprovalError::EnrollmentFailed);
        }
        sqlx::query("UPDATE enrollments SET tries = $2 WHERE id = $1")
            .bind(row.id)
            .bind(tries)
            .execute(&mut *conn)
            .await?;
        return Err(ApprovalError::EnrollCodeWrong {
            tries_left: MAX_TRIES - tries,
        });
    }
    // Done: the device row, the account's first activity, the notice.
    let variant = EnrollmentVariant::parse(&row.variant).ok_or(ApprovalError::EnrollmentState)?;
    let state = activation_state(variant, subject_is_new(conn, row.subject_id).await?);
    let device_row_id = Uuid::new_v4();
    let claimed_device_id = row
        .claimed_device_id
        .ok_or(ApprovalError::EnrollmentState)?;
    let actor = match (&by, issuer_user) {
        (Confirmer::Cli, _) => Actor::Cli,
        (_, Some(u)) if u == row.subject_id => Actor::User(u),
        (_, Some(u)) => Actor::Admin(u),
        (_, None) => Actor::System,
    };
    // A device that re-enrols as an `active` key replaces its own earlier
    // (live) row, with the revocation cascade (B.7 rule 2): its sessions end,
    // its grants and open challenges are void. Never the issuer's own row and
    // never for a `pending` key (`replaceable_on_install`). The removal is a
    // notice of its own (B.1: every add and remove), so an owner whose phone
    // was replaced by a claim on its install id learns that the phone is no
    // longer a key. The push subscription stays: it is keyed by (account,
    // install id) and now belongs to the new key of the same install.
    let mut event_ids = Vec::with_capacity(2);
    if let Some(old) = replaceable_on_install(conn, &row, claimed_device_id, state, true).await? {
        sqlx::query(
            "UPDATE approval_devices SET state = 'revoked', revoked_at = NOW(), revoke_reason = $2
              WHERE id = $1",
        )
        .bind(old)
        .bind(replaced_reason(variant))
        .execute(&mut *conn)
        .await?;
        let old = AuthenticatorRef::key_device(old);
        super::cascade::on_authenticator_revoked(conn, ctx, row.subject_id, old).await?;
        let mut ev = NewSecurityEvent::new(
            row.subject_id,
            EventType::AuthenticatorRemoved,
            actor,
            serde_json::json!({"type": "key_device", "by": removed_by(actor)}),
        );
        ev.authenticator = Some(old);
        event_ids.push(record(conn, ev).await?);
    }
    sqlx::query(
        "INSERT INTO approval_devices (id, user_id, device_id, public_key, public_key_hash,
             attestation_chain, attestation_properties, uv_class, api_level, label, state,
             enrolled_via, enrollment_id, activated_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13,
                 CASE WHEN $11 = 'active' THEN NOW() END)",
    )
    .bind(device_row_id)
    .bind(row.subject_id)
    .bind(claimed_device_id)
    .bind(row.claimed_public_key.clone().unwrap_or_default())
    .bind(row.claimed_key_hash.clone().unwrap_or_default())
    .bind(row.claimed_attestation.clone().unwrap_or_default())
    .bind(
        row.claimed_properties
            .clone()
            .unwrap_or_else(|| serde_json::json!({})),
    )
    .bind(
        row.claimed_uv_class
            .clone()
            .unwrap_or_else(|| "per_use".into()),
    )
    .bind(row.claimed_api_level)
    .bind(
        row.claimed_label
            .clone()
            .unwrap_or_else(|| "Android phone".into()),
    )
    .bind(state.as_str())
    .bind(variant.as_str())
    .bind(row.id)
    .execute(&mut *conn)
    .await?;
    sqlx::query(
        "UPDATE enrollments SET state = 'done', done_at = NOW(), ended_at = NOW() WHERE id = $1",
    )
    .bind(row.id)
    .execute(&mut *conn)
    .await?;
    crate::auth::strong::mark_first_activity(conn, row.subject_id).await?;
    let mut ev = NewSecurityEvent::new(
        row.subject_id,
        EventType::AuthenticatorAdded,
        actor,
        serde_json::json!({"type": "key_device"}),
    );
    ev.authenticator = Some(AuthenticatorRef::key_device(device_row_id));
    event_ids.push(record(conn, ev).await?);
    let mut done = row.clone();
    done.state = "done".into();
    let wire = to_wire(conn, &done).await?;
    Ok((
        wire,
        AfterDone {
            subject_id: row.subject_id,
            issuer_user,
            enrollment_id: row.id,
            event_ids,
        },
    ))
}

/// The `enroll` arm of `/api/approvals/{id}/answer`: the signature is
/// verified first (only the issuing device can spend a try), then the code
/// (B.2 rev 4). The challenge is spent either way.
pub async fn answer_enroll_tx(
    conn: &mut PgConnection,
    ctx: &AuthCtx<'_>,
    dev: &KeyDeviceSession,
    challenge: &ChallengeRow,
    decision: Decision,
    response: &[u8],
    signature: &str,
) -> Result<(Enrollment, AfterDone), ApprovalError> {
    if decision != Decision::Approve {
        return Err(ApprovalError::Validation(
            "an enrolment is approved or denied".into(),
        ));
    }
    // Only the issuing device answers (its key, its install id).
    super::transition::lock_accounts(conn, &[dev.auth.id, challenge.subject_id]).await?;
    lock_issued_by(conn, challenge.requester_id, dev).await?;
    verify(
        challenge,
        ctx.origin(),
        &dev.device.public_key,
        decision,
        response,
        signature,
    )?;
    set_state(
        conn,
        challenge.id,
        &[ChallengeState::Issued],
        ChallengeState::Consumed,
    )
    .await?;
    confirm_tx(
        conn,
        ctx,
        challenge.requester_id,
        Confirmer::KeyDeviceAnswer(dev),
        response,
    )
    .await
}

/// SM2 "cancel" (`issued`/`claimed` → `cancelled`).
pub async fn cancel_tx(
    conn: &mut PgConnection,
    _ctx: &AuthCtx<'_>,
    id: Uuid,
    _by: EnrollmentCanceller,
) -> Result<(), ApprovalError> {
    let n = sqlx::query(
        "UPDATE enrollments SET state = 'cancelled', ended_at = NOW()
          WHERE id = $1 AND state IN ('issued', 'claimed')",
    )
    .bind(id)
    .execute(&mut *conn)
    .await?
    .rows_affected();
    if n == 1 {
        Ok(())
    } else {
        Err(ApprovalError::EnrollmentState)
    }
}

/// B.7 rule 2: enrolments issued by ended sessions expire.
pub async fn expire_for_sessions(conn: &mut PgConnection, sids: &[Uuid]) -> sqlx::Result<u64> {
    Ok(sqlx::query(
        "UPDATE enrollments SET state = 'expired', ended_at = NOW()
          WHERE issuer_session_id = ANY($1) AND state IN ('issued', 'claimed')",
    )
    .bind(sids)
    .execute(&mut *conn)
    .await?
    .rows_affected())
}

/// B.7 rule 2: enrolments issued by a revoked key device expire.
pub async fn expire_for_device(conn: &mut PgConnection, device_row_id: Uuid) -> sqlx::Result<u64> {
    Ok(sqlx::query(
        "UPDATE enrollments SET state = 'expired', ended_at = NOW()
          WHERE issuer_device_id = $1 AND state IN ('issued', 'claimed')",
    )
    .bind(device_row_id)
    .execute(&mut *conn)
    .await?
    .rows_affected())
}

/// Timers step: setup tokens and codes past due.
pub async fn expire_due(conn: &mut PgConnection) -> sqlx::Result<u64> {
    Ok(sqlx::query(
        "UPDATE enrollments SET state = 'expired', ended_at = NOW()
          WHERE (state = 'issued' AND expires_at <= NOW())
             OR (state = 'claimed' AND code_expires_at <= NOW())",
    )
    .execute(&mut *conn)
    .await?
    .rows_affected())
}

/// After a `done` commit: deliver the notice and tell the issuer's and
/// the subject's sockets.
pub async fn after_done(state: &AppState, after: AfterDone) {
    if let Err(e) = crate::security_events::notices::deliver(state, &after.event_ids).await {
        tracing::warn!(error = %e, "enrolment notice delivery failed; the sweep retries");
    }
    super::devices::notify_authenticators_changed(state, after.subject_id).await;
    if let Some(issuer) = after.issuer_user {
        enrollment_updated(state, issuer, after.enrollment_id, "done").await;
    }
}

/// WS hint `enrollment_updated {enrollment_id, state}` to the issuer.
pub async fn enrollment_updated(state: &AppState, user_id: Uuid, enrollment_id: Uuid, st: &str) {
    if let Err(e) = crate::ws::broadcast_to_user(
        &state.redis,
        user_id,
        &crate::ws::ServerEvent::EnrollmentUpdated {
            enrollment_id,
            state: st.to_string(),
        },
    )
    .await
    {
        tracing::warn!(error = %e, "enrollment_updated hint failed");
    }
}

/// Tell the issuing account (its key device's or its PC session's) the
/// enrolment's current state (`enrollment_updated`). Issuers without an
/// account (CLI, request, ticket) have no socket to tell.
pub async fn notify_issuer(state: &AppState, enrollment_id: Uuid) {
    let row: Option<(String, Option<Uuid>)> = sqlx::query_as(
        "SELECT e.state, COALESCE(d.user_id, s.user_id) FROM enrollments e
           LEFT JOIN approval_devices d ON d.id = e.issuer_device_id
           LEFT JOIN sessions s ON s.id = e.issuer_session_id
          WHERE e.id = $1",
    )
    .bind(enrollment_id)
    .fetch_optional(&state.db)
    .await
    .ok()
    .flatten();
    if let Some((st, Some(user))) = row {
        enrollment_updated(state, user, enrollment_id, &st).await;
    }
}

/// After a wrong code or a deny: tell the issuer the new state.
pub async fn notify_issuer_after_failure(state: &AppState, enrollment_id: Uuid) {
    notify_issuer(state, enrollment_id).await;
}
