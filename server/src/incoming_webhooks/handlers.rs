//! Incoming webhook management (session-authenticated) and token-authenticated
//! CRUD handlers.

use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use uuid::Uuid;

use super::error::IncomingWebhookError;
use super::queries;
use super::types::{CreateWebhookRequest, IncomingWebhook, ModifyWebhookRequest, WebhookResponse};
use crate::api::AppState;
use crate::auth::mfa_crypto::{decrypt_mfa_secret, encrypt_mfa_secret};
use crate::auth::AuthUser;
use crate::chat::types::AuthorProfile;
use crate::db::{self, ChannelType};
use crate::permissions::{require_guild_permission, GuildPermissions};

/// Base URL for building execute URLs: `PUBLIC_BASE_URL` if configured,
/// otherwise derived from the request's forwarded/host headers.
pub(super) fn base_url(state: &AppState, headers: &HeaderMap) -> String {
    if let Some(configured) = &state.config.public_base_url {
        return configured.trim_end_matches('/').to_string();
    }
    let proto = headers
        .get("x-forwarded-proto")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("https");
    let host = headers
        .get("host")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("localhost");
    format!("{proto}://{host}")
}

/// Generate a Discord-length URL-safe webhook token (68 chars, base64url of
/// 51 random bytes) so client-side `[\w-]{60,}` style regexes pass.
fn generate_token() -> String {
    use base64::Engine;
    use rand::RngCore;
    let mut bytes = [0u8; 51];
    rand::thread_rng().fill_bytes(&mut bytes);
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

/// Constant-time token comparison via fixed-length SHA-256 digests.
pub(super) fn token_matches(provided: &str, stored: &str) -> bool {
    use sha2::{Digest, Sha256};
    let a = Sha256::digest(provided.as_bytes());
    let b = Sha256::digest(stored.as_bytes());
    a == b
}

/// The server encryption key (same key + AES-256-GCM scheme the outgoing
/// webhooks module uses for signing secrets).
fn encryption_key(state: &AppState) -> Result<Vec<u8>, IncomingWebhookError> {
    let key_hex = state.config.mfa_encryption_key.as_ref().ok_or_else(|| {
        IncomingWebhookError::Validation(
            "Server encryption key not configured (MFA_ENCRYPTION_KEY)".to_string(),
        )
    })?;
    let key = hex::decode(key_hex).map_err(|_| {
        IncomingWebhookError::Validation("Server encryption key misconfigured".to_string())
    })?;
    if key.len() != 32 {
        return Err(IncomingWebhookError::Validation(
            "Server encryption key misconfigured".to_string(),
        ));
    }
    Ok(key)
}

/// Encrypt a webhook token for storage at rest.
fn encrypt_token(state: &AppState, token: &str) -> Result<String, IncomingWebhookError> {
    let key = encryption_key(state)?;
    encrypt_mfa_secret(token, &key).map_err(|e| {
        IncomingWebhookError::Validation(format!("Failed to encrypt webhook token: {e}"))
    })
}

/// Recover the plaintext token from its stored form. Falls back to treating
/// the stored value as plaintext (rows seeded before encryption / without a
/// configured key), mirroring the outgoing module's legacy handling.
pub(super) fn resolve_token(state: &AppState, stored: &str) -> String {
    if let Ok(key) = encryption_key(state) {
        if let Ok(plain) = decrypt_mfa_secret(stored, &key) {
            return plain;
        }
    }
    stored.to_string()
}

fn validate_name(name: &str) -> Result<String, IncomingWebhookError> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > 80 {
        return Err(IncomingWebhookError::Validation(
            "Webhook name must be 1-80 characters".to_string(),
        ));
    }
    Ok(name.to_string())
}

/// Effective avatar from the request (`avatar_url` wins over Discord's
/// `avatar`); non-https values are dropped.
fn effective_avatar(avatar: Option<String>, avatar_url: Option<String>) -> Option<String> {
    avatar_url.or(avatar).filter(|u| u.starts_with("https://"))
}

/// Channel types that can host an incoming webhook.
fn assert_webhook_channel(channel_type: &ChannelType) -> Result<(), IncomingWebhookError> {
    match channel_type {
        ChannelType::Text | ChannelType::Announcement | ChannelType::Forum => Ok(()),
        ChannelType::Voice | ChannelType::Dm => Err(IncomingWebhookError::Validation(
            "Webhooks can only be created in text, announcement, or forum channels".to_string(),
        )),
    }
}

