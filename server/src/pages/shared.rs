//! Shared helpers used across the pages handler sub-modules.
//!
//! Contains request validation helpers and permission checks that are reused
//! by multiple pages handler files (platform pages, guild pages, revisions,
//! categories).

use tracing::error;
use uuid::Uuid;

use crate::api::AppState;
use crate::permissions::{require_guild_permission, GuildPermissions, PermissionError};

use super::error::PagesError;
use super::{
    queries, CreatePageRequest, UpdatePageRequest, MAX_CONTENT_SIZE, MAX_SLUG_LENGTH,
    MAX_TITLE_LENGTH,
};

/// Convert `PermissionError` to `PagesError`.
pub(super) fn map_permission_error(err: PermissionError) -> PagesError {
    match err {
        PermissionError::NotGuildMember => PagesError::Forbidden,
        PermissionError::DatabaseError(msg) => {
            error!("Permission database error: {}", msg);
            PagesError::Internal("Internal server error".to_string())
        }
        other => PagesError::Permission(other),
    }
}

/// Check `MANAGE_PAGES` permission for guild.
pub(super) async fn check_manage_pages_permission(
    state: &AppState,
    guild_id: Uuid,
    user_id: Uuid,
) -> Result<(), PagesError> {
    require_guild_permission(&state.db, guild_id, user_id, GuildPermissions::MANAGE_PAGES)
        .await
        .map(|_| ())
        .map_err(map_permission_error)
}

pub(super) fn validate_create_request(req: &CreatePageRequest) -> Result<(), PagesError> {
    if req.title.is_empty() {
        return Err(PagesError::Validation("Title is required".to_string()));
    }
    if req.title.chars().count() > MAX_TITLE_LENGTH {
        return Err(PagesError::Validation(format!(
            "Title exceeds {MAX_TITLE_LENGTH} characters"
        )));
    }
    if req.content.is_empty() {
        return Err(PagesError::Validation("Content is required".to_string()));
    }
    if req.content.len() > MAX_CONTENT_SIZE {
        return Err(PagesError::Validation(format!(
            "Content exceeds {MAX_CONTENT_SIZE} bytes"
        )));
    }
    Ok(())
}

pub(super) fn validate_update_request(req: &UpdatePageRequest) -> Result<(), PagesError> {
    if let Some(ref title) = req.title {
        if title.is_empty() {
            return Err(PagesError::Validation("Title cannot be empty".to_string()));
        }
        if title.chars().count() > MAX_TITLE_LENGTH {
            return Err(PagesError::Validation(format!(
                "Title exceeds {MAX_TITLE_LENGTH} characters"
            )));
        }
    }
    if let Some(ref content) = req.content {
        if content.is_empty() {
            return Err(PagesError::Validation(
                "Content cannot be empty".to_string(),
            ));
        }
        if content.len() > MAX_CONTENT_SIZE {
            return Err(PagesError::Validation(format!(
                "Content exceeds {MAX_CONTENT_SIZE} bytes"
            )));
        }
    }
    Ok(())
}

pub(super) fn validate_slug(slug: &str) -> Result<(), PagesError> {
    if slug.is_empty() {
        return Err(PagesError::Validation("Slug cannot be empty".to_string()));
    }
    if slug.len() > MAX_SLUG_LENGTH {
        return Err(PagesError::Validation(format!(
            "Slug exceeds {MAX_SLUG_LENGTH} characters"
        )));
    }
    if queries::is_reserved_slug(slug) {
        return Err(PagesError::Validation(format!(
            "'{slug}' is a reserved slug"
        )));
    }
    // Validate slug format (lowercase alphanumeric with dashes)
    let valid = slug
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        && !slug.starts_with('-')
        && !slug.ends_with('-')
        && !slug.contains("--");

    if !valid {
        return Err(PagesError::Validation(
            "Invalid slug format. Use lowercase letters, numbers, and single dashes (e.g., 'terms-of-service', 'faq-page')".to_string(),
        ));
    }
    Ok(())
}
