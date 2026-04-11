//! Guild settings, usage stats, channels list, and banner upload handlers.

use axum::extract::{Multipart, Path, State};
use axum::http::StatusCode;
use axum::Json;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use sqlx::QueryBuilder;
use uuid::Uuid;

use super::error::GuildError;
use super::queries::{core as core_q, limits};
use super::types::{Guild, GuildSettings, UpdateGuildSettingsRequest};
use crate::api::AppState;
use crate::auth::AuthUser;
use crate::db::{self, ChannelType};
use crate::discovery::types::TAG_REGEX;
use crate::permissions::{require_guild_permission, GuildPermissions, PermissionError};
use crate::util::format_file_size;
use crate::ws::{broadcast_to_user, ServerEvent};

// ============================================================================
// Response Types
// ============================================================================

/// Channel with unread message count for the current user.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct ChannelWithUnread {
    #[serde(flatten)]
    #[schema(inline)]
    pub channel: db::Channel,
    /// Number of unread messages (only for text channels).
    pub unread_count: i64,
    /// User's read cursor for this channel.
    pub last_read_message_id: Option<Uuid>,
    /// Most recent message in this channel.
    pub last_message_id: Option<Uuid>,
}

/// Position specification for a channel in reorder request.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct ChannelPosition {
    pub id: Uuid,
    pub position: i32,
    #[serde(default)]
    pub category_id: Option<Uuid>,
}

/// Request to reorder channels in a guild.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct ReorderChannelsRequest {
    pub channels: Vec<ChannelPosition>,
}

/// A single resource usage metric (current count vs limit).
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct UsageStat {
    pub current: i64,
    pub limit: i64,
}

/// Guild resource usage statistics.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct GuildUsageStats {
    pub guild_id: Uuid,
    pub plan: String,
    pub members: UsageStat,
    pub channels: UsageStat,
    pub roles: UsageStat,
    pub emojis: UsageStat,
    pub bots: UsageStat,
    pub pages: UsageStat,
}

// ============================================================================
// Handlers
// ============================================================================

/// List guild channels with unread counts
#[utoipa::path(
    get,
    path = "/api/guilds/{id}/channels",
    tag = "guilds",
    params(("id" = Uuid, Path, description = "Guild ID")),
    responses((status = 200, body = Vec<ChannelWithUnread>)),
    security(("bearer_auth" = []))
)]
#[tracing::instrument(skip(state))]
pub async fn list_channels(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(guild_id): Path<Uuid>,
) -> Result<Json<Vec<ChannelWithUnread>>, GuildError> {
    // Verify membership before fetching channels
    let is_member = db::is_guild_member(&state.db, guild_id, auth.id).await?;
    if !is_member {
        return Err(GuildError::Forbidden);
    }

    let all_channels = db::get_guild_channels(&state.db, guild_id).await?;
    let all_channel_ids: Vec<Uuid> = all_channels.iter().map(|c| c.id).collect();

    // Batch permission check: fetch membership + roles once, batch-fetch overrides
    let accessible_ids = crate::permissions::filter_accessible_channels(
        &state.db,
        guild_id,
        auth.id,
        &all_channel_ids,
    )
    .await
    .map_err(|e| match e {
        crate::permissions::PermissionError::NotGuildMember => GuildError::Forbidden,
        other => GuildError::Permission(other),
    })?;

    let accessible_set: std::collections::HashSet<Uuid> = accessible_ids.into_iter().collect();
    let channels: Vec<db::Channel> = all_channels
        .into_iter()
        .filter(|c| accessible_set.contains(&c.id))
        .collect();

    // Collect text channel IDs for batched unread count query
    let text_channel_ids: Vec<Uuid> = channels
        .iter()
        .filter(|c| c.channel_type == ChannelType::Text)
        .map(|c| c.id)
        .collect();

    // Single CTE query: unread counts, read cursors, and last message IDs in one round trip
    let channel_states: std::collections::HashMap<Uuid, (i64, Option<Uuid>, Option<Uuid>)> =
        if text_channel_ids.is_empty() {
            std::collections::HashMap::new()
        } else {
            core_q::fetch_channel_states(&state.db, auth.id, &text_channel_ids)
                .await?
                .into_iter()
                .map(|(channel_id, unread, cursor, last_msg)| {
                    (channel_id, (unread, cursor, last_msg))
                })
                .collect()
        };

    // Build result with unread counts, read cursors, and last message IDs
    let result: Vec<ChannelWithUnread> = channels
        .into_iter()
        .map(|channel| {
            let is_text = channel.channel_type == ChannelType::Text;
            let (unread_count, last_read_message_id, last_message_id) = if is_text {
                channel_states
                    .get(&channel.id)
                    .map(|(u, r, l)| (*u, *r, *l))
                    .unwrap_or((0, None, None))
            } else {
                (0, None, None)
            };
            ChannelWithUnread {
                channel,
                unread_count,
                last_read_message_id,
                last_message_id,
            }
        })
        .collect();

    Ok(Json(result))
}

