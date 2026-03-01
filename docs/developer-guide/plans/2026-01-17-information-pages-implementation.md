# Information Pages Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Implement information pages at platform and guild level for ToS, rules, FAQ with markdown support, mermaid diagrams, and acceptance tracking.

**Architecture:** Pages stored in PostgreSQL with audit logging. Platform pages managed by system admins (existing `system_admins` table), guild pages by users with MANAGE_PAGES permission. Frontend uses Solid.js stores with secure markdown rendering via marked + DOMPurify.

**Tech Stack:** Rust (Axum), PostgreSQL (sqlx), Solid.js, marked, mermaid, DOMPurify

**Design Doc:** `docs/plans/2026-01-16-information-pages-design.md`

**Last Updated:** 2026-01-18 (Review refinements)

---

## Pre-Implementation Notes

### Key Codebase Discoveries (from 2026-01-18 review)

1. **Platform Admin:** Use existing `system_admins` table (NOT new `platform_roles`)
   - Table already exists in `20260113000001_permission_system.sql`
   - Query: `SELECT 1 FROM system_admins WHERE user_id = $1`
2. **Package Manager:** Use `bun` (NOT `npm`)
3. **`marked` already installed:** Skip in dependency install step
4. **Permission bit 21:** Available for `MANAGE_PAGES`

---

## Phase 1: Database & Backend Foundation

### Task 1: Database Migration - Tables

**Files:**
- Create: `server/migrations/20260118000000_information_pages.sql`

**Step 1: Write the migration**

> **Note:** We reuse existing `system_admins` table for platform admin checks. No need to create `platform_roles`.

```sql
-- server/migrations/20260118000000_information_pages.sql

-- Pages table (platform pages have guild_id = NULL)
-- Platform admin checks use existing system_admins table
CREATE TABLE pages (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v7(),
    guild_id UUID REFERENCES guilds(id) ON DELETE CASCADE,

    title VARCHAR(100) NOT NULL,
    slug VARCHAR(100) NOT NULL
        CONSTRAINT slug_format CHECK (slug ~ '^[a-z0-9]([a-z0-9\-]*[a-z0-9])?$'),

    content TEXT NOT NULL,
    content_hash VARCHAR(64) NOT NULL,  -- SHA-256 for version tracking

    position INT NOT NULL DEFAULT 0,
    requires_acceptance BOOLEAN DEFAULT FALSE,

    created_by UUID NOT NULL REFERENCES users(id),
    updated_by UUID NOT NULL REFERENCES users(id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    deleted_at TIMESTAMPTZ
);

-- Unique slug per guild (or platform)
CREATE UNIQUE INDEX idx_pages_unique_slug
    ON pages(COALESCE(guild_id, '00000000-0000-0000-0000-000000000000'::uuid), slug)
    WHERE deleted_at IS NULL;

-- Fast lookup by position
CREATE INDEX idx_pages_guild_position ON pages(guild_id, position) WHERE deleted_at IS NULL;
CREATE INDEX idx_pages_platform_position ON pages(position) WHERE guild_id IS NULL AND deleted_at IS NULL;

-- Page audit log
CREATE TABLE page_audit_log (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v7(),
    page_id UUID NOT NULL REFERENCES pages(id) ON DELETE CASCADE,
    action VARCHAR(20) NOT NULL,  -- 'create', 'update', 'delete', 'restore'
    actor_id UUID NOT NULL REFERENCES users(id),
    previous_content_hash VARCHAR(64),
    ip_address INET,
    user_agent TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT valid_action CHECK (action IN ('create', 'update', 'delete', 'restore'))
);

CREATE INDEX idx_page_audit_log_page ON page_audit_log(page_id);
CREATE INDEX idx_page_audit_log_actor ON page_audit_log(actor_id);

-- User acceptance tracking
CREATE TABLE page_acceptances (
    user_id UUID REFERENCES users(id) ON DELETE CASCADE,
    page_id UUID REFERENCES pages(id) ON DELETE CASCADE,
    content_hash VARCHAR(64) NOT NULL,  -- Hash at time of acceptance
    accepted_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (user_id, page_id)
);

-- Role-based visibility for guild pages
CREATE TABLE page_visibility (
    page_id UUID REFERENCES pages(id) ON DELETE CASCADE,
    role_id UUID REFERENCES guild_roles(id) ON DELETE CASCADE,
    PRIMARY KEY (page_id, role_id)
);

-- Trigger: Ensure role belongs to same guild as page
CREATE OR REPLACE FUNCTION check_page_visibility_guild()
RETURNS TRIGGER AS $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pages p
        JOIN guild_roles gr ON gr.guild_id = p.guild_id
        WHERE p.id = NEW.page_id AND gr.id = NEW.role_id
    ) THEN
        RAISE EXCEPTION 'Role must belong to same guild as page';
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER page_visibility_guild_check
    BEFORE INSERT OR UPDATE ON page_visibility
    FOR EACH ROW EXECUTE FUNCTION check_page_visibility_guild();

-- Note: Platform admin management uses existing system_admins table
-- No INSERT needed - admins are already managed via that table
```

