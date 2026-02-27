//! Pages Commands
//!
//! Tauri commands for information pages management.

use serde::{Deserialize, Serialize};
use tauri::{command, State};
use tracing::{debug, error};

use crate::AppState;

/// Validate that a string is safe for use as a URL path segment.
/// Rejects path traversal attempts (`..`, `/`), percent-encoded sequences, and control characters.
fn validate_path_segment(value: &str, name: &str) -> Result<(), String> {
    if value.is_empty() {
        return Err(format!("{name} cannot be empty"));
    }
    if value.contains('/') || value.contains('\\') || value.contains("..") {
        return Err(format!("{name} contains invalid characters"));
    }
    if value.contains('%') {
        return Err(format!("{name} contains percent-encoded characters"));
    }
    if value.bytes().any(|b| b < 0x20) {
        return Err(format!("{name} contains control characters"));
    }
    Ok(())
}

/// Page from server (full content).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Page {
    pub id: String,
    pub guild_id: Option<String>,
    pub title: String,
    pub slug: String,
    pub content: String,
    pub content_hash: String,
    pub position: i32,
    pub requires_acceptance: bool,
    pub category_id: Option<String>,
    pub created_by: String,
    pub updated_by: String,
    pub created_at: String,
    pub updated_at: String,
    pub deleted_at: Option<String>,
}

/// Page list item (without content for listing).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PageListItem {
    pub id: String,
    pub guild_id: Option<String>,
    pub title: String,
    pub slug: String,
    pub position: i32,
    pub requires_acceptance: bool,
    pub category_id: Option<String>,
    pub updated_at: String,
}

// ============================================================================
// Platform Pages
// ============================================================================

/// List all platform pages.
#[command]
pub async fn list_platform_pages(state: State<'_, AppState>) -> Result<Vec<PageListItem>, String> {
    let (server_url, token) = {
        let auth = state.auth.read().await;
        (auth.server_url.clone(), auth.access_token.clone())
    };

    let server_url = server_url.ok_or("Not authenticated")?;
    let token = token.ok_or("Not authenticated")?;

    debug!("Fetching platform pages");

    let response = state
        .http
        .get(format!("{server_url}/api/pages"))
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .map_err(|e| {
            error!("Failed to fetch platform pages: {}", e);
            format!("Connection failed: {e}")
        })?;

    if !response.status().is_success() {
        let status = response.status();
        error!("Failed to fetch platform pages: {}", status);
        return Err(format!("Failed to fetch platform pages: {status}"));
    }

    let pages: Vec<PageListItem> = response
        .json()
        .await
        .map_err(|e| format!("Invalid response: {e}"))?;

    debug!("Fetched {} platform pages", pages.len());
    Ok(pages)
}

/// Get a platform page by slug.
#[command]
pub async fn get_platform_page(state: State<'_, AppState>, slug: String) -> Result<Page, String> {
    validate_path_segment(&slug, "slug")?;

    let (server_url, token) = {
        let auth = state.auth.read().await;
        (auth.server_url.clone(), auth.access_token.clone())
    };

    let server_url = server_url.ok_or("Not authenticated")?;
    let token = token.ok_or("Not authenticated")?;

    debug!("Fetching platform page: {}", slug);

    let response = state
        .http
        .get(format!("{server_url}/api/pages/by-slug/{slug}"))
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .map_err(|e| {
            error!("Failed to fetch platform page: {}", e);
            format!("Connection failed: {e}")
        })?;

    if !response.status().is_success() {
        let status = response.status();
        error!("Failed to fetch platform page: {}", status);
        return Err(format!("Failed to fetch platform page: {status}"));
    }

    let page: Page = response
        .json()
        .await
        .map_err(|e| format!("Invalid response: {e}"))?;

    debug!("Fetched platform page: {}", page.title);
    Ok(page)
}

/// Create a platform page (admin only).
/// Maximum content size in bytes (100KB), matching server limit
const MAX_CONTENT_SIZE: usize = 102_400;

