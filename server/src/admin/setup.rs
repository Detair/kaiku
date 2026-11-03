//! Admin setup (F5c; C27 "Admin setup"): an admin sets up a member's phone
//! from their own key device.
//!
//! The setup code is typed on the admin's phone as an `enroll` approval
//! (claimant byte 1 or 2). At `done` a new account (B.0) is active at once;
//! an existing account gets an `admin` request (SM3) with the new phone
//! pending.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use serde::Deserialize;
use uuid::Uuid;

use crate::api::AppState;
use crate::approvals::context::ClaimantCheck;
use crate::approvals::devices::key_device_session;
use crate::approvals::enrollments::{
    issue_tx, EnrollmentIssued, EnrollmentVariant, IssueParams, Issuer,
};
use crate::approvals::error::ApprovalError;
use crate::approvals::transition::lock_accounts;
use crate::auth::ctx::AuthCtx;
use crate::auth::error::AuthError;
use crate::auth::step_up::{self, StepUpAction, StepUpTarget};
use crate::auth::AuthUser;
use crate::security_events::spawn_delivery;

/// `claimant_check` of an admin setup.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct ClaimantBody {
    /// `in_person` or `called_back`.
    pub method: String,
    /// How the admin knows it is the member (1..500 characters).
    pub note: String,
}

/// `POST /api/admin/users/{user_id}/enrollments` body.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct AdminEnrollmentRequest {
    /// `setup` or `recovery`.
    pub mode: String,
    pub claimant_check: ClaimantBody,
    /// Active key devices of the account to revoke at completion.
    #[serde(default)]
    pub lost_device_ids: Vec<Uuid>,
}

const fn auth(e: AuthError) -> ApprovalError {
    ApprovalError::Auth(e)
}