**Step 2: Run migration**

Run: `cd server && sqlx migrate run`
Expected: Migration succeeds

**Step 3: Commit**

```bash
git add server/migrations/
git commit -m "feat(pages): add database schema for information pages"
```

---

### Task 2: Add MANAGE_PAGES Permission

**Files:**
- Modify: `server/src/permissions/guild.rs`

**Step 1: Add MANAGE_PAGES to GuildPermissions**

Add after `MANAGE_INVITES` (bit 20):

```rust
// === Pages (bit 21) ===
/// Permission to create, edit, delete, and reorder pages
const MANAGE_PAGES       = 1 << 21;
```

**Step 2: Add to OFFICER_DEFAULT preset**

Update `OFFICER_DEFAULT`:
```rust
pub const OFFICER_DEFAULT: Self = Self::MODERATOR_DEFAULT
    .union(Self::BAN_MEMBERS)
    .union(Self::MANAGE_CHANNELS)
    .union(Self::MANAGE_PAGES);
```

**Step 3: Update test for bit positions**

Add to `test_no_bit_overlaps`:
```rust
GuildPermissions::MANAGE_PAGES,
```

**Step 4: Run tests**

Run: `cd server && cargo test permissions`
Expected: All tests pass

**Step 5: Commit**

```bash
git commit -am "feat(permissions): add MANAGE_PAGES permission"
```

---

### Task 3: Backend Types & Constants

**Files:**
- Create: `server/src/pages/mod.rs`
- Create: `server/src/pages/types.rs`
- Create: `server/src/pages/constants.rs`
- Modify: `server/src/lib.rs`

**Step 1: Create module structure**

```rust
// server/src/pages/mod.rs
pub mod constants;
pub mod types;

pub use constants::*;
pub use types::*;
```

**Step 2: Create constants**

```rust
// server/src/pages/constants.rs

/// Maximum pages per scope (guild or platform)
pub const MAX_PAGES_PER_SCOPE: usize = 10;

/// Maximum content size in bytes (100KB)
pub const MAX_CONTENT_SIZE: usize = 102_400;

/// Maximum title length
pub const MAX_TITLE_LENGTH: usize = 100;

/// Maximum slug length
pub const MAX_SLUG_LENGTH: usize = 100;

/// Deleted slug cooldown in days
pub const DELETED_SLUG_COOLDOWN_DAYS: i64 = 7;

/// Reserved slugs that cannot be used
pub const RESERVED_SLUGS: &[&str] = &[
    "admin", "api", "new", "edit", "delete", "settings",
    "create", "update", "list", "all", "me", "system"
];
```

**Step 3: Create types**

```rust
// server/src/pages/types.rs

use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct Page {
    pub id: Uuid,
    pub guild_id: Option<Uuid>,
    pub title: String,
    pub slug: String,
    pub content: String,
    pub content_hash: String,
    pub position: i32,
    pub requires_acceptance: bool,
    pub created_by: Uuid,
    pub updated_by: Uuid,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339::option")]
    pub deleted_at: Option<OffsetDateTime>,
}

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct PageListItem {
    pub id: Uuid,
    pub guild_id: Option<Uuid>,
    pub title: String,
    pub slug: String,
    pub position: i32,
    pub requires_acceptance: bool,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
}

#[derive(Debug, Deserialize)]
pub struct CreatePageRequest {
    pub title: String,
    pub slug: Option<String>,  // Auto-generated if not provided
    pub content: String,
    pub requires_acceptance: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct UpdatePageRequest {
    pub title: Option<String>,
    pub slug: Option<String>,
    pub content: Option<String>,
    pub requires_acceptance: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct ReorderRequest {
    pub page_ids: Vec<Uuid>,
}

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct PageAcceptance {
    pub user_id: Uuid,
    pub page_id: Uuid,
    pub content_hash: String,
    #[serde(with = "time::serde::rfc3339")]
    pub accepted_at: OffsetDateTime,
}

// Note: Platform admin checks use existing system_admins table
// No separate PlatformRole type needed
```

**Step 4: Add module to lib.rs**

```rust
pub mod pages;
```

**Step 5: Verify compilation**

Run: `cd server && cargo check`
Expected: Compiles

**Step 6: Commit**

```bash
git add server/src/pages/ server/src/lib.rs
git commit -m "feat(pages): add types and constants"
```

---

### Task 4: Backend Queries

**Files:**
- Create: `server/src/pages/queries.rs`
- Modify: `server/src/pages/mod.rs`

**Step 1: Create database queries**