#[command]
pub async fn create_platform_page(
    state: State<'_, AppState>,
    title: String,
    content: String,
    slug: Option<String>,
    requires_acceptance: Option<bool>,
) -> Result<Page, String> {
    // Validate content size before sending to server
    if content.len() > MAX_CONTENT_SIZE {
        return Err(format!(
            "Content exceeds maximum size of {} KB",
            MAX_CONTENT_SIZE / 1024
        ));
    }

    let (server_url, token) = {
        let auth = state.auth.read().await;
        (auth.server_url.clone(), auth.access_token.clone())
    };

    let server_url = server_url.ok_or("Not authenticated")?;
    let token = token.ok_or("Not authenticated")?;

    debug!("Creating platform page: {}", title);

    let response = state
        .http
        .post(format!("{server_url}/api/pages"))
        .header("Authorization", format!("Bearer {token}"))
        .json(&serde_json::json!({
            "title": title,
            "slug": slug,
            "content": content,
            "requires_acceptance": requires_acceptance,
        }))
        .send()
        .await
        .map_err(|e| {
            error!("Failed to create platform page: {}", e);
            format!("Connection failed: {e}")
        })?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        error!("Failed to create platform page: {} - {}", status, body);
        return Err(format!("Failed to create platform page: {status}"));
    }

    let page: Page = response
        .json()
        .await
        .map_err(|e| format!("Invalid response: {e}"))?;

    debug!("Created platform page: {}", page.id);
    Ok(page)
}

/// Update a platform page (admin only).
#[command]
pub async fn update_platform_page(
    state: State<'_, AppState>,
    page_id: String,
    title: Option<String>,
    slug: Option<String>,
    content: Option<String>,
    requires_acceptance: Option<bool>,
) -> Result<Page, String> {
    validate_path_segment(&page_id, "page_id")?;

    // Validate content size if provided
    if let Some(ref c) = content {
        if c.len() > MAX_CONTENT_SIZE {
            return Err(format!(
                "Content exceeds maximum size of {} KB",
                MAX_CONTENT_SIZE / 1024
            ));
        }
    }

    let (server_url, token) = {
        let auth = state.auth.read().await;
        (auth.server_url.clone(), auth.access_token.clone())
    };

    let server_url = server_url.ok_or("Not authenticated")?;
    let token = token.ok_or("Not authenticated")?;

    debug!("Updating platform page: {}", page_id);

    let response = state
        .http
        .patch(format!("{server_url}/api/pages/{page_id}"))
        .header("Authorization", format!("Bearer {token}"))
        .json(&serde_json::json!({
            "title": title,
            "slug": slug,
            "content": content,
            "requires_acceptance": requires_acceptance,
        }))
        .send()
        .await
        .map_err(|e| {
            error!("Failed to update platform page: {}", e);
            format!("Connection failed: {e}")
        })?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        error!("Failed to update platform page: {} - {}", status, body);
        return Err(format!("Failed to update platform page: {status}"));
    }

    let page: Page = response
        .json()
        .await
        .map_err(|e| format!("Invalid response: {e}"))?;

    debug!("Updated platform page: {}", page.id);
    Ok(page)
}

/// Delete a platform page (admin only).
#[command]
pub async fn delete_platform_page(
    state: State<'_, AppState>,
    page_id: String,
) -> Result<(), String> {
    validate_path_segment(&page_id, "page_id")?;

    let (server_url, token) = {
        let auth = state.auth.read().await;
        (auth.server_url.clone(), auth.access_token.clone())
    };

    let server_url = server_url.ok_or("Not authenticated")?;
    let token = token.ok_or("Not authenticated")?;

    debug!("Deleting platform page: {}", page_id);

    let response = state
        .http
        .delete(format!("{server_url}/api/pages/{page_id}"))
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .map_err(|e| {
            error!("Failed to delete platform page: {}", e);
            format!("Connection failed: {e}")
        })?;

    if !response.status().is_success() {
        let status = response.status();
        error!("Failed to delete platform page: {}", status);
        return Err(format!("Failed to delete platform page: {status}"));
    }

    debug!("Deleted platform page: {}", page_id);
    Ok(())
}

