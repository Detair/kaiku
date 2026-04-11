//! Guild Discovery Handlers

use axum::extract::{Path, Query, State};
use axum::Json;
use uuid::Uuid;

use super::error::DiscoveryError;
use super::queries;
use super::types::{
    DiscoverQuery, DiscoverResponse, DiscoverableGuild, JoinDiscoverableResponse, TAG_REGEX,
};
use crate::api::AppState;
use crate::auth::AuthUser;

// ============================================================================
// Handlers
// ============================================================================

/// Browse discoverable guilds with optional search, tag filter, and sorting.
#[utoipa::path(
    get,
    path = "/api/discover/guilds",
    tag = "discovery",
    params(DiscoverQuery),
    responses(
        (status = 200, description = "List of discoverable guilds", body = DiscoverResponse),
        (status = 400, description = "Validation error (invalid search query or tags)"),
        (status = 404, description = "Discovery disabled"),
    ),
)]
#[tracing::instrument(skip(state))]
pub async fn browse_guilds(
    State(state): State<AppState>,
    Query(query): Query<DiscoverQuery>,
) -> Result<Json<DiscoverResponse>, DiscoveryError> {
    if !state.config.enable_guild_discovery {
        return Err(DiscoveryError::Disabled);
    }

    // Validate search query length (Issue #6)
    if let Some(ref q) = query.q {
        if q.len() > 200 {
            return Err(DiscoveryError::Validation(
                "Search query too long (max 200 characters)".to_string(),
            ));
        }
    }

    // Validate tag filter content and count (same rules as tag creation)
    if let Some(ref tags) = query.tags {
        if tags.len() > 10 {
            return Err(DiscoveryError::Validation(
                "Maximum 10 tags for filtering".to_string(),
            ));
        }
        for tag in tags {
            if tag.len() < 2 || tag.len() > 32 {
                return Err(DiscoveryError::Validation(
                    "Each filter tag must be 2-32 characters".to_string(),
                ));
            }
            if !TAG_REGEX.is_match(tag) {
                return Err(DiscoveryError::Validation(
                    "Tags may only contain letters, numbers, and hyphens".to_string(),
                ));
            }
        }
    }

    let limit = query.limit.unwrap_or(20).clamp(1, 50);
    let offset = query.offset.unwrap_or(0).clamp(0, 10_000);

    let rows = queries::browse_discoverable_guilds(
        &state.db,
        query.q.as_deref(),
        query.tags.as_deref(),
        &query.sort,
        limit,
        offset,
    )
    .await?;

    // Extract total from the first row's window function.
    // When offset exceeds total rows, the query returns 0 rows and OVER() can't report the count,
    // so we fall back to a separate count query.
    let total = if let Some(first) = rows.first() {
        first.total_count
    } else if offset > 0 {
        queries::count_discoverable_guilds(&state.db, query.q.as_deref(), query.tags.as_deref())
            .await?
    } else {
        0
    };

    let guilds: Vec<DiscoverableGuild> = rows.into_iter().map(|row| row.guild).collect();

    Ok(Json(DiscoverResponse {
        guilds,
        total,
        limit,
        offset,
    }))
}

