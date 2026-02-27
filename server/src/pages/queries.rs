//! Database queries for information pages.

use chrono::{Duration, Utc};
use sha2::{Digest, Sha256};
use sqlx::{Executor, PgPool, Postgres};
use uuid::Uuid;

use crate::pages::{
    Page, PageAcceptance, PageCategory, PageListItem, PageRevision, RevisionListItem,
    DELETED_SLUG_COOLDOWN_DAYS, RESERVED_SLUGS,
};

/// Generate SHA-256 hash of content for version tracking.
#[must_use]
pub fn hash_content(content: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    hex::encode(hasher.finalize())
}

/// Maximum slug length.
const MAX_SLUG_LENGTH: usize = 100;

/// Generate URL-friendly slug from title.
#[must_use]
pub fn slugify(title: &str) -> String {
    let slug: String = title
        .to_lowercase()
        .chars()
        // Only keep ASCII alphanumeric characters (matches frontend behavior)
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-");

    // Truncate to max length (hard truncation at character boundary)
    if slug.len() <= MAX_SLUG_LENGTH {
        slug
    } else {
        slug.chars().take(MAX_SLUG_LENGTH).collect()
    }
}

/// Check if slug is a reserved system path.
#[must_use]
pub fn is_reserved_slug(slug: &str) -> bool {
    RESERVED_SLUGS.contains(&slug)
}

/// Count active pages in a scope (guild or platform).
pub async fn count_pages(pool: &PgPool, guild_id: Option<Uuid>) -> Result<i64, sqlx::Error> {
    let count: i64 = match guild_id {
        Some(gid) => {
            sqlx::query_scalar(
                r"SELECT COUNT(*) FROM pages WHERE guild_id = $1 AND deleted_at IS NULL",
            )
            .bind(gid)
            .fetch_one(pool)
            .await?
        }
        None => {
            sqlx::query_scalar(
                r"SELECT COUNT(*) FROM pages WHERE guild_id IS NULL AND deleted_at IS NULL",
            )
            .fetch_one(pool)
            .await?
        }
    };
    Ok(count)
}

/// Check if slug exists in scope (optionally excluding a specific page ID).
pub async fn slug_exists(
    pool: &PgPool,
    guild_id: Option<Uuid>,
    slug: &str,
    exclude_id: Option<Uuid>,
) -> Result<bool, sqlx::Error> {
    let exists: bool = match guild_id {
        Some(gid) => {
            sqlx::query_scalar(
                r"SELECT EXISTS(
                    SELECT 1 FROM pages
                    WHERE guild_id = $1 AND slug = $2 AND deleted_at IS NULL
                    AND ($3::uuid IS NULL OR id != $3)
                )",
            )
            .bind(gid)
            .bind(slug)
            .bind(exclude_id)
            .fetch_one(pool)
            .await?
        }
        None => {
            sqlx::query_scalar(
                r"SELECT EXISTS(
                    SELECT 1 FROM pages
                    WHERE guild_id IS NULL AND slug = $1 AND deleted_at IS NULL
                    AND ($2::uuid IS NULL OR id != $2)
                )",
            )
            .bind(slug)
            .bind(exclude_id)
            .fetch_one(pool)
            .await?
        }
    };
    Ok(exists)
}

/// Check if slug was recently deleted (cooldown period).
pub async fn slug_recently_deleted(
    pool: &PgPool,
    guild_id: Option<Uuid>,
    slug: &str,
) -> Result<bool, sqlx::Error> {
    let cutoff = Utc::now() - Duration::days(DELETED_SLUG_COOLDOWN_DAYS);

    let exists: bool = match guild_id {
        Some(gid) => {
            sqlx::query_scalar(
                r"SELECT EXISTS(
                    SELECT 1 FROM pages
                    WHERE guild_id = $1 AND slug = $2
                    AND deleted_at IS NOT NULL AND deleted_at > $3
                )",
            )
            .bind(gid)
            .bind(slug)
            .bind(cutoff)
            .fetch_one(pool)
            .await?
        }
        None => {
            sqlx::query_scalar(
                r"SELECT EXISTS(
                    SELECT 1 FROM pages
                    WHERE guild_id IS NULL AND slug = $1
                    AND deleted_at IS NOT NULL AND deleted_at > $2
                )",
            )
            .bind(slug)
            .bind(cutoff)
            .fetch_one(pool)
            .await?
        }
    };
    Ok(exists)
}

