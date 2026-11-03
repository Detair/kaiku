//! The account-request state machine (SM3).
//!
//! States `waiting`, `ready`, `completed`; terminal `cancelled`,
//! `withdrawn`, `expired`, `void`. Every transition runs under the
//! subject's account lock (B.7 rule 1; `lock_scope` adds the acting admin or
//! contact) and re-checks the request's state, deadline, factor
//! generations and recorded cancellers.

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgConnection;
use uuid::Uuid;

use super::types::{
    AccountRequest, AccountRequestSummary, AuthenticatorLabel, CancelVia, LostDevice,
    RequestSubject, StagedView, StartedBy,
};
use crate::approvals::secrets::{keyed_hash, Secret256, SecretPurpose};
use crate::approvals::transition::{lock_scope, AffectedRoute, LockScope};
use crate::auth::ctx::AuthCtx;
use crate::auth::error::{AuthError, AuthResult};
use crate::auth::factors::{self, ExitPath, Factor, FactorRef, StateCause};
use crate::auth::owner_rule::OwnerAuthority;
use crate::auth::strong::{AuthenticatorKind, AuthenticatorRef};
use crate::security_events::{record, Actor, EventType, NewSecurityEvent};

/// The Finish window after `ready` (SM3).
pub const FINISH_HOURS: i64 = 24;
/// The "ready soon" notice comes 1 h before `ready`.
pub const READY_SOON_HOURS: i64 = 1;

/// The kind of a request.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RequestKind {
    SelfService,
    Admin,
}

impl RequestKind {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::SelfService => "self",
            Self::Admin => "admin",
        }
    }
}

/// What a request is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Purpose {
    Recovery,
    Bootstrap,
    AdminSetup,
    AdminRecovery,
}

impl Purpose {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Recovery => "recovery",
            Self::Bootstrap => "bootstrap",
            Self::AdminSetup => "admin_setup",
            Self::AdminRecovery => "admin_recovery",
        }
    }
}

/// A recorded independent canceller (SM3 rev 4/5).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CancellerRoute {
    Admin(Uuid),
    Contact(Uuid),
    Authenticator(AuthenticatorRef),
}

#[derive(Serialize, Deserialize)]
struct RouteJson {
    route: String,
    id: Uuid,
    #[serde(rename = "type", skip_serializing_if = "Option::is_none", default)]
    kind: Option<String>,
}

impl CancellerRoute {
    fn to_json(self) -> serde_json::Value {
        let r = match self {
            Self::Admin(id) => RouteJson {
                route: "admin".into(),
                id,
                kind: None,
            },
            Self::Contact(id) => RouteJson {
                route: "contact".into(),
                id,
                kind: None,
            },
            Self::Authenticator(a) => RouteJson {
                route: "authenticator".into(),
                id: a.id,
                kind: Some(a.as_str().into()),
            },
        };
        serde_json::to_value(r).unwrap_or_default()
    }

    fn from_json(v: &serde_json::Value) -> Option<Self> {
        let r: RouteJson = serde_json::from_value(v.clone()).ok()?;
        match r.route.as_str() {
            "admin" => Some(Self::Admin(r.id)),
            "contact" => Some(Self::Contact(r.id)),
            "authenticator" => Some(Self::Authenticator(AuthenticatorRef {
                kind: AuthenticatorKind::parse(r.kind.as_deref()?)?,
                id: r.id,
            })),
            _ => None,
        }
    }

    /// The JSON a GIN containment query matches.
    #[must_use]
    pub fn containment(affected: &AffectedRoute) -> serde_json::Value {
        let route = match *affected {
            AffectedRoute::Admin(id) => Self::Admin(id),
            AffectedRoute::Contact(id) => Self::Contact(id),
            AffectedRoute::Authenticator(a) => Self::Authenticator(a),
        };
        serde_json::Value::Array(vec![route.to_json()])
    }
}

/// A stored request.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct RequestRow {
    pub id: Uuid,
    pub user_id: Uuid,
    pub kind: String,
    pub purpose: String,
    pub factors: serde_json::Value,
    pub lost_device_ids: Vec<Uuid>,
    pub cancellers: serde_json::Value,
    pub started_by: Option<Uuid>,
    pub started_by_device: Option<Uuid>,
    pub enrollment_id: Option<Uuid>,
    pub request_secret_hash: Option<Vec<u8>>,
    pub staged: serde_json::Value,
    pub state: String,
    pub created_at: DateTime<Utc>,
    pub ready_at: DateTime<Utc>,
    pub expires_at: Option<DateTime<Utc>>,
    pub ready_soon_sent: bool,
    pub cancelled_by: Option<serde_json::Value>,
    pub cancel_reason: Option<String>,
    pub ended_at: Option<DateTime<Utc>>,
}

impl RequestRow {
    #[must_use]
    pub fn is_open(&self) -> bool {
        matches!(self.state.as_str(), "waiting" | "ready")
    }

    fn routes(&self) -> Vec<CancellerRoute> {
        self.cancellers
            .as_array()
            .map(|a| a.iter().filter_map(CancellerRoute::from_json).collect())
            .unwrap_or_default()
    }