```rust
// server/src/pages/queries.rs

use sqlx::PgPool;
use uuid::Uuid;
use sha2::{Sha256, Digest};

use crate::pages::{
    Page, PageListItem, PageAcceptance,
    RESERVED_SLUGS, DELETED_SLUG_COOLDOWN_DAYS, MAX_PAGES_PER_SCOPE,
};

/// Generate SHA-256 hash of content
pub fn hash_content(content: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    format!("{:x}", hasher.finalize())
}

/// Generate slug from title
pub fn slugify(title: &str) -> String {
    title
        .to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}

/// Check if slug is reserved
pub fn is_reserved_slug(slug: &str) -> bool {
    RESERVED_SLUGS.contains(&slug)
}

/// Check if user is platform admin (uses existing system_admins table)
pub async fn is_platform_admin(pool: &PgPool, user_id: Uuid) -> Result<bool, sqlx::Error> {
    let exists = sqlx::query_scalar!(
        r#"SELECT EXISTS(SELECT 1 FROM system_admins WHERE user_id = $1) as "exists!""#,
        user_id
    )
    .fetch_one(pool)
    .await?;

    Ok(exists)
}

/// Count pages in scope
pub async fn count_pages(pool: &PgPool, guild_id: Option<Uuid>) -> Result<i64, sqlx::Error> {
    let count = match guild_id {
        Some(gid) => sqlx::query_scalar!(
            r#"SELECT COUNT(*) as "count!" FROM pages WHERE guild_id = $1 AND deleted_at IS NULL"#,
            gid
        ).fetch_one(pool).await?,
        None => sqlx::query_scalar!(
            r#"SELECT COUNT(*) as "count!" FROM pages WHERE guild_id IS NULL AND deleted_at IS NULL"#
        ).fetch_one(pool).await?,
    };
    Ok(count)
}

/// Check if slug exists in scope
pub async fn slug_exists(
    pool: &PgPool,
    guild_id: Option<Uuid>,
    slug: &str,
    exclude_id: Option<Uuid>,
) -> Result<bool, sqlx::Error> {
    let exists = match guild_id {
        Some(gid) => sqlx::query_scalar!(
            r#"SELECT EXISTS(
                SELECT 1 FROM pages
                WHERE guild_id = $1 AND slug = $2 AND deleted_at IS NULL
                AND ($3::uuid IS NULL OR id != $3)
            ) as "exists!""#,
            gid, slug, exclude_id
        ).fetch_one(pool).await?,
        None => sqlx::query_scalar!(
            r#"SELECT EXISTS(
                SELECT 1 FROM pages
                WHERE guild_id IS NULL AND slug = $2 AND deleted_at IS NULL
                AND ($3::uuid IS NULL OR id != $3)
            ) as "exists!""#,
            slug, exclude_id
        ).fetch_one(pool).await?,
    };
    Ok(exists)
}

/// Check for recently deleted slug (cooldown period)
pub async fn slug_recently_deleted(
    pool: &PgPool,
    guild_id: Option<Uuid>,
    slug: &str,
) -> Result<bool, sqlx::Error> {
    let cutoff = time::OffsetDateTime::now_utc()
        - time::Duration::days(DELETED_SLUG_COOLDOWN_DAYS);

    let exists = match guild_id {
        Some(gid) => sqlx::query_scalar!(
            r#"SELECT EXISTS(
                SELECT 1 FROM pages
                WHERE guild_id = $1 AND slug = $2
                AND deleted_at IS NOT NULL AND deleted_at > $3
            ) as "exists!""#,
            gid, slug, cutoff
        ).fetch_one(pool).await?,
        None => sqlx::query_scalar!(
            r#"SELECT EXISTS(
                SELECT 1 FROM pages
                WHERE guild_id IS NULL AND slug = $2
                AND deleted_at IS NOT NULL AND deleted_at > $3
            ) as "exists!""#,
            slug, cutoff
        ).fetch_one(pool).await?,
    };
    Ok(exists)
}

/// List pages for scope
pub async fn list_pages(
    pool: &PgPool,
    guild_id: Option<Uuid>,
) -> Result<Vec<PageListItem>, sqlx::Error> {
    let pages = match guild_id {
        Some(gid) => sqlx::query_as!(
            PageListItem,
            r#"SELECT id, guild_id, title, slug, position, requires_acceptance, updated_at
            FROM pages WHERE guild_id = $1 AND deleted_at IS NULL
            ORDER BY position"#,
            gid
        ).fetch_all(pool).await?,
        None => sqlx::query_as!(
            PageListItem,
            r#"SELECT id, guild_id, title, slug, position, requires_acceptance, updated_at
            FROM pages WHERE guild_id IS NULL AND deleted_at IS NULL
            ORDER BY position"#
        ).fetch_all(pool).await?,
    };
    Ok(pages)
}

/// Get page by slug
pub async fn get_page_by_slug(
    pool: &PgPool,
    guild_id: Option<Uuid>,
    slug: &str,
) -> Result<Option<Page>, sqlx::Error> {
    let page = match guild_id {
        Some(gid) => sqlx::query_as!(
            Page,
            r#"SELECT * FROM pages WHERE guild_id = $1 AND slug = $2 AND deleted_at IS NULL"#,
            gid, slug
        ).fetch_optional(pool).await?,
        None => sqlx::query_as!(
            Page,
            r#"SELECT * FROM pages WHERE guild_id IS NULL AND slug = $1 AND deleted_at IS NULL"#,
            slug
        ).fetch_optional(pool).await?,
    };
    Ok(page)
}

/// Get page by ID
pub async fn get_page_by_id(pool: &PgPool, id: Uuid) -> Result<Option<Page>, sqlx::Error> {
    sqlx::query_as!(Page, r#"SELECT * FROM pages WHERE id = $1"#, id)
        .fetch_optional(pool)
        .await
}

/// Create page
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

    sqlx::query_as!(
        Page,
        r#"INSERT INTO pages (guild_id, title, slug, content, content_hash, position, requires_acceptance, created_by, updated_by)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $8)
        RETURNING *"#,
        guild_id, title, slug, content, content_hash, position, requires_acceptance, created_by
    )
    .fetch_one(pool)
    .await
}

/// Update page
pub async fn update_page(
    pool: &PgPool,
    id: Uuid,
    title: Option<&str>,
    slug: Option<&str>,
    content: Option<&str>,
    requires_acceptance: Option<bool>,
    updated_by: Uuid,
) -> Result<Page, sqlx::Error> {
    let page = get_page_by_id(pool, id).await?.ok_or(sqlx::Error::RowNotFound)?;

    let new_title = title.unwrap_or(&page.title);
    let new_slug = slug.unwrap_or(&page.slug);
    let new_content = content.unwrap_or(&page.content);
    let new_requires_acceptance = requires_acceptance.unwrap_or(page.requires_acceptance);
    let new_content_hash = if content.is_some() { hash_content(new_content) } else { page.content_hash.clone() };

    sqlx::query_as!(
        Page,
        r#"UPDATE pages SET
            title = $2, slug = $3, content = $4, content_hash = $5,
            requires_acceptance = $6, updated_by = $7, updated_at = NOW()
        WHERE id = $1 RETURNING *"#,
        id, new_title, new_slug, new_content, new_content_hash, new_requires_acceptance, updated_by
    )
    .fetch_one(pool)
    .await
}

/// Soft delete page
pub async fn soft_delete_page(pool: &PgPool, id: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query!(r#"UPDATE pages SET deleted_at = NOW() WHERE id = $1"#, id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Restore deleted page
pub async fn restore_page(pool: &PgPool, id: Uuid) -> Result<Page, sqlx::Error> {
    sqlx::query_as!(
        Page,
        r#"UPDATE pages SET deleted_at = NULL WHERE id = $1 RETURNING *"#,
        id
    )
    .fetch_one(pool)
    .await
}

/// Reorder pages
pub async fn reorder_pages(
    pool: &PgPool,
    guild_id: Option<Uuid>,
    page_ids: &[Uuid],
) -> Result<(), sqlx::Error> {
    for (position, page_id) in page_ids.iter().enumerate() {
        sqlx::query!(
            r#"UPDATE pages SET position = $2 WHERE id = $1"#,
            page_id, position as i32
        )
        .execute(pool)
        .await?;
    }
    Ok(())
}

/// Log audit event
pub async fn log_audit(
    pool: &PgPool,
    page_id: Uuid,
    action: &str,
    actor_id: Uuid,
    previous_content_hash: Option<&str>,
    ip_address: Option<&str>,
    user_agent: Option<&str>,
) -> Result<(), sqlx::Error> {
    let ip: Option<std::net::IpAddr> = ip_address.and_then(|s| s.parse().ok());

    sqlx::query!(
        r#"INSERT INTO page_audit_log (page_id, action, actor_id, previous_content_hash, ip_address, user_agent)
        VALUES ($1, $2, $3, $4, $5, $6)"#,
        page_id, action, actor_id, previous_content_hash, ip, user_agent
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// Record page acceptance
pub async fn accept_page(
    pool: &PgPool,
    user_id: Uuid,
    page_id: Uuid,
    content_hash: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        r#"INSERT INTO page_acceptances (user_id, page_id, content_hash)
        VALUES ($1, $2, $3)
        ON CONFLICT (user_id, page_id) DO UPDATE SET content_hash = $3, accepted_at = NOW()"#,
        user_id, page_id, content_hash
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// Get pending acceptance pages for user
pub async fn get_pending_acceptance(
    pool: &PgPool,
    user_id: Uuid,
) -> Result<Vec<PageListItem>, sqlx::Error> {
    sqlx::query_as!(
        PageListItem,
        r#"SELECT p.id, p.guild_id, p.title, p.slug, p.position, p.requires_acceptance, p.updated_at
        FROM pages p
        WHERE p.requires_acceptance = true AND p.deleted_at IS NULL
        AND NOT EXISTS (
            SELECT 1 FROM page_acceptances pa
            WHERE pa.page_id = p.id AND pa.user_id = $1 AND pa.content_hash = p.content_hash
        )
        ORDER BY p.guild_id NULLS FIRST, p.position"#,
        user_id
    )
    .fetch_all(pool)
    .await
}
```