/// List active pages for a scope ordered by position.
pub async fn list_pages(
    pool: &PgPool,
    guild_id: Option<Uuid>,
) -> Result<Vec<PageListItem>, sqlx::Error> {
    let pages: Vec<PageListItem> = match guild_id {
        Some(gid) => {
            sqlx::query_as(
                r"SELECT id, guild_id, title, slug, position, requires_acceptance, category_id, updated_at
                FROM pages WHERE guild_id = $1 AND deleted_at IS NULL
                ORDER BY position",
            )
            .bind(gid)
            .fetch_all(pool)
            .await?
        }
        None => {
            sqlx::query_as(
                r"SELECT id, guild_id, title, slug, position, requires_acceptance, category_id, updated_at
                FROM pages WHERE guild_id IS NULL AND deleted_at IS NULL
                ORDER BY position",
            )
            .fetch_all(pool)
            .await?
        }
    };
    Ok(pages)
}

/// Get page by slug.
pub async fn get_page_by_slug(
    pool: &PgPool,
    guild_id: Option<Uuid>,
    slug: &str,
) -> Result<Option<Page>, sqlx::Error> {
    let page: Option<Page> =
        match guild_id {
            Some(gid) => {
                sqlx::query_as(
                    r"SELECT * FROM pages WHERE guild_id = $1 AND slug = $2 AND deleted_at IS NULL",
                )
                .bind(gid)
                .bind(slug)
                .fetch_optional(pool)
                .await?
            }
            None => sqlx::query_as(
                r"SELECT * FROM pages WHERE guild_id IS NULL AND slug = $1 AND deleted_at IS NULL",
            )
            .bind(slug)
            .fetch_optional(pool)
            .await?,
        };
    Ok(page)
}

/// Get page by ID (excludes soft-deleted pages).
pub async fn get_page_by_id(pool: &PgPool, id: Uuid) -> Result<Option<Page>, sqlx::Error> {
    let page: Option<Page> =
        sqlx::query_as(r"SELECT * FROM pages WHERE id = $1 AND deleted_at IS NULL")
            .bind(id)
            .fetch_optional(pool)
            .await?;
    Ok(page)
}

/// Parameters for creating a new page.
pub struct CreatePageParams<'a> {
    pub guild_id: Option<Uuid>,
    pub title: &'a str,
    pub slug: &'a str,
    pub content: &'a str,
    pub requires_acceptance: bool,
    pub category_id: Option<Uuid>,
    pub created_by: Uuid,
}

/// Create a new page.
///
/// Position is computed atomically via an inline subquery to prevent
/// duplicate positions under concurrent inserts in the same scope.
pub async fn create_page(pool: &PgPool, params: CreatePageParams<'_>) -> Result<Page, sqlx::Error> {
    let content_hash = hash_content(params.content);

    let page: Page = sqlx::query_as(
        r"INSERT INTO pages (guild_id, title, slug, content, content_hash, position, requires_acceptance, category_id, created_by, updated_by)
        VALUES ($1, $2, $3, $4, $5,
            (SELECT COUNT(*)::int FROM pages WHERE guild_id IS NOT DISTINCT FROM $1 AND deleted_at IS NULL),
            $6, $7, $8, $8)
        RETURNING *",
    )
    .bind(params.guild_id)
    .bind(params.title)
    .bind(params.slug)
    .bind(params.content)
    .bind(&content_hash)
    .bind(params.requires_acceptance)
    .bind(params.category_id)
    .bind(params.created_by)
    .fetch_one(pool)
    .await?;

    Ok(page)
}