    /// The recorded factor generations (`oidc` has none).
    #[must_use]
    pub fn factor_refs(&self) -> Vec<FactorRef> {
        self.factors
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|f| {
                        Some(FactorRef {
                            factor: Factor::parse(f["factor"].as_str()?)?,
                            generation: f["generation"].as_i64()?,
                        })
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    fn factor_names(&self) -> Vec<String> {
        self.factors
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|f| f["factor"].as_str())
                    .map(|f| {
                        if f == "recovery_codes" {
                            "recovery_code".to_string()
                        } else {
                            f.to_string()
                        }
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// The pending authenticator recorded in `staged`.
    #[must_use]
    pub fn pending_authenticator(&self) -> Option<AuthenticatorRef> {
        let a = &self.staged["authenticator"];
        Some(AuthenticatorRef {
            kind: AuthenticatorKind::parse(a["type"].as_str()?)?,
            id: a["id"].as_str()?.parse().ok()?,
        })
    }
}

fn secret_hash(ctx: &AuthCtx<'_>, s: &Secret256) -> [u8; 32] {
    keyed_hash(ctx.hmac_key(), SecretPurpose::RequestSecret, s.as_bytes())
}

/// The `ready` request of a request secret (the own-key claim, SM2).
pub async fn request_id_by_secret(
    conn: &mut PgConnection,
    ctx: &AuthCtx<'_>,
    secret: &Secret256,
) -> sqlx::Result<Option<Uuid>> {
    sqlx::query_scalar(
        "SELECT id FROM account_requests
          WHERE request_secret_hash = $1 AND state = 'ready' AND expires_at > clock_timestamp()",
    )
    .bind(secret_hash(ctx, secret).to_vec())
    .fetch_optional(&mut *conn)
    .await
}

/// The request of a request secret (any state).
pub async fn by_secret(
    conn: &mut PgConnection,
    ctx: &AuthCtx<'_>,
    secret: &Secret256,
    lock: bool,
) -> sqlx::Result<Option<RequestRow>> {
    let sql = if lock {
        "SELECT * FROM account_requests WHERE request_secret_hash = $1 FOR UPDATE"
    } else {
        "SELECT * FROM account_requests WHERE request_secret_hash = $1"
    };
    sqlx::query_as(sql)
        .bind(secret_hash(ctx, secret).to_vec())
        .fetch_optional(&mut *conn)
        .await
}

async fn by_id(conn: &mut PgConnection, id: Uuid, lock: bool) -> sqlx::Result<Option<RequestRow>> {
    let sql = if lock {
        "SELECT * FROM account_requests WHERE id = $1 FOR UPDATE"
    } else {
        "SELECT * FROM account_requests WHERE id = $1"
    };
    sqlx::query_as(sql)
        .bind(id)
        .fetch_optional(&mut *conn)
        .await
}

/// The account's open request, if any.
pub async fn open_of(conn: &mut PgConnection, user_id: Uuid) -> sqlx::Result<Option<RequestRow>> {
    sqlx::query_as(
        "SELECT * FROM account_requests WHERE user_id = $1 AND state IN ('waiting', 'ready')",
    )
    .bind(user_id)
    .fetch_optional(&mut *conn)
    .await
}

// ============================================================================
// Independent cancellers (SM3 rev 4/5)
// ============================================================================

async fn has_active_strong(conn: &mut PgConnection, user: Uuid) -> sqlx::Result<bool> {
    Ok(crate::auth::strong::active_strong_count(conn, user).await? > 0)
}

/// The independent cancellers of a request of `subject` starting at `start`.
///
/// The routes by which someone other than the requester and the acting
/// admin can cancel it: other
/// admins with an active strong authenticator (for a non-admin's `self`
/// request every admin, a sole admin included); the contact if confirmed at
/// least one delay period before `start` and holding an active strong
/// authenticator; the account's own active strong authenticators.
pub async fn independent_cancellers(
    conn: &mut PgConnection,
    ctx: &AuthCtx<'_>,
    subject: Uuid,
    requester_admin: Option<Uuid>,
    start: DateTime<Utc>,
) -> sqlx::Result<Vec<CancellerRoute>> {
    let mut out = Vec::new();
    let admins: Vec<Uuid> = sqlx::query_scalar(
        "SELECT sa.user_id FROM system_admins sa JOIN users u ON u.id = sa.user_id
          WHERE NOT u.is_system AND sa.user_id <> $1 ORDER BY sa.user_id",
    )
    .bind(subject)
    .fetch_all(&mut *conn)
    .await?;
    for a in admins {
        if Some(a) != requester_admin && has_active_strong(conn, a).await? {
            out.push(CancellerRoute::Admin(a));
        }
    }
    let contact: Option<(Option<Uuid>, Option<DateTime<Utc>>)> = sqlx::query_as(
        "SELECT recovery_contact_id, recovery_contact_confirmed_at FROM users WHERE id = $1",
    )
    .bind(subject)
    .fetch_optional(&mut *conn)
    .await?;
    if let Some((Some(c), Some(confirmed))) = contact {
        let old_enough =
            confirmed + Duration::hours(ctx.config.account_request_delay_hours) <= start;
        if old_enough && has_active_strong(conn, c).await? {
            out.push(CancellerRoute::Contact(c));
        }
    }
    let devices: Vec<Uuid> = sqlx::query_scalar(
        "SELECT id FROM approval_devices WHERE user_id = $1 AND state = 'active' ORDER BY id",
    )
    .bind(subject)
    .fetch_all(&mut *conn)
    .await?;
    out.extend(
        devices
            .into_iter()
            .map(|id| CancellerRoute::Authenticator(AuthenticatorRef::key_device(id))),
    );
    let passkeys: Vec<Uuid> = sqlx::query_scalar(
        "SELECT id FROM passkeys WHERE user_id = $1 AND state = 'active' ORDER BY id",
    )
    .bind(subject)
    .fetch_all(&mut *conn)
    .await?;
    out.extend(
        passkeys
            .into_iter()
            .map(|id| CancellerRoute::Authenticator(AuthenticatorRef::passkey(id))),
    );
    Ok(out)
}

/// Whether a recorded route is still there (B.7 rule 2: the admin is still
/// an admin with an active strong authenticator; the contact is still the
/// account's confirmed contact with one; the authenticator is active).
async fn route_alive(
    conn: &mut PgConnection,
    subject: Uuid,
    route: CancellerRoute,
) -> sqlx::Result<bool> {
    match route {
        CancellerRoute::Admin(a) => {
            let admin: bool = sqlx::query_scalar(
                "SELECT EXISTS (SELECT 1 FROM system_admins WHERE user_id = $1)",
            )
            .bind(a)
            .fetch_one(&mut *conn)
            .await?;
            Ok(admin && has_active_strong(conn, a).await?)
        }
        CancellerRoute::Contact(c) => {
            let still: bool = sqlx::query_scalar(
                "SELECT EXISTS (SELECT 1 FROM users WHERE id = $1 AND recovery_contact_id = $2
                                   AND recovery_contact_confirmed_at IS NOT NULL)",
            )
            .bind(subject)
            .bind(c)
            .fetch_one(&mut *conn)
            .await?;
            Ok(still && has_active_strong(conn, c).await?)
        }
        CancellerRoute::Authenticator(a) => crate::auth::strong::is_active(conn, a).await,
    }
}

/// How many recorded routes are left.
pub async fn routes_left(conn: &mut PgConnection, row: &RequestRow) -> sqlx::Result<usize> {
    let mut n = 0;
    for r in row.routes() {
        if route_alive(conn, row.user_id, r).await? {
            n += 1;
        }
    }
    Ok(n)
}

// ============================================================================
// Start
// ============================================================================

/// A new request.
#[derive(Debug, Clone)]
pub struct NewRequest {
    pub user_id: Uuid,
    pub kind: RequestKind,
    pub purpose: Purpose,
    /// Generations used (password, TOTP, recovery codes).
    pub factors: Vec<FactorRef>,
    /// An OIDC factor was used (no generation).
    pub oidc: bool,
    pub lost_device_ids: Vec<Uuid>,
    pub started_by: Option<Uuid>,
    pub started_by_device: Option<Uuid>,
    pub enrollment_id: Option<Uuid>,
    /// The pending authenticator of an `admin` request.
    pub pending: Option<AuthenticatorRef>,
    /// `self` requests get a request secret for their client.
    pub with_secret: bool,
}

/// A started request: the row, its secret (self) and the events to deliver.
#[derive(Debug)]
pub struct StartedRequest {
    pub row: RequestRow,
    pub request_secret: Option<Secret256>,
    pub event_ids: Vec<Uuid>,
}

/// Start a request under the subject's account lock (the caller holds it).
///
/// One open request per account (409 `ACCOUNT_REQUEST_OPEN`), at least one
/// independent canceller (409 `NO_INDEPENDENT_CANCELLER`), `ready_at` =
/// now + delay; event `request_started` (high).
#[tracing::instrument(skip(conn, ctx, new), fields(subject = %new.user_id, purpose = new.purpose.as_str()))]
pub async fn insert_tx(
    conn: &mut PgConnection,
    ctx: &AuthCtx<'_>,
    new: NewRequest,
) -> AuthResult<StartedRequest> {
    if open_of(conn, new.user_id).await?.is_some() {
        return Err(AuthError::AccountRequestOpen);
    }
    let now = Utc::now();
    let routes = independent_cancellers(conn, ctx, new.user_id, new.started_by, now).await?;
    if routes.is_empty() {
        return Err(AuthError::NoIndependentCanceller);
    }
    let mut factors: Vec<serde_json::Value> = new
        .factors
        .iter()
        .map(|f| serde_json::json!({"factor": f.factor.as_str(), "generation": f.generation}))
        .collect();
    if new.oidc {
        factors.push(serde_json::json!({"factor": "oidc"}));
    }
    let secret = new.with_secret.then(Secret256::random);
    let staged = new.pending.map_or_else(
        || serde_json::json!({}),
        |a| serde_json::json!({"authenticator": {"type": a.as_str(), "id": a.id}}),
    );
    let row: RequestRow = sqlx::query_as(
        "INSERT INTO account_requests
            (id, user_id, kind, purpose, factors, lost_device_ids, cancellers, started_by,
             started_by_device, enrollment_id, request_secret_hash, staged, state, created_at,
             ready_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, 'waiting', $13, $14)
         RETURNING *",
    )
    .bind(Uuid::new_v4())
    .bind(new.user_id)
    .bind(new.kind.as_str())
    .bind(new.purpose.as_str())
    .bind(serde_json::Value::Array(factors))
    .bind(&new.lost_device_ids)
    .bind(serde_json::Value::Array(
        routes.iter().map(|r| r.to_json()).collect(),
    ))
    .bind(new.started_by)
    .bind(new.started_by_device)
    .bind(new.enrollment_id)
    .bind(secret.as_ref().map(|s| secret_hash(ctx, s).to_vec()))
    .bind(staged)
    .bind(now)
    .bind(now + Duration::hours(ctx.config.account_request_delay_hours))
    .fetch_one(&mut *conn)
    .await
    .map_err(|e| match e {
        sqlx::Error::Database(ref d) if d.is_unique_violation() => AuthError::AccountRequestOpen,
        other => AuthError::Database(other),
    })?;
    let actor = new
        .started_by
        .map_or(Actor::User(new.user_id), Actor::Admin);
    let event = request_event(conn, &row, EventType::RequestStarted, actor, None).await?;
    Ok(StartedRequest {
        row,
        request_secret: secret,
        event_ids: vec![event],
    })
}

/// Record a request event `{request_id, purpose, cancelled_by?}`.
async fn request_event(
    conn: &mut PgConnection,
    row: &RequestRow,
    ty: EventType,
    actor: Actor,
    cancelled_by: Option<serde_json::Value>,
) -> sqlx::Result<Uuid> {
    let mut details = serde_json::json!({"request_id": row.id, "purpose": row.purpose});
    if let Some(c) = cancelled_by {
        details["cancelled_by"] = c;
    }
    let mut ev = NewSecurityEvent::new(row.user_id, ty, actor, details);
    ev.request_id = Some(row.id);
    record(conn, ev).await
}

/// Lock the subject (+ `actors`) and the request; `None` if the request is
/// unknown.
async fn lock_request(
    conn: &mut PgConnection,
    id: Uuid,
    actors: &[Uuid],
) -> AuthResult<Option<RequestRow>> {
    let Some(seen) = by_id(conn, id, false).await? else {
        return Ok(None);
    };
    // A completion revokes the devices named lost: the requests that record
    // them (or the subject as admin or contact) are locked too (Codex M7).
    let mut affected = Vec::new();
    if !seen.lost_device_ids.is_empty() {
        affected.push(AffectedRoute::Admin(seen.user_id));
        affected.push(AffectedRoute::Contact(seen.user_id));
        affected.extend(
            seen.lost_device_ids
                .iter()
                .map(|d| AffectedRoute::Authenticator(AuthenticatorRef::key_device(*d))),
        );
    }
    lock_scope(
        conn,
        &LockScope {
            subject: seen.user_id,
            actors: actors.to_vec(),
            affected,
        },
    )
    .await
    .map_err(|e| match e {
        crate::approvals::transition::TransitionError::Db(d) => AuthError::Database(d),
        _ => AuthError::ServiceUnavailable,
    })?;
    Ok(by_id(conn, id, true).await?)
}

/// End the request's child enrolment and pending authenticator (rule 2).
async fn void_children(
    conn: &mut PgConnection,
    ctx: &AuthCtx<'_>,
    row: &RequestRow,
) -> AuthResult<()> {
    sqlx::query(
        "UPDATE enrollments SET state = 'cancelled', ended_at = NOW()
          WHERE (issuer_request_id = $1 OR id = $2) AND state IN ('issued', 'claimed')",
    )
    .bind(row.id)
    .bind(row.enrollment_id)
    .execute(&mut *conn)
    .await?;
    if let Some(a) = row.pending_authenticator() {
        match a.kind {
            AuthenticatorKind::KeyDevice => {
                sqlx::query(
                    "UPDATE approval_devices SET state = 'revoked', revoked_at = NOW(),
                            revoke_reason = 'request'
                      WHERE id = $1 AND state = 'pending'",
                )
                .bind(a.id)
                .execute(&mut *conn)
                .await?;
            }
            AuthenticatorKind::Passkey => {
                sqlx::query("DELETE FROM passkeys WHERE id = $1 AND state = 'pending'")
                    .bind(a.id)
                    .execute(&mut *conn)
                    .await?;
            }
        }
    }
    let _ = ctx;
    Ok(())
}

// ============================================================================
// Cancel, withdraw, void
// ============================================================================

/// Cancel a locked request (SM3): the caller locked it ([`lock_for_cancel`])
/// and decided `by` (the B.2 owner rule) in the same transaction.
pub async fn cancel_locked_tx(
    conn: &mut PgConnection,
    ctx: &AuthCtx<'_>,
    row: &RequestRow,
    by: &OwnerAuthority,
    actor_id: Uuid,
    reason: Option<&str>,
) -> AuthResult<Vec<Uuid>> {
    if row.state == "completed" {
        return Err(AuthError::AccountRequestCompleted);
    }
    if !row.is_open() {
        return Err(AuthError::CancelNotAllowed);
    }
    let cancelled_by = serde_json::json!({"kind": by.kind(), "user_id": actor_id});
    sqlx::query(
        "UPDATE account_requests SET state = 'cancelled', ended_at = NOW(), cancelled_by = $2,
                cancel_reason = $3
          WHERE id = $1",
    )
    .bind(row.id)
    .bind(&cancelled_by)
    .bind(reason.map(|r| r.chars().take(500).collect::<String>()))
    .execute(&mut *conn)
    .await?;
    void_children(conn, ctx, row).await?;
    // `self`: the generations it used are compromised (a factor replaced
    // since is untouched); `admin`: nothing more.
    if row.kind == "self" {
        for f in row.factor_refs() {
            if f.factor == Factor::RecoveryCodes {
                continue;
            }
            factors::mark_compromised_tx(
                conn,
                ctx,
                row.user_id,
                f,
                StateCause::Cancel,
                actor_of(by, actor_id, row.user_id),
            )
            .await?;
        }
    }
    let event = request_event(
        conn,
        row,
        EventType::RequestCancelled,
        actor_of(by, actor_id, row.user_id),
        Some(cancelled_by),
    )
    .await?;
    Ok(vec![event])
}

const fn actor_of(by: &OwnerAuthority, actor: Uuid, subject: Uuid) -> Actor {
    match by {
        OwnerAuthority::Admin { .. } => Actor::Admin(actor),
        OwnerAuthority::Contact { .. } => Actor::RecoveryContact(actor),
        OwnerAuthority::Part1 { .. } | OwnerAuthority::Part2 { .. } => Actor::User(subject),
        OwnerAuthority::Cli => Actor::Cli,
    }
}

/// Lock a request for a cancel by `actor` (subject + actor locks).
pub async fn lock_for_cancel(
    conn: &mut PgConnection,
    id: Uuid,
    actor: Uuid,
) -> AuthResult<RequestRow> {
    lock_request(conn, id, &[actor])
        .await?
        .ok_or(AuthError::CancelNotAllowed)
}

/// Withdraw (the request client, `self`): no other effect; event
/// `request_withdrawn` (high) follows the start notice.
#[tracing::instrument(skip_all)]
pub async fn withdraw_tx(
    conn: &mut PgConnection,
    ctx: &AuthCtx<'_>,
    secret: &Secret256,
) -> AuthResult<Vec<Uuid>> {
    let seen = by_secret(conn, ctx, secret, false)
        .await?
        .ok_or_else(|| AuthError::NotFound("Unknown request".into()))?;
    let row = lock_request(conn, seen.id, &[])
        .await?
        .ok_or_else(|| AuthError::NotFound("Unknown request".into()))?;
    if !row.is_open() {
        return Err(AuthError::AccountRequestNotReady { state: row.state });
    }
    sqlx::query("UPDATE account_requests SET state = 'withdrawn', ended_at = NOW() WHERE id = $1")
        .bind(row.id)
        .execute(&mut *conn)
        .await?;
    void_children(conn, ctx, &row).await?;
    Ok(vec![
        request_event(
            conn,
            &row,
            EventType::RequestWithdrawn,
            Actor::User(row.user_id),
            None,
        )
        .await?,
    ])
}

/// Why a request becomes `void` (B.7 rule 2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VoidCause {
    GenerationEnded,
    NoRouteLeft,
    IssuerGone,
    EntitlementConsumed,
}

/// `void`: no factor is disabled; event `request_void`.
pub async fn void_row_tx(
    conn: &mut PgConnection,
    ctx: &AuthCtx<'_>,
    row: &RequestRow,
    why: VoidCause,
) -> AuthResult<Option<Uuid>> {
    let n = sqlx::query(
        "UPDATE account_requests SET state = 'void', ended_at = NOW()
          WHERE id = $1 AND state IN ('waiting', 'ready')",
    )
    .bind(row.id)
    .execute(&mut *conn)
    .await?
    .rows_affected();
    if n == 0 {
        return Ok(None);
    }
    tracing::info!(request_id = %row.id, cause = ?why, "account request void");
    void_children(conn, ctx, row).await?;
    Ok(Some(
        request_event(conn, row, EventType::RequestVoid, Actor::System, None).await?,
    ))
}

// ============================================================================
// Cascades (B.7 rule 2)
// ============================================================================

/// A factor generation ended: open requests that used it are `void`.
pub async fn on_generation_ended(
    conn: &mut PgConnection,
    ctx: &AuthCtx<'_>,
    user_id: Uuid,
    used: FactorRef,
) -> AuthResult<()> {
    let rows: Vec<RequestRow> = sqlx::query_as(
        "SELECT * FROM account_requests
          WHERE user_id = $1 AND state IN ('waiting', 'ready') AND factors @> $2",
    )
    .bind(user_id)
    .bind(serde_json::json!([{"factor": used.factor.as_str(), "generation": used.generation}]))
    .fetch_all(&mut *conn)
    .await?;
    for r in rows {
        void_row_tx(conn, ctx, &r, VoidCause::GenerationEnded).await?;
    }
    Ok(())
}

/// Re-check the open requests that record `affected` (or, with `subject`,
/// all of that account's open requests): those without a route left become
/// `void`.
pub async fn recheck_routes(
    conn: &mut PgConnection,
    ctx: &AuthCtx<'_>,
    affected: Option<AffectedRoute>,
    subject: Option<Uuid>,
) -> AuthResult<()> {
    let rows: Vec<RequestRow> = match (affected, subject) {
        (Some(a), _) => {
            sqlx::query_as(
                "SELECT * FROM account_requests
                  WHERE state IN ('waiting', 'ready') AND cancellers @> $1",
            )
            .bind(CancellerRoute::containment(&a))
            .fetch_all(&mut *conn)
            .await?
        }
        (None, Some(s)) => sqlx::query_as(
            "SELECT * FROM account_requests WHERE user_id = $1 AND state IN ('waiting', 'ready')",
        )
        .bind(s)
        .fetch_all(&mut *conn)
        .await?,
        (None, None) => Vec::new(),
    };
    for r in rows {
        if routes_left(conn, &r).await? == 0 {
            void_row_tx(conn, ctx, &r, VoidCause::NoRouteLeft).await?;
        }
    }
    Ok(())
}

/// An authenticator was revoked: requests recording it lose a route; an
/// admin's issuing device voids that admin's `admin` requests; the owner as
/// admin or contact route is re-checked.
pub async fn on_authenticator_revoked(
    conn: &mut PgConnection,
    ctx: &AuthCtx<'_>,
    owner: Uuid,
    a: AuthenticatorRef,
) -> AuthResult<()> {
    let issued: Vec<RequestRow> = sqlx::query_as(
        "SELECT * FROM account_requests
          WHERE state IN ('waiting', 'ready') AND kind = 'admin' AND started_by_device = $1",
    )
    .bind(a.id)
    .fetch_all(&mut *conn)
    .await?;
    for r in issued {
        void_row_tx(conn, ctx, &r, VoidCause::IssuerGone).await?;
    }
    recheck_routes(conn, ctx, Some(AffectedRoute::Authenticator(a)), None).await?;
    recheck_routes(conn, ctx, Some(AffectedRoute::Admin(owner)), None).await?;
    recheck_routes(conn, ctx, Some(AffectedRoute::Contact(owner)), None).await?;
    Ok(())
}

/// An admin was demoted: their `admin` requests are `void`; requests that
/// record them as a route are re-checked.
pub async fn on_admin_demoted(
    conn: &mut PgConnection,
    ctx: &AuthCtx<'_>,
    admin: Uuid,
) -> AuthResult<()> {
    let started: Vec<RequestRow> = sqlx::query_as(
        "SELECT * FROM account_requests
          WHERE state IN ('waiting', 'ready') AND kind = 'admin' AND started_by = $1",
    )
    .bind(admin)
    .fetch_all(&mut *conn)
    .await?;
    for r in started {
        void_row_tx(conn, ctx, &r, VoidCause::IssuerGone).await?;
    }
    recheck_routes(conn, ctx, Some(AffectedRoute::Admin(admin)), None).await
}

/// A strong authenticator of `user` became active by another path: the
/// bootstrap entitlement is consumed and an open bootstrap request is
/// `void` (unless it is the one activating, `except`).
pub async fn on_strong_activated(
    conn: &mut PgConnection,
    ctx: &AuthCtx<'_>,
    user: Uuid,
    except: Option<Uuid>,
) -> AuthResult<()> {
    sqlx::query("UPDATE users SET bootstrap_open = false WHERE id = $1 AND bootstrap_open")
        .bind(user)
        .execute(&mut *conn)
        .await?;
    let rows: Vec<RequestRow> = sqlx::query_as(
        "SELECT * FROM account_requests
          WHERE user_id = $1 AND purpose = 'bootstrap' AND state IN ('waiting', 'ready')",
    )
    .bind(user)
    .fetch_all(&mut *conn)
    .await?;
    for r in rows {
        if Some(r.id) != except {
            void_row_tx(conn, ctx, &r, VoidCause::EntitlementConsumed).await?;
        }
    }
    Ok(())
}

// ============================================================================
// Completion (rule 1 + 4)
// ============================================================================

/// What a completion did.
#[derive(Debug, Default)]
pub struct CompletionOutcome {
    pub recovery_codes: Option<Vec<String>>,
    pub ended_sessions: Vec<Uuid>,
    pub revoked_devices: Vec<Uuid>,
    pub event_ids: Vec<Uuid>,
}

/// Complete a locked request (rule 1 + 4).
///
/// `ready` (or `admin` at its timer), within the deadline, generations unchanged, a recorded route
/// left (else `void`). Applies the staged choices (Task 22), activates at most one
/// authenticator, revokes devices named lost, consumes `bootstrap_open`,
/// replaces (`self`) or invalidates (`admin`) the recovery codes, ends the
/// other sessions when devices were named lost or the password/TOTP
/// changed, and records `request_completed`.
#[tracing::instrument(skip_all, fields(request_id = %row.id))]
pub async fn complete_locked_tx(
    conn: &mut PgConnection,
    ctx: &AuthCtx<'_>,
    row: &RequestRow,
) -> AuthResult<CompletionOutcome> {
    let mut out = CompletionOutcome::default();
    let due = match row.kind.as_str() {
        "admin" => row.state == "waiting" && row.ready_at <= Utc::now(),
        _ => row.state == "ready" && row.expires_at.is_some_and(|t| t > Utc::now()),
    };
    // Fable m2: a banned account completes nothing (the request waits).
    let banned: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM global_bans
                         WHERE user_id = $1 AND (expires_at IS NULL OR expires_at > NOW()))",
    )
    .bind(row.user_id)
    .fetch_one(&mut *conn)
    .await?;
    if banned {
        return Err(AuthError::AccountBanned);
    }
    if !due {
        // Past the Finish deadline before the minute sweep: `expired`.
        let state = if row.state == "ready" {
            "expired".to_string()
        } else {
            row.state.clone()
        };
        return Err(AuthError::AccountRequestNotReady { state });
    }
    // Rule 4: generations unchanged, a route left.
    let st = factors::states(conn, row.user_id).await?;
    let unchanged = row.factor_refs().iter().all(|f| match f.factor {
        Factor::Password => st.password.generation == f.generation,
        Factor::Totp => st.totp.generation == f.generation,
        // Fable m3: the code set too, not only through its cascade.
        Factor::RecoveryCodes => st.recovery_codes_generation == f.generation,
    });
    if !unchanged {
        out.event_ids
            .extend(void_row_tx(conn, ctx, row, VoidCause::GenerationEnded).await?);
        return Ok(out);
    }
    if routes_left(conn, row).await? == 0 {
        out.event_ids
            .extend(void_row_tx(conn, ctx, row, VoidCause::NoRouteLeft).await?);
        return Ok(out);
    }
    // `completed` before the effects: the cascades they trigger (a replaced
    // generation, a revoked lost device) must not void this request.
    sqlx::query("UPDATE account_requests SET state = 'completed', ended_at = NOW() WHERE id = $1")
        .bind(row.id)
        .execute(&mut *conn)
        .await?;
    let staged = row.staged.clone();
    // A staged password or TOTP (Task 22): new generations, `active`.
    let password_hash = staged["password_hash"].as_str();
    let mut credentials_changed = password_hash.is_some();
    if let Some(hash) = password_hash {
        factors::replace_tx(conn, ctx, row.user_id, Factor::Password).await?;
        sqlx::query(
            "UPDATE users SET password_hash = $2, must_change_password = false WHERE id = $1",
        )
        .bind(row.user_id)
        .bind(hash)
        .execute(&mut *conn)
        .await?;
    }
    if let Some(secret) = staged["totp_secret"].as_str() {
        factors::replace_tx(conn, ctx, row.user_id, Factor::Totp).await?;
        sqlx::query("UPDATE users SET mfa_secret = $2 WHERE id = $1")
            .bind(row.user_id)
            .bind(secret)
            .execute(&mut *conn)
            .await?;
        credentials_changed = true;
    }
    if let Some(contact) = staged["recovery_contact_id"]
        .as_str()
        .and_then(|s| s.parse::<Uuid>().ok())
    {
        let (_, ev) = crate::auth::recovery_contact::set_tx(
            conn,
            ctx,
            row.user_id,
            Some(contact),
            crate::auth::recovery_contact::ContactActor::Owner,
        )
        .await?;
        out.event_ids.extend(ev);
    }
    // At most one authenticator becomes active.
    let activated = match row.pending_authenticator() {
        Some(a) => activate_pending(conn, row.user_id, a).await?,
        None => false,
    };
    // The request's open child enrolments end with it (rule 2).
    sqlx::query(
        "UPDATE enrollments SET state = 'cancelled', ended_at = NOW()
          WHERE issuer_request_id = $1 AND state IN ('issued', 'claimed')",
    )
    .bind(row.id)
    .execute(&mut *conn)
    .await?;
    // Devices named lost are revoked.
    for d in &row.lost_device_ids {
        let still: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM approval_devices WHERE id = $1 AND user_id = $2
                               AND state = 'active')",
        )
        .bind(d)
        .bind(row.user_id)
        .fetch_one(&mut *conn)
        .await?;
        if still {
            let (_, ev) = crate::approvals::devices::revoke_tx(
                conn,
                ctx,
                *d,
                crate::approvals::devices::RevokeReason::Request,
                Actor::System,
            )
            .await
            .map_err(approval_to_auth)?;
            out.event_ids.extend(ev);
            out.revoked_devices.push(*d);
        }
    }
    // The bootstrap entitlement.
    if row.purpose == "bootstrap" || activated {
        on_strong_activated(conn, ctx, row.user_id, Some(row.id)).await?;
    }
    // Recovery codes: `self` replaces (shown once), `admin` invalidates.
    if row.kind == "self" {
        out.recovery_codes =
            Some(crate::auth::recovery_codes::replace_tx(conn, ctx, row.user_id).await?);
    } else {
        let g = factors::states(conn, row.user_id)
            .await?
            .recovery_codes_generation;
        factors::mark_compromised_tx(
            conn,
            ctx,
            row.user_id,
            FactorRef {
                factor: Factor::RecoveryCodes,
                generation: g,
            },
            StateCause::Admin,
            Actor::System,
        )
        .await?;
    }
    // Other sessions end, except those of active key devices not named lost.
    if !row.lost_device_ids.is_empty() || credentials_changed {
        let sids: Vec<Uuid> = sqlx::query_scalar(
            "SELECT s.id FROM sessions s
              WHERE s.user_id = $1 AND s.ended_at IS NULL
                AND NOT COALESCE(s.authenticator_type = 'key_device' AND s.authenticator_id IN (
                     SELECT d.id FROM approval_devices d
                      WHERE d.user_id = $1 AND d.state = 'active' AND NOT (d.id = ANY($2))), false)",
        )
        .bind(row.user_id)
        .bind(&row.lost_device_ids)
        .fetch_all(&mut *conn)
        .await?;
        out.ended_sessions = crate::auth::revocation::end_sessions_tx(
            conn,
            ctx,
            row.user_id,
            &sids,
            crate::auth::revocation::EndReason::RequestCompleted,
        )
        .await?;
    }
    for f in [Factor::Password, Factor::Totp] {
        factors::clear_throttle_tx(conn, row.user_id, f, ExitPath::CompletedRequest).await?;
    }
    out.event_ids
        .push(request_event(conn, row, EventType::RequestCompleted, Actor::System, None).await?);
    Ok(out)
}

