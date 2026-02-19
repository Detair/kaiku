//! Content Filter API Handlers
//!
//! CRUD endpoints for guild content filter configuration,
//! custom patterns, moderation log, and filter testing.
//! All endpoints require `MANAGE_GUILD` permission.

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::routing::{get, post, put};
use axum::{Json, Router};
use uuid::Uuid;

use super::filter_queries;
use super::filter_types::{
    CreatePatternRequest, FilterError, FilterMatchResponse, GuildFilterConfig, GuildFilterPattern,
    PaginatedModerationLog, PaginationQuery, TestFilterRequest, TestFilterResponse,
    UpdateFilterConfigsRequest, UpdatePatternRequest,
};
use crate::api::AppState;
use crate::auth::AuthUser;
use crate::permissions::{require_guild_permission, GuildPermissions};

/// Maximum custom patterns per guild.
const MAX_CUSTOM_PATTERNS: i64 = 100;

/// Maximum pattern text length.
const MAX_PATTERN_LENGTH: usize = 500;

/// Maximum test input length.
const MAX_TEST_INPUT_LENGTH: usize = 4000;

// ============================================================================
// Router
// ============================================================================

/// Build the filter routes for nesting under `/api/guilds/{id}/filters`.
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list_filter_configs).put(update_filter_configs))
        .route(
            "/patterns",
            get(list_custom_patterns).post(create_custom_pattern),
        )
        .route(
            "/patterns/{pid}",
            put(update_custom_pattern).delete(delete_custom_pattern),
        )
        .route("/log", get(list_moderation_log))
        .route("/test", post(test_filter))
}

// ============================================================================
// Handlers
// ============================================================================

/// List guild filter category configs.
///
/// GET `/api/guilds/{id}/filters`
#[tracing::instrument(skip(state, auth_user))]
async fn list_filter_configs(
    State(state): State<AppState>,
    auth_user: AuthUser,
    Path(guild_id): Path<Uuid>,
) -> Result<Json<Vec<GuildFilterConfig>>, FilterError> {
    require_guild_permission(
        &state.db,
        guild_id,
        auth_user.id,
        GuildPermissions::MANAGE_GUILD,
    )
    .await
    .map_err(|_| FilterError::Forbidden)?;

    let configs = filter_queries::list_filter_configs(&state.db, guild_id).await?;
    Ok(Json(configs))
}

/// Update guild filter category configs (bulk upsert).
///
/// PUT `/api/guilds/{id}/filters`
#[tracing::instrument(skip(state, auth_user, body))]
async fn update_filter_configs(
    State(state): State<AppState>,
    auth_user: AuthUser,
    Path(guild_id): Path<Uuid>,
    Json(body): Json<UpdateFilterConfigsRequest>,
) -> Result<Json<Vec<GuildFilterConfig>>, FilterError> {
    require_guild_permission(
        &state.db,
        guild_id,
        auth_user.id,
        GuildPermissions::MANAGE_GUILD,
    )
    .await
    .map_err(|_| FilterError::Forbidden)?;

    if body.configs.is_empty() {
        return Err(FilterError::Validation(
            "At least one config entry is required".to_string(),
        ));
    }

    let configs = filter_queries::upsert_filter_configs(&state.db, guild_id, &body.configs).await?;

    // Invalidate cached engine so next message uses new config
    state.filter_cache.invalidate(guild_id);

    // Audit log
    crate::permissions::queries::write_audit_log(
        &state.db,
        auth_user.id,
        "guild.filters.updated",
        Some("guild"),
        Some(guild_id),
        Some(serde_json::json!({
            "categories": body.configs.len(),
        })),
        None,
    )
    .await
    .ok();

    Ok(Json(configs))
}

/// List guild custom filter patterns.
///
/// GET `/api/guilds/{id}/filters/patterns`
#[tracing::instrument(skip(state, auth_user))]
async fn list_custom_patterns(
    State(state): State<AppState>,
    auth_user: AuthUser,
    Path(guild_id): Path<Uuid>,
) -> Result<Json<Vec<GuildFilterPattern>>, FilterError> {
    require_guild_permission(
        &state.db,
        guild_id,
        auth_user.id,
        GuildPermissions::MANAGE_GUILD,
    )
    .await
    .map_err(|_| FilterError::Forbidden)?;

    let patterns = filter_queries::list_custom_patterns(&state.db, guild_id).await?;
    Ok(Json(patterns))
}

