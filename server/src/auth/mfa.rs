//! TOTP set-up and removal (C27 `totp.set`, `totp.remove`).

use axum::extract::State;
use axum::Json;
use fred::prelude::*;
use totp_rs::{Algorithm, Secret, TOTP};
use uuid::Uuid;

use super::ctx::AuthCtx;
use super::error::{AuthError, AuthResult};
use super::factors::Factor;
use super::mfa_crypto::encrypt_mfa_secret;
use super::middleware::AuthUser;
use super::reauth;
use super::step_up::{self, StepUpAction, StepUpTarget};
use super::types::{MfaSetupResponse, MfaVerifyRequest};
use crate::api::AppState;
use crate::approvals::transition::lock_accounts;
use crate::security_events::{spawn_delivery, EventType};

/// Setup MFA (TOTP).
///
/// POST /auth/mfa/setup
///
/// Stores the TOTP secret as pending in Redis (5-minute TTL).
/// MFA is only activated once the user verifies a valid TOTP code via
/// `POST /auth/mfa/verify`, which moves the secret to permanent DB storage.
#[utoipa::path(
    post,
    path = "/auth/mfa/setup",
    tag = "auth",
    responses(
        (status = 200, description = "MFA setup initiated", body = MfaSetupResponse),
    ),
    security(("bearer_auth" = [])),
)]
#[tracing::instrument(skip(state), fields(user_id = %auth_user.id))]
pub async fn mfa_setup(
    State(state): State<AppState>,
    auth_user: AuthUser,
) -> AuthResult<Json<MfaSetupResponse>> {
    // C27: `totp.set` from a full session; the grant is consumed at verify.
    super::guards::require_full(&auth_user)?;
    {
        let mut tx = state.db.begin().await?;
        step_up::peek_tx(
            &mut tx,
            &auth_user,
            StepUpAction::TotpSet,
            &StepUpTarget::None,
        )
        .await?;
        tx.commit().await?;
    }
    // Check if encryption key is configured
    let encryption_key = state
        .config
        .mfa_encryption_key
        .as_ref()
        .ok_or_else(|| AuthError::Internal("MFA encryption not configured".to_string()))?;

    // Decode encryption key from hex
    let key_bytes = hex::decode(encryption_key)
        .map_err(|_| AuthError::Internal("Invalid MFA encryption key".to_string()))?;

    if key_bytes.len() != 32 {
        return Err(AuthError::Internal(
            "MFA encryption key must be 32 bytes".to_string(),
        ));
    }

    // Generate a new TOTP secret (20 bytes = 160 bits, standard for TOTP)
    let secret = Secret::default();
    let secret_str = secret.to_encoded().to_string();

    // Encrypt the secret before storing
    let encrypted_secret = encrypt_mfa_secret(&secret_str, &key_bytes)
        .map_err(|e| AuthError::Internal(format!("Failed to encrypt MFA secret: {e}")))?;

    // Store as pending in Redis (5-minute TTL) instead of directly in DB.
    // The secret is only persisted to the DB after successful TOTP verification.
    let redis_key = format!("mfa:pending:{}", auth_user.id);
    state
        .redis
        .set::<(), _, _>(
            &redis_key,
            &encrypted_secret,
            Some(Expiration::EX(300)),
            None,
            false,
        )
        .await
        .map_err(|e| AuthError::Internal(format!("Failed to store pending MFA secret: {e}")))?;

    // Create TOTP instance for QR code
    let totp = TOTP::new(
        Algorithm::SHA1,
        6,
        1,
        30,
        secret
            .to_bytes()
            .map_err(|_| AuthError::Internal("Invalid TOTP secret encoding".into()))?,
        Some("Kaiku".to_string()),
        auth_user.username.clone(),
    )
    .map_err(|e| AuthError::Internal(format!("Failed to create TOTP: {e}")))?;

    // Generate QR code URI (otpauth://)
    let qr_code_url = totp.get_url();

    Ok(Json(MfaSetupResponse {
        secret: secret_str,
        qr_code_url,
    }))
}