/// Join a discoverable guild (requires authentication).
#[utoipa::path(
    post,
    path = "/api/discover/guilds/{id}/join",
    tag = "discovery",
    params(("id" = Uuid, Path, description = "Guild ID")),
    responses(
        (status = 200, description = "Joined the guild", body = JoinDiscoverableResponse),
        (status = 401, description = "Authentication required"),
        (status = 404, description = "Guild not found or not discoverable"),
    ),
    security(("bearer_auth" = []))
)]
#[tracing::instrument(skip(state))]
pub async fn join_discoverable(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(guild_id): Path<Uuid>,
) -> Result<Json<JoinDiscoverableResponse>, DiscoveryError> {
    if !state.config.enable_guild_discovery {
        return Err(DiscoveryError::Disabled);
    }

    // Verify guild is discoverable and not suspended
    let guild_name = queries::find_discoverable_guild_name(&state.db, guild_id)
        .await?
        .ok_or(DiscoveryError::NotFound)?;

    if queries::is_user_globally_banned(&state.db, auth.id).await? {
        return Err(DiscoveryError::Forbidden(
            "You are banned and cannot join guilds via discovery".to_string(),
        ));
    }

    let mut tx = state.db.begin().await?;

    // Serialize member joins per guild so limit checks are strict under concurrency.
    queries::acquire_guild_join_lock(&mut tx, guild_id).await?;

    // Check guild-specific ban
    if queries::is_user_guild_banned(&mut tx, guild_id, auth.id).await? {
        return Err(DiscoveryError::Forbidden(
            "You are banned from this guild".to_string(),
        ));
    }

    // Check member limit before attempting insert
    let member_count = queries::count_guild_members(&mut tx, guild_id).await?;
    if member_count >= state.config.max_members_per_guild {
        return Err(DiscoveryError::LimitExceeded(format!(
            "Guild has reached the maximum number of members ({})",
            state.config.max_members_per_guild
        )));
    }

    // Atomic insert with ON CONFLICT to avoid TOCTOU race
    let rows_affected =
        queries::insert_guild_member_ignore_conflict(&mut tx, guild_id, auth.id).await?;

    tx.commit().await?;

    // If no rows affected, user was already a member
    if rows_affected == 0 {
        return Ok(Json(JoinDiscoverableResponse {
            guild_id,
            guild_name,
            already_member: true,
        }));
    }

    // Initialize read state for all text channels
    if let Err(err) =
        crate::guild::members::initialize_channel_read_state(&state.db, guild_id, auth.id).await
    {
        tracing::error!(
            ?err,
            guild_id = %guild_id,
            user_id = %auth.id,
            "Failed to initialize channel read state after discovery join"
        );
        // Non-fatal: member was already inserted, read state can be retried on channel access
    }

    // Broadcast MemberJoined to bot ecosystem (non-blocking)
    {
        let db = state.db.clone();
        let redis = state.redis.clone();
        let gid = guild_id;
        let uid = auth.id;
        let span = tracing::info_span!("discovery_member_joined_broadcast", guild_id = %gid, user_id = %uid);
        let handle = tokio::spawn(tracing::Instrument::instrument(
            async move {
                let user_info = match queries::find_user_name_pair(&db, uid).await {
                    Ok(info) => info,
                    Err(err) => {
                        tracing::error!(
                            user_id = %uid,
                            guild_id = %gid,
                            %err,
                            "Failed to look up user for MemberJoined event"
                        );
                        return;
                    }
                };

                if let Some((username, display_name)) = user_info {
                    crate::ws::bot_events::publish_member_joined(
                        &db,
                        &redis,
                        gid,
                        uid,
                        &username,
                        &display_name,
                    )
                    .await;
                    crate::webhooks::dispatch::dispatch_guild_event(
                        &db,
                        &redis,
                        gid,
                        crate::webhooks::events::BotEventType::MemberJoined,
                        serde_json::json!({
                            "guild_id": gid,
                            "user_id": uid,
                            "username": username,
                            "display_name": display_name,
                        }),
                    )
                    .await;
                } else {
                    tracing::warn!(
                        user_id = %uid,
                        guild_id = %gid,
                        "Skipping MemberJoined broadcast: user not found"
                    );
                }
            },
            span,
        ));
        let watcher_gid = guild_id;
        let watcher_uid = auth.id;
        tokio::spawn(async move {
            if let Err(err) = handle.await {
                tracing::error!(
                    guild_id = %watcher_gid,
                    user_id = %watcher_uid,
                    "MemberJoined broadcast task panicked: {err}",
                );
            }
        });
    }

    Ok(Json(JoinDiscoverableResponse {
        guild_id,
        guild_name,
        already_member: false,
    }))
}