/// Reorder platform pages (admin only).
#[command]
pub async fn reorder_platform_pages(
    state: State<'_, AppState>,
    page_ids: Vec<String>,
) -> Result<(), String> {
    let (server_url, token) = {
        let auth = state.auth.read().await;
        (auth.server_url.clone(), auth.access_token.clone())
    };

    let server_url = server_url.ok_or("Not authenticated")?;
    let token = token.ok_or("Not authenticated")?;

    debug!("Reordering platform pages");

    let response = state
        .http
        .post(format!("{server_url}/api/pages/reorder"))
        .header("Authorization", format!("Bearer {token}"))
        .json(&serde_json::json!({
            "page_ids": page_ids,
        }))
        .send()
        .await
        .map_err(|e| {
            error!("Failed to reorder platform pages: {}", e);
            format!("Connection failed: {e}")
        })?;

    if !response.status().is_success() {
        let status = response.status();
        error!("Failed to reorder platform pages: {}", status);
        return Err(format!("Failed to reorder platform pages: {status}"));
    }

    debug!("Reordered platform pages");
    Ok(())
}

// ============================================================================
// Guild Pages
// ============================================================================

/// List guild pages.
#[command]
pub async fn list_guild_pages(
    state: State<'_, AppState>,
    guild_id: String,
) -> Result<Vec<PageListItem>, String> {
    validate_path_segment(&guild_id, "guild_id")?;

    let (server_url, token) = {
        let auth = state.auth.read().await;
        (auth.server_url.clone(), auth.access_token.clone())
    };

    let server_url = server_url.ok_or("Not authenticated")?;
    let token = token.ok_or("Not authenticated")?;

    debug!("Fetching guild pages for: {}", guild_id);

    let response = state
        .http
        .get(format!("{server_url}/api/guilds/{guild_id}/pages"))
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .map_err(|e| {
            error!("Failed to fetch guild pages: {}", e);
            format!("Connection failed: {e}")
        })?;

    if !response.status().is_success() {
        let status = response.status();
        error!("Failed to fetch guild pages: {}", status);
        return Err(format!("Failed to fetch guild pages: {status}"));
    }

    let pages: Vec<PageListItem> = response
        .json()
        .await
        .map_err(|e| format!("Invalid response: {e}"))?;

    debug!("Fetched {} guild pages", pages.len());
    Ok(pages)
}

/// Get a guild page by slug.
#[command]
pub async fn get_guild_page(
    state: State<'_, AppState>,
    guild_id: String,
    slug: String,
) -> Result<Page, String> {
    validate_path_segment(&guild_id, "guild_id")?;
    validate_path_segment(&slug, "slug")?;

    let (server_url, token) = {
        let auth = state.auth.read().await;
        (auth.server_url.clone(), auth.access_token.clone())
    };

    let server_url = server_url.ok_or("Not authenticated")?;
    let token = token.ok_or("Not authenticated")?;

    debug!("Fetching guild page: {}/{}", guild_id, slug);

    let response = state
        .http
        .get(format!(
            "{server_url}/api/guilds/{guild_id}/pages/by-slug/{slug}"
        ))
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .map_err(|e| {
            error!("Failed to fetch guild page: {}", e);
            format!("Connection failed: {e}")
        })?;

    if !response.status().is_success() {
        let status = response.status();
        error!("Failed to fetch guild page: {}", status);
        return Err(format!("Failed to fetch guild page: {status}"));
    }

    let page: Page = response
        .json()
        .await
        .map_err(|e| format!("Invalid response: {e}"))?;

    debug!("Fetched guild page: {}", page.title);
    Ok(page)
}

