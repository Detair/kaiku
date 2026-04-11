//! Guild Invite Handlers

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use chrono::{Duration, Utc};
use rand::Rng;
use uuid::Uuid;

use super::error::GuildError;
use super::queries::{core as core_q, invites as queries};
use super::types::{CreateInviteRequest, GuildInvite, InviteResponse};
use crate::api::AppState;
use crate::auth::AuthUser;

/// Generate a cryptographically random 8-character invite code
fn generate_invite_code() -> String {
    const CHARSET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
    let mut rng = rand::thread_rng();
    (0..8)
        .map(|_| {
            let idx = rng.gen_range(0..CHARSET.len());
            CHARSET[idx] as char
        })
        .collect()
}

/// Parse expiry string to duration
fn parse_expiry(expires_in: &str) -> Option<Duration> {
    match expires_in {
        "30m" => Some(Duration::minutes(30)),
        "1h" => Some(Duration::hours(1)),
        "1d" => Some(Duration::days(1)),
        "7d" => Some(Duration::days(7)),
        "never" => None,
        _ => Some(Duration::days(7)), // Default to 7 days
    }
}

/// List invites for a guild (owner only)
#[utoipa::path(
    get,
    path = "/api/guilds/{id}/invites",
    tag = "invites",
    params(("id" = Uuid, Path, description = "Guild ID")),
    responses((status = 200, body = Vec<GuildInvite>)),
    security(("bearer_auth" = []))
)]
#[tracing::instrument(skip(state))]
pub async fn list_invites(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(guild_id): Path<Uuid>,
) -> Result<Json<Vec<GuildInvite>>, GuildError> {
    // Verify ownership
    let owner_id = core_q::fetch_guild_owner(&state.db, guild_id).await?;
    if owner_id != auth.id {
        return Err(GuildError::Forbidden);
    }

    // Get active invites (not expired)
    let invites = queries::list_active_invites(&state.db, guild_id).await?;

    Ok(Json(invites))
}

/// Create a new invite (owner only)
#[utoipa::path(
    post,
    path = "/api/guilds/{id}/invites",
    tag = "invites",
    params(("id" = Uuid, Path, description = "Guild ID")),
    request_body = CreateInviteRequest,
    responses((status = 200, body = GuildInvite)),
    security(("bearer_auth" = []))
)]
#[tracing::instrument(skip(state))]
pub async fn create_invite(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(guild_id): Path<Uuid>,
    Json(body): Json<CreateInviteRequest>,
) -> Result<Json<GuildInvite>, GuildError> {
    // Verify ownership
    let owner_id = core_q::fetch_guild_owner(&state.db, guild_id).await?;
    if owner_id != auth.id {
        return Err(GuildError::Forbidden);
    }

    // Check rate limit (max 10 active invites per guild)
    let active_count = queries::count_active_invites(&state.db, guild_id).await?;

    if active_count >= 10 {
        return Err(GuildError::Validation(
            "Maximum 10 active invites per guild".to_string(),
        ));
    }

    // Generate unique code (retry if collision)
    let mut code = generate_invite_code();
    let mut attempts = 0;
    while attempts < 5 {
        if !queries::invite_code_exists(&state.db, &code).await? {
            break;
        }
        code = generate_invite_code();
        attempts += 1;
    }

    // Calculate expiry
    let expires_at = parse_expiry(&body.expires_in).map(|d| Utc::now() + d);

    // Insert invite
    let invite = queries::insert_invite(&state.db, guild_id, &code, auth.id, expires_at).await?;

    Ok(Json(invite))
}

/// Delete/revoke an invite (owner only)
#[utoipa::path(
    delete,
    path = "/api/guilds/{id}/invites/{code}",
    tag = "invites",
    params(
        ("id" = Uuid, Path, description = "Guild ID"),
        ("code" = String, Path, description = "Invite code")
    ),
    responses((status = 204, description = "Invite deleted")),
    security(("bearer_auth" = []))
)]
#[tracing::instrument(skip(state))]
pub async fn delete_invite(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((guild_id, code)): Path<(Uuid, String)>,
) -> Result<StatusCode, GuildError> {
    // Verify ownership
    let owner_id = core_q::fetch_guild_owner(&state.db, guild_id).await?;
    if owner_id != auth.id {
        return Err(GuildError::Forbidden);
    }

    // Delete the invite
    let rows_affected = queries::delete_invite(&state.db, guild_id, &code).await?;

    if rows_affected == 0 {
        return Err(GuildError::NotFound);
    }

    Ok(StatusCode::NO_CONTENT)
}

