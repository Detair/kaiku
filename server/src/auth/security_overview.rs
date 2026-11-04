//! `GET /api/me/security` (C27 "Security overview and migration prompts",
//! F8) and the 30-day `bootstrap_reminder` (C28).

use axum::extract::State;
use axum::Json;
use serde::Serialize;
use sqlx::PgConnection;
use uuid::Uuid;

use super::ctx::AuthCtx;
use super::error::AuthResult;
use super::factors::{self, FactorState};
use super::AuthUser;
use crate::api::AppState;

/// `session` of the overview.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct OverviewSession {
    pub assurance: String,
    pub auth_methods: Vec<String>,
}

/// `{state}` of a password or TOTP.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct FactorView {
    /// `active`, `throttled` or `compromised`.
    pub state: String,
}

/// `GET /api/me/security` answer.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct SecurityOverview {
    pub session: OverviewSession,
    pub password: Option<FactorView>,
    pub totp: Option<FactorView>,
    pub recovery_codes_left: i64,
    pub key_devices: i64,
    pub passkeys: i64,
    pub may_approve: bool,
    pub bootstrap_open: bool,
    pub recovery_contact: Option<super::recovery_contact::RecoveryContact>,
    pub open_request: Option<super::account_requests::types::AccountRequestSummary>,
    /// Clients ignore prompts they do not know.
    pub prompts: Vec<String>,
}

/// Build the overview of `auth`'s account.
pub async fn overview(
    conn: &mut PgConnection,
    ctx: &AuthCtx<'_>,
    auth: &AuthUser,
) -> AuthResult<SecurityOverview> {
    let user = auth.id;
    let st = factors::states(conn, user).await?;
    let (key_devices, passkeys, may_approve): (i64, i64, bool) = sqlx::query_as(
        "SELECT (SELECT COUNT(*) FROM approval_devices WHERE user_id = $1 AND state = 'active'),
                (SELECT COUNT(*) FROM passkeys WHERE user_id = $1 AND state = 'active'),
                may_approve
           FROM users WHERE id = $1",
    )
    .bind(user)
    .fetch_one(&mut *conn)
    .await?;
    let recovery_codes_left = super::recovery_codes::left_conn(conn, user).await?;
    let recovery_contact = super::recovery_contact::current(conn, ctx, user).await?;
    let open: Option<Uuid> = sqlx::query_scalar(
        "SELECT id FROM account_requests
          WHERE user_id = $1 AND state IN ('waiting', 'ready')
          ORDER BY created_at DESC LIMIT 1",
    )
    .bind(user)
    .fetch_optional(&mut *conn)
    .await?;
    let open_request = match open {
        Some(id) => match super::account_requests::core::get(conn, id).await? {
            Some(row) => Some(super::account_requests::core::to_summary(conn, &row, user).await?),
            None => None,
        },
        None => None,
    };
    let session = auth.session.as_ref().map_or_else(
        || OverviewSession {
            assurance: "probation".into(),
            auth_methods: vec!["legacy".into()],
        },
        |s| OverviewSession {
            assurance: s.assurance.as_str().into(),
            auth_methods: s.auth_methods.clone(),
        },
    );
    let probation = session.assurance != "full";
    let totp_state = st.has_totp.then_some(st.totp.state);
    let mut prompts = Vec::new();
    let mut push = |p: &str, on: bool| {
        if on {
            prompts.push(p.to_string());
        }
    };
    push("sign_in_with_passkey", probation && passkeys > 0);
    push("add_passkey", passkeys == 0);
    push("set_up_phone", key_devices == 0);
    push("secure_my_account", st.bootstrap_open);
    push(
        "name_recovery_contact",
        recovery_contact
            .as_ref()
            .is_none_or(|c| c.confirmed_at.is_none()),
    );
    push(
        "renew_totp",
        totp_state.is_some_and(|s| s == FactorState::Compromised),
    );
    push("renew_recovery_codes", recovery_codes_left == 0);
    push("open_request", open_request.is_some());
    Ok(SecurityOverview {
        session,
        password: st.has_password.then(|| FactorView {
            state: st.password.state.as_str().into(),
        }),
        totp: totp_state.map(|s| FactorView {
            state: s.as_str().into(),
        }),
        recovery_codes_left,
        key_devices,
        passkeys,
        may_approve,
        bootstrap_open: st.bootstrap_open,
        recovery_contact,
        open_request,
        prompts,
    })
}

/// `GET /api/me/security` (any session).
#[utoipa::path(
    get,
    path = "/api/me/security",
    tag = "auth",
    responses((status = 200, body = SecurityOverview)),
    security(("bearer_auth" = [])),
)]
#[tracing::instrument(skip_all, fields(user_id = %auth.id))]
pub async fn get_security(
    State(state): State<AppState>,
    auth: AuthUser,
) -> AuthResult<Json<SecurityOverview>> {
    let ctx = AuthCtx::from(&state);
    let mut conn = state.db.acquire().await?;
    Ok(Json(overview(&mut conn, &ctx, &auth).await?))
}

/// Days between two `bootstrap_reminder`s.
pub const BOOTSTRAP_REMINDER_DAYS: i32 = 30;

/// The minute sweep: a `bootstrap_reminder` to accounts with `bootstrap_open`
/// whose last reminder (or creation) is 30 days old.
pub async fn bootstrap_reminders(ctx: &AuthCtx<'_>) -> AuthResult<Vec<Uuid>> {
    use crate::security_events::{record, Actor, EventType, NewSecurityEvent};
    let due: Vec<Uuid> = sqlx::query_scalar(
        "SELECT id FROM users
          WHERE bootstrap_open AND NOT is_system
            AND COALESCE(bootstrap_reminded_at, created_at)
                <= NOW() - make_interval(days => $1)
          ORDER BY id LIMIT 200",
    )
    .bind(BOOTSTRAP_REMINDER_DAYS)
    .fetch_all(ctx.db)
    .await?;
    let mut events = Vec::with_capacity(due.len());
    for user in due {
        let mut tx = ctx.db.begin().await?;
        crate::approvals::transition::lock_accounts(&mut tx, &[user]).await?;
        let n = sqlx::query(
            "UPDATE users SET bootstrap_reminded_at = NOW()
              WHERE id = $1 AND bootstrap_open
                AND COALESCE(bootstrap_reminded_at, created_at)
                    <= NOW() - make_interval(days => $2)",
        )
        .bind(user)
        .bind(BOOTSTRAP_REMINDER_DAYS)
        .execute(&mut *tx)
        .await?
        .rows_affected();
        if n == 1 {
            events.push(
                record(
                    &mut tx,
                    NewSecurityEvent::new(
                        user,
                        EventType::BootstrapReminder,
                        Actor::System,
                        serde_json::json!({}),
                    ),
                )
                .await?,
            );
        }
        tx.commit().await?;
    }
    Ok(events)
}