fn approval_to_auth(e: crate::approvals::error::ApprovalError) -> AuthError {
    match e {
        crate::approvals::error::ApprovalError::Auth(a) => a,
        crate::approvals::error::ApprovalError::Database(d) => AuthError::Database(d),
        other => AuthError::Internal(other.to_string()),
    }
}

/// Activate the request's pending key device or passkey.
async fn activate_pending(
    conn: &mut PgConnection,
    user: Uuid,
    a: AuthenticatorRef,
) -> AuthResult<bool> {
    let n = match a.kind {
        AuthenticatorKind::KeyDevice => sqlx::query(
            "UPDATE approval_devices SET state = 'active', activated_at = NOW()
              WHERE id = $1 AND user_id = $2 AND state = 'pending'",
        )
        .bind(a.id)
        .bind(user)
        .execute(&mut *conn)
        .await?
        .rows_affected(),
        AuthenticatorKind::Passkey => sqlx::query(
            "UPDATE passkeys SET state = 'active', ready_at = NULL
              WHERE id = $1 AND user_id = $2 AND state = 'pending'",
        )
        .bind(a.id)
        .bind(user)
        .execute(&mut *conn)
        .await?
        .rows_affected(),
    };
    if n > 0 {
        crate::auth::strong::mark_first_activity(conn, user).await?;
    }
    Ok(n > 0)
}