/// Create a page with initial revision using an existing mutable transaction.
pub async fn create_page_with_initial_revision_in_tx(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    params: CreatePageParams<'_>,
) -> Result<Page, sqlx::Error> {
    let content_hash = hash_content(params.content);

    let page: Page = sqlx::query_as(
        r"INSERT INTO pages (guild_id, title, slug, content, content_hash, position, requires_acceptance, category_id, created_by, updated_by)
        VALUES ($1, $2, $3, $4, $5,
            (SELECT COUNT(*)::int FROM pages WHERE guild_id IS NOT DISTINCT FROM $1 AND deleted_at IS NULL),
            $6, $7, $8, $8)
        RETURNING *",
    )
    .bind(params.guild_id)
    .bind(params.title)
    .bind(params.slug)
    .bind(params.content)
    .bind(&content_hash)
    .bind(params.requires_acceptance)
    .bind(params.category_id)
    .bind(params.created_by)
    .fetch_one(&mut **tx)
    .await?;

    create_revision_with_executor(
        &mut **tx,
        page.id,
        &page.content,
        &page.content_hash,
        &page.title,
        params.created_by,
    )
    .await?;

    Ok(page)
}

/// Parameters for updating an existing page.
pub struct UpdatePageParams<'a> {
    pub id: Uuid,
    pub title: Option<&'a str>,
    pub slug: Option<&'a str>,
    pub content: Option<&'a str>,
    pub requires_acceptance: Option<bool>,
    /// `None` = no change, `Some(None)` = remove category, `Some(Some(id))` = set category.
    pub category_id: Option<Option<Uuid>>,
    pub updated_by: Uuid,
}

/// Update an existing page.
pub async fn update_page(pool: &PgPool, params: UpdatePageParams<'_>) -> Result<Page, sqlx::Error> {
    let page = get_page_by_id(pool, params.id)
        .await?
        .ok_or(sqlx::Error::RowNotFound)?;

    let new_title = params.title.unwrap_or(&page.title);
    let new_slug = params.slug.unwrap_or(&page.slug);
    let new_content = params.content.unwrap_or(&page.content);
    let new_requires_acceptance = params
        .requires_acceptance
        .unwrap_or(page.requires_acceptance);
    let new_content_hash = if params.content.is_some() {
        hash_content(new_content)
    } else {
        page.content_hash.clone()
    };
    let new_category_id = match params.category_id {
        Some(cat) => cat,
        None => page.category_id,
    };

    let updated_page: Page = sqlx::query_as(
        r"UPDATE pages SET
            title = $2, slug = $3, content = $4, content_hash = $5,
            requires_acceptance = $6, category_id = $7, updated_by = $8, updated_at = NOW()
        WHERE id = $1 RETURNING *",
    )
    .bind(params.id)
    .bind(new_title)
    .bind(new_slug)
    .bind(new_content)
    .bind(&new_content_hash)
    .bind(new_requires_acceptance)
    .bind(new_category_id)
    .bind(params.updated_by)
    .fetch_one(pool)
    .await?;

    Ok(updated_page)
}