async fn creator_profile(state: &AppState, user_id: Option<Uuid>) -> Option<AuthorProfile> {
    let user_id = user_id?;
    db::find_user_by_id(&state.db, user_id)
        .await
        .ok()
        .flatten()
        .map(AuthorProfile::from)
}

/// Look up webhook + assert `MANAGE_WEBHOOKS` in its guild.
async fn find_managed_webhook(
    state: &AppState,
    user_id: Uuid,
    webhook_id: Uuid,
) -> Result<IncomingWebhook, IncomingWebhookError> {
    let webhook = queries::find_webhook_by_id(&state.db, webhook_id)
        .await?
        .ok_or(IncomingWebhookError::UnknownWebhook)?;
    require_guild_permission(
        &state.db,
        webhook.guild_id,
        user_id,
        GuildPermissions::MANAGE_WEBHOOKS,
    )
    .await?;
    Ok(webhook)
}

// ============================================================================
// Management handlers (session auth + MANAGE_WEBHOOKS)
// ============================================================================

/// `POST /api/channels/{channel_id}/webhooks` — create an incoming webhook.
#[utoipa::path(
    post,
    path = "/api/channels/{channel_id}/webhooks",
    tag = "incoming-webhooks",
    params(("channel_id" = Uuid, Path, description = "Channel ID")),
    request_body = CreateWebhookRequest,
    responses((status = 200, body = WebhookResponse)),
    security(("bearer_auth" = [])),
)]
#[tracing::instrument(skip(state, headers, body), fields(user_id = %auth.id))]
pub async fn create_channel_webhook(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(channel_id): Path<Uuid>,
    headers: HeaderMap,
    Json(body): Json<CreateWebhookRequest>,
) -> Result<Json<WebhookResponse>, IncomingWebhookError> {
    let name = validate_name(&body.name)?;
    let channel = db::find_channel_by_id(&state.db, channel_id)
        .await?
        .ok_or(IncomingWebhookError::UnknownChannel)?;
    let guild_id = channel
        .guild_id
        .ok_or(IncomingWebhookError::UnknownChannel)?;
    assert_webhook_channel(&channel.channel_type)?;
    require_guild_permission(
        &state.db,
        guild_id,
        auth.id,
        GuildPermissions::MANAGE_WEBHOOKS,
    )
    .await?;

    let avatar = effective_avatar(body.avatar, body.avatar_url);
    let token = generate_token();
    // Encrypted at rest (AES-256-GCM, same scheme as outgoing signing
    // secrets); the response carries the plaintext for the copyable URL.
    let stored_token = encrypt_token(&state, &token)?;
    let mut webhook = queries::create_webhook(
        &state.db,
        guild_id,
        channel_id,
        &name,
        avatar.as_deref(),
        &stored_token,
        auth.id,
    )
    .await?;
    webhook.token = token;

    let user = creator_profile(&state, webhook.created_by).await;
    Ok(Json(WebhookResponse::new(
        webhook,
        &base_url(&state, &headers),
        user,
    )))
}

/// `GET /api/channels/{channel_id}/webhooks` — list a channel's webhooks.
#[utoipa::path(
    get,
    path = "/api/channels/{channel_id}/webhooks",
    tag = "incoming-webhooks",
    params(("channel_id" = Uuid, Path, description = "Channel ID")),
    responses((status = 200, body = [WebhookResponse])),
    security(("bearer_auth" = [])),
)]
#[tracing::instrument(skip(state, headers), fields(user_id = %auth.id))]
pub async fn list_channel_webhooks(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(channel_id): Path<Uuid>,
    headers: HeaderMap,
) -> Result<Json<Vec<WebhookResponse>>, IncomingWebhookError> {
    let channel = db::find_channel_by_id(&state.db, channel_id)
        .await?
        .ok_or(IncomingWebhookError::UnknownChannel)?;
    let guild_id = channel
        .guild_id
        .ok_or(IncomingWebhookError::UnknownChannel)?;
    require_guild_permission(
        &state.db,
        guild_id,
        auth.id,
        GuildPermissions::MANAGE_WEBHOOKS,
    )
    .await?;

    let webhooks = queries::list_channel_webhooks(&state.db, channel_id).await?;
    Ok(Json(
        webhook_responses(&state, webhooks, &base_url(&state, &headers)).await,
    ))
}