// ============================================================================
// Timers (B.7 rule 3)
// ============================================================================

/// The minute sweep of the requests (B.7 rule 3).
///
/// 1 h before `ready` a notice; at `ready_at` a `self`
/// request becomes `ready` (Finish window 24 h) and an `admin` request
/// completes; a `ready` request past its window expires. Each request is
/// one transition under its subject's lock.
pub async fn timers(ctx: &AuthCtx<'_>) -> AuthResult<Vec<Uuid>> {
    let due: Vec<Uuid> = sqlx::query_scalar(
        "SELECT id FROM account_requests
          WHERE (state = 'waiting'
                 AND (ready_at <= NOW()
                      OR (NOT ready_soon_sent AND ready_at <= NOW() + make_interval(hours => $1))))
             OR (state = 'ready' AND expires_at <= NOW())
          ORDER BY ready_at LIMIT 200",
    )
    .bind(i32::try_from(READY_SOON_HOURS).unwrap_or(1))
    .fetch_all(ctx.db)
    .await?;
    let mut events = Vec::new();
    for id in due {
        let mut tx = ctx.db.begin().await?;
        let Some(row) = lock_request(&mut tx, id, &[]).await? else {
            continue;
        };
        let now = Utc::now();
        if row.state == "waiting" && row.ready_at <= now {
            if routes_left(&mut tx, &row).await? == 0 {
                events.extend(void_row_tx(&mut tx, ctx, &row, VoidCause::NoRouteLeft).await?);
            } else if row.kind == "admin" {
                // A banned account's admin request waits (Fable m2).
                match complete_locked_tx(&mut tx, ctx, &row).await {
                    Ok(out) => events.extend(out.event_ids),
                    Err(AuthError::AccountBanned) => continue,
                    Err(e) => return Err(e),
                }
            } else {
                sqlx::query(
                    "UPDATE account_requests SET state = 'ready', expires_at = $2 WHERE id = $1",
                )
                .bind(row.id)
                .bind(now + Duration::hours(FINISH_HOURS))
                .execute(&mut *tx)
                .await?;
                events.push(
                    request_event(&mut tx, &row, EventType::RequestReady, Actor::System, None)
                        .await?,
                );
            }
        } else if row.state == "waiting" && !row.ready_soon_sent {
            sqlx::query("UPDATE account_requests SET ready_soon_sent = true WHERE id = $1")
                .bind(row.id)
                .execute(&mut *tx)
                .await?;
            events.push(
                request_event(
                    &mut tx,
                    &row,
                    EventType::RequestReadySoon,
                    Actor::System,
                    None,
                )
                .await?,
            );
        } else if row.state == "ready" && row.expires_at.is_some_and(|t| t <= now) {
            sqlx::query(
                "UPDATE account_requests SET state = 'expired', ended_at = NOW() WHERE id = $1",
            )
            .bind(row.id)
            .execute(&mut *tx)
            .await?;
            void_children(&mut tx, ctx, &row).await?;
            events.push(
                request_event(
                    &mut tx,
                    &row,
                    EventType::RequestExpired,
                    Actor::System,
                    None,
                )
                .await?,
            );
        }
        tx.commit().await?;
    }
    Ok(events)
}

