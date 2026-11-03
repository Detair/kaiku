//! The recovery contact (spec Part C, SM3 rev 5; C28 "Recovery contact").
//!
//! Another member of the server, never an admin, the account itself or the
//! system account, named by the account (full + `recovery_contact.set`) or by
//! an admin for a non-admin account (`admin.recovery_contact.set`). The
//! contact confirms the role once from a full session; a new contact needs a
//! new confirmation. A confirmed contact receives the account's
//! high-severity notices (without `ip`/`place`) and may cancel its account
//! requests; it counts as an independent canceller only for requests that
//! start at least one delay period after the confirmation (`counts_from`).

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgConnection;
use uuid::Uuid;

use super::ctx::AuthCtx;
use super::error::{AuthError, AuthResult};
use super::middleware::AuthUser;
use super::step_up::{self, StepUpAction, StepUpTarget};
use crate::api::AppState;
use crate::approvals::transition::lock_accounts;
use crate::security_events::{record, spawn_delivery, Actor, EventType, NewSecurityEvent};

/// `RecoveryContact` (C28).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, utoipa::ToSchema)]
pub struct RecoveryContact {
    pub user_id: Uuid,
    pub username: String,
    pub display_name: String,
    pub named_at: DateTime<Utc>,
    pub confirmed_at: Option<DateTime<Utc>>,
    /// `confirmed_at` + the request delay: requests that start earlier do
    /// not record the contact as a canceller.
    pub counts_from: Option<DateTime<Utc>>,
}

/// The account's contact, if any.
pub async fn current(
    conn: &mut PgConnection,
    ctx: &AuthCtx<'_>,
    account: Uuid,
) -> sqlx::Result<Option<RecoveryContact>> {
    let row: Option<(
        Uuid,
        String,
        String,
        Option<DateTime<Utc>>,
        Option<DateTime<Utc>>,
    )> = sqlx::query_as(
        "SELECT c.id, c.username, c.display_name, u.recovery_contact_named_at,
                    u.recovery_contact_confirmed_at
               FROM users u JOIN users c ON c.id = u.recovery_contact_id
              WHERE u.id = $1",
    )
    .bind(account)
    .fetch_optional(&mut *conn)
    .await?;
    Ok(row.map(
        |(user_id, username, display_name, named_at, confirmed_at)| RecoveryContact {
            user_id,
            username,
            display_name,
            named_at: named_at.unwrap_or_else(Utc::now),
            confirmed_at,
            counts_from: confirmed_at
                .map(|t| t + Duration::hours(ctx.config.account_request_delay_hours)),
        },
    ))
}

/// Who named or confirmed (for the events and the admin rule).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContactActor {
    /// The account itself.
    Owner,
    /// An admin (only for non-admin accounts).
    Admin(Uuid),
    /// The host CLI.
    Cli,
}