/// `GET /api/guilds/{guild_id}/webhooks` — list a guild's webhooks.
#[utoipa::path(
    get,
    path = "/api/guilds/{guild_id}/webhooks",
    tag = "incoming-webhooks",
    params(("guild_id" = Uuid, Path, description = "Guild ID")),
    responses((status = 200, body = [WebhookResponse])),
    security(("bearer_auth" = [])),
)]
#[tracing::instrument(skip(state, headers), fields(user_id = %auth.id))]
pub async fn list_guild_webhooks(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(guild_id): Path<Uuid>,
    headers: HeaderMap,
) -> Result<Json<Vec<WebhookResponse>>, IncomingWebhookError> {
    require_guild_permission(
        &state.db,
        guild_id,
        auth.id,
        GuildPermissions::MANAGE_WEBHOOKS,
    )
    .await?;
    let webhooks = queries::list_guild_webhooks(&state.db, guild_id).await?;
    Ok(Json(
        webhook_responses(&state, webhooks, &base_url(&state, &headers)).await,
    ))
}

/// Bulk-build responses with creator profiles.
async fn webhook_responses(
    state: &AppState,
    webhooks: Vec<IncomingWebhook>,
    base: &str,
) -> Vec<WebhookResponse> {
    let creator_ids: Vec<Uuid> = webhooks.iter().filter_map(|w| w.created_by).collect();
    let users = db::find_users_by_ids(&state.db, &creator_ids)
        .await
        .unwrap_or_default();
    let user_map: std::collections::HashMap<Uuid, AuthorProfile> = users
        .into_iter()
        .map(|u| (u.id, AuthorProfile::from(u)))
        .collect();
    webhooks
        .into_iter()
        .map(|mut w| {
            w.token = resolve_token(state, &w.token);
            let user = w.created_by.and_then(|id| user_map.get(&id).cloned());
            WebhookResponse::new(w, base, user)
        })
        .collect()
}

/// `GET /api/webhooks/{webhook_id}` — fetch one webhook.
#[utoipa::path(
    get,
    path = "/api/webhooks/{webhook_id}",
    tag = "incoming-webhooks",
    params(("webhook_id" = Uuid, Path, description = "Webhook ID")),
    responses((status = 200, body = WebhookResponse)),
    security(("bearer_auth" = [])),
)]
#[tracing::instrument(skip(state, headers), fields(user_id = %auth.id))]
pub async fn get_webhook(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(webhook_id): Path<Uuid>,
    headers: HeaderMap,
) -> Result<Json<WebhookResponse>, IncomingWebhookError> {
    let mut webhook = find_managed_webhook(&state, auth.id, webhook_id).await?;
    webhook.token = resolve_token(&state, &webhook.token);
    let user = creator_profile(&state, webhook.created_by).await;
    Ok(Json(WebhookResponse::new(
        webhook,
        &base_url(&state, &headers),
        user,
    )))
}

/// `PATCH /api/webhooks/{webhook_id}` — modify name/avatar or move channel.
#[utoipa::path(
    patch,
    path = "/api/webhooks/{webhook_id}",
    tag = "incoming-webhooks",
    params(("webhook_id" = Uuid, Path, description = "Webhook ID")),
    request_body = ModifyWebhookRequest,
    responses((status = 200, body = WebhookResponse)),
    security(("bearer_auth" = [])),
)]
#[tracing::instrument(skip(state, headers, body), fields(user_id = %auth.id))]
pub async fn modify_webhook(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(webhook_id): Path<Uuid>,
    headers: HeaderMap,
    Json(body): Json<ModifyWebhookRequest>,
) -> Result<Json<WebhookResponse>, IncomingWebhookError> {
    let webhook = find_managed_webhook(&state, auth.id, webhook_id).await?;

    let name = body.name.as_deref().map(validate_name).transpose()?;
    // Moving requires the target channel to be a valid webhook host in the
    // same guild.
    if let Some(target_channel) = body.channel_id {
        let channel = db::find_channel_by_id(&state.db, target_channel)
            .await?
            .ok_or(IncomingWebhookError::UnknownChannel)?;
        if channel.guild_id != Some(webhook.guild_id) {
            return Err(IncomingWebhookError::Validation(
                "Webhooks can only be moved within the same guild".to_string(),
            ));
        }
        assert_webhook_channel(&channel.channel_type)?;
    }
    let avatar = effective_avatar(body.avatar, body.avatar_url);

    let mut updated = queries::update_webhook(
        &state.db,
        webhook_id,
        name.as_deref(),
        avatar.as_deref(),
        body.channel_id,
    )
    .await?;
    updated.token = resolve_token(&state, &updated.token);
    let user = creator_profile(&state, updated.created_by).await;
    Ok(Json(WebhookResponse::new(
        updated,
        &base_url(&state, &headers),
        user,
    )))
}