// ============================================================================
// Wire views
// ============================================================================

/// The request client's view.
pub async fn to_wire(conn: &mut PgConnection, row: &RequestRow) -> sqlx::Result<AccountRequest> {
    let authenticator = match row.pending_authenticator() {
        Some(a) => crate::auth::strong::authenticator_label(conn, a)
            .await?
            .map(|label| AuthenticatorLabel {
                kind: a.as_str().into(),
                label,
            }),
        None => None,
    };
    let recovery_contact = match row.staged["recovery_contact_id"].as_str() {
        Some(id) => {
            sqlx::query_scalar("SELECT username FROM users WHERE id = $1::uuid")
                .bind(id)
                .fetch_optional(&mut *conn)
                .await?
        }
        None => None,
    };
    Ok(AccountRequest {
        id: row.id,
        kind: row.kind.clone(),
        purpose: row.purpose.clone(),
        state: row.state.clone(),
        created_at: row.created_at,
        ready_at: row.ready_at,
        expires_at: row.expires_at,
        factors: row.factor_names(),
        lost_key_devices: row.lost_device_ids.len() as i64,
        staged: StagedView {
            authenticator,
            password: row.staged["password_hash"].is_string(),
            totp: row.staged["totp_secret"].is_string(),
            recovery_contact,
        },
    })
}