/// Name (or, with `None`, remove) the account's contact.
///
/// Under the account locks (B.7 rule 1). An admin, the account itself, the
/// system account or an unknown account → 400 `RECOVERY_CONTACT_INVALID`. A
/// new contact clears the confirmation. Records `recovery_contact_named`
/// (first contact) or `recovery_contact_changed`; returns the contact and
/// the event ids.
#[tracing::instrument(skip(conn, ctx))]
pub async fn set_tx(
    conn: &mut PgConnection,
    ctx: &AuthCtx<'_>,
    account: Uuid,
    contact: Option<Uuid>,
    actor: ContactActor,
) -> AuthResult<(Option<RecoveryContact>, Vec<Uuid>)> {
    let mut ids = vec![account];
    ids.extend(contact);
    if let ContactActor::Admin(a) = actor {
        ids.push(a);
    }
    let locked = lock_accounts(conn, &ids).await?;
    // Codex M8 (Flash r5): an admin actor is still an unbanned admin under
    // the lock.
    if let ContactActor::Admin(a) = actor {
        if !locked.get(&a).is_some_and(|x| x.is_admin && !x.banned) {
            return Err(AuthError::NotAdmin);
        }
    }
    // F6 rev 4: no immediate actions on another admin's account.
    if matches!(actor, ContactActor::Admin(_)) && locked.get(&account).is_some_and(|a| a.is_admin) {
        return Err(AuthError::AdminTargetIsAdmin);
    }
    if let Some(c) = contact {
        let ok = c != account
            && locked
                .get(&c)
                .is_some_and(|a| a.exists && !a.is_system && !a.is_admin);
        if !ok {
            return Err(AuthError::RecoveryContactInvalid);
        }
    }
    let previous: Option<Uuid> =
        sqlx::query_scalar("SELECT recovery_contact_id FROM users WHERE id = $1")
            .bind(account)
            .fetch_one(&mut *conn)
            .await?;
    if previous == contact {
        return Ok((current(conn, ctx, account).await?, Vec::new()));
    }
    sqlx::query(
        "UPDATE users SET recovery_contact_id = $2,
                recovery_contact_named_at = CASE WHEN $2::uuid IS NULL THEN NULL ELSE NOW() END,
                recovery_contact_confirmed_at = NULL
          WHERE id = $1",
    )
    .bind(account)
    .bind(contact)
    .execute(&mut *conn)
    .await?;
    let event_type = if previous.is_none() {
        EventType::RecoveryContactNamed
    } else {
        EventType::RecoveryContactChanged
    };
    let ev_actor = match actor {
        ContactActor::Owner => Actor::User(account),
        ContactActor::Admin(a) => Actor::Admin(a),
        ContactActor::Cli => Actor::Cli,
    };
    let event = record(
        conn,
        NewSecurityEvent::new(
            account,
            event_type,
            ev_actor,
            serde_json::json!({ "contact_user_id": contact, "previous_contact_user_id": previous }),
        ),
    )
    .await?;
    // A changed contact affects the cancellers of open requests (Task 21).
    crate::approvals::cascade::on_contact_changed(conn, ctx, account).await?;
    Ok((current(conn, ctx, account).await?, vec![event]))
}

/// The contact confirms the role ("I will help <name>"). Only the named
/// contact; records `recovery_contact_confirmed`.
#[tracing::instrument(skip(conn, _ctx))]
pub async fn confirm_tx(
    conn: &mut PgConnection,
    _ctx: &AuthCtx<'_>,
    contact: Uuid,
    account: Uuid,
) -> AuthResult<Vec<Uuid>> {
    let locked = lock_accounts(conn, &[contact, account]).await?;
    // Fable m9: a contact who became an admin is no contact (C28).
    if locked.get(&contact).is_some_and(|c| c.is_admin) {
        return Err(AuthError::RecoveryContactInvalid);
    }
    let updated = sqlx::query(
        "UPDATE users SET recovery_contact_confirmed_at = NOW()
          WHERE id = $1 AND recovery_contact_id = $2 AND recovery_contact_confirmed_at IS NULL",
    )
    .bind(account)
    .bind(contact)
    .execute(&mut *conn)
    .await?
    .rows_affected();
    if updated == 0 {
        let named: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM users WHERE id = $1 AND recovery_contact_id = $2)",
        )
        .bind(account)
        .bind(contact)
        .fetch_one(&mut *conn)
        .await?;
        return if named {
            Ok(Vec::new())
        } else {
            Err(AuthError::NotFound(
                "Not a recovery contact of this account".into(),
            ))
        };
    }
    let event = record(
        conn,
        NewSecurityEvent::new(
            account,
            EventType::RecoveryContactConfirmed,
            Actor::RecoveryContact(contact),
            serde_json::json!({ "contact_user_id": contact }),
        ),
    )
    .await?;
    Ok(vec![event])
}

// ============================================================================
// Routes
// ============================================================================

/// `PUT /api/me/recovery-contact` body.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct SetContactRequest {
    pub user_id: Uuid,
}