/// Create a custom filter pattern.
///
/// POST `/api/guilds/{id}/filters/patterns`
#[tracing::instrument(skip(state, auth_user, body))]
async fn create_custom_pattern(
    State(state): State<AppState>,
    auth_user: AuthUser,
    Path(guild_id): Path<Uuid>,
    Json(body): Json<CreatePatternRequest>,
) -> Result<(StatusCode, Json<GuildFilterPattern>), FilterError> {
    require_guild_permission(
        &state.db,
        guild_id,
        auth_user.id,
        GuildPermissions::MANAGE_GUILD,
    )
    .await
    .map_err(|_| FilterError::Forbidden)?;

    // Validate pattern length
    if body.pattern.is_empty() || body.pattern.len() > MAX_PATTERN_LENGTH {
        return Err(FilterError::Validation(format!(
            "Pattern must be 1-{MAX_PATTERN_LENGTH} characters"
        )));
    }

    // Check max patterns limit
    let count = filter_queries::count_custom_patterns(&state.db, guild_id).await?;
    if count >= MAX_CUSTOM_PATTERNS {
        return Err(FilterError::Validation(format!(
            "Maximum of {MAX_CUSTOM_PATTERNS} custom patterns per guild"
        )));
    }

    // Validate regex if applicable
    if body.is_regex {
        validate_regex(&body.pattern)?;
    }

    let pattern = filter_queries::create_custom_pattern(
        &state.db,
        guild_id,
        &body.pattern,
        body.is_regex,
        body.description.as_deref(),
        auth_user.id,
    )
    .await?;

    // Invalidate cache
    state.filter_cache.invalidate(guild_id);

    // Audit log
    crate::permissions::queries::write_audit_log(
        &state.db,
        auth_user.id,
        "guild.filters.pattern_created",
        Some("guild"),
        Some(guild_id),
        Some(serde_json::json!({
            "pattern_id": pattern.id,
            "is_regex": body.is_regex,
        })),
        None,
    )
    .await
    .ok();

    Ok((StatusCode::CREATED, Json(pattern)))
}