/// The owner's, contact's or admin's view (`cancel` computed for `reader`).
pub async fn to_summary(
    conn: &mut PgConnection,
    row: &RequestRow,
    reader: Uuid,
) -> sqlx::Result<AccountRequestSummary> {
    let (username, display_name): (String, String) =
        sqlx::query_as("SELECT username, display_name FROM users WHERE id = $1")
            .bind(row.user_id)
            .fetch_one(&mut *conn)
            .await?;
    let started_by = match row.started_by {
        Some(a) => sqlx::query_scalar::<_, String>("SELECT username FROM users WHERE id = $1")
            .bind(a)
            .fetch_optional(&mut *conn)
            .await?
            .map(|u| StartedBy {
                user_id: a,
                username: u,
            }),
        None => None,
    };
    let lost: Vec<(Uuid, String)> =
        sqlx::query_as("SELECT id, label FROM approval_devices WHERE id = ANY($1) ORDER BY id")
            .bind(&row.lost_device_ids)
            .fetch_all(&mut *conn)
            .await?;
    let new_authenticator = match row.pending_authenticator() {
        Some(a) => crate::auth::strong::authenticator_label(conn, a)
            .await?
            .map(|label| AuthenticatorLabel {
                kind: a.as_str().into(),
                label,
            }),
        None => None,
    };
    let cancel = if row.is_open() {
        if reader == row.user_id {
            let strong = crate::auth::strong::active_strong_count(conn, reader).await? > 0;
            Some(CancelVia {
                via: if strong { "step_up" } else { "part2" }.into(),
            })
        } else {
            Some(CancelVia {
                via: "step_up".into(),
            })
        }
    } else {
        None
    };
    Ok(AccountRequestSummary {
        id: row.id,
        subject: RequestSubject {
            user_id: row.user_id,
            username,
            display_name,
        },
        kind: row.kind.clone(),
        purpose: row.purpose.clone(),
        state: row.state.clone(),
        created_at: row.created_at,
        ready_at: row.ready_at,
        started_by,
        lost_devices: lost
            .into_iter()
            .map(|(id, label)| LostDevice { id, label })
            .collect(),
        new_authenticator,
        cancel,
    })
}

