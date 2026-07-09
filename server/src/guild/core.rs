//! Core guild CRUD handlers: create, list, get, update, delete.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use sqlx::QueryBuilder;
use uuid::Uuid;
use validator::Validate;

use super::error::GuildError;
use super::queries::core as core_q;
use super::types::{CreateGuildRequest, Guild, GuildWithMemberCount, UpdateGuildRequest};
use crate::api::AppState;
use crate::auth::AuthUser;
use crate::discovery::types::TAG_REGEX;

/// Create a new guild
#[utoipa::path(
    post,
    path = "/api/guilds",
    tag = "guilds",
    request_body = CreateGuildRequest,
    responses((status = 200, body = Guild)),
    security(("bearer_auth" = []))
)]
#[tracing::instrument(skip(state))]
pub async fn create_guild(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(body): Json<CreateGuildRequest>,
) -> Result<Json<Guild>, GuildError> {
    // Validate request
    body.validate()
        .map_err(|e| GuildError::Validation(crate::validation::format_validation_errors(&e)))?;

    // Validate tags if provided
    if let Some(ref tags) = body.tags {
        if tags.len() > 5 {
            return Err(GuildError::Validation("Maximum 5 tags allowed".to_string()));
        }
        for tag in tags {
            if tag.len() < 2 || tag.len() > 32 {
                return Err(GuildError::Validation(
                    "Each tag must be 2-32 characters".to_string(),
                ));
            }
            if !TAG_REGEX.is_match(tag) {
                return Err(GuildError::Validation(
                    "Tags may only contain letters, numbers, and hyphens".to_string(),
                ));
            }
        }
    }

    // Validate banner_url if provided
    if let Some(ref url) = body.banner_url {
        if !url.is_empty() {
            if url.len() > 2048 {
                return Err(GuildError::Validation(
                    "Banner URL too long (max 2048 characters)".to_string(),
                ));
            }
            if !url.starts_with("https://") {
                return Err(GuildError::Validation(
                    "Banner URL must use HTTPS".to_string(),
                ));
            }
        }
    }

    let mut tx = state.db.begin().await?;

    // Serialize guild creation per owner to enforce strict user guild limits.
    core_q::lock_guild_create_for_user(&mut tx, auth.id).await?;

    // Check guild creation limit
    let owned_count = core_q::count_user_owned_guilds_tx(&mut tx, auth.id).await?;
    if owned_count >= state.config.max_guilds_per_user {
        return Err(GuildError::LimitExceeded(format!(
            "Maximum number of guilds reached ({})",
            state.config.max_guilds_per_user
        )));
    }

    // Prepare discovery fields
    let discoverable = body.discoverable.unwrap_or(false);
    let tags: Vec<String> = body
        .tags
        .map(|t| t.into_iter().map(|s| s.to_lowercase()).collect())
        .unwrap_or_default();
    let banner_url: Option<String> = body.banner_url.filter(|u| !u.is_empty());

    // Insert guild with discovery fields
    let guild_id = Uuid::now_v7();
    let guild = core_q::insert_guild(
        &mut tx,
        guild_id,
        &body.name,
        auth.id,
        &body.description,
        discoverable,
        &tags,
        &banner_url,
    )
    .await?;

    // Add owner as member
    core_q::insert_owner_member(&mut tx, guild_id, auth.id).await?;

    // Create default @everyone role with sensible default permissions
    core_q::insert_default_everyone_role(
        &mut tx,
        guild_id,
        crate::permissions::GuildPermissions::EVERYONE_DEFAULT.bits() as i64,
    )
    .await?;

    tx.commit().await?;

    Ok(Json(guild))
}