/// Issue an `admin` enrolment (F5c) → 201 `EnrollmentIssued`.
///
/// From the admin's own key device (else 403 `KEY_DEVICE_REQUIRED`), full +
/// `admin.enrollment.issue`/user id per code. Refusals: 400
/// `ENROLLMENT_TARGET_SELF`; for an existing account 409
/// `ACCOUNT_REQUEST_OPEN` and 409 `NO_INDEPENDENT_CANCELLER` (checked again
/// at `done`).
#[utoipa::path(
    post,
    path = "/api/admin/users/{user_id}/enrollments",
    tag = "admin",
    params(("user_id" = Uuid, Path, description = "The account")),
    request_body = AdminEnrollmentRequest,
    responses(
        (status = 201, body = EnrollmentIssued),
        (status = 400, description = "ENROLLMENT_TARGET_SELF or VALIDATION_ERROR"),
        (status = 403, description = "KEY_DEVICE_REQUIRED or STEP_UP_REQUIRED"),
        (status = 409, description = "ACCOUNT_REQUEST_OPEN or NO_INDEPENDENT_CANCELLER"),
    ),
    security(("bearer_auth" = [])),
)]
#[tracing::instrument(skip_all, fields(user_id = %admin.id, account = %account))]
pub async fn issue_admin_enrollment(
    State(state): State<AppState>,
    admin: AuthUser,
    Path(account): Path<Uuid>,
    Json(body): Json<AdminEnrollmentRequest>,
) -> Result<(StatusCode, Json<EnrollmentIssued>), ApprovalError> {
    crate::approvals::devices::require_key_device_snapshot(&admin)?;
    crate::auth::guards::require_full(&admin)?;
    if !matches!(body.mode.as_str(), "setup" | "recovery") {
        return Err(ApprovalError::Validation("mode: setup or recovery".into()));
    }
    let claimant = match ClaimantCheck::parse(&body.claimant_check.method) {
        Some(c @ (ClaimantCheck::InPerson | ClaimantCheck::CalledBack)) => c,
        _ => {
            return Err(ApprovalError::Validation(
                "claimant_check.method: in_person or called_back".into(),
            ))
        }
    };
    let note = body.claimant_check.note.trim();
    let n = note.chars().count();
    if n == 0 || n > 500 || note.chars().any(|c| c.is_control() && c != '\n') {
        return Err(ApprovalError::Validation(
            "claimant_check.note: 1 to 500 characters".into(),
        ));
    }
    if account == admin.id {
        return Err(auth(AuthError::EnrollmentTargetSelf));
    }
    let sid = admin.session_id.ok_or(ApprovalError::KeyDeviceRequired)?;
    let ctx = AuthCtx::from(&state);
    let mut tx = state.db.begin().await?;
    let locked = lock_accounts(&mut tx, &[admin.id, account]).await?;
    let target = locked
        .get(&account)
        .filter(|a| a.exists && !a.is_system)
        .ok_or(ApprovalError::NotFound)?;
    if target.banned {
        return Err(ApprovalError::AccountBanned);
    }
    let dev = key_device_session(&mut tx, &admin).await?;
    let existing: bool =
        sqlx::query_scalar("SELECT first_activity_at IS NOT NULL FROM users WHERE id = $1")
            .bind(account)
            .fetch_one(&mut *tx)
            .await?;
    if existing {
        if crate::auth::account_requests::core::open_of(&mut tx, account)
            .await?
            .is_some()
        {
            return Err(auth(AuthError::AccountRequestOpen));
        }
        let routes = crate::auth::account_requests::core::independent_cancellers(
            &mut tx,
            &ctx,
            account,
            Some(admin.id),
            chrono::Utc::now(),
        )
        .await?;
        if routes.is_empty() {
            return Err(auth(AuthError::NoIndependentCanceller));
        }
    }
    // Fable m4: F5c names lost devices only in a recovery.
    if body.mode != "recovery" && !body.lost_device_ids.is_empty() {
        return Err(ApprovalError::Validation(
            "lost_device_ids: only in recovery mode".into(),
        ));
    }
    let mut lost = body.lost_device_ids.clone();
    lost.sort_unstable();
    lost.dedup();
    if !lost.is_empty() {
        let active: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM approval_devices
              WHERE user_id = $1 AND state = 'active' AND id = ANY($2)",
        )
        .bind(account)
        .bind(&lost)
        .fetch_one(&mut *tx)
        .await?;
        if usize::try_from(active).ok() != Some(lost.len()) {
            return Err(ApprovalError::Validation(
                "lost_device_ids: active key devices of the account".into(),
            ));
        }
    }
    let target_ref = StepUpTarget::Id(account);
    let label = step_up::target_label_tx(
        &mut tx,
        admin.id,
        StepUpAction::AdminEnrollmentIssue,
        &target_ref,
    )
    .await?;
    let grant = step_up::consume_tx(
        &mut tx,
        &admin,
        StepUpAction::AdminEnrollmentIssue,
        &target_ref,
    )
    .await?;
    let ev = step_up::record_action_tx(
        &mut tx,
        &admin,
        &grant,
        StepUpAction::AdminEnrollmentIssue,
        &label,
    )
    .await?;
    let issued = issue_tx(
        &mut tx,
        &ctx,
        IssueParams {
            variant: EnrollmentVariant::Admin,
            subject_id: account,
            issuer: Issuer::KeyDevice {
                session_id: sid,
                device_id: dev.device.id,
            },
            claimant,
            claimant_note: Some(note.to_string()),
            lost_device_ids: lost,
        },
    )
    .await?;
    sqlx::query("UPDATE enrollments SET admin_mode = $2 WHERE id = $1")
        .bind(issued.enrollment_id)
        .bind(&body.mode)
        .execute(&mut *tx)
        .await?;
    crate::permissions::queries::write_audit_log(
        &mut *tx,
        admin.id,
        "admin.users.enrollment_issue",
        Some("user"),
        Some(account),
        Some(serde_json::json!({
            "enrollment_id": issued.enrollment_id,
            "mode": body.mode,
            "claimant_check": claimant.as_str(),
        })),
        None,
    )
    .await?;
    tx.commit().await?;
    spawn_delivery(&state, vec![ev]);
    Ok((StatusCode::CREATED, Json(issued)))
}