/// Reorder channels in a guild.
///
/// `POST /api/guilds/:guild_id/channels/reorder`
#[utoipa::path(
    post,
    path = "/api/guilds/{id}/channels/reorder",
    tag = "guilds",
    params(("id" = Uuid, Path, description = "Guild ID")),
    request_body = ReorderChannelsRequest,
    responses((status = 204, description = "Channels reordered")),
    security(("bearer_auth" = []))
)]
#[tracing::instrument(skip(state, body))]
pub async fn reorder_channels(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(guild_id): Path<Uuid>,
    Json(body): Json<ReorderChannelsRequest>,
) -> Result<StatusCode, GuildError> {
    // Check MANAGE_CHANNELS permission
    let _ctx = require_guild_permission(
        &state.db,
        guild_id,
        auth.id,
        GuildPermissions::MANAGE_CHANNELS,
    )
    .await
    .map_err(|e| match e {
        PermissionError::NotGuildMember => GuildError::Forbidden,
        other => GuildError::Permission(other),
    })?;

    if body.channels.is_empty() {
        return Ok(StatusCode::NO_CONTENT);
    }

    // Update positions in transaction
    let mut tx = state.db.begin().await?;

    for ch in &body.channels {
        // Validate category type restriction when moving to a new category
        if let Some(cat_id) = ch.category_id {
            if let Some(cat_type) = core_q::fetch_category_type(&mut tx, cat_id).await? {
                if cat_type != "mixed" {
                    if let Some(ch_type) = core_q::fetch_channel_type(&mut tx, ch.id).await? {
                        if cat_type != ch_type {
                            return Err(GuildError::Validation(format!(
                                "Cannot move {ch_type} channel to {cat_type}-only category"
                            )));
                        }
                    }
                }
            }
        }

        core_q::update_channel_position(&mut tx, ch.id, guild_id, ch.position, ch.category_id)
            .await?;
    }

    tx.commit().await?;

    Ok(StatusCode::NO_CONTENT)
}

/// Mark all text channels in a guild as read.
/// POST /api/guilds/{id}/read-all
#[utoipa::path(
    post,
    path = "/api/guilds/{id}/read-all",
    tag = "guilds",
    params(("id" = Uuid, Path, description = "Guild ID")),
    responses((status = 204, description = "All channels marked as read")),
    security(("bearer_auth" = []))
)]
#[tracing::instrument(skip(state))]
pub async fn mark_all_channels_read(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(guild_id): Path<Uuid>,
) -> Result<StatusCode, GuildError> {
    // Verify guild membership
    let is_member = db::is_guild_member(&state.db, guild_id, auth.id).await?;
    if !is_member {
        return Err(GuildError::Forbidden);
    }

    let now = chrono::Utc::now();

    // Batch UPSERT channel_read_state for all text channels in this guild
    // Uses a subquery to get the latest message ID per channel
    let rows = core_q::mark_all_guild_channels_read(&state.db, auth.id, guild_id, now).await?;

    // Broadcast ChannelRead events for each updated channel
    for (channel_id, last_read_message_id) in &rows {
        if let Err(e) = broadcast_to_user(
            &state.redis,
            auth.id,
            &ServerEvent::ChannelRead {
                channel_id: *channel_id,
                last_read_message_id: *last_read_message_id,
            },
        )
        .await
        {
            tracing::warn!(
                user_id = %auth.id,
                channel_id = %channel_id,
                error = %e,
                "Failed to broadcast ChannelRead event"
            );
        }
    }

    Ok(StatusCode::NO_CONTENT)
}

/// Get guild settings.
/// GET /api/guilds/{id}/settings
#[utoipa::path(
    get,
    path = "/api/guilds/{id}/settings",
    tag = "guilds",
    params(("id" = Uuid, Path, description = "Guild ID")),
    responses((status = 200, body = GuildSettings)),
    security(("bearer_auth" = []))
)]
#[tracing::instrument(skip(state))]
pub async fn get_guild_settings(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(guild_id): Path<Uuid>,
) -> Result<Json<GuildSettings>, GuildError> {
    // Verify guild membership
    let is_member = db::is_guild_member(&state.db, guild_id, auth.id).await?;
    if !is_member {
        return Err(GuildError::Forbidden);
    }

    let settings = core_q::fetch_guild_settings(&state.db, guild_id, auth.id).await?;
    Ok(Json(settings))
}

