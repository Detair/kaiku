//! Page revision handlers (list, get, restore) for both platform and guild pages.

use axum::extract::{Path, State};
use axum::Json;
use tracing::error;
use uuid::Uuid;

use crate::api::AppState;
use crate::auth::AuthUser;
use crate::permissions::is_system_admin;

use super::error::PagesError;
use super::shared::check_manage_pages_permission;
use super::{queries, Page, PageRevision, RevisionListItem};

/// List revisions for a guild page.
///
/// Note: Does not check guild membership — consistent with `list_guild_pages`.
/// Guild information pages are intentionally readable by any authenticated user
/// who has the guild ID. The `require_auth` middleware ensures authentication.
#[utoipa::path(
    get,
    path = "/api/guilds/{id}/pages/{page_id}/revisions",
    tag = "pages",
    params(
        ("id" = Uuid, Path, description = "Guild ID"),
        ("page_id" = Uuid, Path, description = "Page ID")
    ),
    responses((status = 200, description = "List of revisions", body = Vec<RevisionListItem>)),
    security(("bearer_auth" = []))
)]
pub async fn list_guild_page_revisions(
    State(state): State<AppState>,
    Path((guild_id, page_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<Vec<RevisionListItem>>, PagesError> {
    let page = queries::get_page_by_id(&state.db, page_id)
        .await
        .map_err(|e| {
            error!("Failed to get page {}: {}", page_id, e);
            PagesError::Database(e)
        })?
        .ok_or(PagesError::NotFound)?;

    if page.guild_id != Some(guild_id) {
        return Err(PagesError::NotFound);
    }

    let revisions = queries::list_revisions(&state.db, page_id)
        .await
        .map_err(|e| {
            error!("Failed to list revisions for page {}: {}", page_id, e);
            PagesError::Database(e)
        })?;

    Ok(Json(revisions))
}

/// Get a specific revision of a guild page.
///
/// Note: Does not check guild membership — consistent with `list_guild_pages`.
/// Guild information pages are intentionally readable by any authenticated user
/// who has the guild ID. The `require_auth` middleware ensures authentication.
#[utoipa::path(
    get,
    path = "/api/guilds/{id}/pages/{page_id}/revisions/{n}",
    tag = "pages",
    params(
        ("id" = Uuid, Path, description = "Guild ID"),
        ("page_id" = Uuid, Path, description = "Page ID"),
        ("n" = i32, Path, description = "Revision number")
    ),
    responses((status = 200, description = "Revision content", body = PageRevision)),
    security(("bearer_auth" = []))
)]
pub async fn get_guild_page_revision(
    State(state): State<AppState>,
    Path((guild_id, page_id, n)): Path<(Uuid, Uuid, i32)>,
) -> Result<Json<PageRevision>, PagesError> {
    let page = queries::get_page_by_id(&state.db, page_id)
        .await
        .map_err(|e| {
            error!("Failed to get page {}: {}", page_id, e);
            PagesError::Database(e)
        })?
        .ok_or(PagesError::NotFound)?;

    if page.guild_id != Some(guild_id) {
        return Err(PagesError::NotFound);
    }

    queries::get_revision(&state.db, page_id, n)
        .await
        .map_err(|e| {
            error!("Failed to get revision {} for page {}: {}", n, page_id, e);
            PagesError::Database(e)
        })?
        .map(Json)
        .ok_or(PagesError::RevisionNotFound)
}

/// Restore a guild page to a previous revision.
#[utoipa::path(
    post,
    path = "/api/guilds/{id}/pages/{page_id}/revisions/{n}/restore",
    tag = "pages",
    params(
        ("id" = Uuid, Path, description = "Guild ID"),
        ("page_id" = Uuid, Path, description = "Page ID"),
        ("n" = i32, Path, description = "Revision number")
    ),
    responses((status = 200, description = "Page restored", body = Page)),
    security(("bearer_auth" = []))
)]
pub async fn restore_guild_page_revision(
    State(state): State<AppState>,
    user: AuthUser,
    Path((guild_id, page_id, n)): Path<(Uuid, Uuid, i32)>,
) -> Result<Json<Page>, PagesError> {
    check_manage_pages_permission(&state, guild_id, user.id).await?;

    let old_page = queries::get_page_by_id(&state.db, page_id)
        .await
        .map_err(|e| {
            error!("Failed to get page {}: {}", page_id, e);
            PagesError::Database(e)
        })?
        .ok_or(PagesError::NotFound)?;

    if old_page.guild_id != Some(guild_id) {
        return Err(PagesError::NotFound);
    }

    let revision = queries::get_revision(&state.db, page_id, n)
        .await
        .map_err(|e| {
            error!("Failed to get revision {} for page {}: {}", n, page_id, e);
            PagesError::Database(e)
        })?
        .ok_or(PagesError::RevisionNotFound)?;

    let content = revision.content.as_deref().ok_or_else(|| {
        error!(
            "Revision {} for page {} has NULL content",
            n, revision.page_id
        );
        PagesError::CorruptRevision("Revision data is corrupted (missing content)".to_string())
    })?;
    let title = revision.title.as_deref().unwrap_or(&old_page.title);

    // Update page with revision content
    let page = queries::update_page(
        &state.db,
        queries::UpdatePageParams {
            id: page_id,
            title: Some(title),
            slug: None,
            content: Some(content),
            requires_acceptance: None,
            category_id: None,
            updated_by: user.id,
        },
    )
    .await
    .map_err(|e| {
        error!(
            "Failed to restore page {} to revision {}: {}",
            page_id, n, e
        );
        PagesError::Database(e)
    })?;

    // Create a new revision for the restore (best-effort — concurrent operations
    // may collide on the unique constraint; the restore itself already succeeded)
    if let Err(e) = queries::create_revision(
        &state.db,
        page.id,
        &page.content,
        &page.content_hash,
        &page.title,
        user.id,
    )
    .await
    {
        error!(
            "Revision snapshot failed after restore for page {} (concurrent edit?): {}",
            page.id, e
        );
    }

    // Prune old revisions
    let max_revisions = queries::get_effective_revision_limit(
        &state.db,
        guild_id,
        state.config.max_revisions_per_page,
    )
    .await
    .unwrap_or_else(|e| {
        error!(
            "Failed to get effective revision limit, using instance default: {}",
            e
        );
        state.config.max_revisions_per_page
    });

    if let Err(e) = queries::prune_revisions(&state.db, page.id, max_revisions).await {
        error!("Failed to prune revisions for page {}: {}", page.id, e);
    }

    // Audit log
    if let Err(e) = queries::log_audit(
        &state.db,
        page_id,
        "restore",
        user.id,
        Some(&old_page.content_hash),
        None,
        None,
    )
    .await
    {
        error!("Failed to log audit for page {}: {}", page_id, e);
    }

    Ok(Json(page))
}