/// Create a guild page.
#[command]
pub async fn create_guild_page(
    state: State<'_, AppState>,
    guild_id: String,
    title: String,
    content: String,
    slug: Option<String>,
    requires_acceptance: Option<bool>,
    category_id: Option<String>,
) -> Result<Page, String> {
    validate_path_segment(&guild_id, "guild_id")?;

    let (server_url, token) = {
        let auth = state.auth.read().await;
        (auth.server_url.clone(), auth.access_token.clone())
    };

    let server_url = server_url.ok_or("Not authenticated")?;
    let token = token.ok_or("Not authenticated")?;

    debug!("Creating guild page: {} in {}", title, guild_id);

    // Validate content size before sending to server
    if content.len() > MAX_CONTENT_SIZE {
        return Err(format!(
            "Content exceeds maximum size of {} KB",
            MAX_CONTENT_SIZE / 1024
        ));
    }

    let response = state
        .http
        .post(format!("{server_url}/api/guilds/{guild_id}/pages"))
        .header("Authorization", format!("Bearer {token}"))
        .json(&serde_json::json!({
            "title": title,
            "slug": slug,
            "content": content,
            "requires_acceptance": requires_acceptance,
            "category_id": category_id,
        }))
        .send()
        .await
        .map_err(|e| {
            error!("Failed to create guild page: {}", e);
            format!("Connection failed: {e}")
        })?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        error!("Failed to create guild page: {} - {}", status, body);
        return Err(format!("Failed to create guild page: {status}"));
    }

    let page: Page = response
        .json()
        .await
        .map_err(|e| format!("Invalid response: {e}"))?;

    debug!("Created guild page: {}", page.id);
    Ok(page)
}

/// Update a guild page.
#[allow(clippy::too_many_arguments, clippy::option_option)]
#[command]
pub async fn update_guild_page(
    state: State<'_, AppState>,
    guild_id: String,
    page_id: String,
    title: Option<String>,
    slug: Option<String>,
    content: Option<String>,
    requires_acceptance: Option<bool>,
    category_id: Option<Option<String>>,
) -> Result<Page, String> {
    validate_path_segment(&guild_id, "guild_id")?;
    validate_path_segment(&page_id, "page_id")?;

    let (server_url, token) = {
        let auth = state.auth.read().await;
        (auth.server_url.clone(), auth.access_token.clone())
    };

    let server_url = server_url.ok_or("Not authenticated")?;
    let token = token.ok_or("Not authenticated")?;

    debug!("Updating guild page: {} in {}", page_id, guild_id);

    // Validate content size if provided
    if let Some(ref c) = content {
        if c.len() > MAX_CONTENT_SIZE {
            return Err(format!(
                "Content exceeds maximum size of {} KB",
                MAX_CONTENT_SIZE / 1024
            ));
        }
    }

    // Build request body — only include category_id if explicitly provided
    // None = no change, Some(None) = remove category, Some(Some(id)) = set category
    let mut body = serde_json::json!({
        "title": title,
        "slug": slug,
        "content": content,
        "requires_acceptance": requires_acceptance,
    });
    if let Some(cat) = &category_id {
        body["category_id"] = serde_json::json!(cat);
    }

    let response = state
        .http
        .patch(format!(
            "{server_url}/api/guilds/{guild_id}/pages/{page_id}"
        ))
        .header("Authorization", format!("Bearer {token}"))
        .json(&body)
        .send()
        .await
        .map_err(|e| {
            error!("Failed to update guild page: {}", e);
            format!("Connection failed: {e}")
        })?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        error!("Failed to update guild page: {} - {}", status, body);
        return Err(format!("Failed to update guild page: {status}"));
    }

    let page: Page = response
        .json()
        .await
        .map_err(|e| format!("Invalid response: {e}"))?;

    debug!("Updated guild page: {}", page.id);
    Ok(page)
}