/// Update guild settings (requires `MANAGE_GUILD`).
/// PATCH /api/guilds/{id}/settings
#[utoipa::path(
    patch,
    path = "/api/guilds/{id}/settings",
    tag = "guilds",
    params(("id" = Uuid, Path, description = "Guild ID")),
    request_body = UpdateGuildSettingsRequest,
    responses((status = 200, body = GuildSettings)),
    security(("bearer_auth" = []))
)]
#[tracing::instrument(skip(state))]
pub async fn update_guild_settings(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(guild_id): Path<Uuid>,
    Json(body): Json<UpdateGuildSettingsRequest>,
) -> Result<Json<GuildSettings>, GuildError> {
    // Check MANAGE_GUILD permission
    require_guild_permission(&state.db, guild_id, auth.id, GuildPermissions::MANAGE_GUILD)
        .await
        .map_err(GuildError::Permission)?;

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

    // Validate banner_url if provided (empty string clears the banner)
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

    let mut has_changes = false;
    let mut builder = QueryBuilder::new("UPDATE guilds SET ");
    {
        let mut sep = builder.separated(", ");
        if let Some(threads_enabled) = body.threads_enabled {
            sep.push("threads_enabled = ")
                .push_bind_unseparated(threads_enabled);
            has_changes = true;
        }
        if let Some(discoverable) = body.discoverable {
            sep.push("discoverable = ")
                .push_bind_unseparated(discoverable);
            has_changes = true;
        }
        if let Some(tags) = body.tags {
            let tags: Vec<String> = tags.into_iter().map(|t| t.to_lowercase()).collect();
            sep.push("tags = ").push_bind_unseparated(tags);
            has_changes = true;
        }
        if let Some(banner_url) = body.banner_url {
            // Normalize empty string to NULL (clears the banner)
            let normalized: Option<String> = if banner_url.is_empty() {
                None
            } else {
                Some(banner_url)
            };
            sep.push("banner_url = ").push_bind_unseparated(normalized);
            has_changes = true;
        }
    }

    if !has_changes {
        return get_guild_settings(State(state), auth, Path(guild_id)).await;
    }

    let (threads_enabled, discoverable, tags, banner_url) =
        core_q::update_guild_settings_dynamic(&state.db, builder, guild_id).await?;

    // Fetch per-member discovery prompt dismissal status
    let discovery_prompt_dismissed =
        core_q::fetch_discovery_prompt_dismissed(&state.db, guild_id, auth.id).await?;

    Ok(Json(GuildSettings {
        threads_enabled,
        discoverable,
        tags,
        banner_url,
        discovery_prompt_dismissed,
    }))
}

/// Dismiss the discovery setup prompt for the current user in a guild.
/// POST /api/guilds/{id}/dismiss-discovery-prompt
#[utoipa::path(
    post,
    path = "/api/guilds/{id}/dismiss-discovery-prompt",
    tag = "guilds",
    params(("id" = Uuid, Path, description = "Guild ID")),
    responses(
        (status = 204, description = "Prompt dismissed successfully"),
        (status = 403, description = "Not a member of this guild"),
    ),
    security(("bearer_auth" = []))
)]
#[tracing::instrument(skip(state))]
pub async fn dismiss_discovery_prompt(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(guild_id): Path<Uuid>,
) -> Result<StatusCode, GuildError> {
    // Verify guild membership
    let is_member = db::is_guild_member(&state.db, guild_id, auth.id).await?;
    if !is_member {
        return Err(GuildError::Forbidden);
    }

    core_q::dismiss_discovery_prompt(&state.db, guild_id, auth.id).await?;

    Ok(StatusCode::NO_CONTENT)
}

/// Get guild resource usage stats.
/// GET /api/guilds/{id}/usage
#[utoipa::path(
    get,
    path = "/api/guilds/{id}/usage",
    tag = "guilds",
    params(("id" = Uuid, Path, description = "Guild ID")),
    responses((status = 200, body = GuildUsageStats)),
    security(("bearer_auth" = []))
)]
#[tracing::instrument(skip(state))]
pub async fn get_guild_usage(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(guild_id): Path<Uuid>,
) -> Result<Json<GuildUsageStats>, GuildError> {
    // Verify membership
    let is_member = db::is_guild_member(&state.db, guild_id, auth.id).await?;
    if !is_member {
        return Err(GuildError::Forbidden);
    }

    // Fetch plan
    let plan = core_q::fetch_guild_plan(&state.db, guild_id).await?;

    // Run count queries in parallel
    let (members, channels, roles, emojis, bots, pages, page_limit) = tokio::join!(
        limits::get_member_count(&state.db, guild_id),
        limits::count_guild_channels(&state.db, guild_id),
        limits::count_guild_roles(&state.db, guild_id),
        limits::count_guild_emojis(&state.db, guild_id),
        limits::count_guild_bots(&state.db, guild_id),
        crate::pages::count_pages(&state.db, Some(guild_id)),
        crate::pages::get_effective_page_limit(
            &state.db,
            guild_id,
            state.config.max_pages_per_guild,
        ),
    );

    Ok(Json(GuildUsageStats {
        guild_id,
        plan,
        members: UsageStat {
            current: members?,
            limit: state.config.max_members_per_guild,
        },
        channels: UsageStat {
            current: channels?,
            limit: state.config.max_channels_per_guild,
        },
        roles: UsageStat {
            current: roles?,
            limit: state.config.max_roles_per_guild,
        },
        emojis: UsageStat {
            current: emojis?,
            limit: state.config.max_emojis_per_guild,
        },
        bots: UsageStat {
            current: bots?,
            limit: state.config.max_bots_per_guild,
        },
        pages: UsageStat {
            current: pages?,
            limit: page_limit.unwrap_or(state.config.max_pages_per_guild),
        },
    }))
}