**Step 2: Update mod.rs**

```rust
pub mod queries;
pub use queries::*;
```

**Step 3: Verify compilation**

Run: `cd server && cargo check`

**Step 4: Commit**

```bash
git add server/src/pages/
git commit -m "feat(pages): add database queries"
```

---

### Task 5: Backend API Handlers

**Files:**
- Create: `server/src/pages/handlers.rs`
- Create: `server/src/pages/router.rs`
- Modify: `server/src/pages/mod.rs`
- Modify: `server/src/api/mod.rs`

**Step 1: Create handlers**

```rust
// server/src/pages/handlers.rs

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use uuid::Uuid;

use crate::{
    api::AppState,
    auth::AuthUser,
    pages::{
        queries, CreatePageRequest, UpdatePageRequest, ReorderRequest,
        Page, PageListItem, RESERVED_SLUGS, MAX_CONTENT_SIZE, MAX_PAGES_PER_SCOPE,
    },
    permissions::{compute_guild_permissions, GuildPermissions},
};

// ============================================================================
// Platform Pages (admin only)
// ============================================================================

pub async fn list_platform_pages(
    State(state): State<AppState>,
) -> Result<Json<Vec<PageListItem>>, (StatusCode, String)> {
    let pages = queries::list_pages(&state.db, None)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(pages))
}

pub async fn get_platform_page(
    State(state): State<AppState>,
    Path(slug): Path<String>,
) -> Result<Json<Page>, (StatusCode, String)> {
    queries::get_page_by_slug(&state.db, None, &slug)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .map(Json)
        .ok_or((StatusCode::NOT_FOUND, "Page not found".to_string()))
}

pub async fn create_platform_page(
    State(state): State<AppState>,
    user: AuthUser,
    Json(req): Json<CreatePageRequest>,
) -> Result<Json<Page>, (StatusCode, String)> {
    // Verify platform admin
    if !queries::is_platform_admin(&state.db, user.id).await.unwrap_or(false) {
        return Err((StatusCode::FORBIDDEN, "Platform admin required".to_string()));
    }

    // Validate
    if req.content.len() > MAX_CONTENT_SIZE {
        return Err((StatusCode::BAD_REQUEST, "Content too large".to_string()));
    }

    let slug = req.slug.clone().unwrap_or_else(|| queries::slugify(&req.title));

    if queries::is_reserved_slug(&slug) {
        return Err((StatusCode::BAD_REQUEST, "Reserved slug".to_string()));
    }

    if queries::slug_exists(&state.db, None, &slug, None).await.unwrap_or(true) {
        return Err((StatusCode::CONFLICT, "Slug already exists".to_string()));
    }

    if queries::count_pages(&state.db, None).await.unwrap_or(MAX_PAGES_PER_SCOPE as i64) >= MAX_PAGES_PER_SCOPE as i64 {
        return Err((StatusCode::BAD_REQUEST, "Maximum pages reached".to_string()));
    }

    let page = queries::create_page(
        &state.db, None, &req.title, &slug, &req.content,
        req.requires_acceptance.unwrap_or(false), user.id
    ).await.map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    queries::log_audit(&state.db, page.id, "create", user.id, None, None, None).await.ok();

    Ok(Json(page))
}

pub async fn update_platform_page(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<Uuid>,
    Json(req): Json<UpdatePageRequest>,
) -> Result<Json<Page>, (StatusCode, String)> {
    if !queries::is_platform_admin(&state.db, user.id).await.unwrap_or(false) {
        return Err((StatusCode::FORBIDDEN, "Platform admin required".to_string()));
    }

    let old_page = queries::get_page_by_id(&state.db, id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .ok_or((StatusCode::NOT_FOUND, "Page not found".to_string()))?;

    if old_page.guild_id.is_some() {
        return Err((StatusCode::BAD_REQUEST, "Not a platform page".to_string()));
    }

    if let Some(ref content) = req.content {
        if content.len() > MAX_CONTENT_SIZE {
            return Err((StatusCode::BAD_REQUEST, "Content too large".to_string()));
        }
    }

    if let Some(ref slug) = req.slug {
        if queries::is_reserved_slug(slug) {
            return Err((StatusCode::BAD_REQUEST, "Reserved slug".to_string()));
        }
        if queries::slug_exists(&state.db, None, slug, Some(id)).await.unwrap_or(true) {
            return Err((StatusCode::CONFLICT, "Slug already exists".to_string()));
        }
    }

    let page = queries::update_page(
        &state.db, id,
        req.title.as_deref(),
        req.slug.as_deref(),
        req.content.as_deref(),
        req.requires_acceptance,
        user.id
    ).await.map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    queries::log_audit(&state.db, id, "update", user.id, Some(&old_page.content_hash), None, None).await.ok();

    Ok(Json(page))
}

pub async fn delete_platform_page(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, (StatusCode, String)> {
    if !queries::is_platform_admin(&state.db, user.id).await.unwrap_or(false) {
        return Err((StatusCode::FORBIDDEN, "Platform admin required".to_string()));
    }

    let page = queries::get_page_by_id(&state.db, id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .ok_or((StatusCode::NOT_FOUND, "Page not found".to_string()))?;

    if page.guild_id.is_some() {
        return Err((StatusCode::BAD_REQUEST, "Not a platform page".to_string()));
    }

    queries::soft_delete_page(&state.db, id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    queries::log_audit(&state.db, id, "delete", user.id, Some(&page.content_hash), None, None).await.ok();

    Ok(StatusCode::NO_CONTENT)
}

pub async fn reorder_platform_pages(
    State(state): State<AppState>,
    user: AuthUser,
    Json(req): Json<ReorderRequest>,
) -> Result<StatusCode, (StatusCode, String)> {
    if !queries::is_platform_admin(&state.db, user.id).await.unwrap_or(false) {
        return Err((StatusCode::FORBIDDEN, "Platform admin required".to_string()));
    }

    queries::reorder_pages(&state.db, None, &req.page_ids)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(StatusCode::NO_CONTENT)
}

// ============================================================================
// Guild Pages
// ============================================================================

async fn check_manage_pages_permission(
    state: &AppState,
    guild_id: Uuid,
    user_id: Uuid,
) -> Result<(), (StatusCode, String)> {
    let perms = compute_guild_permissions(&state.db, guild_id, user_id)
        .await
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Permission check failed".to_string()))?;

    if !perms.has(GuildPermissions::MANAGE_PAGES) {
        return Err((StatusCode::FORBIDDEN, "MANAGE_PAGES permission required".to_string()));
    }
    Ok(())
}

pub async fn list_guild_pages(
    State(state): State<AppState>,
    Path(guild_id): Path<Uuid>,
) -> Result<Json<Vec<PageListItem>>, (StatusCode, String)> {
    let pages = queries::list_pages(&state.db, Some(guild_id))
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(pages))
}

pub async fn get_guild_page(
    State(state): State<AppState>,
    Path((guild_id, slug)): Path<(Uuid, String)>,
) -> Result<Json<Page>, (StatusCode, String)> {
    queries::get_page_by_slug(&state.db, Some(guild_id), &slug)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .map(Json)
        .ok_or((StatusCode::NOT_FOUND, "Page not found".to_string()))
}

pub async fn create_guild_page(
    State(state): State<AppState>,
    user: AuthUser,
    Path(guild_id): Path<Uuid>,
    Json(req): Json<CreatePageRequest>,
) -> Result<Json<Page>, (StatusCode, String)> {
    check_manage_pages_permission(&state, guild_id, user.id).await?;

    if req.content.len() > MAX_CONTENT_SIZE {
        return Err((StatusCode::BAD_REQUEST, "Content too large".to_string()));
    }

    let slug = req.slug.clone().unwrap_or_else(|| queries::slugify(&req.title));

    if queries::is_reserved_slug(&slug) {
        return Err((StatusCode::BAD_REQUEST, "Reserved slug".to_string()));
    }

    if queries::slug_exists(&state.db, Some(guild_id), &slug, None).await.unwrap_or(true) {
        return Err((StatusCode::CONFLICT, "Slug already exists".to_string()));
    }

    if queries::count_pages(&state.db, Some(guild_id)).await.unwrap_or(MAX_PAGES_PER_SCOPE as i64) >= MAX_PAGES_PER_SCOPE as i64 {
        return Err((StatusCode::BAD_REQUEST, "Maximum pages reached".to_string()));
    }

    let page = queries::create_page(
        &state.db, Some(guild_id), &req.title, &slug, &req.content,
        req.requires_acceptance.unwrap_or(false), user.id
    ).await.map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    queries::log_audit(&state.db, page.id, "create", user.id, None, None, None).await.ok();

    Ok(Json(page))
}

pub async fn update_guild_page(
    State(state): State<AppState>,
    user: AuthUser,
    Path((guild_id, id)): Path<(Uuid, Uuid)>,
    Json(req): Json<UpdatePageRequest>,
) -> Result<Json<Page>, (StatusCode, String)> {
    check_manage_pages_permission(&state, guild_id, user.id).await?;

    let old_page = queries::get_page_by_id(&state.db, id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .ok_or((StatusCode::NOT_FOUND, "Page not found".to_string()))?;

    if old_page.guild_id != Some(guild_id) {
        return Err((StatusCode::NOT_FOUND, "Page not found".to_string()));
    }

    if let Some(ref content) = req.content {
        if content.len() > MAX_CONTENT_SIZE {
            return Err((StatusCode::BAD_REQUEST, "Content too large".to_string()));
        }
    }

    if let Some(ref slug) = req.slug {
        if queries::is_reserved_slug(slug) {
            return Err((StatusCode::BAD_REQUEST, "Reserved slug".to_string()));
        }
        if queries::slug_exists(&state.db, Some(guild_id), slug, Some(id)).await.unwrap_or(true) {
            return Err((StatusCode::CONFLICT, "Slug already exists".to_string()));
        }
    }

    let page = queries::update_page(
        &state.db, id,
        req.title.as_deref(),
        req.slug.as_deref(),
        req.content.as_deref(),
        req.requires_acceptance,
        user.id
    ).await.map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    queries::log_audit(&state.db, id, "update", user.id, Some(&old_page.content_hash), None, None).await.ok();

    Ok(Json(page))
}

pub async fn delete_guild_page(
    State(state): State<AppState>,
    user: AuthUser,
    Path((guild_id, id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, (StatusCode, String)> {
    check_manage_pages_permission(&state, guild_id, user.id).await?;

    let page = queries::get_page_by_id(&state.db, id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .ok_or((StatusCode::NOT_FOUND, "Page not found".to_string()))?;

    if page.guild_id != Some(guild_id) {
        return Err((StatusCode::NOT_FOUND, "Page not found".to_string()));
    }

    queries::soft_delete_page(&state.db, id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    queries::log_audit(&state.db, id, "delete", user.id, Some(&page.content_hash), None, None).await.ok();

    Ok(StatusCode::NO_CONTENT)
}

pub async fn reorder_guild_pages(
    State(state): State<AppState>,
    user: AuthUser,
    Path(guild_id): Path<Uuid>,
    Json(req): Json<ReorderRequest>,
) -> Result<StatusCode, (StatusCode, String)> {
    check_manage_pages_permission(&state, guild_id, user.id).await?;

    queries::reorder_pages(&state.db, Some(guild_id), &req.page_ids)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(StatusCode::NO_CONTENT)
}

// ============================================================================
// Acceptance
// ============================================================================

pub async fn accept_page(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, (StatusCode, String)> {
    let page = queries::get_page_by_id(&state.db, id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .ok_or((StatusCode::NOT_FOUND, "Page not found".to_string()))?;

    queries::accept_page(&state.db, user.id, id, &page.content_hash)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(StatusCode::NO_CONTENT)
}

pub async fn get_pending_acceptance(
    State(state): State<AppState>,
    user: AuthUser,
) -> Result<Json<Vec<PageListItem>>, (StatusCode, String)> {
    let pages = queries::get_pending_acceptance(&state.db, user.id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(pages))
}
```