/// Delete a guild page.
#[command]
pub async fn delete_guild_page(
    state: State<'_, AppState>,
    guild_id: String,
    page_id: String,
) -> Result<(), String> {
    validate_path_segment(&guild_id, "guild_id")?;
    validate_path_segment(&page_id, "page_id")?;

    let (server_url, token) = {
        let auth = state.auth.read().await;
        (auth.server_url.clone(), auth.access_token.clone())
    };

    let server_url = server_url.ok_or("Not authenticated")?;
    let token = token.ok_or("Not authenticated")?;

    debug!("Deleting guild page: {} in {}", page_id, guild_id);

    let response = state
        .http
        .delete(format!(
            "{server_url}/api/guilds/{guild_id}/pages/{page_id}"
        ))
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .map_err(|e| {
            error!("Failed to delete guild page: {}", e);
            format!("Connection failed: {e}")
        })?;

    if !response.status().is_success() {
        let status = response.status();
        error!("Failed to delete guild page: {}", status);
        return Err(format!("Failed to delete guild page: {status}"));
    }

    debug!("Deleted guild page: {}", page_id);
    Ok(())
}

/// Reorder guild pages.
#[command]
pub async fn reorder_guild_pages(
    state: State<'_, AppState>,
    guild_id: String,
    page_ids: Vec<String>,
) -> Result<(), String> {
    validate_path_segment(&guild_id, "guild_id")?;

    let (server_url, token) = {
        let auth = state.auth.read().await;
        (auth.server_url.clone(), auth.access_token.clone())
    };

    let server_url = server_url.ok_or("Not authenticated")?;
    let token = token.ok_or("Not authenticated")?;

    debug!("Reordering guild pages in: {}", guild_id);

    let response = state
        .http
        .post(format!("{server_url}/api/guilds/{guild_id}/pages/reorder"))
        .header("Authorization", format!("Bearer {token}"))
        .json(&serde_json::json!({
            "page_ids": page_ids,
        }))
        .send()
        .await
        .map_err(|e| {
            error!("Failed to reorder guild pages: {}", e);
            format!("Connection failed: {e}")
        })?;

    if !response.status().is_success() {
        let status = response.status();
        error!("Failed to reorder guild pages: {}", status);
        return Err(format!("Failed to reorder guild pages: {status}"));
    }

    debug!("Reordered guild pages in: {}", guild_id);
    Ok(())
}

// ============================================================================
// Page Acceptance
// ============================================================================

/// Accept a page.
#[command]
pub async fn accept_page(state: State<'_, AppState>, page_id: String) -> Result<(), String> {
    validate_path_segment(&page_id, "page_id")?;

    let (server_url, token) = {
        let auth = state.auth.read().await;
        (auth.server_url.clone(), auth.access_token.clone())
    };

    let server_url = server_url.ok_or("Not authenticated")?;
    let token = token.ok_or("Not authenticated")?;

    debug!("Accepting page: {}", page_id);

    let response = state
        .http
        .post(format!("{server_url}/api/pages/{page_id}/accept"))
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .map_err(|e| {
            error!("Failed to accept page: {}", e);
            format!("Connection failed: {e}")
        })?;

    if !response.status().is_success() {
        let status = response.status();
        error!("Failed to accept page: {}", status);
        return Err(format!("Failed to accept page: {status}"));
    }

    debug!("Accepted page: {}", page_id);
    Ok(())
}

/// Get pages pending acceptance.
#[command]
pub async fn get_pending_acceptance(
    state: State<'_, AppState>,
) -> Result<Vec<PageListItem>, String> {
    let (server_url, token) = {
        let auth = state.auth.read().await;
        (auth.server_url.clone(), auth.access_token.clone())
    };

    let server_url = server_url.ok_or("Not authenticated")?;
    let token = token.ok_or("Not authenticated")?;

    debug!("Fetching pending acceptance pages");

    let response = state
        .http
        .get(format!("{server_url}/api/pages/pending-acceptance"))
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .map_err(|e| {
            error!("Failed to fetch pending acceptance: {}", e);
            format!("Connection failed: {e}")
        })?;

    if !response.status().is_success() {
        let status = response.status();
        error!("Failed to fetch pending acceptance: {}", status);
        return Err(format!("Failed to fetch pending acceptance: {status}"));
    }

    let pages: Vec<PageListItem> = response
        .json()
        .await
        .map_err(|e| format!("Invalid response: {e}"))?;

    debug!("Fetched {} pages pending acceptance", pages.len());
    Ok(pages)
}

