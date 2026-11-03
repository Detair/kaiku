//! Admin actions on an account's sign-in (C27 "Admin setup", F6 rev 4).
//!
//! Every change needs a full session and a step-up grant for the action and
//! target. Immediate actions on another admin's account answer 403
//! `ADMIN_TARGET_IS_ADMIN`; setup, recovery and Cancel stay allowed.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgConnection;
use uuid::Uuid;

use crate::api::AppState;
use crate::approvals::transition::{lock_accounts, LockedAccount};
use crate::auth::ctx::AuthCtx;
use crate::auth::error::{AuthError, AuthResult};
use crate::auth::factors::{self, ExitPath, Factor, FactorRef, FactorState, StateCause};
use crate::auth::step_up::{self, StepUpAction, StepUpTarget};
use crate::auth::strong::AuthenticatorRef;
use crate::auth::AuthUser;
use crate::security_events::{record, spawn_delivery, Actor, EventType, NewSecurityEvent};

/// 403 `ADMIN_TARGET_IS_ADMIN` for an immediate action on another admin's
/// account (the admin's own account is allowed).
pub fn ensure_not_admin_target(target: &LockedAccount, admin: Uuid) -> AuthResult<()> {
    if target.is_admin && target.id != admin {
        return Err(AuthError::AdminTargetIsAdmin);
    }
    Ok(())
}

/// Lock the admin and the target (ascending, B.7), refuse an unknown or
/// system target and another admin's account.
async fn lock_target(
    conn: &mut PgConnection,
    admin: Uuid,
    target: Uuid,
) -> AuthResult<LockedAccount> {
    let locked = lock_accounts(conn, &[admin, target]).await?;
    let t = locked
        .get(&target)
        .filter(|a| a.exists && !a.is_system)
        .copied()
        .ok_or_else(|| AuthError::NotFound("User not found".into()))?;
    ensure_not_admin_target(&t, admin)?;
    Ok(t)
}

/// Consume the grant for `action`/`target` and record the step-up action.
async fn consume(
    conn: &mut PgConnection,
    auth: &AuthUser,
    action: StepUpAction,
    target: StepUpTarget,
) -> AuthResult<Uuid> {
    let label = step_up::target_label_tx(conn, auth.id, action, &target).await?;
    let grant = step_up::consume_tx(conn, auth, action, &target).await?;
    Ok(step_up::record_action_tx(conn, auth, &grant, action, &label).await?)
}

/// `PUT …/may-approve` body and answer.
#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct MayApprove {
    pub may_approve: bool,
}

/// `PUT /api/admin/users/{user_id}/may-approve {may_approve}` (full +
/// `admin.may_approve`/user id).
#[utoipa::path(
    put,
    path = "/api/admin/users/{user_id}/may-approve",
    tag = "admin",
    params(("user_id" = Uuid, Path, description = "The account")),
    request_body = MayApprove,
    responses((status = 200, body = MayApprove), (status = 403, description = "ADMIN_TARGET_IS_ADMIN or STEP_UP_REQUIRED")),
    security(("bearer_auth" = [])),
)]
#[tracing::instrument(skip_all, fields(user_id = %auth.id, account = %account))]
pub async fn set_may_approve(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(account): Path<Uuid>,
    Json(body): Json<MayApprove>,
) -> AuthResult<Json<MayApprove>> {
    crate::auth::guards::require_full(&auth)?;
    let mut tx = state.db.begin().await?;
    lock_target(&mut tx, auth.id, account).await?;
    let ev = consume(
        &mut tx,
        &auth,
        StepUpAction::AdminMayApprove,
        StepUpTarget::Id(account),
    )
    .await?;
    sqlx::query("UPDATE users SET may_approve = $2 WHERE id = $1")
        .bind(account)
        .bind(body.may_approve)
        .execute(&mut *tx)
        .await?;
    let changed = record(
        &mut tx,
        NewSecurityEvent::new(
            account,
            EventType::MayApproveChanged,
            Actor::Admin(auth.id),
            serde_json::json!({"may_approve": body.may_approve, "by": "admin"}),
        ),
    )
    .await?;
    crate::permissions::queries::write_audit_log(
        &mut *tx,
        auth.id,
        "admin.users.may_approve",
        Some("user"),
        Some(account),
        Some(serde_json::json!({"may_approve": body.may_approve})),
        None,
    )
    .await
    .map_err(|e| AuthError::Internal(e.to_string()))?;
    tx.commit().await?;
    spawn_delivery(&state, vec![ev, changed]);
    Ok(Json(body))
}

