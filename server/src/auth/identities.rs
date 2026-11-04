//! Linked-identity management handlers.
//!
//! Lets an authenticated user view the external (OIDC) identities linked to
//! their account, start a flow to link an additional one, and unlink an
//! existing one. The link flow itself reuses the OIDC machinery in
//! [`super::login`]; the callback distinguishes a link from a login by the
//! `OidcFlowPurpose::Link` carried in the encrypted flow state.
//!
//! A system admin may link only from an elevated session (C24): otherwise a
//! stolen, non-elevated admin token could link an attacker-controlled
//! identity, sign in through it and elevate from that new session.

use std::collections::HashMap;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::Response;
use axum::Json;
use uuid::Uuid;

use super::error::{AuthError, AuthResult};
use super::login::start_oidc_flow;
use super::middleware::AuthUser;
use super::oidc::OidcFlowPurpose;
use super::step_up::{self, StepUpAction, StepUpTarget};
use super::types::{IdentityInfo, IdentityListResponse, OidcAuthorizeQuery};
use crate::api::AppState;
use crate::db;

/// List the external identities linked to the authenticated account.
///
/// GET /auth/me/identities
#[utoipa::path(
    get,
    path = "/auth/me/identities",
    tag = "auth",
    responses(
        (status = 200, description = "Linked external identities", body = IdentityListResponse),
    ),
    security(("bearer_auth" = [])),
)]
#[tracing::instrument(skip(state))]
pub async fn list_identities(
    State(state): State<AppState>,
    auth_user: AuthUser,
) -> AuthResult<Json<IdentityListResponse>> {
    let identities = db::list_user_identities(&state.db, auth_user.id).await?;

    // Resolve provider slugs to human-readable names (fall back to the slug if
    // the provider has since been removed).
    let provider_names: HashMap<String, String> = db::list_oidc_providers(&state.db)
        .await?
        .into_iter()
        .map(|p| (p.slug, p.display_name))
        .collect();

    let infos = identities
        .into_iter()
        .map(|i| IdentityInfo {
            provider_name: provider_names
                .get(&i.provider_slug)
                .cloned()
                .unwrap_or_else(|| i.provider_slug.clone()),
            id: i.id,
            provider_slug: i.provider_slug,
            email: i.email,
            created_at: i.created_at,
            last_used_at: i.last_used_at,
        })
        .collect();

    Ok(Json(IdentityListResponse { identities: infos }))
}

/// Begin linking an additional external identity to the authenticated account.
///
/// GET /auth/me/identities/authorize/{provider}
#[utoipa::path(
    get,
    path = "/auth/me/identities/authorize/{provider}",
    tag = "auth",
    params(
        ("provider" = String, Path, description = "OIDC provider slug"),
        ("redirect_uri" = Option<String>, Query, description = "Optional localhost redirect (Tauri)"),
    ),
    responses(
        (status = 307, description = "Redirect to provider authorization URL"),
        (status = 400, description = "OIDC not configured or auth method disabled"),
        (status = 403, description = "A system admin must use an elevated session (`ELEVATION_REQUIRED`)"),
    ),
    security(("bearer_auth" = [])),
)]
#[tracing::instrument(skip(state, query), fields(provider = %provider))]
pub async fn link_authorize(
    State(state): State<AppState>,
    auth_user: AuthUser,
    Path(provider): Path<String>,
    axum::extract::Query(query): axum::extract::Query<OidcAuthorizeQuery>,
) -> Result<Response, AuthError> {
    // C27: a full session and the step-up `identity.link`, consumed when
    // the flow starts (an OIDC identity is a sign-in method, B.1).
    super::guards::require_full(&auth_user)?;
    if !admin_may_link(&state, auth_user.id, auth_user.session_id).await? {
        return Err(AuthError::ElevationRequired);
    }
    let mut tx = state.db.begin().await?;
    crate::approvals::transition::lock_accounts(&mut tx, &[auth_user.id]).await?;
    let grant = step_up::consume_tx(
        &mut tx,
        &auth_user,
        StepUpAction::IdentityLink,
        &StepUpTarget::None,
    )
    .await?;
    let event = step_up::record_action_tx(
        &mut tx,
        &auth_user,
        &grant,
        StepUpAction::IdentityLink,
        &provider,
    )
    .await?;
    // The grant is spent before the provider round trip (no transaction is
    // held across it); a failed start needs a new step-up.
    tx.commit().await?;
    crate::security_events::spawn_delivery(&state, vec![event]);
    let want_json = query.response.as_deref() == Some("json");
    start_oidc_flow(
        &state,
        &provider,
        query.redirect_uri.as_deref(),
        None,
        OidcFlowPurpose::Link {
            user_id: auth_user.id,
            session_id: auth_user.session_id,
        },
        want_json,
    )
    .await
}