// ============================================================================
// Page Revisions
// ============================================================================

/// Page revision (full content).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PageRevision {
    pub id: String,
    pub page_id: String,
    pub revision_number: i32,
    pub content: Option<String>,
    pub content_hash: Option<String>,
    pub title: Option<String>,
    pub created_by: Option<String>,
    pub created_at: String,
}

/// Revision list item (metadata only).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RevisionListItem {
    pub id: String,
    pub page_id: String,
    pub revision_number: i32,
    pub content_hash: Option<String>,
    pub title: Option<String>,
    pub created_by: Option<String>,
    pub created_at: String,
}

/// List revisions for a guild page.
#[command]
pub async fn list_page_revisions(
    state: State<'_, AppState>,
    guild_id: String,
    page_id: String,
) -> Result<Vec<RevisionListItem>, String> {
    validate_path_segment(&guild_id, "guild_id")?;
    validate_path_segment(&page_id, "page_id")?;

    let (server_url, token) = {
        let auth = state.auth.read().await;
        (auth.server_url.clone(), auth.access_token.clone())
    };

    let server_url = server_url.ok_or("Not authenticated")?;
    let token = token.ok_or("Not authenticated")?;

    debug!(
        "Fetching revisions for page: {} in guild: {}",
        page_id, guild_id
    );

    let response = state
        .http
        .get(format!(
            "{server_url}/api/guilds/{guild_id}/pages/{page_id}/revisions"
        ))
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .map_err(|e| {
            error!("Failed to fetch page revisions: {}", e);
            format!("Connection failed: {e}")
        })?;

    if !response.status().is_success() {
        let status = response.status();
        error!("Failed to fetch page revisions: {}", status);
        return Err(format!("Failed to fetch page revisions: {status}"));
    }

    let revisions: Vec<RevisionListItem> = response
        .json()
        .await
        .map_err(|e| format!("Invalid response: {e}"))?;

    debug!(
        "Fetched {} revisions for page: {}",
        revisions.len(),
        page_id
    );
    Ok(revisions)
}

/// Get a specific page revision by number.
#[command]
pub async fn get_page_revision(
    state: State<'_, AppState>,
    guild_id: String,
    page_id: String,
    revision_number: i32,
) -> Result<PageRevision, String> {
    validate_path_segment(&guild_id, "guild_id")?;
    validate_path_segment(&page_id, "page_id")?;

    let (server_url, token) = {
        let auth = state.auth.read().await;
        (auth.server_url.clone(), auth.access_token.clone())
    };

    let server_url = server_url.ok_or("Not authenticated")?;
    let token = token.ok_or("Not authenticated")?;

    debug!(
        "Fetching revision {} for page: {} in guild: {}",
        revision_number, page_id, guild_id
    );

    let response = state
        .http
        .get(format!(
            "{server_url}/api/guilds/{guild_id}/pages/{page_id}/revisions/{revision_number}"
        ))
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .map_err(|e| {
            error!("Failed to fetch page revision: {}", e);
            format!("Connection failed: {e}")
        })?;

    if !response.status().is_success() {
        let status = response.status();
        error!("Failed to fetch page revision: {}", status);
        return Err(format!("Failed to fetch page revision: {status}"));
    }

    let revision: PageRevision = response
        .json()
        .await
        .map_err(|e| format!("Invalid response: {e}"))?;

    debug!("Fetched revision {} for page: {}", revision_number, page_id);
    Ok(revision)
}