**Step 2: Create router**

```rust
// server/src/pages/router.rs

use axum::{
    routing::{get, post, patch, delete},
    Router,
};

use crate::api::AppState;
use super::handlers;

pub fn platform_pages_router() -> Router<AppState> {
    Router::new()
        .route("/", get(handlers::list_platform_pages).post(handlers::create_platform_page))
        .route("/:slug", get(handlers::get_platform_page))
        .route("/:id", patch(handlers::update_platform_page).delete(handlers::delete_platform_page))
        .route("/reorder", post(handlers::reorder_platform_pages))
}

pub fn guild_pages_router() -> Router<AppState> {
    Router::new()
        .route("/", get(handlers::list_guild_pages).post(handlers::create_guild_page))
        .route("/:slug", get(handlers::get_guild_page))
        .route("/:id", patch(handlers::update_guild_page).delete(handlers::delete_guild_page))
        .route("/reorder", post(handlers::reorder_guild_pages))
}

pub fn acceptance_router() -> Router<AppState> {
    Router::new()
        .route("/:id/accept", post(handlers::accept_page))
        .route("/pending-acceptance", get(handlers::get_pending_acceptance))
}
```

**Step 3: Update pages mod.rs**

```rust
pub mod constants;
pub mod handlers;
pub mod queries;
pub mod router;
pub mod types;

pub use constants::*;
pub use queries::*;
pub use router::*;
pub use types::*;
```