/// Name the contact: full + `recovery_contact.set`/`user_id` → 200
/// `RecoveryContact`.
#[utoipa::path(
    put,
    path = "/api/me/recovery-contact",
    tag = "auth",
    request_body = SetContactRequest,
    responses(
        (status = 200, body = RecoveryContact),
        (status = 400, description = "RECOVERY_CONTACT_INVALID"),
        (status = 403, description = "FULL_SESSION_REQUIRED or STEP_UP_REQUIRED"),
    ),
    security(("bearer_auth" = [])),
)]
#[tracing::instrument(skip_all, fields(user_id = %auth.id))]
pub async fn put_own(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(body): Json<SetContactRequest>,
) -> AuthResult<Json<RecoveryContact>> {
    super::guards::require_full(&auth)?;
    let ctx = AuthCtx::from(&state);
    let target = StepUpTarget::Id(body.user_id);
    let mut tx = state.db.begin().await?;
    let (contact, mut events) = set_tx(
        &mut tx,
        &ctx,
        auth.id,
        Some(body.user_id),
        ContactActor::Owner,
    )
    .await?;
    let label =
        step_up::target_label_tx(&mut tx, auth.id, StepUpAction::RecoveryContactSet, &target)
            .await?;
    let grant =
        step_up::consume_tx(&mut tx, &auth, StepUpAction::RecoveryContactSet, &target).await?;
    events.push(
        step_up::record_action_tx(
            &mut tx,
            &auth,
            &grant,
            StepUpAction::RecoveryContactSet,
            &label,
        )
        .await?,
    );
    tx.commit().await?;
    spawn_delivery(&state, events);
    crate::security_events::notices::spawn_pending_delivery(&state);
    contact.map(Json).ok_or(AuthError::RecoveryContactInvalid)
}

/// Remove the contact: full + `recovery_contact.set`/`""` → 204.
#[utoipa::path(
    delete,
    path = "/api/me/recovery-contact",
    tag = "auth",
    responses((status = 204), (status = 403, description = "FULL_SESSION_REQUIRED or STEP_UP_REQUIRED")),
    security(("bearer_auth" = [])),
)]
#[tracing::instrument(skip_all, fields(user_id = %auth.id))]
pub async fn delete_own(State(state): State<AppState>, auth: AuthUser) -> AuthResult<StatusCode> {
    super::guards::require_full(&auth)?;
    let ctx = AuthCtx::from(&state);
    let mut tx = state.db.begin().await?;
    let (_, mut events) = set_tx(&mut tx, &ctx, auth.id, None, ContactActor::Owner).await?;
    let grant = step_up::consume_tx(
        &mut tx,
        &auth,
        StepUpAction::RecoveryContactSet,
        &StepUpTarget::None,
    )
    .await?;
    events.push(
        step_up::record_action_tx(&mut tx, &auth, &grant, StepUpAction::RecoveryContactSet, "")
            .await?,
    );
    tx.commit().await?;
    spawn_delivery(&state, events);
    crate::security_events::notices::spawn_pending_delivery(&state);
    Ok(StatusCode::NO_CONTENT)
}

/// One account the caller is the contact of.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct ContactFor {
    pub user_id: Uuid,
    pub username: String,
    pub display_name: String,
    pub named_at: DateTime<Utc>,
    pub confirmed_at: Option<DateTime<Utc>>,
}

/// `GET /api/me/recovery-contact-for` answer.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct ContactForList {
    pub accounts: Vec<ContactFor>,
}