/// Restore a page to a specific revision.
#[command]
pub async fn restore_page_revision(
    state: State<'_, AppState>,
    guild_id: String,
    page_id: String,
    revision_number: i32,
) -> Result<Page, String> {
    validate_path_segment(&guild_id, "guild_id")?;
    validate_path_segment(&page_id, "page_id")?;

    let (server_url, token) = {
        let auth = state.auth.read().await;
        (auth.server_url.clone(), auth.access_token.clone())
    };

    let server_url = server_url.ok_or("Not authenticated")?;
    let token = token.ok_or("Not authenticated")?;

    debug!(
        "Restoring page {} to revision {} in guild: {}",
        page_id, revision_number, guild_id
    );

    let response = state
        .http
        .post(format!(
            "{server_url}/api/guilds/{guild_id}/pages/{page_id}/revisions/{revision_number}/restore"
        ))
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .map_err(|e| {
            error!("Failed to restore page revision: {}", e);
            format!("Connection failed: {e}")
        })?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        error!("Failed to restore page revision: {} - {}", status, body);
        return Err(format!("Failed to restore page revision: {status}"));
    }

    let page: Page = response
        .json()
        .await
        .map_err(|e| format!("Invalid response: {e}"))?;

    debug!("Restored page {} to revision {}", page_id, revision_number);
    Ok(page)
}

// ============================================================================
// Page Categories
// ============================================================================

/// Page category.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PageCategory {
    pub id: String,
    pub guild_id: String,
    pub name: String,
    pub position: i32,
    pub created_at: String,
}

/// List page categories for a guild.
#[command]
pub async fn list_page_categories(
    state: State<'_, AppState>,
    guild_id: String,
) -> Result<Vec<PageCategory>, String> {
    validate_path_segment(&guild_id, "guild_id")?;

    let (server_url, token) = {
        let auth = state.auth.read().await;
        (auth.server_url.clone(), auth.access_token.clone())
    };

    let server_url = server_url.ok_or("Not authenticated")?;
    let token = token.ok_or("Not authenticated")?;

    debug!("Fetching page categories for guild: {}", guild_id);

    let response = state
        .http
        .get(format!(
            "{server_url}/api/guilds/{guild_id}/page-categories"
        ))
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .map_err(|e| {
            error!("Failed to fetch page categories: {}", e);
            format!("Connection failed: {e}")
        })?;

    if !response.status().is_success() {
        let status = response.status();
        error!("Failed to fetch page categories: {}", status);
        return Err(format!("Failed to fetch page categories: {status}"));
    }

    let categories: Vec<PageCategory> = response
        .json()
        .await
        .map_err(|e| format!("Invalid response: {e}"))?;

    debug!(
        "Fetched {} page categories for guild: {}",
        categories.len(),
        guild_id
    );
    Ok(categories)
}

/// Create a page category.
#[command]
pub async fn create_page_category(
    state: State<'_, AppState>,
    guild_id: String,
    name: String,
) -> Result<PageCategory, String> {
    validate_path_segment(&guild_id, "guild_id")?;

    let (server_url, token) = {
        let auth = state.auth.read().await;
        (auth.server_url.clone(), auth.access_token.clone())
    };

    let server_url = server_url.ok_or("Not authenticated")?;
    let token = token.ok_or("Not authenticated")?;

    debug!("Creating page category '{}' in guild: {}", name, guild_id);

    let response = state
        .http
        .post(format!(
            "{server_url}/api/guilds/{guild_id}/page-categories"
        ))
        .header("Authorization", format!("Bearer {token}"))
        .json(&serde_json::json!({
            "name": name,
        }))
        .send()
        .await
        .map_err(|e| {
            error!("Failed to create page category: {}", e);
            format!("Connection failed: {e}")
        })?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        error!("Failed to create page category: {} - {}", status, body);
        return Err(format!("Failed to create page category: {status}"));
    }

    let category: PageCategory = response
        .json()
        .await
        .map_err(|e| format!("Invalid response: {e}"))?;

    debug!("Created page category: {}", category.id);
    Ok(category)
}