/// Soft delete a page.
pub async fn soft_delete_page(pool: &PgPool, id: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query(r"UPDATE pages SET deleted_at = NOW() WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Restore a soft-deleted page.
pub async fn restore_page(pool: &PgPool, id: Uuid) -> Result<Page, sqlx::Error> {
    let page: Page =
        sqlx::query_as(r"UPDATE pages SET deleted_at = NULL WHERE id = $1 RETURNING *")
            .bind(id)
            .fetch_one(pool)
            .await?;

    Ok(page)
}

/// Reorder pages by updating their positions.
///
/// Verifies all page IDs belong to the specified scope before reordering.
/// Wrapped in a transaction to prevent partial updates on failure.
pub async fn reorder_pages(
    pool: &PgPool,
    guild_id: Option<Uuid>,
    page_ids: &[Uuid],
) -> Result<(), sqlx::Error> {
    // Check for duplicate IDs
    let unique: std::collections::HashSet<&Uuid> = page_ids.iter().collect();
    if unique.len() != page_ids.len() {
        return Err(sqlx::Error::Protocol(
            "Duplicate page IDs in reorder request".to_string(),
        ));
    }

    let mut tx = pool.begin().await?;

    // Verify all pages belong to the correct scope and count matches
    let existing_count: i64 = match guild_id {
        Some(gid) => {
            sqlx::query_scalar(
                r"SELECT COUNT(*) FROM pages WHERE guild_id = $1 AND deleted_at IS NULL",
            )
            .bind(gid)
            .fetch_one(&mut *tx)
            .await?
        }
        None => {
            sqlx::query_scalar(
                r"SELECT COUNT(*) FROM pages WHERE guild_id IS NULL AND deleted_at IS NULL",
            )
            .fetch_one(&mut *tx)
            .await?
        }
    };

    if page_ids.len() as i64 != existing_count {
        return Err(sqlx::Error::Protocol(
            "Page count mismatch during reorder".to_string(),
        ));
    }

    // Verify all provided page IDs actually belong to this scope (security check)
    let valid_count: i64 = match guild_id {
        Some(gid) => {
            sqlx::query_scalar(
                r"SELECT COUNT(*) FROM pages
                   WHERE id = ANY($1) AND guild_id = $2 AND deleted_at IS NULL",
            )
            .bind(page_ids)
            .bind(gid)
            .fetch_one(&mut *tx)
            .await?
        }
        None => {
            sqlx::query_scalar(
                r"SELECT COUNT(*) FROM pages
                   WHERE id = ANY($1) AND guild_id IS NULL AND deleted_at IS NULL",
            )
            .bind(page_ids)
            .fetch_one(&mut *tx)
            .await?
        }
    };

    if valid_count != page_ids.len() as i64 {
        return Err(sqlx::Error::Protocol(
            "Invalid page IDs: some pages do not belong to this scope".to_string(),
        ));
    }

    // Now safe to update positions
    for (position, page_id) in page_ids.iter().enumerate() {
        sqlx::query(r"UPDATE pages SET position = $2 WHERE id = $1")
            .bind(page_id)
            .bind(position as i32)
            .execute(&mut *tx)
            .await?;
    }

    tx.commit().await?;
    Ok(())
}

/// Log an audit event for a page.
pub async fn log_audit(
    pool: &PgPool,
    page_id: Uuid,
    action: &str,
    actor_id: Uuid,
    previous_content_hash: Option<&str>,
    ip_address: Option<&str>,
    user_agent: Option<&str>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r"INSERT INTO page_audit_log (page_id, action, actor_id, previous_content_hash, ip_address, user_agent)
        VALUES ($1, $2, $3, $4, $5::inet, $6)",
    )
    .bind(page_id)
    .bind(action)
    .bind(actor_id)
    .bind(previous_content_hash)
    .bind(ip_address)
    .bind(user_agent)
    .execute(pool)
    .await?;
    Ok(())
}

/// Record user acceptance of a page.
pub async fn accept_page(
    pool: &PgPool,
    user_id: Uuid,
    page_id: Uuid,
    content_hash: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r"INSERT INTO page_acceptances (user_id, page_id, content_hash)
        VALUES ($1, $2, $3)
        ON CONFLICT (user_id, page_id) DO UPDATE SET content_hash = $3, accepted_at = NOW()",
    )
    .bind(user_id)
    .bind(page_id)
    .bind(content_hash)
    .execute(pool)
    .await?;
    Ok(())
}

/// Get user's acceptance record for a page.
pub async fn get_acceptance(
    pool: &PgPool,
    user_id: Uuid,
    page_id: Uuid,
) -> Result<Option<PageAcceptance>, sqlx::Error> {
    let acceptance: Option<PageAcceptance> =
        sqlx::query_as(r"SELECT * FROM page_acceptances WHERE user_id = $1 AND page_id = $2")
            .bind(user_id)
            .bind(page_id)
            .fetch_optional(pool)
            .await?;

    Ok(acceptance)
}