/// `PUT …/factors` body: `"active"` (allow again) or `"compromised"` (mark
/// as unsafe).
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct FactorStatesBody {
    pub password: Option<String>,
    pub totp: Option<String>,
}

/// `PUT …/factors` answer.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct FactorStatesView {
    pub password: Option<String>,
    pub totp: Option<String>,
}

/// `PUT /api/admin/users/{user_id}/factors {password?, totp?}` (full +
/// `admin.factor.set_state`/user id; the B.1 exit table: an admin clears
/// `throttled` only, `compromised` leaves by replacement).
#[utoipa::path(
    put,
    path = "/api/admin/users/{user_id}/factors",
    tag = "admin",
    params(("user_id" = Uuid, Path, description = "The account")),
    request_body = FactorStatesBody,
    responses((status = 200, body = FactorStatesView), (status = 400, description = "VALIDATION_ERROR"), (status = 403, description = "ADMIN_TARGET_IS_ADMIN")),
    security(("bearer_auth" = [])),
)]
#[tracing::instrument(skip_all, fields(user_id = %auth.id, account = %account))]
pub async fn set_factor_state(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(account): Path<Uuid>,
    Json(body): Json<FactorStatesBody>,
) -> AuthResult<Json<FactorStatesView>> {
    crate::auth::guards::require_full(&auth)?;
    let wanted: Vec<(Factor, FactorState)> = [
        (Factor::Password, body.password.as_deref()),
        (Factor::Totp, body.totp.as_deref()),
    ]
    .into_iter()
    .filter_map(|(f, s)| s.map(|s| (f, s)))
    .map(|(f, s)| match s {
        "active" => Ok((f, FactorState::Active)),
        "compromised" => Ok((f, FactorState::Compromised)),
        _ => Err(AuthError::Validation(
            "state must be active or compromised".into(),
        )),
    })
    .collect::<AuthResult<_>>()?;
    if wanted.is_empty() {
        return Err(AuthError::Validation("nothing to change".into()));
    }
    let ctx = AuthCtx::from(&state);
    let mut tx = state.db.begin().await?;
    lock_target(&mut tx, auth.id, account).await?;
    let st = factors::states(&mut tx, account).await?;
    // Validate everything before the grant: a refusal leaves the grant.
    for (f, want) in &wanted {
        let (has, cur) = match f {
            Factor::Password => (st.has_password, st.password.state),
            _ => (st.has_totp, st.totp.state),
        };
        if !has {
            return Err(AuthError::Validation(
                "the account has no such factor".into(),
            ));
        }
        if *want == FactorState::Active && cur == FactorState::Compromised {
            return Err(AuthError::Validation(
                "a compromised factor is replaced by its owner, not allowed again".into(),
            ));
        }
    }
    let mut events = vec![
        consume(
            &mut tx,
            &auth,
            StepUpAction::AdminFactorSetState,
            StepUpTarget::Id(account),
        )
        .await?,
    ];
    for (f, want) in wanted {
        let generation = match f {
            Factor::Password => st.password.generation,
            _ => st.totp.generation,
        };
        if want == FactorState::Compromised {
            factors::mark_compromised_tx(
                &mut tx,
                &ctx,
                account,
                FactorRef {
                    factor: f,
                    generation,
                },
                StateCause::Admin,
                Actor::Admin(auth.id),
            )
            .await?;
        } else if factors::clear_throttle_tx(&mut tx, account, f, ExitPath::Admin).await? {
            events.push(
                record(
                    &mut tx,
                    NewSecurityEvent::new(
                        account,
                        EventType::ThrottleReset,
                        Actor::Admin(auth.id),
                        serde_json::json!({"factor": f.as_str(), "by": "admin"}),
                    ),
                )
                .await?,
            );
        }
    }
    let after = factors::states(&mut tx, account).await?;
    crate::permissions::queries::write_audit_log(
        &mut *tx,
        auth.id,
        "admin.users.factors",
        Some("user"),
        Some(account),
        Some(serde_json::json!({
            "password": after.has_password.then(|| after.password.state.as_str()),
            "totp": after.has_totp.then(|| after.totp.state.as_str()),
        })),
        None,
    )
    .await
    .map_err(|e| AuthError::Internal(e.to_string()))?;
    tx.commit().await?;
    spawn_delivery(&state, events);
    // The `*_compromised` events mark_compromised_tx recorded.
    crate::security_events::notices::spawn_pending_delivery(&state);
    Ok(Json(FactorStatesView {
        password: after
            .has_password
            .then(|| after.password.state.as_str().to_string()),
        totp: after
            .has_totp
            .then(|| after.totp.state.as_str().to_string()),
    }))
}