/// Upload guild banner
#[utoipa::path(
    post,
    path = "/api/guilds/{id}/banner",
    tag = "guilds",
    params(("id" = Uuid, Path, description = "Guild ID")),
    request_body(content = String, content_type = "multipart/form-data"),
    responses(
        (status = 200, body = Guild),
        (status = 400, description = "Bad request (invalid file)"),
        (status = 403, description = "Forbidden (requires MANAGE_GUILD)"),
        (status = 413, description = "Payload too large")
    ),
    security(("bearer_auth" = []))
)]
#[tracing::instrument(skip(state, multipart))]
pub async fn upload_guild_banner(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(guild_id): Path<Uuid>,
    mut multipart: Multipart,
) -> Result<Json<Guild>, GuildError> {
    // Check permission
    let _ctx =
        require_guild_permission(&state.db, guild_id, auth.id, GuildPermissions::MANAGE_GUILD)
            .await
            .map_err(|e| match e {
                PermissionError::NotGuildMember => GuildError::Forbidden,
                other => GuildError::Permission(other),
            })?;

    // Check if S3 is configured
    let s3 = state
        .s3
        .as_ref()
        .ok_or_else(|| GuildError::Internal("File storage not configured".to_string()))?;

    // Get the file from multipart
    let mut file_data = None;
    let mut filename = None;
    let mut content_type = None;

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| GuildError::Internal(format!("Multipart error: {e}")))?
    {
        if field.name() == Some("banner") {
            filename = field.file_name().map(ToString::to_string);
            content_type = field.content_type().map(ToString::to_string);

            let data = field
                .bytes()
                .await
                .map_err(|e| GuildError::Internal(format!("Upload error: {e}")))?;

            file_data = Some(data);
            break;
        }
    }

    let data = file_data.ok_or(GuildError::Validation(
        "No banner file provided".to_string(),
    ))?;

    // Validate file size (using 5MB limit for banners)
    let max_size = 5 * 1024 * 1024;
    if data.len() > max_size {
        return Err(GuildError::Validation(format!(
            "Banner file too large ({}). Maximum size is {}",
            format_file_size(data.len()),
            format_file_size(max_size)
        )));
    }

    // Validate mime type
    let mime = content_type.unwrap_or_else(|| "application/octet-stream".to_string());
    if !mime.starts_with("image/") {
        return Err(GuildError::Validation("File must be an image".to_string()));
    }

    if mime.contains("svg") {
        return Err(GuildError::Validation(
            "SVG files are not allowed for banners".to_string(),
        ));
    }

    // Validate actual content format
    let detected_format = image::guess_format(&data).map_err(|_| {
        GuildError::Validation(
            "Unable to detect image format. File may be corrupted or not a valid image."
                .to_string(),
        )
    })?;

    match detected_format {
        image::ImageFormat::Png
        | image::ImageFormat::Jpeg
        | image::ImageFormat::Gif
        | image::ImageFormat::WebP => {}
        _ => {
            return Err(GuildError::Validation(format!(
                "Unsupported image format: {detected_format:?}. Only PNG, JPEG, GIF, and WebP are allowed."
            )));
        }
    }

    // Generate S3 key: guilds/{guild_id}/banner-{timestamp}_{filename}
    let timestamp = Utc::now().timestamp();
    let safe_filename = filename
        .unwrap_or_else(|| "banner.png".to_string())
        .replace(|c: char| !c.is_alphanumeric() && c != '.', "_");

    let key = format!("guilds/{guild_id}/banner-{timestamp}_{safe_filename}");

    s3.upload(&key, data.to_vec(), &mime)
        .await
        .map_err(|e| GuildError::Internal(format!("S3 upload failed: {e}")))?;

    // Store redirect URL — /api/files/ endpoint generates presigned URLs on-the-fly
    let url = crate::api::files::file_url(&key);

    // Update guild
    let updated_guild = core_q::update_guild_banner(&state.db, guild_id, &url).await?;

    Ok(Json(updated_guild))
}