/// Get pages requiring acceptance that user hasn't accepted (or accepted old version).
pub async fn get_pending_acceptance(
    pool: &PgPool,
    user_id: Uuid,
) -> Result<Vec<PageListItem>, sqlx::Error> {
    let pages: Vec<PageListItem> = sqlx::query_as(
        r"SELECT p.id, p.guild_id, p.title, p.slug, p.position, p.requires_acceptance, p.category_id, p.updated_at
        FROM pages p
        WHERE p.requires_acceptance = true AND p.deleted_at IS NULL
        AND NOT EXISTS (
            SELECT 1 FROM page_acceptances pa
            WHERE pa.page_id = p.id AND pa.user_id = $1 AND pa.content_hash = p.content_hash
        )
        ORDER BY p.guild_id NULLS FIRST, p.position",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await?;

    Ok(pages)
}

/// Check if scope has reached maximum pages limit.
pub async fn is_at_page_limit(
    pool: &PgPool,
    guild_id: Option<Uuid>,
    max_limit: i64,
) -> Result<bool, sqlx::Error> {
    let count = count_pages(pool, guild_id).await?;
    Ok(count >= max_limit)
}

// ========================================================================
// Effective Limits (per-guild overrides)
// ========================================================================

/// Get the effective page limit for a guild, checking per-guild override first.
///
/// Uses `fetch_one` because the guild must exist (validated by caller).
/// The column is nullable, so `Option<i32>` handles NULL (no override) correctly.
pub async fn get_effective_page_limit(
    pool: &PgPool,
    guild_id: Uuid,
    config_default: i64,
) -> Result<i64, sqlx::Error> {
    let override_val: Option<i32> =
        sqlx::query_scalar(r"SELECT max_pages FROM guilds WHERE id = $1")
            .bind(guild_id)
            .fetch_one(pool)
            .await?;

    Ok(override_val.map(i64::from).unwrap_or(config_default))
}

/// Get the effective revision limit for a guild, checking per-guild override first.
///
/// Uses `fetch_one` because the guild must exist (validated by caller).
/// The column is nullable, so `Option<i32>` handles NULL (no override) correctly.
pub async fn get_effective_revision_limit(
    pool: &PgPool,
    guild_id: Uuid,
    config_default: i64,
) -> Result<i64, sqlx::Error> {
    let override_val: Option<i32> =
        sqlx::query_scalar(r"SELECT max_revisions FROM guilds WHERE id = $1")
            .bind(guild_id)
            .fetch_one(pool)
            .await?;

    Ok(override_val.map(i64::from).unwrap_or(config_default))
}

// ========================================================================
// Revision Queries
// ========================================================================

/// Create a new revision snapshot.
///
/// Computes the next revision number via an inline subquery within the INSERT.
/// Retries transient unique-collision races on `uq_page_revision` a few times
/// to reduce snapshot loss under concurrent edits.
pub async fn create_revision(
    pool: &PgPool,
    page_id: Uuid,
    content: &str,
    content_hash: &str,
    title: &str,
    created_by: Uuid,
) -> Result<PageRevision, sqlx::Error> {
    const MAX_RETRIES: usize = 3;
    for attempt in 0..MAX_RETRIES {
        match create_revision_with_executor(pool, page_id, content, content_hash, title, created_by)
            .await
        {
            Ok(revision) => return Ok(revision),
            Err(sqlx::Error::Database(db_err))
                if db_err.is_unique_violation() && attempt + 1 < MAX_RETRIES => {}
            Err(err) => return Err(err),
        }
    }

    unreachable!("revision retry loop should always return before exhausting");
}

async fn create_revision_with_executor<'e, E>(
    executor: E,
    page_id: Uuid,
    content: &str,
    content_hash: &str,
    title: &str,
    created_by: Uuid,
) -> Result<PageRevision, sqlx::Error>
where
    E: Executor<'e, Database = Postgres>,
{
    let revision: PageRevision = sqlx::query_as(
        r"INSERT INTO page_revisions (page_id, revision_number, content, content_hash, title, created_by)
        VALUES ($1, COALESCE((SELECT MAX(revision_number) FROM page_revisions WHERE page_id = $1), 0) + 1, $2, $3, $4, $5)
        RETURNING *",
    )
    .bind(page_id)
    .bind(content)
    .bind(content_hash)
    .bind(title)
    .bind(created_by)
    .fetch_one(executor)
    .await?;

    Ok(revision)
}