/// Update a page category.
#[command]
pub async fn update_page_category(
    state: State<'_, AppState>,
    guild_id: String,
    category_id: String,
    name: String,
) -> Result<PageCategory, String> {
    validate_path_segment(&guild_id, "guild_id")?;
    validate_path_segment(&category_id, "category_id")?;

    let (server_url, token) = {
        let auth = state.auth.read().await;
        (auth.server_url.clone(), auth.access_token.clone())
    };

    let server_url = server_url.ok_or("Not authenticated")?;
    let token = token.ok_or("Not authenticated")?;

    debug!(
        "Updating page category: {} in guild: {}",
        category_id, guild_id
    );

    let response = state
        .http
        .patch(format!(
            "{server_url}/api/guilds/{guild_id}/page-categories/{category_id}"
        ))
        .header("Authorization", format!("Bearer {token}"))
        .json(&serde_json::json!({
            "name": name,
        }))
        .send()
        .await
        .map_err(|e| {
            error!("Failed to update page category: {}", e);
            format!("Connection failed: {e}")
        })?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        error!("Failed to update page category: {} - {}", status, body);
        return Err(format!("Failed to update page category: {status}"));
    }

    let category: PageCategory = response
        .json()
        .await
        .map_err(|e| format!("Invalid response: {e}"))?;

    debug!("Updated page category: {}", category.id);
    Ok(category)
}

/// Delete a page category.
#[command]
pub async fn delete_page_category(
    state: State<'_, AppState>,
    guild_id: String,
    category_id: String,
) -> Result<(), String> {
    validate_path_segment(&guild_id, "guild_id")?;
    validate_path_segment(&category_id, "category_id")?;

    let (server_url, token) = {
        let auth = state.auth.read().await;
        (auth.server_url.clone(), auth.access_token.clone())
    };

    let server_url = server_url.ok_or("Not authenticated")?;
    let token = token.ok_or("Not authenticated")?;

    debug!(
        "Deleting page category: {} in guild: {}",
        category_id, guild_id
    );

    let response = state
        .http
        .delete(format!(
            "{server_url}/api/guilds/{guild_id}/page-categories/{category_id}"
        ))
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .map_err(|e| {
            error!("Failed to delete page category: {}", e);
            format!("Connection failed: {e}")
        })?;

    if !response.status().is_success() {
        let status = response.status();
        error!("Failed to delete page category: {}", status);
        return Err(format!("Failed to delete page category: {status}"));
    }

    debug!("Deleted page category: {}", category_id);
    Ok(())
}

/// Reorder page categories.
#[command]
pub async fn reorder_page_categories(
    state: State<'_, AppState>,
    guild_id: String,
    category_ids: Vec<String>,
) -> Result<(), String> {
    validate_path_segment(&guild_id, "guild_id")?;

    let (server_url, token) = {
        let auth = state.auth.read().await;
        (auth.server_url.clone(), auth.access_token.clone())
    };

    let server_url = server_url.ok_or("Not authenticated")?;
    let token = token.ok_or("Not authenticated")?;

    debug!("Reordering page categories in guild: {}", guild_id);

    let response = state
        .http
        .post(format!(
            "{server_url}/api/guilds/{guild_id}/page-categories/reorder"
        ))
        .header("Authorization", format!("Bearer {token}"))
        .json(&serde_json::json!({
            "category_ids": category_ids,
        }))
        .send()
        .await
        .map_err(|e| {
            error!("Failed to reorder page categories: {}", e);
            format!("Connection failed: {e}")
        })?;

    if !response.status().is_success() {
        let status = response.status();
        error!("Failed to reorder page categories: {}", status);
        return Err(format!("Failed to reorder page categories: {status}"));
    }

    debug!("Reordered page categories in guild: {}", guild_id);
    Ok(())
}
