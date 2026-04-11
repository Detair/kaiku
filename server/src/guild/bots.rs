//! Bot management handlers for guilds: install, list, remove, slash commands.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use serde::Serialize;
use uuid::Uuid;

use super::error::GuildError;
use super::queries::core as core_q;
use super::types::GuildCommandInfo;
use crate::api::AppState;
use crate::auth::AuthUser;
use crate::db;
use crate::permissions::{require_guild_permission, GuildPermissions, PermissionError};

/// A bot installed in a guild.
#[derive(Debug, Serialize, sqlx::FromRow, utoipa::ToSchema)]
pub struct InstalledBot {
    pub application_id: Uuid,
    pub bot_user_id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub installed_by: Uuid,
    pub installed_at: chrono::DateTime<chrono::Utc>,
}

/// Install a bot into a guild.
///
/// `POST /api/guilds/:guild_id/bots/:bot_id/add`
#[utoipa::path(
    post,
    path = "/api/guilds/{id}/bots/{bot_id}/add",
    tag = "guilds",
    params(
        ("id" = Uuid, Path, description = "Guild ID"),
        ("bot_id" = Uuid, Path, description = "Bot user ID")
    ),
    responses((status = 204, description = "Bot added")),
    security(("bearer_auth" = []))
)]
#[tracing::instrument(skip(state))]
pub async fn add_bot_to_guild(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((guild_id, bot_id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, GuildError> {
    let _ctx =
        require_guild_permission(&state.db, guild_id, auth.id, GuildPermissions::MANAGE_GUILD)
            .await
            .map_err(|e| match e {
                PermissionError::NotGuildMember => GuildError::Forbidden,
                other => GuildError::Permission(other),
            })?;

    // Validate bot exists and is installable (outside lock)
    if core_q::fetch_bot_user(&state.db, bot_id).await?.is_none() {
        return Err(GuildError::NotFound);
    }

    let app = core_q::fetch_bot_application(&state.db, bot_id)
        .await?
        .ok_or(GuildError::NotFound)?;

    if !app.public && app.owner_id != auth.id {
        return Err(GuildError::NotFound);
    }

    let application_id = app.id;

    // Advisory lock seed 63 = bot_install (see db/mod.rs registry)
    let mut tx = state.db.begin().await?;

    core_q::lock_bot_install(&mut tx, guild_id).await?;

    let bot_count = core_q::count_guild_bots_tx(&mut tx, guild_id).await?;

    if bot_count >= state.config.max_bots_per_guild {
        return Err(GuildError::LimitExceeded(format!(
            "Maximum number of bots per guild reached ({})",
            state.config.max_bots_per_guild
        )));
    }

    core_q::insert_bot_installation(&mut tx, guild_id, application_id, auth.id).await?;

    tx.commit().await?;

    Ok(StatusCode::NO_CONTENT)
}

/// List bots installed in a guild.
///
/// `GET /api/guilds/:guild_id/bots`
#[utoipa::path(
    get,
    path = "/api/guilds/{id}/bots",
    tag = "guilds",
    params(("id" = Uuid, Path, description = "Guild ID")),
    responses((status = 200, body = Vec<InstalledBot>)),
    security(("bearer_auth" = []))
)]
#[tracing::instrument(skip(state))]
pub async fn list_guild_bots(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(guild_id): Path<Uuid>,
) -> Result<Json<Vec<InstalledBot>>, GuildError> {
    // Verify membership
    let is_member = db::is_guild_member(&state.db, guild_id, auth.id).await?;
    if !is_member {
        return Err(GuildError::Forbidden);
    }

    let bots = core_q::list_guild_bots(&state.db, guild_id).await?;

    Ok(Json(bots))
}

/// Remove a bot from a guild.
///
/// `DELETE /api/guilds/:guild_id/bots/:bot_id`
#[utoipa::path(
    delete,
    path = "/api/guilds/{id}/bots/{bot_id}",
    tag = "guilds",
    params(
        ("id" = Uuid, Path, description = "Guild ID"),
        ("bot_id" = Uuid, Path, description = "Bot user ID")
    ),
    responses((status = 204, description = "Bot removed")),
    security(("bearer_auth" = []))
)]
#[tracing::instrument(skip(state))]
pub async fn remove_bot_from_guild(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((guild_id, bot_id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, GuildError> {
    let _ctx =
        require_guild_permission(&state.db, guild_id, auth.id, GuildPermissions::MANAGE_GUILD)
            .await
            .map_err(|e| match e {
                PermissionError::NotGuildMember => GuildError::Forbidden,
                other => GuildError::Permission(other),
            })?;

    // Look up application_id from bot_user_id
    let application_id = core_q::fetch_bot_application_id(&state.db, bot_id)
        .await?
        .ok_or(GuildError::NotFound)?;

    let rows_affected =
        core_q::delete_bot_installation(&state.db, guild_id, application_id).await?;

    if rows_affected == 0 {
        return Err(GuildError::NotFound);
    }

    Ok(StatusCode::NO_CONTENT)
}

/// List available slash commands in a guild (from installed bots).
///
/// Returns both guild-scoped and global commands from all installed bots.
/// Guild-scoped commands take precedence over global commands with the same name.
///
/// `GET /api/guilds/:guild_id/commands`
#[utoipa::path(
    get,
    path = "/api/guilds/{id}/commands",
    tag = "guilds",
    params(("id" = Uuid, Path, description = "Guild ID")),
    responses((status = 200, body = Vec<GuildCommandInfo>)),
    security(("bearer_auth" = []))
)]
#[tracing::instrument(skip(state))]
pub async fn list_guild_commands(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(guild_id): Path<Uuid>,
) -> Result<Json<Vec<GuildCommandInfo>>, GuildError> {
    // Verify membership
    let is_member = db::is_guild_member(&state.db, guild_id, auth.id).await?;
    if !is_member {
        return Err(GuildError::Forbidden);
    }

    // Return all commands from installed bots (no DISTINCT ON).
    let rows = core_q::list_guild_slash_commands(&state.db, guild_id).await?;

    // Compute ambiguity: count how many distinct apps provide each command name.
    let mut name_counts: std::collections::HashMap<String, usize> =
        std::collections::HashMap::new();
    for (name, _, _, _) in &rows {
        *name_counts.entry(name.clone()).or_insert(0) += 1;
    }

    let result: Vec<GuildCommandInfo> = rows
        .into_iter()
        .map(|(name, description, bot_name, application_id)| {
            let is_ambiguous = name_counts.get(&name).copied().unwrap_or(0) > 1;
            GuildCommandInfo {
                name,
                description,
                bot_name,
                application_id,
                is_ambiguous,
            }
        })
        .collect();

    Ok(Json(result))
}