/// `POST /api/admin/users/{user_id}/authenticators/{id}/remove` → 204
/// ("Remove device now", full + `admin.authenticator.remove`/id).
///
/// No last-strong-authenticator check: the account then recovers through
/// F6. The removed key device gets one last notice before its push rows go.
#[utoipa::path(
    post,
    path = "/api/admin/users/{user_id}/authenticators/{id}/remove",
    tag = "admin",
    params(
        ("user_id" = Uuid, Path, description = "The account"),
        ("id" = Uuid, Path, description = "Key device or passkey id"),
    ),
    responses((status = 204), (status = 403, description = "ADMIN_TARGET_IS_ADMIN"), (status = 404, description = "Unknown authenticator")),
    security(("bearer_auth" = [])),
)]
#[tracing::instrument(skip_all, fields(user_id = %auth.id, account = %account, authenticator = %id))]
pub async fn remove_authenticator_now(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((account, id)): Path<(Uuid, Uuid)>,
) -> AuthResult<StatusCode> {
    use crate::approvals::devices::{after_revoke, revoke_tx, RevokeReason};
    crate::auth::guards::require_full(&auth)?;
    let ctx = AuthCtx::from(&state);
    let mut tx = state.db.begin().await?;
    lock_target(&mut tx, auth.id, account).await?;
    let device: Option<Uuid> = sqlx::query_scalar(
        "SELECT device_id FROM approval_devices
          WHERE id = $1 AND user_id = $2 AND state <> 'revoked'",
    )
    .bind(id)
    .bind(account)
    .fetch_optional(&mut *tx)
    .await?;
    let passkey: Option<String> = if device.is_none() {
        sqlx::query_scalar(
            "SELECT label FROM passkeys WHERE id = $1 AND user_id = $2 AND state <> 'revoked'",
        )
        .bind(id)
        .bind(account)
        .fetch_optional(&mut *tx)
        .await?
    } else {
        None
    };
    if device.is_none() && passkey.is_none() {
        return Err(AuthError::NotFound("authenticator".into()));
    }
    let mut events = vec![
        consume(
            &mut tx,
            &auth,
            StepUpAction::AdminAuthenticatorRemove,
            StepUpTarget::Id(id),
        )
        .await?,
    ];
    if device.is_some() {
        let (_, ev) = revoke_tx(
            &mut tx,
            &ctx,
            id,
            RevokeReason::Admin,
            Actor::Admin(auth.id),
        )
        .await
        .map_err(|e| match e {
            crate::approvals::error::ApprovalError::Auth(a) => a,
            crate::approvals::error::ApprovalError::Database(d) => AuthError::Database(d),
            other => AuthError::Internal(other.to_string()),
        })?;
        events.extend(ev);
    } else {
        sqlx::query(
            "UPDATE passkeys SET state = 'revoked', revoked_at = NOW(), revoke_reason = 'admin'
              WHERE id = $1 AND user_id = $2 AND state <> 'revoked'",
        )
        .bind(id)
        .bind(account)
        .execute(&mut *tx)
        .await?;
        crate::approvals::cascade::on_authenticator_revoked(
            &mut tx,
            &ctx,
            account,
            AuthenticatorRef::passkey(id),
        )
        .await?;
        let mut removed = NewSecurityEvent::new(
            account,
            EventType::AuthenticatorRemoved,
            Actor::Admin(auth.id),
            serde_json::json!({"type": "passkey", "label": passkey, "by": "admin"}),
        );
        removed.authenticator = Some(AuthenticatorRef::passkey(id));
        events.push(record(&mut tx, removed).await?);
    }
    crate::permissions::queries::write_audit_log(
        &mut *tx,
        auth.id,
        "admin.users.authenticator_remove",
        Some("user"),
        Some(account),
        Some(serde_json::json!({"authenticator_id": id})),
        None,
    )
    .await
    .map_err(|e| AuthError::Internal(e.to_string()))?;
    tx.commit().await?;
    if let Some(install) = device {
        // The removed phone gets its last notice before its push rows go.
        after_revoke(&state, account, install, events).await;
    } else {
        spawn_delivery(&state, events);
        crate::approvals::devices::notify_authenticators_changed(&state, account).await;
    }
    Ok(StatusCode::NO_CONTENT)
}

/// An authenticator in the admin's view of a user.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct AdminAuthenticator {
    pub id: Uuid,
    pub label: String,
    /// `pending` or `active`.
    pub state: String,
    pub ready_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub last_used_at: Option<DateTime<Utc>>,
}