**Step 4: Integrate with main router**

Update `server/src/api/mod.rs`:

```rust
use crate::pages;

// In create_router(), add to protected_routes:
.nest("/api/pages", pages::platform_pages_router())
.nest("/api/pages", pages::acceptance_router())
.nest("/api/guilds/:guild_id/pages", pages::guild_pages_router())
```

**Step 5: Verify and commit**

Run: `cd server && cargo check`

```bash
git add server/src/pages/ server/src/api/mod.rs
git commit -m "feat(pages): add API handlers and routes"
```

---

## Phase 2: Tauri Commands & Frontend Store

### Task 6: Tauri Commands

**Files:**
- Create: `client/src-tauri/src/commands/pages.rs`
- Modify: `client/src-tauri/src/commands/mod.rs`
- Modify: `client/src-tauri/src/main.rs`

See design doc for full implementation of Tauri commands wrapping all API endpoints.

**Step 1: Create pages commands file with all commands**
**Step 2: Register commands in mod.rs and main.rs**
**Step 3: Verify and commit**

```bash
git add client/src-tauri/src/commands/
git commit -m "feat(pages): add Tauri commands"
```

---

### Task 7: TypeScript Types & Tauri Bindings

**Files:**
- Modify: `client/src/lib/types.ts`
- Modify: `client/src/lib/tauri.ts`