/// Generic hand-back code: the session that started the link flow has ended
/// (missing, expired or revoked), or the flow state names no session.
pub(super) const LINK_SESSION_INVALID: &str = "LINK_SESSION_INVALID";
/// Hand-back code for a system admin linking from a non-elevated session
/// (same code as the admin middleware's 403).
pub(super) const ELEVATION_REQUIRED: &str = "ELEVATION_REQUIRED";
/// Hand-back code for a banned account.
pub(super) const ACCOUNT_BANNED: &str = "ACCOUNT_BANNED";

/// Whether `user_id` may link an identity from `session_id`: always for a
/// non-admin; a system admin only from an unexpired elevated session.
pub(super) async fn admin_may_link(
    state: &AppState,
    user_id: Uuid,
    session_id: Option<Uuid>,
) -> AuthResult<bool> {
    if crate::permissions::queries::get_system_admin(&state.db, user_id)
        .await?
        .is_none()
    {
        return Ok(true);
    }
    let Some(session_id) = session_id else {
        return Ok(false);
    };
    crate::admin::queries::has_active_elevated_session(&state.db, user_id, session_id)
        .await
        .map_err(|e| AuthError::Internal(format!("Elevation lookup failed: {e}")))
}

/// Re-check, in the link callback, the session that started the flow:
/// `Some(code)` refuses the link with that hand-back code.
///
/// Refused when the state names no session (legacy `link_user_id` states),
/// the session is missing, ended, expired or denylisted, the account is
/// banned, or a system admin's session is no longer elevated. The committed
/// session row decides; an unreadable denylist is skipped (B.4 rev 5).
pub(super) async fn link_refusal(
    state: &AppState,
    user_id: Uuid,
    session_id: Option<Uuid>,
) -> AuthResult<Option<&'static str>> {
    let Some(sid) = session_id else {
        return Ok(Some(LINK_SESSION_INVALID));
    };
    let live = crate::admin::queries::is_active_session(&state.db, user_id, sid)
        .await
        .map_err(|e| AuthError::Internal(format!("Session lookup failed: {e}")))?;
    if !live {
        return Ok(Some(LINK_SESSION_INVALID));
    }
    // The committed row above decides; a denylist hit only rejects
    // earlier, and an unreachable denylist is skipped (B.4 rev 5).
    if super::session_check::denylist(&state.redis, sid).await
        == super::session_check::Denylist::Hit
    {
        return Ok(Some(LINK_SESSION_INVALID));
    }
    if super::queries::is_user_globally_banned(&state.db, user_id).await? {
        return Ok(Some(ACCOUNT_BANNED));
    }
    if !admin_may_link(state, user_id, Some(sid)).await? {
        return Ok(Some(ELEVATION_REQUIRED));
    }
    Ok(None)
}

/// Unlink an external identity from the authenticated account.
///
/// `DELETE /auth/me/identities/{id}`
#[utoipa::path(
    delete,
    path = "/auth/me/identities/{id}",
    tag = "auth",
    params(("id" = Uuid, Path, description = "Identity ID to unlink")),
    responses(
        (status = 204, description = "Identity unlinked"),
        (status = 404, description = "Identity not found or not owned by the caller"),
        (status = 409, description = "Cannot remove the account's only login method"),
    ),
    security(("bearer_auth" = [])),
)]
#[tracing::instrument(skip(state))]
pub async fn unlink_identity(
    State(state): State<AppState>,
    auth_user: AuthUser,
    Path(identity_id): Path<Uuid>,
) -> AuthResult<StatusCode> {
    // C27: a full session and the step-up `identity.unlink`/id. Ownership,
    // the last-sign-in-method guard, the grant and the delete run in one
    // transaction under the account lock. Ownership failure is a 404 so
    // other accounts' identity ids cannot be probed.
    super::guards::require_full(&auth_user)?;
    let target = StepUpTarget::Id(identity_id);
    let mut tx = state.db.begin().await?;
    crate::approvals::transition::lock_accounts(&mut tx, &[auth_user.id]).await?;
    let label =
        step_up::target_label_tx(&mut tx, auth_user.id, StepUpAction::IdentityUnlink, &target)
            .await?;
    match db::unlink_identity_tx(&mut tx, auth_user.id, identity_id).await? {
        db::UnlinkOutcome::Deleted => {
            let grant =
                step_up::consume_tx(&mut tx, &auth_user, StepUpAction::IdentityUnlink, &target)
                    .await?;
            let event = step_up::record_action_tx(
                &mut tx,
                &auth_user,
                &grant,
                StepUpAction::IdentityUnlink,
                &label,
            )
            .await?;
            tx.commit().await?;
            crate::security_events::spawn_delivery(&state, vec![event]);
            tracing::info!(user_id = %auth_user.id, identity_id = %identity_id, "Unlinked OIDC identity");
            Ok(StatusCode::NO_CONTENT)
        }
        db::UnlinkOutcome::NotFound => {
            // The grant check comes first for an existing identity only, so
            // an unknown id answers the same with or without a grant.
            Err(AuthError::NotFound("Identity not found".to_string()))
        }
        db::UnlinkOutcome::WouldLockOut => Err(AuthError::CannotUnlinkLastIdentity),
    }
}