/// List revisions for a page (metadata only, newest first).
///
/// Capped at 200 rows to prevent oversized responses if per-guild
/// revision limits are set very high.
pub async fn list_revisions(
    pool: &PgPool,
    page_id: Uuid,
) -> Result<Vec<RevisionListItem>, sqlx::Error> {
    let revisions: Vec<RevisionListItem> = sqlx::query_as(
        r"SELECT id, page_id, revision_number, content_hash, title, created_by, created_at
        FROM page_revisions WHERE page_id = $1
        ORDER BY revision_number DESC
        LIMIT 200",
    )
    .bind(page_id)
    .fetch_all(pool)
    .await?;

    Ok(revisions)
}

/// Get a specific revision by number.
pub async fn get_revision(
    pool: &PgPool,
    page_id: Uuid,
    revision_number: i32,
) -> Result<Option<PageRevision>, sqlx::Error> {
    let revision: Option<PageRevision> =
        sqlx::query_as(r"SELECT * FROM page_revisions WHERE page_id = $1 AND revision_number = $2")
            .bind(page_id)
            .bind(revision_number)
            .fetch_optional(pool)
            .await?;

    Ok(revision)
}

/// Delete revisions beyond the limit (keeps the newest `max_revisions`).
/// Returns the number of deleted revisions.
pub async fn prune_revisions(
    pool: &PgPool,
    page_id: Uuid,
    max_revisions: i64,
) -> Result<u64, sqlx::Error> {
    let result = sqlx::query(
        r"DELETE FROM page_revisions
        WHERE page_id = $1 AND revision_number NOT IN (
            SELECT revision_number FROM page_revisions
            WHERE page_id = $1
            ORDER BY revision_number DESC
            LIMIT $2
        )",
    )
    .bind(page_id)
    .bind(max_revisions)
    .execute(pool)
    .await?;

    Ok(result.rows_affected())
}

// ========================================================================
// Category Queries
// ========================================================================

/// List categories for a guild ordered by position.
pub async fn list_categories(
    pool: &PgPool,
    guild_id: Uuid,
) -> Result<Vec<PageCategory>, sqlx::Error> {
    let categories: Vec<PageCategory> =
        sqlx::query_as(r"SELECT * FROM page_categories WHERE guild_id = $1 ORDER BY position")
            .bind(guild_id)
            .fetch_all(pool)
            .await?;

    Ok(categories)
}

/// Get a single category by ID.
pub async fn get_category(
    pool: &PgPool,
    category_id: Uuid,
) -> Result<Option<PageCategory>, sqlx::Error> {
    let category: Option<PageCategory> =
        sqlx::query_as(r"SELECT * FROM page_categories WHERE id = $1")
            .bind(category_id)
            .fetch_optional(pool)
            .await?;

    Ok(category)
}

/// Create a new page category.
///
/// Uses an atomic INSERT with inline subquery to prevent position race conditions.
pub async fn create_category(
    pool: &PgPool,
    guild_id: Uuid,
    name: &str,
) -> Result<PageCategory, sqlx::Error> {
    let category: PageCategory = sqlx::query_as(
        r"INSERT INTO page_categories (guild_id, name, position)
        VALUES ($1, $2, (SELECT COUNT(*)::int FROM page_categories WHERE guild_id = $1))
        RETURNING *",
    )
    .bind(guild_id)
    .bind(name)
    .fetch_one(pool)
    .await?;

    Ok(category)
}