/// The accounts that named the caller (any session).
#[utoipa::path(
    get,
    path = "/api/me/recovery-contact-for",
    tag = "auth",
    responses((status = 200, body = ContactForList)),
    security(("bearer_auth" = [])),
)]
#[tracing::instrument(skip_all, fields(user_id = %auth.id))]
pub async fn list_for(
    State(state): State<AppState>,
    auth: AuthUser,
) -> AuthResult<Json<ContactForList>> {
    let rows: Vec<(
        Uuid,
        String,
        String,
        Option<DateTime<Utc>>,
        Option<DateTime<Utc>>,
    )> = sqlx::query_as(
        "SELECT id, username, display_name, recovery_contact_named_at,
                    recovery_contact_confirmed_at
               FROM users WHERE recovery_contact_id = $1 ORDER BY username",
    )
    .bind(auth.id)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(ContactForList {
        accounts: rows
            .into_iter()
            .map(
                |(user_id, username, display_name, named_at, confirmed_at)| ContactFor {
                    user_id,
                    username,
                    display_name,
                    named_at: named_at.unwrap_or_else(Utc::now),
                    confirmed_at,
                },
            )
            .collect(),
    }))
}

/// The contact confirms (full, no step-up) → 204.
#[utoipa::path(
    post,
    path = "/api/me/recovery-contact-for/{user_id}/confirm",
    tag = "auth",
    params(("user_id" = Uuid, Path, description = "The account that named the caller")),
    responses((status = 204), (status = 403, description = "FULL_SESSION_REQUIRED"), (status = 404)),
    security(("bearer_auth" = [])),
)]
#[tracing::instrument(skip_all, fields(user_id = %auth.id, account = %account))]
pub async fn confirm(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(account): Path<Uuid>,
) -> AuthResult<StatusCode> {
    super::guards::require_full(&auth)?;
    let ctx = AuthCtx::from(&state);
    let mut tx = state.db.begin().await?;
    let events = confirm_tx(&mut tx, &ctx, auth.id, account).await?;
    tx.commit().await?;
    spawn_delivery(&state, events);
    Ok(StatusCode::NO_CONTENT)
}

/// An admin names the contact of a non-admin account (C28):
/// `admin.recovery_contact.set`/`user_id`; an admin target → 403
/// `ADMIN_TARGET_IS_ADMIN`.
#[utoipa::path(
    put,
    path = "/api/admin/users/{user_id}/recovery-contact",
    tag = "admin",
    params(("user_id" = Uuid, Path, description = "The account")),
    request_body = SetContactRequest,
    responses(
        (status = 200, body = RecoveryContact),
        (status = 400, description = "RECOVERY_CONTACT_INVALID"),
        (status = 403, description = "ADMIN_TARGET_IS_ADMIN or STEP_UP_REQUIRED"),
    ),
    security(("bearer_auth" = [])),
)]
#[tracing::instrument(skip_all, fields(user_id = %auth.id, account = %account))]
pub async fn put_admin(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(account): Path<Uuid>,
    Json(body): Json<SetContactRequest>,
) -> AuthResult<Json<RecoveryContact>> {
    super::guards::require_full(&auth)?;
    let ctx = AuthCtx::from(&state);
    let target = StepUpTarget::Id(account);
    let mut tx = state.db.begin().await?;
    let (contact, mut events) = set_tx(
        &mut tx,
        &ctx,
        account,
        Some(body.user_id),
        ContactActor::Admin(auth.id),
    )
    .await?;
    let label = step_up::target_label_tx(
        &mut tx,
        auth.id,
        StepUpAction::AdminRecoveryContactSet,
        &target,
    )
    .await?;
    let grant = step_up::consume_tx(
        &mut tx,
        &auth,
        StepUpAction::AdminRecoveryContactSet,
        &target,
    )
    .await?;
    events.push(
        step_up::record_action_tx(
            &mut tx,
            &auth,
            &grant,
            StepUpAction::AdminRecoveryContactSet,
            &label,
        )
        .await?,
    );
    tx.commit().await?;
    spawn_delivery(&state, events);
    contact.map(Json).ok_or(AuthError::RecoveryContactInvalid)
}

/// `/api/me/recovery-contact*` (bearer).
pub fn router() -> axum::Router<AppState> {
    use axum::routing::{get, post, put};
    axum::Router::new()
        .route("/recovery-contact", put(put_own).delete(delete_own))
        .route("/recovery-contact-for", get(list_for))
        .route("/recovery-contact-for/{user_id}/confirm", post(confirm))
}
