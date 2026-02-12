//! Database queries for information pages.

use chrono::{Duration, Utc};
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use uuid::Uuid;

use crate::pages::{
    Page, PageAcceptance, PageListItem, DELETED_SLUG_COOLDOWN_DAYS, MAX_PAGES_PER_SCOPE,
    RESERVED_SLUGS,
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

    // Truncate to max length without breaking mid-word if possible
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
            sqlx::query_scalar!(
                r#"SELECT COUNT(*) as "count!" FROM pages WHERE guild_id = $1 AND deleted_at IS NULL"#,
                gid
            )
            .fetch_one(pool)
            .await?
        }
        None => {
            sqlx::query_scalar!(
                r#"SELECT COUNT(*) as "count!" FROM pages WHERE guild_id IS NULL AND deleted_at IS NULL"#
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
            sqlx::query_scalar!(
                r#"SELECT EXISTS(
                    SELECT 1 FROM pages
                    WHERE guild_id = $1 AND slug = $2 AND deleted_at IS NULL
                    AND ($3::uuid IS NULL OR id != $3)
                ) as "exists!""#,
                gid,
                slug,
                exclude_id
            )
            .fetch_one(pool)
            .await?
        }
        None => {
            sqlx::query_scalar!(
                r#"SELECT EXISTS(
                    SELECT 1 FROM pages
                    WHERE guild_id IS NULL AND slug = $1 AND deleted_at IS NULL
                    AND ($2::uuid IS NULL OR id != $2)
                ) as "exists!""#,
                slug,
                exclude_id
            )
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
            sqlx::query_scalar!(
                r#"SELECT EXISTS(
                    SELECT 1 FROM pages
                    WHERE guild_id = $1 AND slug = $2
                    AND deleted_at IS NOT NULL AND deleted_at > $3
                ) as "exists!""#,
                gid,
                slug,
                cutoff
            )
            .fetch_one(pool)
            .await?
        }
        None => {
            sqlx::query_scalar!(
                r#"SELECT EXISTS(
                    SELECT 1 FROM pages
                    WHERE guild_id IS NULL AND slug = $1
                    AND deleted_at IS NOT NULL AND deleted_at > $2
                ) as "exists!""#,
                slug,
                cutoff
            )
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
            sqlx::query_as!(
                PageListItem,
                r"SELECT id, guild_id, title, slug, position, requires_acceptance, updated_at
                FROM pages WHERE guild_id = $1 AND deleted_at IS NULL
                ORDER BY position",
                gid
            )
            .fetch_all(pool)
            .await?
        }
        None => {
            sqlx::query_as!(
                PageListItem,
                r"SELECT id, guild_id, title, slug, position, requires_acceptance, updated_at
                FROM pages WHERE guild_id IS NULL AND deleted_at IS NULL
                ORDER BY position"
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
                sqlx::query_as!(
                    Page,
                    r"SELECT * FROM pages WHERE guild_id = $1 AND slug = $2 AND deleted_at IS NULL",
                    gid,
                    slug
                )
                .fetch_optional(pool)
                .await?
            }
            None => sqlx::query_as!(
                Page,
                r"SELECT * FROM pages WHERE guild_id IS NULL AND slug = $1 AND deleted_at IS NULL",
                slug
            )
            .fetch_optional(pool)
            .await?,
        };
    Ok(page)
}

/// Get page by ID (excludes soft-deleted pages).
pub async fn get_page_by_id(pool: &PgPool, id: Uuid) -> Result<Option<Page>, sqlx::Error> {
    let page: Option<Page> = sqlx::query_as!(
        Page,
        r"SELECT * FROM pages WHERE id = $1 AND deleted_at IS NULL",
        id,
    )
    .fetch_optional(pool)
    .await?;
    Ok(page)
}

/// Create a new page.
#[allow(clippy::too_many_arguments)]
pub async fn create_page(
    pool: &PgPool,
    guild_id: Option<Uuid>,
    title: &str,
    slug: &str,
    content: &str,
    requires_acceptance: bool,
    created_by: Uuid,
) -> Result<Page, sqlx::Error> {
    let content_hash = hash_content(content);
    let position = count_pages(pool, guild_id).await? as i32;

    let page: Page = sqlx::query_as!(
        Page,
        r"INSERT INTO pages (guild_id, title, slug, content, content_hash, position, requires_acceptance, created_by, updated_by)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $8)
        RETURNING *",
        guild_id,
        title,
        slug,
        content,
        content_hash,
        position,
        requires_acceptance,
        created_by
    )
    .fetch_one(pool)
    .await?;

    Ok(page)
}