/// Confirm a new TOTP secret (C27 `totp.set`): the code of the pending
/// secret from `/auth/mfa/setup`, a full session and the step-up grant,
/// consumed here. A new TOTP generation starts (state `active`, B.1 renewal)
/// and the event `authenticator_added {type: totp}` is recorded.
///
/// POST /auth/mfa/verify
#[utoipa::path(
    post,
    path = "/auth/mfa/verify",
    tag = "auth",
    request_body = MfaVerifyRequest,
    responses(
        (status = 200, description = "TOTP set up"),
        (status = 401, description = "INVALID_MFA"),
        (status = 403, description = "FULL_SESSION_REQUIRED or STEP_UP_REQUIRED"),
    ),
    security(("bearer_auth" = [])),
)]
#[tracing::instrument(skip(state, request), fields(user_id = %auth_user.id))]
pub async fn mfa_verify(
    State(state): State<AppState>,
    auth_user: AuthUser,
    Json(request): Json<MfaVerifyRequest>,
) -> AuthResult<Json<serde_json::Value>> {
    super::guards::require_full(&auth_user)?;
    let redis_key = format!("mfa:pending:{}", auth_user.id);
    let pending: Option<String> = state.redis.get(&redis_key).await.map_err(|e| {
        tracing::error!(error = %e, "Cannot read the pending TOTP secret");
        AuthError::ServiceUnavailable
    })?;
    let Some(encrypted_secret) = pending else {
        return Err(AuthError::Validation(
            "Start the authenticator setup first".to_string(),
        ));
    };
    // Flash r5: a missing grant answers before the code is spent.
    {
        let mut tx = state.db.begin().await?;
        step_up::peek_tx(
            &mut tx,
            &auth_user,
            StepUpAction::TotpSet,
            &StepUpTarget::None,
        )
        .await?;
        tx.commit().await?;
    }
    // Accepted once per step (replay store).
    if !reauth::accept_totp(
        &state,
        auth_user.id,
        &encrypted_secret,
        &auth_user.username,
        &request.code,
    )
    .await?
    {
        return Err(AuthError::InvalidMfaCode);
    }

    let ctx = AuthCtx::from(&state);
    let mut tx = state.db.begin().await?;
    lock_accounts(&mut tx, &[auth_user.id]).await?;
    let grant = step_up::consume_tx(
        &mut tx,
        &auth_user,
        StepUpAction::TotpSet,
        &StepUpTarget::None,
    )
    .await?;
    let generation = super::factors::replace_tx(&mut tx, &ctx, auth_user.id, Factor::Totp).await?;
    sqlx::query("UPDATE users SET mfa_secret = $2, updated_at = NOW() WHERE id = $1")
        .bind(auth_user.id)
        .bind(&encrypted_secret)
        .execute(&mut *tx)
        .await?;
    super::strong::mark_first_activity(&mut tx, auth_user.id).await?;
    let events = vec![
        super::profile::record_own_with(
            &mut tx,
            &auth_user,
            &grant,
            EventType::AuthenticatorAdded,
            serde_json::json!({ "type": "totp", "generation": generation }),
        )
        .await?,
        step_up::record_action_tx(&mut tx, &auth_user, &grant, StepUpAction::TotpSet, "").await?,
    ];
    tx.commit().await?;
    let _ = state.redis.del::<(), _>(&redis_key).await;
    spawn_delivery(&state, events);
    // C26: `authenticators_changed` on a TOTP change (Flash r5).
    crate::approvals::devices::notify_authenticators_changed(&state, auth_user.id).await;
    Ok(Json(serde_json::json!({
        "success": true,
        "message": "MFA verification successful"
    })))
}

/// Turn TOTP off (C27 `totp.remove`): a full session and the step-up grant.
/// A code in the body is accepted and ignored (a code is never a step-up,
/// F7). Event `authenticator_removed {type: totp, by: self}`.
///
/// POST /auth/mfa/disable
#[utoipa::path(
    post,
    path = "/auth/mfa/disable",
    tag = "auth",
    responses(
        (status = 200, description = "TOTP turned off"),
        (status = 403, description = "FULL_SESSION_REQUIRED or STEP_UP_REQUIRED"),
    ),
    security(("bearer_auth" = [])),
)]
#[tracing::instrument(skip(state), fields(user_id = %auth_user.id))]
pub async fn mfa_disable(
    State(state): State<AppState>,
    auth_user: AuthUser,
) -> AuthResult<Json<serde_json::Value>> {
    super::guards::require_full(&auth_user)?;
    let ctx = AuthCtx::from(&state);
    let mut tx = state.db.begin().await?;
    lock_accounts(&mut tx, &[auth_user.id]).await?;
    let grant = step_up::consume_tx(
        &mut tx,
        &auth_user,
        StepUpAction::TotpRemove,
        &StepUpTarget::None,
    )
    .await?;
    let had: bool = sqlx::query_scalar("SELECT mfa_secret IS NOT NULL FROM users WHERE id = $1")
        .bind(auth_user.id)
        .fetch_one(&mut *tx)
        .await?;
    if !had {
        return Err(AuthError::Validation("MFA not enabled".to_string()));
    }
    super::factors::replace_tx(&mut tx, &ctx, auth_user.id, Factor::Totp).await?;
    set_mfa_secret_tx(&mut tx, auth_user.id).await?;
    let events = vec![
        super::profile::record_own_with(
            &mut tx,
            &auth_user,
            &grant,
            EventType::AuthenticatorRemoved,
            serde_json::json!({ "type": "totp", "by": "self" }),
        )
        .await?,
        step_up::record_action_tx(&mut tx, &auth_user, &grant, StepUpAction::TotpRemove, "")
            .await?,
    ];
    tx.commit().await?;
    spawn_delivery(&state, events);
    crate::approvals::devices::notify_authenticators_changed(&state, auth_user.id).await;
    Ok(Json(serde_json::json!({
        "success": true,
        "message": "MFA disabled successfully"
    })))
}

async fn set_mfa_secret_tx(conn: &mut sqlx::PgConnection, user_id: Uuid) -> sqlx::Result<()> {
    sqlx::query("UPDATE users SET mfa_secret = NULL, updated_at = NOW() WHERE id = $1")
        .bind(user_id)
        .execute(&mut *conn)
        .await?;
    Ok(())
}