/// List revisions for a platform page.
#[utoipa::path(
    get,
    path = "/api/pages/{page_id}/revisions",
    tag = "pages",
    params(("page_id" = Uuid, Path, description = "Page ID")),
    responses((status = 200, description = "List of revisions", body = Vec<RevisionListItem>)),
    security(("bearer_auth" = []))
)]
pub async fn list_platform_page_revisions(
    State(state): State<AppState>,
    user: AuthUser,
    Path(page_id): Path<Uuid>,
) -> Result<Json<Vec<RevisionListItem>>, PagesError> {
    // Revision history is admin-only (may contain redacted content)
    let is_admin = is_system_admin(&state.db, user.id).await.map_err(|e| {
        error!("Permission check failed: {}", e);
        PagesError::Internal("Permission check failed".to_string())
    })?;
    if !is_admin {
        return Err(PagesError::AdminRequired);
    }

    let page = queries::get_page_by_id(&state.db, page_id)
        .await
        .map_err(|e| {
            error!("Failed to get page {}: {}", page_id, e);
            PagesError::Database(e)
        })?
        .ok_or(PagesError::NotFound)?;

    if page.guild_id.is_some() {
        return Err(PagesError::NotPlatformPage);
    }

    let revisions = queries::list_revisions(&state.db, page_id)
        .await
        .map_err(|e| {
            error!("Failed to list revisions for page {}: {}", page_id, e);
            PagesError::Database(e)
        })?;

    Ok(Json(revisions))
}