/// `ready_at` of a pending authenticator: its open account request's
/// `ready_at` (C26 `KEY_DEVICE_PENDING {ready_at}`, `PASSKEY_PENDING`).
pub async fn pending_ready_at(
    conn: &mut PgConnection,
    a: AuthenticatorRef,
) -> sqlx::Result<Option<DateTime<Utc>>> {
    sqlx::query_scalar(
        "SELECT ready_at FROM account_requests
          WHERE state IN ('waiting', 'ready') AND staged -> 'authenticator' @> $1",
    )
    .bind(serde_json::json!({"type": a.as_str(), "id": a.id}))
    .fetch_optional(&mut *conn)
    .await
}

/// Record the request's staged (pending) authenticator; a previous one is
/// voided (one live child, SM2).
pub async fn stage_authenticator_tx(
    conn: &mut PgConnection,
    request_id: Uuid,
    a: AuthenticatorRef,
) -> sqlx::Result<()> {
    let old: Option<serde_json::Value> =
        sqlx::query_scalar("SELECT staged -> 'authenticator' FROM account_requests WHERE id = $1")
            .bind(request_id)
            .fetch_optional(&mut *conn)
            .await?
            .flatten();
    if let Some(old) = old {
        if let (Some(t), Some(id)) = (old["type"].as_str(), old["id"].as_str()) {
            let id: Option<Uuid> = id.parse().ok();
            if t == "key_device" {
                sqlx::query(
                    "UPDATE approval_devices SET state = 'revoked', revoked_at = NOW(),
                            revoke_reason = 'request'
                      WHERE id = $1 AND state = 'pending'",
                )
                .bind(id)
                .execute(&mut *conn)
                .await?;
            } else {
                sqlx::query("DELETE FROM passkeys WHERE id = $1 AND state = 'pending'")
                    .bind(id)
                    .execute(&mut *conn)
                    .await?;
            }
        }
    }
    sqlx::query(
        "UPDATE account_requests
            SET staged = jsonb_set(staged, '{authenticator}', $2) WHERE id = $1",
    )
    .bind(request_id)
    .bind(serde_json::json!({"type": a.as_str(), "id": a.id}))
    .execute(&mut *conn)
    .await?;
    Ok(())
}

