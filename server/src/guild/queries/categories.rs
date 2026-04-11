//! Channel-category queries.
//!
//! All functions return `Result<T, CategoryError>` so handlers can use `?`
//! directly.

use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

use super::super::categories::{Category, CategoryError, CategoryType};

/// List all categories in a guild ordered by `position`.
pub async fn list_categories(
    pool: &PgPool,
    guild_id: Uuid,
) -> Result<Vec<Category>, CategoryError> {
    let categories = sqlx::query_as::<_, Category>(
        r"
        SELECT id, guild_id, name, position, parent_id, category_type, created_at
        FROM channel_categories
        WHERE guild_id = $1
        ORDER BY position
        ",
    )
    .bind(guild_id)
    .fetch_all(pool)
    .await?;
    Ok(categories)
}

/// Look up the `parent_id` of a category.
///
/// Used to enforce the 2-level nesting constraint. Returns `None` if no such
/// category exists, `Some(None)` if the category is top-level, and
/// `Some(Some(parent))` if it already has a parent.
pub async fn fetch_category_parent(
    pool: &PgPool,
    parent_id: Uuid,
    guild_id: Uuid,
) -> Result<Option<Option<Uuid>>, CategoryError> {
    let row: Option<(Option<Uuid>,)> =
        sqlx::query_as("SELECT parent_id FROM channel_categories WHERE id = $1 AND guild_id = $2")
            .bind(parent_id)
            .bind(guild_id)
            .fetch_optional(pool)
            .await?;
    Ok(row.map(|(p,)| p))
}

/// Transactional variant of [`fetch_category_parent`] used by the reorder
/// endpoint.
pub async fn fetch_category_parent_tx(
    tx: &mut Transaction<'_, Postgres>,
    parent_id: Uuid,
    guild_id: Uuid,
) -> Result<Option<Option<Uuid>>, CategoryError> {
    let row: Option<(Option<Uuid>,)> =
        sqlx::query_as("SELECT parent_id FROM channel_categories WHERE id = $1 AND guild_id = $2")
            .bind(parent_id)
            .bind(guild_id)
            .fetch_optional(&mut **tx)
            .await?;
    Ok(row.map(|(p,)| p))
}

/// Check whether a category has any subcategories.
pub async fn has_subcategories(pool: &PgPool, category_id: Uuid) -> Result<bool, CategoryError> {
    let exists = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(SELECT 1 FROM channel_categories WHERE parent_id = $1)",
    )
    .bind(category_id)
    .fetch_one(pool)
    .await?;
    Ok(exists)
}

/// Check whether a category contains channels of the given type. Used when
/// changing `category_type` to make sure no conflicting channels remain.
pub async fn category_has_channel_type(
    pool: &PgPool,
    category_id: Uuid,
    channel_type: &str,
) -> Result<bool, CategoryError> {
    let exists = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(SELECT 1 FROM channels WHERE category_id = $1 AND channel_type::TEXT = $2)",
    )
    .bind(category_id)
    .bind(channel_type)
    .fetch_one(pool)
    .await?;
    Ok(exists)
}

/// Insert a new category with auto-computed `position`.
pub async fn insert_category(
    pool: &PgPool,
    category_id: Uuid,
    guild_id: Uuid,
    name: &str,
    parent_id: Option<Uuid>,
    category_type: CategoryType,
) -> Result<Category, CategoryError> {
    let category = sqlx::query_as::<_, Category>(
        r"
        INSERT INTO channel_categories (id, guild_id, name, parent_id, category_type, position)
        VALUES ($1, $2, $3, $4, $5, (
            SELECT COALESCE(MAX(position) + 1, 0)
            FROM channel_categories
            WHERE guild_id = $2 AND parent_id IS NOT DISTINCT FROM $4
        ))
        RETURNING id, guild_id, name, position, parent_id, category_type, created_at
        ",
    )
    .bind(category_id)
    .bind(guild_id)
    .bind(name)
    .bind(parent_id)
    .bind(category_type)
    .fetch_one(pool)
    .await?;
    Ok(category)
}

/// Apply a partial update to a category.
///
/// `parent_id_present` toggles whether the `parent_id` column should be
/// updated at all (so that `Some(None)` clears the parent and `None` keeps it).
#[allow(clippy::too_many_arguments)]
pub async fn update_category(
    pool: &PgPool,
    category_id: Uuid,
    guild_id: Uuid,
    name: Option<&str>,
    position: Option<i32>,
    parent_id_present: bool,
    parent_id: Option<Uuid>,
    category_type: Option<CategoryType>,
) -> Result<Category, CategoryError> {
    let category = sqlx::query_as::<_, Category>(
        r"
        UPDATE channel_categories
        SET
            name = COALESCE($3, name),
            position = COALESCE($4, position),
            parent_id = CASE WHEN $5 THEN $6 ELSE parent_id END,
            category_type = COALESCE($7, category_type)
        WHERE id = $1 AND guild_id = $2
        RETURNING id, guild_id, name, position, parent_id, category_type, created_at
        ",
    )
    .bind(category_id)
    .bind(guild_id)
    .bind(name)
    .bind(position)
    .bind(parent_id_present)
    .bind(parent_id)
    .bind(category_type)
    .fetch_optional(pool)
    .await?
    .ok_or(CategoryError::NotFound)?;
    Ok(category)
}

/// Delete a category. Returns the number of rows affected so callers can
/// distinguish "not found" from a successful delete.
pub async fn delete_category(
    pool: &PgPool,
    category_id: Uuid,
    guild_id: Uuid,
) -> Result<u64, CategoryError> {
    let result = sqlx::query("DELETE FROM channel_categories WHERE id = $1 AND guild_id = $2")
        .bind(category_id)
        .bind(guild_id)
        .execute(pool)
        .await?;
    Ok(result.rows_affected())
}

/// Update a category's position and parent inside a transaction (used by the
/// reorder endpoint).
pub async fn update_category_position(
    tx: &mut Transaction<'_, Postgres>,
    category_id: Uuid,
    guild_id: Uuid,
    position: i32,
    parent_id: Option<Uuid>,
) -> Result<(), CategoryError> {
    sqlx::query(
        r"
        UPDATE channel_categories
        SET position = $3, parent_id = $4
        WHERE id = $1 AND guild_id = $2
        ",
    )
    .bind(category_id)
    .bind(guild_id)
    .bind(position)
    .bind(parent_id)
    .execute(&mut **tx)
    .await?;
    Ok(())
}