Add TypeScript interfaces for `Page`, `PageListItem` and Tauri invoke wrappers for all commands.

**Step 1: Add types to types.ts**
**Step 2: Add invoke wrappers to tauri.ts**
**Step 3: Commit**

```bash
git commit -am "feat(pages): add TypeScript types and Tauri bindings"
```

---

### Task 8: Pages Store

**Files:**
- Create: `client/src/stores/pages.ts`

Create Solid.js store following existing patterns (createStore, exported state + action functions).

**Step 1: Create store with all state and actions**
**Step 2: Commit**

```bash
git add client/src/stores/pages.ts
git commit -m "feat(pages): add pages store"
```

---

## Phase 3: Frontend Components

### Task 9: Markdown Renderer

**Files:**
- Create: `client/src/components/pages/MarkdownPreview.tsx`

**Step 1: Install dependencies**

> **Note:** `marked` is already installed. Only install missing packages.

Run: `cd client && bun add dompurify mermaid && bun add -d @types/dompurify`

**Step 2: Create secure markdown renderer**

Use DOMPurify for HTML sanitization with allowlist, mermaid for diagrams with strict security level.

**Step 3: Commit**

```bash
git add client/src/components/pages/
git commit -m "feat(pages): add secure markdown renderer"
```

---

### Task 10: Page Editor Component