/// Update a custom filter pattern.
///
/// PUT `/api/guilds/{id}/filters/patterns/{pid}`
#[tracing::instrument(skip(state, auth_user, body))]
async fn update_custom_pattern(
    State(state): State<AppState>,
    auth_user: AuthUser,
    Path((guild_id, pattern_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<UpdatePatternRequest>,
) -> Result<Json<GuildFilterPattern>, FilterError> {
    require_guild_permission(
        &state.db,
        guild_id,
        auth_user.id,
        GuildPermissions::MANAGE_GUILD,
    )
    .await
    .map_err(|_| FilterError::Forbidden)?;

    // Validate pattern length if provided
    if let Some(ref pattern) = body.pattern {
        if pattern.is_empty() || pattern.len() > MAX_PATTERN_LENGTH {
            return Err(FilterError::Validation(format!(
                "Pattern must be 1-{MAX_PATTERN_LENGTH} characters"
            )));
        }
    }

    // Validate regex when the resulting pattern will be treated as regex.
    // We need to check: will the DB row have is_regex=true after this update?
    match body.is_regex {
        Some(true) => {
            // Explicitly enabling regex — validate whichever pattern text will be stored
            if let Some(ref pattern) = body.pattern {
                validate_regex(pattern)?;
            } else {
                // Changing is_regex to true without new pattern: validate existing text
                let existing = filter_queries::get_custom_pattern(&state.db, pattern_id, guild_id)
                    .await?
                    .ok_or(FilterError::NotFound)?;
                if !existing.is_regex {
                    validate_regex(&existing.pattern)?;
                }
            }
        }
        Some(false) => {
            // Explicitly disabling regex — no regex validation needed
        }
        None => {
            // is_regex not changing — validate new pattern text if the existing row is regex
            if let Some(ref pattern) = body.pattern {
                let existing = filter_queries::get_custom_pattern(&state.db, pattern_id, guild_id)
                    .await?
                    .ok_or(FilterError::NotFound)?;
                if existing.is_regex {
                    validate_regex(pattern)?;
                }
            }
        }
    }

    // Convert description for the query: Option<Option<&str>>
    // body.description is Option<Option<String>> from double-option deserialization:
    //   None → don't change, Some(None) → clear to null, Some(Some(s)) → set to s
    let description = body.description.as_ref().map(|inner| inner.as_deref());

    let pattern = filter_queries::update_custom_pattern(
        &state.db,
        pattern_id,
        guild_id,
        body.pattern.as_deref(),
        body.is_regex,
        description,
        body.enabled,
    )
    .await?
    .ok_or(FilterError::NotFound)?;

    // Invalidate cache
    state.filter_cache.invalidate(guild_id);

    Ok(Json(pattern))
}

/// Delete a custom filter pattern.
///
/// DELETE `/api/guilds/{id}/filters/patterns/{pid}`
#[tracing::instrument(skip(state, auth_user))]
async fn delete_custom_pattern(
    State(state): State<AppState>,
    auth_user: AuthUser,
    Path((guild_id, pattern_id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, FilterError> {
    require_guild_permission(
        &state.db,
        guild_id,
        auth_user.id,
        GuildPermissions::MANAGE_GUILD,
    )
    .await
    .map_err(|_| FilterError::Forbidden)?;

    let deleted = filter_queries::delete_custom_pattern(&state.db, pattern_id, guild_id).await?;
    if !deleted {
        return Err(FilterError::NotFound);
    }

    // Invalidate cache
    state.filter_cache.invalidate(guild_id);

    // Audit log
    crate::permissions::queries::write_audit_log(
        &state.db,
        auth_user.id,
        "guild.filters.pattern_deleted",
        Some("guild"),
        Some(guild_id),
        Some(serde_json::json!({ "pattern_id": pattern_id })),
        None,
    )
    .await
    .ok();

    Ok(StatusCode::NO_CONTENT)
}

/// List moderation action log for a guild (paginated).
///
/// GET `/api/guilds/{id}/filters/log`
#[tracing::instrument(skip(state, auth_user))]
async fn list_moderation_log(
    State(state): State<AppState>,
    auth_user: AuthUser,
    Path(guild_id): Path<Uuid>,
    Query(query): Query<PaginationQuery>,
) -> Result<Json<PaginatedModerationLog>, FilterError> {
    require_guild_permission(
        &state.db,
        guild_id,
        auth_user.id,
        GuildPermissions::MANAGE_GUILD,
    )
    .await
    .map_err(|_| FilterError::Forbidden)?;

    let limit = query.limit.clamp(1, 100);
    let offset = query.offset.max(0);

    let (items, total) =
        filter_queries::list_moderation_log(&state.db, guild_id, limit, offset).await?;

    Ok(Json(PaginatedModerationLog {
        items,
        total,
        limit,
        offset,
    }))
}

/// Test content against active filters (dry-run).
///
/// POST `/api/guilds/{id}/filters/test`
#[tracing::instrument(skip(state, auth_user, body))]
async fn test_filter(
    State(state): State<AppState>,
    auth_user: AuthUser,
    Path(guild_id): Path<Uuid>,
    Json(body): Json<TestFilterRequest>,
) -> Result<Json<TestFilterResponse>, FilterError> {
    require_guild_permission(
        &state.db,
        guild_id,
        auth_user.id,
        GuildPermissions::MANAGE_GUILD,
    )
    .await
    .map_err(|_| FilterError::Forbidden)?;

    if body.content.is_empty() || body.content.len() > MAX_TEST_INPUT_LENGTH {
        return Err(FilterError::Validation(format!(
            "Test content must be 1-{MAX_TEST_INPUT_LENGTH} characters"
        )));
    }

    // Build a fresh ephemeral engine from DB without touching the shared cache
    let engine = state
        .filter_cache
        .build_ephemeral(&state.db, guild_id)
        .await
        .map_err(|e| FilterError::Validation(format!("Failed to build filter engine: {e}")))?;

    let result = engine.check(&body.content);

    Ok(Json(TestFilterResponse {
        blocked: result.blocked,
        matches: result
            .matches
            .into_iter()
            .map(|m| FilterMatchResponse {
                category: m.category,
                action: m.action,
                matched_pattern: m.matched_pattern,
            })
            .collect(),
    }))
}

// ============================================================================
// Helpers
// ============================================================================

/// Validate a regex pattern for compilation and ReDoS protection.
fn validate_regex(pattern: &str) -> Result<(), FilterError> {
    // Try to compile
    let regex = regex::Regex::new(pattern)
        .map_err(|e| FilterError::Validation(format!("Invalid regex: {e}")))?;

    // Basic ReDoS protection: test against a sample input with timeout
    let test_input = "a".repeat(1000);
    let start = std::time::Instant::now();
    let _ = regex.is_match(&test_input);
    let elapsed = start.elapsed();

    if elapsed > std::time::Duration::from_millis(10) {
        return Err(FilterError::Validation(
            "Regex pattern is too slow (possible ReDoS). Simplify the pattern.".to_string(),
        ));
    }

    Ok(())
}