/// Update a category's name.
pub async fn update_category(
    pool: &PgPool,
    category_id: Uuid,
    name: &str,
) -> Result<PageCategory, sqlx::Error> {
    let category: PageCategory =
        sqlx::query_as(r"UPDATE page_categories SET name = $2 WHERE id = $1 RETURNING *")
            .bind(category_id)
            .bind(name)
            .fetch_one(pool)
            .await?;

    Ok(category)
}

/// Delete a category. Pages with this category become uncategorized (ON DELETE SET NULL).
pub async fn delete_category(pool: &PgPool, category_id: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query(r"DELETE FROM page_categories WHERE id = $1")
        .bind(category_id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Reorder categories by updating their positions.
pub async fn reorder_categories(
    pool: &PgPool,
    guild_id: Uuid,
    category_ids: &[Uuid],
) -> Result<(), sqlx::Error> {
    let unique: std::collections::HashSet<&Uuid> = category_ids.iter().collect();
    if unique.len() != category_ids.len() {
        return Err(sqlx::Error::Protocol(
            "Duplicate category IDs in reorder request".to_string(),
        ));
    }

    let mut tx = pool.begin().await?;

    let existing_count: i64 =
        sqlx::query_scalar(r"SELECT COUNT(*) FROM page_categories WHERE guild_id = $1")
            .bind(guild_id)
            .fetch_one(&mut *tx)
            .await?;

    if category_ids.len() as i64 != existing_count {
        return Err(sqlx::Error::Protocol(
            "Category count mismatch during reorder".to_string(),
        ));
    }

    let valid_count: i64 = sqlx::query_scalar(
        r"SELECT COUNT(*) FROM page_categories WHERE id = ANY($1) AND guild_id = $2",
    )
    .bind(category_ids)
    .bind(guild_id)
    .fetch_one(&mut *tx)
    .await?;

    if valid_count != category_ids.len() as i64 {
        return Err(sqlx::Error::Protocol(
            "Invalid category IDs: some do not belong to this guild".to_string(),
        ));
    }

    for (position, cat_id) in category_ids.iter().enumerate() {
        sqlx::query(r"UPDATE page_categories SET position = $2 WHERE id = $1")
            .bind(cat_id)
            .bind(position as i32)
            .execute(&mut *tx)
            .await?;
    }

    tx.commit().await?;
    Ok(())
}

/// Count categories in a guild.
pub async fn count_categories(pool: &PgPool, guild_id: Uuid) -> Result<i64, sqlx::Error> {
    let count: i64 =
        sqlx::query_scalar(r"SELECT COUNT(*) FROM page_categories WHERE guild_id = $1")
            .bind(guild_id)
            .fetch_one(pool)
            .await?;

    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hash_content() {
        let hash = hash_content("Hello, World!");
        assert_eq!(hash.len(), 64); // SHA-256 produces 64 hex chars
        assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));

        // Same content produces same hash
        assert_eq!(hash, hash_content("Hello, World!"));

        // Different content produces different hash
        assert_ne!(hash, hash_content("Hello, World"));
    }

    #[test]
    fn test_slugify() {
        assert_eq!(slugify("Hello World"), "hello-world");
        assert_eq!(slugify("Terms of Service"), "terms-of-service");
        assert_eq!(slugify("FAQ & Help"), "faq-help");
        assert_eq!(slugify("  Leading  Spaces  "), "leading-spaces");
        assert_eq!(slugify("Multiple---Dashes"), "multiple-dashes");
        assert_eq!(slugify("123 Numbers"), "123-numbers");
    }

    #[test]
    fn test_is_reserved_slug() {
        assert!(is_reserved_slug("admin"));
        assert!(is_reserved_slug("api"));
        assert!(is_reserved_slug("new"));
        assert!(is_reserved_slug("settings"));

        assert!(is_reserved_slug("library"));
        assert!(is_reserved_slug("revisions"));
        assert!(is_reserved_slug("categories"));

        assert!(!is_reserved_slug("terms-of-service"));
        assert!(!is_reserved_slug("rules"));
        assert!(!is_reserved_slug("faq"));
    }
}
