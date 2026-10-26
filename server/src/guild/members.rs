//! Guild membership handlers: leave, list, kick.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use uuid::Uuid;

use super::error::GuildError;
use super::queries::core as core_q;
use super::types::GuildMember;
use crate::api::AppState;
use crate::auth::AuthUser;
use crate::db;

/// Leave guild
#[utoipa::path(
    post,
    path = "/api/guilds/{id}/leave",
    tag = "guilds",
    params(("id" = Uuid, Path, description = "Guild ID")),
    responses((status = 204, description = "Left guild")),
    security(("bearer_auth" = []))
)]
#[tracing::instrument(skip(state))]
pub async fn leave_guild(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(guild_id): Path<Uuid>,
) -> Result<StatusCode, GuildError> {
    // Verify membership
    let is_member = db::is_guild_member(&state.db, guild_id, auth.id).await?;
    if !is_member {
        return Err(GuildError::NotFound);
    }

    // Check if owner (owners can't leave, must transfer ownership first)
    let owner_id = core_q::fetch_guild_owner_required(&state.db, guild_id).await?;

    if owner_id == auth.id {
        return Err(GuildError::Validation(
            "Guild owner must transfer ownership before leaving".to_string(),
        ));
    }

    // Remove membership
    core_q::delete_guild_member(&state.db, guild_id, auth.id).await?;

    // Dispatch MemberLeft to bot ecosystem (non-blocking)
    {
        let db = state.db.clone();
        let redis = state.redis.clone();
        let gid = guild_id;
        let uid = auth.id;
        tokio::spawn(async move {
            crate::ws::bot_events::publish_member_left(&db, &redis, gid, uid).await;
            crate::webhooks::dispatch::dispatch_guild_event(
                &db,
                &redis,
                gid,
                crate::webhooks::events::BotEventType::MemberLeft,
                serde_json::json!({ "guild_id": gid, "user_id": uid }),
            )
            .await;
        });
    }

    Ok(StatusCode::NO_CONTENT)
}

/// List guild members
#[utoipa::path(
    get,
    path = "/api/guilds/{id}/members",
    tag = "guilds",
    params(("id" = Uuid, Path, description = "Guild ID")),
    responses((status = 200, body = Vec<GuildMember>)),
    security(("bearer_auth" = []))
)]
#[tracing::instrument(skip(state))]
pub async fn list_members(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(guild_id): Path<Uuid>,
) -> Result<Json<Vec<GuildMember>>, GuildError> {
    // Verify membership
    let is_member = db::is_guild_member(&state.db, guild_id, auth.id).await?;
    if !is_member {
        return Err(GuildError::Forbidden);
    }

    let mut members = core_q::list_guild_members(&state.db, guild_id).await?;

    // Online means "has a live socket" (per device, see presence::connections).
    let ids: Vec<Uuid> = members.iter().map(|m| m.user_id).collect();
    let live = crate::presence::connections::live_users_or_log(&state.redis, &ids).await;
    for member in &mut members {
        let is_live = live.as_ref().map(|l| l.contains(&member.user_id));
        member.status =
            crate::presence::connections::effective_status(&member.status, is_live).to_string();
    }

    Ok(Json(members))
}

/// Kick a member from guild (owner only)
#[utoipa::path(
    delete,
    path = "/api/guilds/{id}/members/{user_id}",
    tag = "guilds",
    params(
        ("id" = Uuid, Path, description = "Guild ID"),
        ("user_id" = Uuid, Path, description = "User ID")
    ),
    responses((status = 204, description = "Member kicked")),
    security(("bearer_auth" = []))
)]
#[tracing::instrument(skip(state))]
pub async fn kick_member(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((guild_id, user_id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, GuildError> {
    // Verify ownership
    let owner_id = core_q::fetch_guild_owner(&state.db, guild_id).await?;

    if owner_id != auth.id {
        return Err(GuildError::Forbidden);
    }

    // Cannot kick yourself (owner)
    if user_id == auth.id {
        return Err(GuildError::Validation(
            "Cannot kick yourself from the guild".to_string(),
        ));
    }

    // Remove membership
    let rows_affected = core_q::delete_guild_member(&state.db, guild_id, user_id).await?;

    if rows_affected == 0 {
        return Err(GuildError::NotFound);
    }

    // Dispatch MemberLeft to bot ecosystem (non-blocking)
    {
        let db = state.db.clone();
        let redis = state.redis.clone();
        let gid = guild_id;
        let uid = user_id;
        tokio::spawn(async move {
            crate::ws::bot_events::publish_member_left(&db, &redis, gid, uid).await;
            crate::webhooks::dispatch::dispatch_guild_event(
                &db,
                &redis,
                gid,
                crate::webhooks::events::BotEventType::MemberLeft,
                serde_json::json!({ "guild_id": gid, "user_id": uid }),
            )
            .await;
        });
    }

    Ok(StatusCode::NO_CONTENT)
}

/// Initialize `channel_read_state` for the guild's read-tracked channels the
/// user may view, so pre-existing messages don't appear as unread.
///
/// Called by the invite-join and discovery-join handlers after a user joins a guild.
pub(crate) async fn initialize_channel_read_state(
    db: &sqlx::PgPool,
    guild_id: Uuid,
    user_id: Uuid,
) -> Result<(), GuildError> {
    core_q::initialize_channel_read_state(db, guild_id, user_id).await
}