/// Join a guild via invite code (any authenticated user)
#[utoipa::path(
    post,
    path = "/api/invites/{code}/join",
    tag = "invites",
    params(("code" = String, Path, description = "Invite code")),
    responses((status = 200, body = InviteResponse)),
    security(("bearer_auth" = []))
)]
#[tracing::instrument(skip(state))]
pub async fn join_via_invite(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(code): Path<String>,
) -> Result<Json<InviteResponse>, GuildError> {
    // Find the invite
    let invite = queries::fetch_active_invite_by_code(&state.db, &code)
        .await?
        .ok_or(GuildError::Validation(
            "Invalid or expired invite code".to_string(),
        ))?;

    if queries::is_user_globally_banned(&state.db, auth.id).await? {
        return Err(GuildError::Forbidden);
    }

    let mut tx = state.db.begin().await?;

    // Serialize member joins per guild so limit checks are strict under concurrency.
    queries::lock_member_join(&mut tx, invite.guild_id).await?;

    // Check guild-specific ban
    if queries::is_user_banned_from_guild(&mut tx, invite.guild_id, auth.id).await? {
        return Err(GuildError::ForbiddenMsg(
            "You are banned from this guild".to_string(),
        ));
    }

    // Check if already a member
    if queries::is_guild_member_tx(&mut tx, invite.guild_id, auth.id).await? {
        tx.commit().await?;

        // Already a member, just return guild info
        let guild_name = queries::fetch_guild_name(&state.db, invite.guild_id).await?;

        return Ok(Json(InviteResponse {
            id: invite.id,
            code: invite.code,
            guild_id: invite.guild_id,
            guild_name,
            expires_at: invite.expires_at,
            use_count: invite.use_count,
            created_at: invite.created_at,
        }));
    }

    // Live count inside advisory lock (seed 53) for strict limit enforcement.
    let member_count = queries::count_guild_members_tx(&mut tx, invite.guild_id).await?;
    if member_count >= state.config.max_members_per_guild {
        return Err(GuildError::LimitExceeded(format!(
            "Guild has reached the maximum number of members ({})",
            state.config.max_members_per_guild
        )));
    }

    // Add as member (ON CONFLICT DO NOTHING to handle duplicate join attempts)
    let rows_affected =
        queries::insert_member_idempotent(&mut tx, invite.guild_id, auth.id).await?;

    // If no rows affected, user was already a member (race with the earlier check)
    if rows_affected == 0 {
        tx.commit().await?;

        let guild_name = queries::fetch_guild_name(&state.db, invite.guild_id).await?;

        return Ok(Json(InviteResponse {
            id: invite.id,
            code: invite.code,
            guild_id: invite.guild_id,
            guild_name,
            expires_at: invite.expires_at,
            use_count: invite.use_count,
            created_at: invite.created_at,
        }));
    }

    // Increment use count
    queries::increment_invite_use_count(&mut tx, invite.id).await?;

    tx.commit().await?;

    // Initialize read state for all text channels (best-effort, non-critical)
    if let Err(err) =
        super::handlers::initialize_channel_read_state(&state.db, invite.guild_id, auth.id).await
    {
        tracing::error!(
            ?err,
            guild_id = %invite.guild_id,
            user_id = %auth.id,
            "Failed to initialize channel read state after invite join"
        );
        // Non-fatal: member was already inserted, read state can be retried on channel access
    }

    // Get guild name for response
    let guild_name = queries::fetch_guild_name(&state.db, invite.guild_id).await?;

    Ok(Json(InviteResponse {
        id: invite.id,
        code: invite.code,
        guild_id: invite.guild_id,
        guild_name,
        expires_at: invite.expires_at,
        use_count: invite.use_count + 1,
        created_at: invite.created_at,
    }))
}