/// Update an existing page.
#[allow(clippy::too_many_arguments)]
pub async fn update_page(
    pool: &PgPool,
    id: Uuid,
    title: Option<&str>,
    slug: Option<&str>,
    content: Option<&str>,
    requires_acceptance: Option<bool>,
    updated_by: Uuid,
) -> Result<Page, sqlx::Error> {
    let page = get_page_by_id(pool, id)
        .await?
        .ok_or(sqlx::Error::RowNotFound)?;

    let new_title = title.unwrap_or(&page.title);
    let new_slug = slug.unwrap_or(&page.slug);
    let new_content = content.unwrap_or(&page.content);
    let new_requires_acceptance = requires_acceptance.unwrap_or(page.requires_acceptance);
    let new_content_hash = if content.is_some() {
        hash_content(new_content)
    } else {
        page.content_hash.clone()
    };

    let updated_page: Page = sqlx::query_as!(
        Page,
        r"UPDATE pages SET
            title = $2, slug = $3, content = $4, content_hash = $5,
            requires_acceptance = $6, updated_by = $7, updated_at = NOW()
        WHERE id = $1 RETURNING *",
        id,
        new_title,
        new_slug,
        new_content,
        new_content_hash,
        new_requires_acceptance,
        updated_by
    )
    .fetch_one(pool)
    .await?;

    Ok(updated_page)
}

/// Soft delete a page.
pub async fn soft_delete_page(pool: &PgPool, id: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query!(r"UPDATE pages SET deleted_at = NOW() WHERE id = $1", id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Restore a soft-deleted page.
pub async fn restore_page(pool: &PgPool, id: Uuid) -> Result<Page, sqlx::Error> {
    let page: Page = sqlx::query_as!(
        Page,
        r"UPDATE pages SET deleted_at = NULL WHERE id = $1 RETURNING *",
        id
    )
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
            sqlx::query_scalar!(
                r#"SELECT COUNT(*) as "count!" FROM pages WHERE guild_id = $1 AND deleted_at IS NULL"#,
                gid
            )
            .fetch_one(&mut *tx)
            .await?
        }
        None => {
            sqlx::query_scalar!(
                r#"SELECT COUNT(*) as "count!" FROM pages WHERE guild_id IS NULL AND deleted_at IS NULL"#
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
        sqlx::query!(
            r"UPDATE pages SET position = $2 WHERE id = $1",
            page_id,
            position as i32
        )
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
    sqlx::query!(
        r"INSERT INTO page_acceptances (user_id, page_id, content_hash)
        VALUES ($1, $2, $3)
        ON CONFLICT (user_id, page_id) DO UPDATE SET content_hash = $3, accepted_at = NOW()",
        user_id,
        page_id,
        content_hash
    )
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
    let acceptance: Option<PageAcceptance> = sqlx::query_as!(
        PageAcceptance,
        r"SELECT * FROM page_acceptances WHERE user_id = $1 AND page_id = $2",
        user_id,
        page_id
    )
    .fetch_optional(pool)
    .await?;

    Ok(acceptance)
}

/// Get pages requiring acceptance that user hasn't accepted (or accepted old version).
pub async fn get_pending_acceptance(
    pool: &PgPool,
    user_id: Uuid,
) -> Result<Vec<PageListItem>, sqlx::Error> {
    let pages: Vec<PageListItem> = sqlx::query_as!(
        PageListItem,
        r"SELECT p.id, p.guild_id, p.title, p.slug, p.position, p.requires_acceptance, p.updated_at
        FROM pages p
        WHERE p.requires_acceptance = true AND p.deleted_at IS NULL
        AND NOT EXISTS (
            SELECT 1 FROM page_acceptances pa
            WHERE pa.page_id = p.id AND pa.user_id = $1 AND pa.content_hash = p.content_hash
        )
        ORDER BY p.guild_id NULLS FIRST, p.position",
        user_id
    )
    .fetch_all(pool)
    .await?;

    Ok(pages)
}

/// Check if scope has reached maximum pages limit.
pub async fn is_at_page_limit(pool: &PgPool, guild_id: Option<Uuid>) -> Result<bool, sqlx::Error> {
    let count = count_pages(pool, guild_id).await?;
    Ok(count >= MAX_PAGES_PER_SCOPE as i64)
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

        assert!(!is_reserved_slug("terms-of-service"));
        assert!(!is_reserved_slug("rules"));
        assert!(!is_reserved_slug("faq"));
    }
}