/// List guilds for the current user with member counts
#[utoipa::path(
    get,
    path = "/api/guilds",
    tag = "guilds",
    responses((status = 200, body = Vec<GuildWithMemberCount>)),
    security(("bearer_auth" = []))
)]
#[tracing::instrument(skip(state))]
pub async fn list_guilds(
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<Json<Vec<GuildWithMemberCount>>, GuildError> {
    // Query guilds with member count in a single query
    let rows = core_q::list_guilds_with_member_count(&state.db, auth.id).await?;

    let guilds = rows
        .into_iter()
        .map(
            |(
                id,
                name,
                owner_id,
                icon_url,
                description,
                threads_enabled,
                discoverable,
                tags,
                banner_url,
                plan,
                created_at,
                member_count,
            )| {
                GuildWithMemberCount {
                    guild: Guild {
                        id,
                        name,
                        owner_id,
                        icon_url,
                        description,
                        threads_enabled,
                        discoverable,
                        tags,
                        banner_url,
                        plan,
                        created_at,
                    },
                    member_count,
                }
            },
        )
        .collect();

    Ok(Json(guilds))
}

/// Get guild details
#[utoipa::path(
    get,
    path = "/api/guilds/{id}",
    tag = "guilds",
    params(("id" = Uuid, Path, description = "Guild ID")),
    responses((status = 200, body = Guild)),
    security(("bearer_auth" = []))
)]
#[tracing::instrument(skip(state))]
pub async fn get_guild(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(guild_id): Path<Uuid>,
) -> Result<Json<Guild>, GuildError> {
    // Verify membership
    let is_member = crate::db::is_guild_member(&state.db, guild_id, auth.id).await?;
    if !is_member {
        return Err(GuildError::Forbidden);
    }

    let guild = core_q::fetch_guild(&state.db, guild_id).await?;

    Ok(Json(guild))
}

/// Update guild
#[utoipa::path(
    patch,
    path = "/api/guilds/{id}",
    tag = "guilds",
    params(("id" = Uuid, Path, description = "Guild ID")),
    request_body = UpdateGuildRequest,
    responses((status = 200, body = Guild)),
    security(("bearer_auth" = []))
)]
#[tracing::instrument(skip(state))]
pub async fn update_guild(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(guild_id): Path<Uuid>,
    Json(body): Json<UpdateGuildRequest>,
) -> Result<Json<Guild>, GuildError> {
    // Validate request
    body.validate()
        .map_err(|e| GuildError::Validation(crate::validation::format_validation_errors(&e)))?;

    // Verify ownership
    let owner_id = core_q::fetch_guild_owner(&state.db, guild_id).await?;

    if owner_id != auth.id {
        return Err(GuildError::Forbidden);
    }

    // Build dynamic update query
    let mut has_changes = false;
    let mut builder = QueryBuilder::new("UPDATE guilds SET ");
    {
        let mut sep = builder.separated(", ");
        if let Some(name) = body.name {
            sep.push("name = ").push_bind_unseparated(name);
            has_changes = true;
        }
        if let Some(desc) = body.description {
            sep.push("description = ").push_bind_unseparated(desc);
            has_changes = true;
        }
        if let Some(icon) = body.icon_url {
            sep.push("icon_url = ").push_bind_unseparated(icon);
            has_changes = true;
        }
    }

    if !has_changes {
        return get_guild(State(state), auth, Path(guild_id)).await;
    }

    let updated_guild = core_q::update_guild_dynamic(&state.db, builder, guild_id).await?;

    Ok(Json(updated_guild))
}

/// Delete guild (owner only)
#[utoipa::path(
    delete,
    path = "/api/guilds/{id}",
    tag = "guilds",
    params(("id" = Uuid, Path, description = "Guild ID")),
    responses((status = 204, description = "Guild deleted")),
    security(("bearer_auth" = []))
)]
#[tracing::instrument(skip(state))]
pub async fn delete_guild(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(guild_id): Path<Uuid>,
) -> Result<StatusCode, GuildError> {
    // Verify ownership
    let owner_id = core_q::fetch_guild_owner(&state.db, guild_id).await?;

    if owner_id != auth.id {
        return Err(GuildError::Forbidden);
    }

    core_q::delete_guild(&state.db, guild_id).await?;

    Ok(StatusCode::NO_CONTENT)
}