/// `DELETE /api/webhooks/{webhook_id}` — delete a webhook.
#[utoipa::path(
    delete,
    path = "/api/webhooks/{webhook_id}",
    tag = "incoming-webhooks",
    params(("webhook_id" = Uuid, Path, description = "Webhook ID")),
    responses((status = 204)),
    security(("bearer_auth" = [])),
)]
#[tracing::instrument(skip(state), fields(user_id = %auth.id))]
pub async fn delete_webhook(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(webhook_id): Path<Uuid>,
) -> Result<StatusCode, IncomingWebhookError> {
    find_managed_webhook(&state, auth.id, webhook_id).await?;
    queries::delete_webhook(&state.db, webhook_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

// ============================================================================
// Token-authenticated handlers (no session; the URL token is the credential)
// ============================================================================

/// Look up a webhook by id + token, Discord status semantics:
/// unknown id → 404 (code 10015), wrong token → 401 (code 50027).
pub(super) async fn find_webhook_with_token(
    state: &AppState,
    webhook_id: Uuid,
    token: &str,
) -> Result<IncomingWebhook, IncomingWebhookError> {
    let mut webhook = queries::find_webhook_by_id(&state.db, webhook_id)
        .await?
        .ok_or(IncomingWebhookError::UnknownWebhook)?;
    let stored_plain = resolve_token(state, &webhook.token);
    if !token_matches(token, &stored_plain) {
        return Err(IncomingWebhookError::InvalidToken);
    }
    webhook.token = stored_plain;
    Ok(webhook)
}

/// `GET /api/webhooks/{webhook_id}/{token}` — webhook object, no `user`.
#[utoipa::path(
    get,
    path = "/api/webhooks/{webhook_id}/{token}",
    tag = "incoming-webhooks",
    params(
        ("webhook_id" = Uuid, Path, description = "Webhook ID"),
        ("token" = String, Path, description = "Webhook token"),
    ),
    responses((status = 200, body = WebhookResponse)),
)]
#[tracing::instrument(skip(state, headers, token))]
pub async fn get_webhook_with_token(
    State(state): State<AppState>,
    Path((webhook_id, token)): Path<(Uuid, String)>,
    headers: HeaderMap,
) -> Result<Json<WebhookResponse>, IncomingWebhookError> {
    let webhook = find_webhook_with_token(&state, webhook_id, &token).await?;
    Ok(Json(WebhookResponse::new(
        webhook,
        &base_url(&state, &headers),
        None,
    )))
}

/// `PATCH /api/webhooks/{webhook_id}/{token}` — modify name/avatar only.
#[utoipa::path(
    patch,
    path = "/api/webhooks/{webhook_id}/{token}",
    tag = "incoming-webhooks",
    params(
        ("webhook_id" = Uuid, Path, description = "Webhook ID"),
        ("token" = String, Path, description = "Webhook token"),
    ),
    request_body = ModifyWebhookRequest,
    responses((status = 200, body = WebhookResponse)),
)]
#[tracing::instrument(skip(state, headers, token, body))]
pub async fn modify_webhook_with_token(
    State(state): State<AppState>,
    Path((webhook_id, token)): Path<(Uuid, String)>,
    headers: HeaderMap,
    Json(body): Json<ModifyWebhookRequest>,
) -> Result<Json<WebhookResponse>, IncomingWebhookError> {
    find_webhook_with_token(&state, webhook_id, &token).await?;
    let name = body.name.as_deref().map(validate_name).transpose()?;
    let avatar = effective_avatar(body.avatar, body.avatar_url);
    // channel_id is deliberately ignored on the token route (Discord parity).
    let updated = queries::update_webhook(
        &state.db,
        webhook_id,
        name.as_deref(),
        avatar.as_deref(),
        None,
    )
    .await?;
    Ok(Json(WebhookResponse::new(
        updated,
        &base_url(&state, &headers),
        None,
    )))
}

/// `DELETE /api/webhooks/{webhook_id}/{token}` — delete via token.
#[utoipa::path(
    delete,
    path = "/api/webhooks/{webhook_id}/{token}",
    tag = "incoming-webhooks",
    params(
        ("webhook_id" = Uuid, Path, description = "Webhook ID"),
        ("token" = String, Path, description = "Webhook token"),
    ),
    responses((status = 204)),
)]
#[tracing::instrument(skip(state, token))]
pub async fn delete_webhook_with_token(
    State(state): State<AppState>,
    Path((webhook_id, token)): Path<(Uuid, String)>,
) -> Result<StatusCode, IncomingWebhookError> {
    find_webhook_with_token(&state, webhook_id, &token).await?;
    queries::delete_webhook(&state.db, webhook_id).await?;
    Ok(StatusCode::NO_CONTENT)
}