/// Load a request by id (any state).
pub async fn get(conn: &mut PgConnection, id: Uuid) -> sqlx::Result<Option<RequestRow>> {
    by_id(conn, id, false).await
}

/// Lock a request (subject lock first) for a request-client transition.
pub async fn lock_by_secret(
    conn: &mut PgConnection,
    ctx: &AuthCtx<'_>,
    secret: &Secret256,
) -> AuthResult<RequestRow> {
    let seen = by_secret(conn, ctx, secret, false)
        .await?
        .ok_or_else(|| AuthError::NotFound("Unknown request".into()))?;
    lock_request(conn, seen.id, &[])
        .await?
        .ok_or_else(|| AuthError::NotFound("Unknown request".into()))
}

/// F5c: start the `admin` request (SM3) of an existing account at `done`.
///
/// The new device stays pending; the caller holds the subject's lock.
/// `admin_mode` `recovery` → `admin_recovery`.
pub async fn start_admin_tx(
    conn: &mut PgConnection,
    ctx: &AuthCtx<'_>,
    e: &crate::approvals::enrollments::EnrollmentRow,
    admin: Option<Uuid>,
    pending: AuthenticatorRef,
) -> AuthResult<Vec<Uuid>> {
    let purpose = if e.admin_mode.as_deref() == Some("recovery") {
        Purpose::AdminRecovery
    } else {
        Purpose::AdminSetup
    };
    let started = insert_tx(
        conn,
        ctx,
        NewRequest {
            user_id: e.subject_id,
            kind: RequestKind::Admin,
            purpose,
            factors: Vec::new(),
            oidc: false,
            lost_device_ids: e.lost_device_ids.clone(),
            started_by: admin,
            started_by_device: e.issuer_device_id,
            enrollment_id: Some(e.id),
            pending: Some(pending),
            with_secret: false,
        },
    )
    .await?;
    Ok(started.event_ids)
}

/// Lock a `ready` request for Finish: the subject and the staged recovery
/// contact in ascending id order (B.7), then the row. A contact staged
/// concurrently after the unlocked read answers 503 (retry).
pub async fn lock_for_finish(
    conn: &mut PgConnection,
    ctx: &AuthCtx<'_>,
    secret: &Secret256,
) -> AuthResult<RequestRow> {
    let seen = by_secret(conn, ctx, secret, false)
        .await?
        .ok_or_else(|| AuthError::NotFound("Unknown request".into()))?;
    let contact = staged_contact(&seen);
    let row = lock_request(conn, seen.id, contact.as_slice())
        .await?
        .ok_or_else(|| AuthError::NotFound("Unknown request".into()))?;
    if staged_contact(&row) != contact {
        return Err(AuthError::ServiceUnavailable);
    }
    Ok(row)
}

fn staged_contact(row: &RequestRow) -> Option<Uuid> {
    row.staged["recovery_contact_id"]
        .as_str()
        .and_then(|s| s.parse().ok())
}

/// Lock a request by id (subject lock first).
pub async fn lock_by_id(conn: &mut PgConnection, id: Uuid) -> AuthResult<Option<RequestRow>> {
    lock_request(conn, id, &[]).await
}