/// `open_sessions` in the admin's view.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct OpenSessions {
    pub full: i64,
    pub probation: i64,
}

/// The sign-in part of the admin's user view (C27, F8 "Admins see per
/// user").
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct UserSecurityView {
    pub is_admin: bool,
    pub may_approve: bool,
    pub key_devices: Vec<AdminAuthenticator>,
    pub passkeys: Vec<AdminAuthenticator>,
    pub password_state: Option<String>,
    pub totp_state: Option<String>,
    pub recovery_codes_left: i64,
    pub bootstrap_open: bool,
    pub recovery_contact: Option<crate::auth::recovery_contact::RecoveryContact>,
    pub open_request: Option<crate::auth::account_requests::types::AccountRequestSummary>,
    pub open_sessions: OpenSessions,
}

type AuthenticatorRow = (
    Uuid,
    String,
    String,
    Option<DateTime<Utc>>,
    DateTime<Utc>,
    Option<DateTime<Utc>>,
);

fn to_admin(rows: Vec<AuthenticatorRow>) -> Vec<AdminAuthenticator> {
    rows.into_iter()
        .map(
            |(id, label, state, ready_at, created_at, last_used_at)| AdminAuthenticator {
                id,
                label,
                state,
                ready_at,
                created_at,
                last_used_at,
            },
        )
        .collect()
}

/// Build the view for `user` as `reader` sees it.
pub async fn security_view(
    conn: &mut PgConnection,
    ctx: &AuthCtx<'_>,
    user: Uuid,
    reader: Uuid,
) -> AuthResult<UserSecurityView> {
    let (is_admin, may_approve): (bool, bool) = sqlx::query_as(
        "SELECT EXISTS (SELECT 1 FROM system_admins WHERE user_id = u.id), u.may_approve
           FROM users u WHERE u.id = $1",
    )
    .bind(user)
    .fetch_one(&mut *conn)
    .await?;
    let key_devices: Vec<AuthenticatorRow> = sqlx::query_as(
        "SELECT d.id, d.label, d.state,
                (SELECT r.ready_at FROM account_requests r
                  WHERE r.state IN ('waiting', 'ready')
                    AND r.staged -> 'authenticator' @> jsonb_build_object('type', 'key_device', 'id', d.id)),
                d.created_at, d.last_used_at
           FROM approval_devices d
          WHERE d.user_id = $1 AND d.state <> 'revoked'
          ORDER BY d.created_at, d.id",
    )
    .bind(user)
    .fetch_all(&mut *conn)
    .await?;
    let passkeys: Vec<AuthenticatorRow> = sqlx::query_as(
        "SELECT id, label, state, ready_at, created_at, last_used_at
           FROM passkeys WHERE user_id = $1 AND state <> 'revoked'
          ORDER BY created_at, id",
    )
    .bind(user)
    .fetch_all(&mut *conn)
    .await?;
    let st = factors::states(conn, user).await?;
    let recovery_codes_left = crate::auth::recovery_codes::left_conn(conn, user).await?;
    let recovery_contact = crate::auth::recovery_contact::current(conn, ctx, user).await?;
    let open: Option<Uuid> = sqlx::query_scalar(
        "SELECT id FROM account_requests
          WHERE user_id = $1 AND state IN ('waiting', 'ready')
          ORDER BY created_at DESC LIMIT 1",
    )
    .bind(user)
    .fetch_optional(&mut *conn)
    .await?;
    let open_request = match open {
        Some(id) => match crate::auth::account_requests::core::get(conn, id).await? {
            Some(row) => {
                Some(crate::auth::account_requests::core::to_summary(conn, &row, reader).await?)
            }
            None => None,
        },
        None => None,
    };
    let (full, probation): (i64, i64) = sqlx::query_as(
        "SELECT COUNT(*) FILTER (WHERE assurance = 'full'),
                COUNT(*) FILTER (WHERE assurance <> 'full')
           FROM sessions
          WHERE user_id = $1 AND ended_at IS NULL AND expires_at > NOW()",
    )
    .bind(user)
    .fetch_one(&mut *conn)
    .await?;
    Ok(UserSecurityView {
        is_admin,
        may_approve,
        key_devices: to_admin(key_devices),
        passkeys: to_admin(passkeys),
        password_state: st
            .has_password
            .then(|| st.password.state.as_str().to_string()),
        totp_state: st.has_totp.then(|| st.totp.state.as_str().to_string()),
        recovery_codes_left,
        bootstrap_open: st.bootstrap_open,
        recovery_contact,
        open_request,
        open_sessions: OpenSessions { full, probation },
    })
}