/// Get a specific revision of a platform page.
#[utoipa::path(
    get,
    path = "/api/pages/{page_id}/revisions/{n}",
    tag = "pages",
    params(
        ("page_id" = Uuid, Path, description = "Page ID"),
        ("n" = i32, Path, description = "Revision number")
    ),
    responses((status = 200, description = "Revision content", body = PageRevision)),
    security(("bearer_auth" = []))
)]
pub async fn get_platform_page_revision(
    State(state): State<AppState>,
    user: AuthUser,
    Path((page_id, n)): Path<(Uuid, i32)>,
) -> Result<Json<PageRevision>, PagesError> {
    // Revision history is admin-only (may contain redacted content)
    let is_admin = is_system_admin(&state.db, user.id).await.map_err(|e| {
        error!("Permission check failed: {}", e);
        PagesError::Internal("Permission check failed".to_string())
    })?;
    if !is_admin {
        return Err(PagesError::AdminRequired);
    }

    let page = queries::get_page_by_id(&state.db, page_id)
        .await
        .map_err(|e| {
            error!("Failed to get page {}: {}", page_id, e);
            PagesError::Database(e)
        })?
        .ok_or(PagesError::NotFound)?;

    if page.guild_id.is_some() {
        return Err(PagesError::NotPlatformPage);
    }

    queries::get_revision(&state.db, page_id, n)
        .await
        .map_err(|e| {
            error!("Failed to get revision {} for page {}: {}", n, page_id, e);
            PagesError::Database(e)
        })?
        .map(Json)
        .ok_or(PagesError::RevisionNotFound)
}

/// Restore a platform page to a previous revision (system admin only).
#[utoipa::path(
    post,
    path = "/api/pages/{page_id}/revisions/{n}/restore",
    tag = "pages",
    params(
        ("page_id" = Uuid, Path, description = "Page ID"),
        ("n" = i32, Path, description = "Revision number")
    ),
    responses((status = 200, description = "Page restored", body = Page)),
    security(("bearer_auth" = []))
)]
pub async fn restore_platform_page_revision(
    State(state): State<AppState>,
    user: AuthUser,
    Path((page_id, n)): Path<(Uuid, i32)>,
) -> Result<Json<Page>, PagesError> {
    let is_admin = is_system_admin(&state.db, user.id).await.map_err(|e| {
        error!("Permission check failed: {}", e);
        PagesError::Internal("Permission check failed".to_string())
    })?;
    if !is_admin {
        return Err(PagesError::AdminRequired);
    }

    let old_page = queries::get_page_by_id(&state.db, page_id)
        .await
        .map_err(|e| {
            error!("Failed to get page {}: {}", page_id, e);
            PagesError::Database(e)
        })?
        .ok_or(PagesError::NotFound)?;

    if old_page.guild_id.is_some() {
        return Err(PagesError::NotPlatformPage);
    }

    let revision = queries::get_revision(&state.db, page_id, n)
        .await
        .map_err(|e| {
            error!("Failed to get revision {} for page {}: {}", n, page_id, e);
            PagesError::Database(e)
        })?
        .ok_or(PagesError::RevisionNotFound)?;

    let content = revision.content.as_deref().ok_or_else(|| {
        error!(
            "Revision {} for page {} has NULL content",
            n, revision.page_id
        );
        PagesError::CorruptRevision("Revision data is corrupted (missing content)".to_string())
    })?;
    let title = revision.title.as_deref().unwrap_or(&old_page.title);

    let page = queries::update_page(
        &state.db,
        queries::UpdatePageParams {
            id: page_id,
            title: Some(title),
            slug: None,
            content: Some(content),
            requires_acceptance: None,
            category_id: None,
            updated_by: user.id,
        },
    )
    .await
    .map_err(|e| {
        error!(
            "Failed to restore page {} to revision {}: {}",
            page_id, n, e
        );
        PagesError::Database(e)
    })?;

    // Best-effort — concurrent operations may collide on the unique constraint;
    // the restore itself already succeeded
    if let Err(e) = queries::create_revision(
        &state.db,
        page.id,
        &page.content,
        &page.content_hash,
        &page.title,
        user.id,
    )
    .await
    {
        error!(
            "Revision snapshot failed after restore for page {} (concurrent edit?): {}",
            page.id, e
        );
    }

    if let Err(e) =
        queries::prune_revisions(&state.db, page.id, state.config.max_revisions_per_page).await
    {
        error!("Failed to prune revisions for page {}: {}", page.id, e);
    }

    if let Err(e) = queries::log_audit(
        &state.db,
        page_id,
        "restore",
        user.id,
        Some(&old_page.content_hash),
        None,
        None,
    )
    .await
    {
        error!("Failed to log audit for page {}: {}", page_id, e);
    }

    Ok(Json(page))
}
