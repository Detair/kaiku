//! Linked-identity management handlers.
//!
//! Lets an authenticated user view the external (OIDC) identities linked to
//! their account, start a flow to link an additional one, and unlink an
//! existing one. The link flow itself reuses the OIDC machinery in
//! [`super::login`]; the callback distinguishes a link from a login by the
//! `link_user_id` carried in the encrypted flow state.

use std::collections::HashMap;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::Response;
use axum::Json;
use uuid::Uuid;

use super::error::{AuthError, AuthResult};
use super::login::start_oidc_flow;
use super::middleware::AuthUser;
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
    let want_json = query.response.as_deref() == Some("json");
    start_oidc_flow(
        &state,
        &provider,
        query.redirect_uri.as_deref(),
        Some(auth_user.id),
        want_json,
    )
    .await
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
    // Ownership check, last-login-method guard, and delete run atomically under
    // a user-row lock (see unlink_identity_guarded). Ownership failure is a 404
    // so other accounts' identity IDs can't be probed.
    match db::unlink_identity_guarded(&state.db, auth_user.id, identity_id).await? {
        db::UnlinkOutcome::Deleted => {
            tracing::info!(user_id = %auth_user.id, identity_id = %identity_id, "Unlinked OIDC identity");
            Ok(StatusCode::NO_CONTENT)
        }
        db::UnlinkOutcome::NotFound => Err(AuthError::NotFound("Identity not found".to_string())),
        db::UnlinkOutcome::WouldLockOut => Err(AuthError::CannotUnlinkLastIdentity),
    }
}