**Files:**
- Create: `client/src/components/pages/PageEditor.tsx`
- Create: `client/src/components/pages/MarkdownCheatSheet.tsx`

Side-by-side editor with live preview, toolbar, slug generation, unsaved changes warning.

**Step 1: Create cheat sheet**
**Step 2: Create page editor**
**Step 3: Commit**

```bash
git add client/src/components/pages/
git commit -m "feat(pages): add page editor with live preview"
```

---

### Task 11: Page View & Page Section Components

**Files:**
- Create: `client/src/components/pages/PageView.tsx`
- Create: `client/src/components/pages/PageSection.tsx`
- Create: `client/src/components/pages/PageItem.tsx`

**Step 1: Create PageView**
**Step 2: Create PageItem**
**Step 3: Create PageSection (collapsible, with create button for admins)**
**Step 4: Commit**

```bash
git add client/src/components/pages/
git commit -m "feat(pages): add PageView, PageItem, and PageSection components"
```

---

### Task 12: Acceptance Modal

**Files:**
- Create: `client/src/components/pages/PageAcceptanceModal.tsx`
- Create: `client/src/components/pages/AcceptanceManager.tsx`

Modal with scroll-to-bottom requirement, blocking vs non-blocking modes.

**Step 1: Create acceptance modal**
**Step 2: Create acceptance manager**
**Step 3: Commit**

```bash
git add client/src/components/pages/
git commit -m "feat(pages): add acceptance modal and manager"
```

---

### Task 13: Platform Pages Card (Home View)

**Files:**
- Create: `client/src/components/pages/PlatformPagesCard.tsx`

Card component for Home view showing platform pages with "Action Required" badge.

**Step 1: Create the component**
**Step 2: Commit**

```bash
git add client/src/components/pages/
git commit -m "feat(pages): add PlatformPagesCard for home view"
```

---

## Phase 4: Integration & Testing

### Task 14: Integrate with Sidebar

**Files:**
- Modify: `client/src/components/layout/Sidebar.tsx`

Add PageSection above channel list.

**Step 1: Import and use PageSection**
**Step 2: Commit**

```bash
git commit -am "feat(pages): integrate PageSection with sidebar"
```

---

### Task 15: Add Routes

**Files:**
- Modify: `client/src/App.tsx`

Add routes for page viewing and editing.

**Step 1: Add page routes**
**Step 2: Create route components**
**Step 3: Commit**

```bash
git commit -am "feat(pages): add routes for page viewing and editing"
```

---

### Task 16: Add AcceptanceManager to App

**Files:**
- Modify: `client/src/App.tsx`

Add AcceptanceManager after auth check.

**Step 1: Add AcceptanceManager**
**Step 2: Commit**

```bash
git commit -am "feat(pages): integrate AcceptanceManager"
```

---

### Task 17: Integration Tests

**Files:**
- Create: `server/tests/pages_test.rs`

**Step 1: Write integration tests**
**Step 2: Run tests**

Run: `cd server && cargo test pages --ignored`

**Step 3: Commit**

```bash
git add server/tests/
git commit -m "test(pages): add integration tests"
```

---

## Final Verification

```bash
# Backend
cd server && cargo test
cd server && cargo clippy -- -D warnings

# Frontend
cd client && bun run lint
cd client && bun run build
```

Manual testing checklist:
- [ ] Create platform page as admin
- [ ] Create guild page with MANAGE_PAGES permission
- [ ] View page with markdown and mermaid
- [ ] Accept a page requiring acceptance
- [ ] Platform page blocks until accepted
- [ ] Guild page can be deferred
- [ ] Reorder pages via drag-and-drop (if implemented)
- [ ] Soft delete and verify cooldown
