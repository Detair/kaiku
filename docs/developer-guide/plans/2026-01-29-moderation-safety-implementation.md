# Advanced Moderation & Safety — Implementation Plan

**Lifecycle:** Superseded
**Superseded By:** `2026-01-29-moderation-safety-implementation-v2.md`

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Implement comprehensive platform safety features including content filtering, user reporting workflow, and absolute user blocking to enable safe public communities.

**Architecture:** Three integrated features: (1) Server-side content filter service with guild-configurable rules, (2) User reporting system with admin review queue, (3) Bidirectional user blocking with message/event filtering at database and WebSocket layers.

**Tech Stack:** Rust (moderation service, reports API, blocking queries), PostgreSQL (reports, blocks, filter configs), Solid.js (Safety Settings, Report Modal, Admin Queue, blocked user UI).

---

## Context

### Existing Infrastructure (DO NOT recreate)

| Component | Location | What it does |
|-----------|----------|--------------|
| Admin Dashboard | `client/src/views/AdminDashboard.tsx` | Main admin interface with panel system |
| AdminSidebar | `client/src/components/admin/AdminSidebar.tsx` | Navigation for admin panels |
| Admin Store | `client/src/stores/admin.ts` | Admin state and stats |
| Guild Settings | `client/src/components/guild/settings/` | Guild configuration UI |
| Context Menus | `client/src/lib/contextMenuBuilders.ts` | Message/user right-click actions |
| Permission System | `server/src/permissions/` | Role-based permissions |
| WebSocket Server | `server/src/ws/` | Real-time event distribution |

### What's Missing

1. **Content Filter Service** — No automated content scanning
2. **Reports Table** — No user reporting infrastructure
3. **Admin Queue UI** — No interface for reviewing reports
4. **Blocks Table** — No user blocking relationships
5. **Block-Aware Queries** — Messages/events from blocked users still show
6. **Safety Settings UI** — No guild safety configuration interface

---

## Feature Overview

### Feature 1: Content Filters
Guild admins can enable pre-defined content filters (hate speech, discrimination, harassment) with configurable actions (delete + warn, shadow-ban, log for review).

### Feature 2: User Reporting
Users can report messages and profiles with categories. System admins review reports in an admin queue with context (surrounding messages, user history).

### Feature 3: Absolute User Blocking
Users can block each other bidirectionally. Blocked users' messages are hidden, WebSocket events filtered, and voice calls prevented.

---

## Database Schema

### New Tables

```sql
-- Content filter configurations per guild
CREATE TABLE guild_filter_configs (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    guild_id UUID NOT NULL REFERENCES guilds(id) ON DELETE CASCADE,
    filter_type TEXT NOT NULL, -- 'hate_speech', 'discrimination', 'harassment'
    enabled BOOLEAN NOT NULL DEFAULT false,
    action TEXT NOT NULL DEFAULT 'log', -- 'delete_warn', 'shadow_ban', 'log'
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE(guild_id, filter_type)
);

CREATE INDEX idx_guild_filter_configs_guild_id ON guild_filter_configs(guild_id);

-- User reports for messages and profiles
CREATE TABLE reports (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    reporter_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    reported_user_id UUID REFERENCES users(id) ON DELETE CASCADE,
    reported_message_id UUID REFERENCES messages(id) ON DELETE SET NULL,
    guild_id UUID REFERENCES guilds(id) ON DELETE CASCADE,
    category TEXT NOT NULL, -- 'harassment', 'hate_speech', 'spam', 'nsfw', 'other'
    description TEXT,
    status TEXT NOT NULL DEFAULT 'pending', -- 'pending', 'reviewing', 'resolved', 'dismissed'
    resolution TEXT, -- Admin's resolution notes
    resolved_by UUID REFERENCES users(id) ON DELETE SET NULL,
    resolved_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_reports_status ON reports(status);
CREATE INDEX idx_reports_guild_id ON reports(guild_id);
CREATE INDEX idx_reports_reporter_id ON reports(reporter_id);
CREATE INDEX idx_reports_reported_user_id ON reports(reported_user_id);

-- User blocking relationships (bidirectional)
CREATE TABLE user_blocks (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    blocker_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    blocked_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE(blocker_id, blocked_id),
    CHECK (blocker_id != blocked_id) -- Can't block yourself
);

CREATE INDEX idx_user_blocks_blocker_id ON user_blocks(blocker_id);
CREATE INDEX idx_user_blocks_blocked_id ON user_blocks(blocked_id);

-- Moderation actions log
CREATE TABLE moderation_actions (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    guild_id UUID REFERENCES guilds(id) ON DELETE CASCADE,
    moderator_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    target_user_id UUID REFERENCES users(id) ON DELETE SET NULL,
    action_type TEXT NOT NULL, -- 'warn', 'kick', 'ban', 'delete_message', 'filter_match'
    reason TEXT,
    metadata JSONB, -- Additional context (message_id, filter_type, etc.)
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_moderation_actions_guild_id ON moderation_actions(guild_id);
CREATE INDEX idx_moderation_actions_target_user_id ON moderation_actions(target_user_id);
CREATE INDEX idx_moderation_actions_created_at ON moderation_actions(created_at DESC);
```

---

## Files to Create/Modify

### Server (Rust)

**New Modules:**
- `server/src/moderation/mod.rs` — Moderation service module
- `server/src/moderation/filters.rs` — Content filter patterns and matching
- `server/src/moderation/reports.rs` — Report CRUD handlers
- `server/src/moderation/blocks.rs` — Block relationship handlers

**Modified:**
- `server/src/api/mod.rs` — Add moderation routes
- `server/src/chat/messages.rs` — Integrate filter check before insert
- `server/src/chat/queries.rs` — Block-aware message queries
- `server/src/guild/handlers.rs` — Add safety settings endpoints
- `server/src/ws/mod.rs` — Filter events for blocked users

### Client (TypeScript/Solid.js)

**New Components:**
- `client/src/components/moderation/ReportModal.tsx` — Report submission UI
- `client/src/components/moderation/BlockedUserPlaceholder.tsx` — Hidden message placeholder
- `client/src/components/admin/ReportsPanel.tsx` — Admin report queue
- `client/src/components/admin/ReportDetailModal.tsx` — View report with context
- `client/src/components/guild/settings/SafetyTab.tsx` — Guild safety settings

**Modified:**
- `client/src/lib/contextMenuBuilders.ts` — Add "Report" action
- `client/src/lib/types.ts` — Add Report, Block, FilterConfig types
- `client/src/stores/admin.ts` — Add reports state
- `client/src/stores/user.ts` — Add blocked users state
- `client/src/components/messages/MessageList.tsx` — Handle blocked users
- `client/src/components/admin/AdminSidebar.tsx` — Add Reports panel option

---

## Implementation Tasks

### Task 1: Database Migration

**Files:**
- Create: `server/migrations/20260130000000_moderation_safety.sql`

**Purpose:** Create all required tables for moderation features.

```sql
-- Migration: Add moderation and safety tables

-- Content filter configurations per guild
CREATE TABLE guild_filter_configs (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    guild_id UUID NOT NULL REFERENCES guilds(id) ON DELETE CASCADE,
    filter_type TEXT NOT NULL,
    enabled BOOLEAN NOT NULL DEFAULT false,
    action TEXT NOT NULL DEFAULT 'log',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE(guild_id, filter_type)
);

CREATE INDEX idx_guild_filter_configs_guild_id ON guild_filter_configs(guild_id);

-- User reports
CREATE TABLE reports (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    reporter_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    reported_user_id UUID REFERENCES users(id) ON DELETE CASCADE,
    reported_message_id UUID REFERENCES messages(id) ON DELETE SET NULL,
    guild_id UUID REFERENCES guilds(id) ON DELETE CASCADE,
    category TEXT NOT NULL,
    description TEXT,
    status TEXT NOT NULL DEFAULT 'pending',
    resolution TEXT,
    resolved_by UUID REFERENCES users(id) ON DELETE SET NULL,
    resolved_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_reports_status ON reports(status);
CREATE INDEX idx_reports_guild_id ON reports(guild_id);
CREATE INDEX idx_reports_reporter_id ON reports(reporter_id);
CREATE INDEX idx_reports_reported_user_id ON reports(reported_user_id);

-- User blocking relationships
CREATE TABLE user_blocks (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    blocker_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    blocked_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE(blocker_id, blocked_id),
    CHECK (blocker_id != blocked_id)
);

CREATE INDEX idx_user_blocks_blocker_id ON user_blocks(blocker_id);
CREATE INDEX idx_user_blocks_blocked_id ON user_blocks(blocked_id);

-- Moderation actions log
CREATE TABLE moderation_actions (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    guild_id UUID REFERENCES guilds(id) ON DELETE CASCADE,
    moderator_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    target_user_id UUID REFERENCES users(id) ON DELETE SET NULL,
    action_type TEXT NOT NULL,
    reason TEXT,
    metadata JSONB,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_moderation_actions_guild_id ON moderation_actions(guild_id);
CREATE INDEX idx_moderation_actions_target_user_id ON moderation_actions(target_user_id);
CREATE INDEX idx_moderation_actions_created_at ON moderation_actions(created_at DESC);
```

**Run migration:**
```bash
cd server && sqlx migrate run
```

**Verification:**
```bash
psql $DATABASE_URL -c "\d reports"
psql $DATABASE_URL -c "\d user_blocks"
psql $DATABASE_URL -c "\d guild_filter_configs"
```

**Commit:**
```bash
git add server/migrations/20260130000000_moderation_safety.sql
git commit -m "feat(db): add moderation and safety tables"
```

---

### Task 2: Content Filter Service (Server)

**Files:**
- Create: `server/src/moderation/mod.rs`
- Create: `server/src/moderation/filters.rs`
- Modify: `server/src/lib.rs` (add `mod moderation;`)

**Purpose:** Implement pattern-based content filtering with pre-defined filter sets.

**Step 1: Create moderation module structure**

Create `server/src/moderation/mod.rs`:

```rust
//! Moderation and safety services

pub mod filters;
pub mod reports;
pub mod blocks;

pub use filters::{FilterType, FilterAction, check_content_filters};
pub use reports::{Report, ReportCategory, ReportStatus};
pub use blocks::{block_user, unblock_user, is_blocked};
```

**Step 2: Implement content filters**

Create `server/src/moderation/filters.rs`:

```rust
use regex::Regex;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "TEXT")]
pub enum FilterType {
    HateSpeech,
    Discrimination,
    Harassment,
}

impl FilterType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::HateSpeech => "hate_speech",
            Self::Discrimination => "discrimination",
            Self::Harassment => "harassment",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "TEXT")]
pub enum FilterAction {
    DeleteWarn,  // Delete message + warn user
    ShadowBan,   // Hide message from everyone except author
    Log,         // Log to moderation_actions, no immediate action
}

impl FilterAction {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::DeleteWarn => "delete_warn",
            Self::ShadowBan => "shadow_ban",
            Self::Log => "log",
        }
    }
}

/// Check if content matches any enabled filters for the guild
pub async fn check_content_filters(
    pool: &PgPool,
    guild_id: Uuid,
    content: &str,
) -> Result<Option<(FilterType, FilterAction)>, sqlx::Error> {
    // Get enabled filters for guild
    let configs = sqlx::query!(
        r#"SELECT filter_type, action FROM guild_filter_configs
           WHERE guild_id = $1 AND enabled = true"#,
        guild_id
    )
    .fetch_all(pool)
    .await?;

    let content_lower = content.to_lowercase();

    for config in configs {
        let filter_type = match config.filter_type.as_str() {
            "hate_speech" => FilterType::HateSpeech,
            "discrimination" => FilterType::Discrimination,
            "harassment" => FilterType::Harassment,
            _ => continue,
        };

        if matches_filter(&filter_type, &content_lower) {
            let action = match config.action.as_str() {
                "delete_warn" => FilterAction::DeleteWarn,
                "shadow_ban" => FilterAction::ShadowBan,
                _ => FilterAction::Log,
            };

            return Ok(Some((filter_type, action)));
        }
    }

    Ok(None)
}

/// Check if content matches a specific filter type
fn matches_filter(filter_type: &FilterType, content: &str) -> bool {
    let patterns = match filter_type {
        FilterType::HateSpeech => HATE_SPEECH_PATTERNS,
        FilterType::Discrimination => DISCRIMINATION_PATTERNS,
        FilterType::Harassment => HARASSMENT_PATTERNS,
    };

    for pattern in patterns {
        if pattern.is_match(content) {
            return true;
        }
    }

    false
}

// Pre-compiled regex patterns (lazy_static or once_cell)
lazy_static::lazy_static! {
    static ref HATE_SPEECH_PATTERNS: Vec<Regex> = vec![
        // Note: These are placeholder patterns. Real implementation should use
        // a comprehensive library or service (e.g., Perspective API, Azure Content Moderator)
        Regex::new(r"\b(slur1|slur2|slur3)\b").unwrap(), // Replace with actual patterns
        // Add more patterns
    ];

    static ref DISCRIMINATION_PATTERNS: Vec<Regex> = vec![
        Regex::new(r"\b(discriminatory-term)\b").unwrap(),
        // Add more patterns
    ];

    static ref HARASSMENT_PATTERNS: Vec<Regex> = vec![
        Regex::new(r"(you should (kill yourself|die))").unwrap(),
        Regex::new(r"(kys\b)").unwrap(), // "kill yourself" abbreviation
        // Add more patterns
    ];
}
```

**Add dependency to `server/Cargo.toml`:**
```toml
lazy_static = "1.4"
```

**Step 3: Register module**

In `server/src/lib.rs`, add:
```rust
pub mod moderation;
```

**Verification:**
```bash
cd server && cargo check
```

**Commit:**
```bash
git add server/src/moderation/ server/Cargo.toml
git commit -m "feat(moderation): add content filter service"
```

---

### Task 3: Integrate Filters into Message Creation

**Files:**
- Modify: `server/src/chat/messages.rs`

**Purpose:** Check messages against filters before saving, apply configured actions.

**Step 1: Import moderation service**

Add to imports:
```rust
use crate::moderation::{check_content_filters, FilterAction};
```

**Step 2: Add filter check in create handler**

In the `create()` handler, after content validation and before database insert:

```rust
// Check content filters (only for guild messages, skip DMs and encrypted)
if let Some(guild_id) = channel.guild_id {
    if !message_body.encrypted {
        match check_content_filters(&state.db, guild_id, &message_body.content).await {
            Ok(Some((filter_type, action))) => {
                // Log the match
                log_moderation_action(
                    &state.db,
                    guild_id,
                    auth.id, // The author is the "target"
                    "filter_match",
                    Some(format!("Content matched {:?} filter", filter_type)),
                    serde_json::json!({
                        "filter_type": filter_type.as_str(),
                        "action": action.as_str(),
                        "content_snippet": &message_body.content.chars().take(50).collect::<String>(),
                    }),
                ).await?;

                match action {
                    FilterAction::DeleteWarn => {
                        // Don't insert message, return error to user
                        return Err(ChatError::ContentViolation(
                            "Message blocked by content filter".to_string()
                        ));
                    }
                    FilterAction::ShadowBan => {
                        // Insert message but mark as shadow_banned
                        // (requires adding shadow_banned BOOLEAN column to messages table)
                        // For MVP, treat as DeleteWarn
                        return Err(ChatError::ContentViolation(
                            "Message blocked by content filter".to_string()
                        ));
                    }
                    FilterAction::Log => {
                        // Allow message through, already logged
                    }
                }
            }
            Ok(None) => {
                // No filter match, continue
            }
            Err(e) => {
                tracing::error!("Filter check error: {:?}", e);
                // Don't block message on filter service error
            }
        }
    }
}
```

**Step 3: Add log_moderation_action helper**

```rust
async fn log_moderation_action(
    pool: &PgPool,
    guild_id: Uuid,
    target_user_id: Uuid,
    action_type: &str,
    reason: Option<String>,
    metadata: serde_json::Value,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        r#"INSERT INTO moderation_actions (guild_id, moderator_id, target_user_id, action_type, reason, metadata)
           VALUES ($1, $1, $2, $3, $4, $5)"#, // Use target as moderator for auto-actions
        guild_id,
        target_user_id,
        action_type,
        reason,
        metadata,
    )
    .execute(pool)
    .await?;
    Ok(())
}
```

**Step 4: Add ContentViolation error variant**

In `server/src/chat/error.rs`:
```rust
#[error("Content blocked: {0}")]
ContentViolation(String),
```

Map to HTTP 403:
```rust
ChatError::ContentViolation(_) => (StatusCode::FORBIDDEN, "CONTENT_VIOLATION"),
```

**Verification:**
```bash
cd server && cargo check
```

**Commit:**
```bash
git add server/src/chat/messages.rs server/src/chat/error.rs
git commit -m "feat(moderation): integrate content filters into message creation"
```

---

### Task 4: User Blocking (Server)

**Files:**
- Create: `server/src/moderation/blocks.rs`
- Modify: `server/src/api/mod.rs` (add blocking routes)

**Purpose:** Implement user blocking relationships and query helpers.

Create `server/src/moderation/blocks.rs`:

```rust
use axum::{extract::State, Json};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

use crate::api::AppState;
use crate::auth::AuthUser;

#[derive(Debug, Serialize)]
pub struct Block {
    pub id: Uuid,
    pub blocker_id: Uuid,
    pub blocked_id: Uuid,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Deserialize)]
pub struct BlockUserRequest {
    pub user_id: Uuid,
}

/// POST /api/users/block
pub async fn block_user(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(body): Json<BlockUserRequest>,
) -> Result<Json<Block>, ModerationError> {
    if auth.id == body.user_id {
        return Err(ModerationError::CannotBlockSelf);
    }

    let block = sqlx::query_as!(
        Block,
        r#"INSERT INTO user_blocks (blocker_id, blocked_id)
           VALUES ($1, $2)
           ON CONFLICT (blocker_id, blocked_id) DO UPDATE SET created_at = NOW()
           RETURNING *"#,
        auth.id,
        body.user_id,
    )
    .fetch_one(&state.db)
    .await
    .map_err(ModerationError::Database)?;

    Ok(Json(block))
}

/// DELETE /api/users/block/:user_id
pub async fn unblock_user(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(user_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ModerationError> {
    sqlx::query!(
        r#"DELETE FROM user_blocks WHERE blocker_id = $1 AND blocked_id = $2"#,
        auth.id,
        user_id,
    )
    .execute(&state.db)
    .await
    .map_err(ModerationError::Database)?;

    Ok(Json(serde_json::json!({ "success": true })))
}

/// GET /api/users/blocks
pub async fn list_blocked_users(
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<Json<Vec<Uuid>>, ModerationError> {
    let blocks = sqlx::query!(
        r#"SELECT blocked_id FROM user_blocks WHERE blocker_id = $1"#,
        auth.id,
    )
    .fetch_all(&state.db)
    .await
    .map_err(ModerationError::Database)?;

    Ok(Json(blocks.into_iter().map(|b| b.blocked_id).collect()))
}

/// Check if user A has blocked user B (or vice versa)
pub async fn is_blocked(
    pool: &PgPool,
    user_a: Uuid,
    user_b: Uuid,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query!(
        r#"SELECT EXISTS(
            SELECT 1 FROM user_blocks
            WHERE (blocker_id = $1 AND blocked_id = $2)
               OR (blocker_id = $2 AND blocked_id = $1)
        ) as "exists!""#,
        user_a,
        user_b,
    )
    .fetch_one(pool)
    .await?;

    Ok(result.exists)
}

#[derive(Debug, thiserror::Error)]
pub enum ModerationError {
    #[error("Database error")]
    Database(#[from] sqlx::Error),
    
    #[error("Cannot block yourself")]
    CannotBlockSelf,
}

// Implement IntoResponse for ModerationError...
```

Add routes in `server/src/api/mod.rs`:
```rust
.route("/users/block", post(moderation::blocks::block_user))
.route("/users/block/:user_id", delete(moderation::blocks::unblock_user))
.route("/users/blocks", get(moderation::blocks::list_blocked_users))
```

**Verification:**
```bash
cd server && cargo check
```

**Commit:**
```bash
git add server/src/moderation/blocks.rs server/src/api/mod.rs
git commit -m "feat(moderation): add user blocking endpoints"
```

---

### Task 5: Block-Aware Message Queries

**Files:**
- Modify: `server/src/chat/queries.rs`

**Purpose:** Filter out messages from blocked users in list queries.

**Step 1: Update list_messages query**

Find the `list_messages` function and add block filtering:

```rust
pub async fn list_messages(
    pool: &PgPool,
    channel_id: Uuid,
    requester_id: Uuid,
    limit: i64,
    before: Option<Uuid>,
) -> Result<Vec<Message>, sqlx::Error> {
    let messages = sqlx::query_as!(
        Message,
        r#"SELECT m.* FROM messages m
           WHERE m.channel_id = $1
           -- Filter out messages from blocked users
           AND NOT EXISTS (
               SELECT 1 FROM user_blocks ub
               WHERE (ub.blocker_id = $2 AND ub.blocked_id = m.author_id)
                  OR (ub.blocker_id = m.author_id AND ub.blocked_id = $2)
           )
           AND ($3::UUID IS NULL OR m.id < $3)
           ORDER BY m.created_at DESC
           LIMIT $4"#,
        channel_id,
        requester_id,
        before,
        limit,
    )
    .fetch_all(pool)
    .await?;

    Ok(messages)
}
```

**Key change:** Added `NOT EXISTS` subquery to exclude messages where either user has blocked the other.

**Performance note:** The `user_blocks` indexes on both `blocker_id` and `blocked_id` should make this efficient.

**Verification:**
```bash
cd server && cargo check && cargo test
```

**Commit:**
```bash
git add server/src/chat/queries.rs
git commit -m "feat(moderation): add block filtering to message queries"
```

---

### Task 6: User Reporting (Server)

**Files:**
- Create: `server/src/moderation/reports.rs`
- Modify: `server/src/api/mod.rs` (add report routes)

**Purpose:** Implement user reporting system with admin review queue.

Create `server/src/moderation/reports.rs`:

```rust
use axum::{
    extract::{Path, Query, State},
    Json,
};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

use crate::api::AppState;
use crate::auth::AuthUser;
use crate::permissions::check_guild_permission;

#[derive(Debug, Serialize)]
pub struct Report {
    pub id: Uuid,
    pub reporter_id: Uuid,
    pub reported_user_id: Option<Uuid>,
    pub reported_message_id: Option<Uuid>,
    pub guild_id: Option<Uuid>,
    pub category: String,
    pub description: Option<String>,
    pub status: String,
    pub resolution: Option<String>,
    pub resolved_by: Option<Uuid>,
    pub resolved_at: Option<chrono::DateTime<chrono::Utc>>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Deserialize)]
pub struct CreateReportRequest {
    pub reported_user_id: Option<Uuid>,
    pub reported_message_id: Option<Uuid>,
    pub guild_id: Option<Uuid>,
    pub category: ReportCategory,
    pub description: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReportCategory {
    Harassment,
    HateSpeech,
    Spam,
    Nsfw,
    Other,
}

impl ReportCategory {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Harassment => "harassment",
            Self::HateSpeech => "hate_speech",
            Self::Spam => "spam",
            Self::Nsfw => "nsfw",
            Self::Other => "other",
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct ReportQuery {
    pub status: Option<String>,
    pub guild_id: Option<Uuid>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct ResolveReportRequest {
    pub resolution: String,
}

/// POST /api/reports
pub async fn create_report(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(body): Json<CreateReportRequest>,
) -> Result<Json<Report>, ReportError> {
    // Validation: Must report either a user or message
    if body.reported_user_id.is_none() && body.reported_message_id.is_none() {
        return Err(ReportError::InvalidRequest(
            "Must specify either reported_user_id or reported_message_id".to_string(),
        ));
    }

    // Cannot report yourself
    if body.reported_user_id == Some(auth.id) {
        return Err(ReportError::CannotReportSelf);
    }

    let report = sqlx::query_as!(
        Report,
        r#"INSERT INTO reports (reporter_id, reported_user_id, reported_message_id, guild_id, category, description)
           VALUES ($1, $2, $3, $4, $5, $6)
           RETURNING *"#,
        auth.id,
        body.reported_user_id,
        body.reported_message_id,
        body.guild_id,
        body.category.as_str(),
        body.description,
    )
    .fetch_one(&state.db)
    .await
    .map_err(ReportError::Database)?;

    Ok(Json(report))
}

/// GET /api/reports
/// Admin-only: List reports with filtering
pub async fn list_reports(
    State(state): State<AppState>,
    auth: AuthUser,
    Query(query): Query<ReportQuery>,
) -> Result<Json<Vec<Report>>, ReportError> {
    // Check system admin permission
    if !auth.is_system_admin {
        return Err(ReportError::Forbidden);
    }

    let limit = query.limit.unwrap_or(50).min(100);
    let offset = query.offset.unwrap_or(0);

    let reports = sqlx::query_as!(
        Report,
        r#"SELECT * FROM reports
           WHERE ($1::TEXT IS NULL OR status = $1)
           AND ($2::UUID IS NULL OR guild_id = $2)
           ORDER BY created_at DESC
           LIMIT $3 OFFSET $4"#,
        query.status,
        query.guild_id,
        limit,
        offset,
    )
    .fetch_all(&state.db)
    .await
    .map_err(ReportError::Database)?;

    Ok(Json(reports))
}

/// GET /api/reports/:id
/// Admin-only: Get report details
pub async fn get_report(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(report_id): Path<Uuid>,
) -> Result<Json<Report>, ReportError> {
    if !auth.is_system_admin {
        return Err(ReportError::Forbidden);
    }

    let report = sqlx::query_as!(
        Report,
        r#"SELECT * FROM reports WHERE id = $1"#,
        report_id,
    )
    .fetch_optional(&state.db)
    .await
    .map_err(ReportError::Database)?
    .ok_or(ReportError::NotFound)?;

    Ok(Json(report))
}

/// PATCH /api/reports/:id/resolve
/// Admin-only: Resolve a report
pub async fn resolve_report(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(report_id): Path<Uuid>,
    Json(body): Json<ResolveReportRequest>,
) -> Result<Json<Report>, ReportError> {
    if !auth.is_system_admin {
        return Err(ReportError::Forbidden);
    }

    let report = sqlx::query_as!(
        Report,
        r#"UPDATE reports
           SET status = 'resolved',
               resolution = $1,
               resolved_by = $2,
               resolved_at = NOW(),
               updated_at = NOW()
           WHERE id = $3
           RETURNING *"#,
        body.resolution,
        auth.id,
        report_id,
    )
    .fetch_optional(&state.db)
    .await
    .map_err(ReportError::Database)?
    .ok_or(ReportError::NotFound)?;

    Ok(Json(report))
}

/// PATCH /api/reports/:id/dismiss
/// Admin-only: Dismiss a report
pub async fn dismiss_report(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(report_id): Path<Uuid>,
) -> Result<Json<Report>, ReportError> {
    if !auth.is_system_admin {
        return Err(ReportError::Forbidden);
    }

    let report = sqlx::query_as!(
        Report,
        r#"UPDATE reports
           SET status = 'dismissed',
               resolved_by = $1,
               resolved_at = NOW(),
               updated_at = NOW()
           WHERE id = $2
           RETURNING *"#,
        auth.id,
        report_id,
    )
    .fetch_optional(&state.db)
    .await
    .map_err(ReportError::Database)?
    .ok_or(ReportError::NotFound)?;

    Ok(Json(report))
}

#[derive(Debug, thiserror::Error)]
pub enum ReportError {
    #[error("Database error")]
    Database(#[from] sqlx::Error),

    #[error("Report not found")]
    NotFound,

    #[error("Forbidden")]
    Forbidden,

    #[error("Cannot report yourself")]
    CannotReportSelf,

    #[error("Invalid request: {0}")]
    InvalidRequest(String),
}

impl axum::response::IntoResponse for ReportError {
    fn into_response(self) -> axum::response::Response {
        use axum::http::StatusCode;

        let (status, code) = match self {
            Self::Database(_) => (StatusCode::INTERNAL_SERVER_ERROR, "DATABASE_ERROR"),
            Self::NotFound => (StatusCode::NOT_FOUND, "REPORT_NOT_FOUND"),
            Self::Forbidden => (StatusCode::FORBIDDEN, "FORBIDDEN"),
            Self::CannotReportSelf => (StatusCode::BAD_REQUEST, "CANNOT_REPORT_SELF"),
            Self::InvalidRequest(_) => (StatusCode::BAD_REQUEST, "INVALID_REQUEST"),
        };

        (status, Json(serde_json::json!({ "error": code }))).into_response()
    }
}
```

Add routes in `server/src/api/mod.rs`:
```rust
.route("/reports", post(moderation::reports::create_report))
.route("/reports", get(moderation::reports::list_reports))
.route("/reports/:id", get(moderation::reports::get_report))
.route("/reports/:id/resolve", patch(moderation::reports::resolve_report))
.route("/reports/:id/dismiss", patch(moderation::reports::dismiss_report))
```

**Verification:**
```bash
cd server && cargo check
```

**Commit:**
```bash
git add server/src/moderation/reports.rs server/src/api/mod.rs
git commit -m "feat(moderation): add user reporting endpoints"
```

---

### Task 7: WebSocket Event Filtering for Blocks

**Files:**
- Modify: `server/src/ws/mod.rs` (or wherever WebSocket broadcast logic lives)

**Purpose:** Filter WebSocket events to prevent blocked users from receiving each other's events.

**Step 1: Add block check in event broadcast**

Find the function that broadcasts events to guild/channel members. Example location might vary:

```rust
use crate::moderation::blocks::is_blocked;

/// Broadcast event to all users in a channel
pub async fn broadcast_to_channel(
    state: &AppState,
    channel_id: Uuid,
    event: &WsEvent,
    exclude_user: Option<Uuid>,
) -> Result<(), WsError> {
    // Get all users in channel
    let user_ids = get_channel_members(&state.db, channel_id).await?;

    // Get event author if present (e.g., for message events)
    let event_author = extract_event_author(event);

    for user_id in user_ids {
        if Some(user_id) == exclude_user {
            continue;
        }

        // Filter events if users have blocked each other
        if let Some(author_id) = event_author {
            if is_blocked(&state.db, user_id, author_id).await? {
                continue; // Skip sending to blocked user
            }
        }

        // Send event to user's WebSocket connection
        send_to_user(state, user_id, event).await?;
    }

    Ok(())
}

/// Extract the author user ID from event payload
fn extract_event_author(event: &WsEvent) -> Option<Uuid> {
    match event {
        WsEvent::MessageCreated { message } => Some(message.author_id),
        WsEvent::MessageUpdated { message } => Some(message.author_id),
        WsEvent::MessageDeleted { author_id, .. } => Some(*author_id),
        WsEvent::TypingStart { user_id, .. } => Some(*user_id),
        WsEvent::PresenceUpdate { user_id, .. } => Some(*user_id),
        _ => None, // Guild events, etc. don't need filtering
    }
}
```

**Key points:**
- Extract author_id from event payload
- Check `is_blocked()` before sending to each recipient
- Skip sending if bidirectional block exists

**Performance note:** This adds one DB query per recipient per event. For high-traffic guilds, consider caching blocked relationships in Redis.

**Verification:**
```bash
cd server && cargo check && cargo test
```

**Commit:**
```bash
git add server/src/ws/mod.rs
git commit -m "feat(moderation): filter WebSocket events for blocked users"
```

---

### Task 8: Guild Safety Settings Endpoints

**Files:**
- Modify: `server/src/guild/handlers.rs`

**Purpose:** Allow guild admins to configure content filters.

Add handlers to `server/src/guild/handlers.rs`:

```rust
use crate::moderation::filters::{FilterType, FilterAction};

#[derive(Debug, Deserialize)]
pub struct UpdateFilterConfigRequest {
    pub filter_type: String,
    pub enabled: bool,
    pub action: String,
}

/// GET /api/guilds/:id/safety/filters
pub async fn get_filter_configs(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(guild_id): Path<Uuid>,
) -> Result<Json<Vec<FilterConfig>>, GuildError> {
    // Check admin permission
    check_guild_permission(&state.db, auth.id, guild_id, "manage_guild").await?;

    let configs = sqlx::query_as!(
        FilterConfig,
        r#"SELECT * FROM guild_filter_configs WHERE guild_id = $1"#,
        guild_id,
    )
    .fetch_all(&state.db)
    .await
    .map_err(GuildError::Database)?;

    Ok(Json(configs))
}

/// PUT /api/guilds/:id/safety/filters
pub async fn update_filter_config(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(guild_id): Path<Uuid>,
    Json(body): Json<UpdateFilterConfigRequest>,
) -> Result<Json<FilterConfig>, GuildError> {
    // Check admin permission
    check_guild_permission(&state.db, auth.id, guild_id, "manage_guild").await?;

    // Validate filter_type
    let valid_types = ["hate_speech", "discrimination", "harassment"];
    if !valid_types.contains(&body.filter_type.as_str()) {
        return Err(GuildError::InvalidRequest("Invalid filter type".to_string()));
    }

    // Validate action
    let valid_actions = ["delete_warn", "shadow_ban", "log"];
    if !valid_actions.contains(&body.action.as_str()) {
        return Err(GuildError::InvalidRequest("Invalid filter action".to_string()));
    }

    let config = sqlx::query_as!(
        FilterConfig,
        r#"INSERT INTO guild_filter_configs (guild_id, filter_type, enabled, action)
           VALUES ($1, $2, $3, $4)
           ON CONFLICT (guild_id, filter_type)
           DO UPDATE SET enabled = $3, action = $4, updated_at = NOW()
           RETURNING *"#,
        guild_id,
        body.filter_type,
        body.enabled,
        body.action,
    )
    .fetch_one(&state.db)
    .await
    .map_err(GuildError::Database)?;

    Ok(Json(config))
}

#[derive(Debug, Serialize)]
pub struct FilterConfig {
    pub id: Uuid,
    pub guild_id: Uuid,
    pub filter_type: String,
    pub enabled: bool,
    pub action: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}
```

Add routes in `server/src/api/mod.rs`:
```rust
.route("/guilds/:id/safety/filters", get(guild::handlers::get_filter_configs))
.route("/guilds/:id/safety/filters", put(guild::handlers::update_filter_config))
```

**Verification:**
```bash
cd server && cargo check
```

**Commit:**
```bash
git add server/src/guild/handlers.rs server/src/api/mod.rs
git commit -m "feat(moderation): add guild safety settings endpoints"
```

---

## Client Implementation

### Task 9: Types (Client)

**Files:**
- Modify: `client/src/lib/types.ts`

**Purpose:** Add TypeScript types for moderation features.

```typescript
// Reports
export interface Report {
  id: string;
  reporter_id: string;
  reported_user_id?: string;
  reported_message_id?: string;
  guild_id?: string;
  category: ReportCategory;
  description?: string;
  status: ReportStatus;
  resolution?: string;
  resolved_by?: string;
  resolved_at?: string;
  created_at: string;
  updated_at: string;
}

export type ReportCategory = 'harassment' | 'hate_speech' | 'spam' | 'nsfw' | 'other';
export type ReportStatus = 'pending' | 'reviewing' | 'resolved' | 'dismissed';

// Blocks
export interface Block {
  id: string;
  blocker_id: string;
  blocked_id: string;
  created_at: string;
}

// Filter Configs
export interface FilterConfig {
  id: string;
  guild_id: string;
  filter_type: FilterType;
  enabled: boolean;
  action: FilterAction;
  created_at: string;
  updated_at: string;
}

export type FilterType = 'hate_speech' | 'discrimination' | 'harassment';
export type FilterAction = 'delete_warn' | 'shadow_ban' | 'log';
```

**Commit:**
```bash
git add client/src/lib/types.ts
git commit -m "feat(types): add moderation types"
```

---

### Task 10: Report Modal (Client)

**Files:**
- Create: `client/src/components/moderation/ReportModal.tsx`

**Purpose:** Modal for submitting reports on messages or users.

```tsx
import { Component, createSignal, Show } from 'solid-js';
import { ReportCategory } from '../../lib/types';
import { invoke } from '@tauri-apps/api/core';

interface ReportModalProps {
  isOpen: boolean;
  onClose: () => void;
  reportedUserId?: string;
  reportedMessageId?: string;
  guildId?: string;
}

export const ReportModal: Component<ReportModalProps> = (props) => {
  const [category, setCategory] = createSignal<ReportCategory>('other');
  const [description, setDescription] = createSignal('');
  const [isSubmitting, setIsSubmitting] = createSignal(false);
  const [error, setError] = createSignal('');

  const handleSubmit = async (e: Event) => {
    e.preventDefault();
    setError('');
    setIsSubmitting(true);

    try {
      await invoke('api_request', {
        method: 'POST',
        endpoint: '/reports',
        body: {
          reported_user_id: props.reportedUserId,
          reported_message_id: props.reportedMessageId,
          guild_id: props.guildId,
          category: category(),
          description: description().trim() || undefined,
        },
      });

      // Success
      props.onClose();
      setCategory('other');
      setDescription('');
    } catch (err: any) {
      setError(err.message || 'Failed to submit report');
    } finally {
      setIsSubmitting(false);
    }
  };

  return (
    <Show when={props.isOpen}>
      <div class="fixed inset-0 z-50 flex items-center justify-center bg-black/50">
        <div class="bg-surface-800 rounded-lg shadow-xl w-full max-w-md p-6">
          <h2 class="text-xl font-semibold text-white mb-4">Submit Report</h2>

          <form onSubmit={handleSubmit} class="space-y-4">
            {/* Category */}
            <div>
              <label class="block text-sm font-medium text-gray-300 mb-2">
                Category
              </label>
              <select
                value={category()}
                onChange={(e) => setCategory(e.target.value as ReportCategory)}
                class="w-full bg-surface-700 text-white rounded px-3 py-2 border border-surface-600 focus:border-primary-500 focus:outline-none"
              >
                <option value="harassment">Harassment</option>
                <option value="hate_speech">Hate Speech</option>
                <option value="spam">Spam</option>
                <option value="nsfw">NSFW Content</option>
                <option value="other">Other</option>
              </select>
            </div>

            {/* Description */}
            <div>
              <label class="block text-sm font-medium text-gray-300 mb-2">
                Description (Optional)
              </label>
              <textarea
                value={description()}
                onInput={(e) => setDescription(e.target.value)}
                placeholder="Provide additional context..."
                rows={4}
                class="w-full bg-surface-700 text-white rounded px-3 py-2 border border-surface-600 focus:border-primary-500 focus:outline-none resize-none"
              />
            </div>

            {/* Error */}
            <Show when={error()}>
              <div class="text-red-400 text-sm">{error()}</div>
            </Show>

            {/* Actions */}
            <div class="flex justify-end gap-3">
              <button
                type="button"
                onClick={props.onClose}
                class="px-4 py-2 rounded bg-surface-700 text-white hover:bg-surface-600 transition-colors"
                disabled={isSubmitting()}
              >
                Cancel
              </button>
              <button
                type="submit"
                class="px-4 py-2 rounded bg-red-600 text-white hover:bg-red-700 transition-colors disabled:opacity-50"
                disabled={isSubmitting()}
              >
                {isSubmitting() ? 'Submitting...' : 'Submit Report'}
              </button>
            </div>
          </form>
        </div>
      </div>
    </Show>
  );
};
```

**Commit:**
```bash
git add client/src/components/moderation/ReportModal.tsx
git commit -m "feat(moderation): add report modal component"
```

---

### Task 11: Blocked User Placeholder (Client)

**Files:**
- Create: `client/src/components/moderation/BlockedUserPlaceholder.tsx`

**Purpose:** Show placeholder for blocked users' messages.

```tsx
import { Component } from 'solid-js';

interface BlockedUserPlaceholderProps {
  username: string;
}

export const BlockedUserPlaceholder: Component<BlockedUserPlaceholderProps> = (props) => {
  return (
    <div class="py-3 px-4 my-1 bg-surface-800/50 border border-surface-700 rounded text-sm text-gray-400 italic">
      Message from blocked user @{props.username} (hidden)
    </div>
  );
};
```

**Commit:**
```bash
git add client/src/components/moderation/BlockedUserPlaceholder.tsx
git commit -m "feat(moderation): add blocked user placeholder"
```

---

### Task 12: Context Menu Integration (Client)

**Files:**
- Modify: `client/src/lib/contextMenuBuilders.ts`

**Purpose:** Add "Report Message" and "Block User" options to context menus.

```typescript
import { Message, User } from './types';

export function buildMessageContextMenu(
  message: Message,
  currentUserId: string,
  onReport: () => void,
  onBlock: () => void,
  // ... existing params
): ContextMenuItem[] {
  const items: ContextMenuItem[] = [];

  // ... existing items (Edit, Delete, Reply, etc.)

  // Add report option (if not own message)
  if (message.author_id !== currentUserId) {
    items.push({
      label: 'Report Message',
      icon: 'flag',
      onClick: onReport,
      variant: 'danger',
    });

    items.push({
      label: 'Block User',
      icon: 'ban',
      onClick: onBlock,
      variant: 'danger',
    });
  }

  return items;
}

export function buildUserContextMenu(
  user: User,
  currentUserId: string,
  isBlocked: boolean,
  onBlock: () => void,
  onUnblock: () => void,
  onReport: () => void,
  // ... existing params
): ContextMenuItem[] {
  const items: ContextMenuItem[] = [];

  // ... existing items (Send Message, View Profile, etc.)

  // Block/Unblock
  if (user.id !== currentUserId) {
    if (isBlocked) {
      items.push({
        label: 'Unblock User',
        icon: 'user-check',
        onClick: onUnblock,
      });
    } else {
      items.push({
        label: 'Block User',
        icon: 'ban',
        onClick: onBlock,
        variant: 'danger',
      });
    }

    items.push({
      label: 'Report User',
      icon: 'flag',
      onClick: onReport,
      variant: 'danger',
    });
  }

  return items;
}
```

**Commit:**
```bash
git add client/src/lib/contextMenuBuilders.ts
git commit -m "feat(moderation): add report/block to context menus"
```

---

### Task 13: Safety Settings Tab (Client)

**Files:**
- Create: `client/src/components/guild/settings/SafetyTab.tsx`

**Purpose:** Guild admin UI for configuring content filters.

```tsx
import { Component, createResource, createSignal, For } from 'solid-js';
import { invoke } from '@tauri-apps/api/core';
import { FilterConfig, FilterType, FilterAction } from '../../../lib/types';

interface SafetyTabProps {
  guildId: string;
}

export const SafetyTab: Component<SafetyTabProps> = (props) => {
  const [configs] = createResource(() =>
    invoke<FilterConfig[]>('api_request', {
      method: 'GET',
      endpoint: `/guilds/${props.guildId}/safety/filters`,
    })
  );

  const [saving, setSaving] = createSignal<string | null>(null);

  const filterLabels: Record<FilterType, string> = {
    hate_speech: 'Hate Speech',
    discrimination: 'Discrimination',
    harassment: 'Harassment',
  };

  const actionLabels: Record<FilterAction, string> = {
    delete_warn: 'Delete & Warn',
    shadow_ban: 'Shadow Ban',
    log: 'Log Only',
  };

  const handleToggle = async (filterType: FilterType, currentEnabled: boolean) => {
    setSaving(filterType);
    try {
      await invoke('api_request', {
        method: 'PUT',
        endpoint: `/guilds/${props.guildId}/safety/filters`,
        body: {
          filter_type: filterType,
          enabled: !currentEnabled,
          action: 'log', // Default action
        },
      });
      configs.mutate(); // Refetch
    } catch (err) {
      console.error('Failed to update filter:', err);
    } finally {
      setSaving(null);
    }
  };

  const handleActionChange = async (filterType: FilterType, action: FilterAction) => {
    setSaving(filterType);
    try {
      await invoke('api_request', {
        method: 'PUT',
        endpoint: `/guilds/${props.guildId}/safety/filters`,
        body: {
          filter_type: filterType,
          enabled: true,
          action,
        },
      });
      configs.mutate();
    } catch (err) {
      console.error('Failed to update action:', err);
    } finally {
      setSaving(null);
    }
  };

  return (
    <div class="p-6 space-y-6">
      <div>
        <h2 class="text-2xl font-semibold text-white mb-2">Safety Settings</h2>
        <p class="text-gray-400">Configure automated content moderation filters.</p>
      </div>

      <div class="space-y-4">
        <For each={configs()}>
          {(config) => (
            <div class="bg-surface-800 rounded-lg p-4 border border-surface-700">
              <div class="flex items-center justify-between mb-3">
                <div>
                  <h3 class="text-lg font-medium text-white">
                    {filterLabels[config.filter_type as FilterType]}
                  </h3>
                  <p class="text-sm text-gray-400">
                    Automatically detect and handle {config.filter_type.replace('_', ' ')}.
                  </p>
                </div>

                <button
                  onClick={() => handleToggle(config.filter_type as FilterType, config.enabled)}
                  disabled={saving() === config.filter_type}
                  class={`
                    relative w-12 h-6 rounded-full transition-colors
                    ${config.enabled ? 'bg-primary-500' : 'bg-surface-600'}
                    ${saving() === config.filter_type ? 'opacity-50' : ''}
                  `}
                >
                  <span
                    class={`
                      absolute top-1 left-1 w-4 h-4 bg-white rounded-full transition-transform
                      ${config.enabled ? 'translate-x-6' : ''}
                    `}
                  />
                </button>
              </div>

              {config.enabled && (
                <div>
                  <label class="block text-sm font-medium text-gray-300 mb-2">
                    Action
                  </label>
                  <select
                    value={config.action}
                    onChange={(e) => handleActionChange(
                      config.filter_type as FilterType,
                      e.target.value as FilterAction
                    )}
                    disabled={saving() === config.filter_type}
                    class="bg-surface-700 text-white rounded px-3 py-2 border border-surface-600 focus:border-primary-500 focus:outline-none"
                  >
                    <option value="log">Log Only</option>
                    <option value="delete_warn">Delete & Warn User</option>
                    <option value="shadow_ban">Shadow Ban</option>
                  </select>
                  <p class="text-xs text-gray-500 mt-1">
                    {actionLabels[config.action as FilterAction]}
                  </p>
                </div>
              )}
            </div>
          )}
        </For>
      </div>

      <div class="bg-surface-800/50 border border-surface-700 rounded-lg p-4">
        <h4 class="text-sm font-medium text-white mb-2">Note</h4>
        <p class="text-sm text-gray-400">
          Content filters are based on pattern matching and may not catch all violations.
          Encourage users to report problematic content for manual review.
        </p>
      </div>
    </div>
  );
};
```

**Commit:**
```bash
git add client/src/components/guild/settings/SafetyTab.tsx
git commit -m "feat(moderation): add safety settings tab"
```

---

### Task 14: Admin Reports Panel (Client)

**Files:**
- Create: `client/src/components/admin/ReportsPanel.tsx`

**Purpose:** Admin dashboard panel for reviewing reports.

```tsx
import { Component, createResource, createSignal, For, Show } from 'solid-js';
import { invoke } from '@tauri-apps/api/core';
import { Report, ReportStatus } from '../../lib/types';

export const ReportsPanel: Component = () => {
  const [statusFilter, setStatusFilter] = createSignal<ReportStatus | 'all'>('pending');

  const [reports] = createResource(statusFilter, async (status) => {
    return invoke<Report[]>('api_request', {
      method: 'GET',
      endpoint: '/reports',
      query: status !== 'all' ? { status } : {},
    });
  });

  const [selectedReport, setSelectedReport] = createSignal<Report | null>(null);

  const categoryLabels: Record<string, string> = {
    harassment: 'Harassment',
    hate_speech: 'Hate Speech',
    spam: 'Spam',
    nsfw: 'NSFW',
    other: 'Other',
  };

  const statusColors: Record<ReportStatus, string> = {
    pending: 'text-yellow-400',
    reviewing: 'text-blue-400',
    resolved: 'text-green-400',
    dismissed: 'text-gray-400',
  };

  return (
    <div class="p-6">
      <div class="mb-6">
        <h2 class="text-2xl font-semibold text-white mb-4">Reports Queue</h2>

        {/* Status Filter */}
        <div class="flex gap-2">
          <For each={['all', 'pending', 'reviewing', 'resolved', 'dismissed'] as const}>
            {(status) => (
              <button
                onClick={() => setStatusFilter(status)}
                class={`
                  px-4 py-2 rounded transition-colors
                  ${statusFilter() === status
                    ? 'bg-primary-500 text-white'
                    : 'bg-surface-700 text-gray-300 hover:bg-surface-600'
                  }
                `}
              >
                {status.charAt(0).toUpperCase() + status.slice(1)}
              </button>
            )}
          </For>
        </div>
      </div>

      {/* Reports List */}
      <div class="space-y-3">
        <Show when={!reports.loading} fallback={<div class="text-gray-400">Loading...</div>}>
          <For each={reports()} fallback={<div class="text-gray-400">No reports found.</div>}>
            {(report) => (
              <div
                onClick={() => setSelectedReport(report)}
                class="bg-surface-800 rounded-lg p-4 border border-surface-700 hover:border-primary-500 cursor-pointer transition-colors"
              >
                <div class="flex items-start justify-between">
                  <div class="flex-1">
                    <div class="flex items-center gap-3 mb-2">
                      <span class="text-sm font-medium px-2 py-1 rounded bg-surface-700 text-gray-300">
                        {categoryLabels[report.category]}
                      </span>
                      <span class={`text-sm font-medium ${statusColors[report.status]}`}>
                        {report.status.toUpperCase()}
                      </span>
                    </div>

                    <Show when={report.description}>
                      <p class="text-gray-300 text-sm mb-2">{report.description}</p>
                    </Show>

                    <div class="text-xs text-gray-500">
                      Reported by {report.reporter_id.slice(0, 8)}... •{' '}
                      {new Date(report.created_at).toLocaleString()}
                    </div>
                  </div>

                  <div class="text-sm text-gray-400">
                    <Show when={report.reported_message_id}>
                      <div>Message ID: {report.reported_message_id?.slice(0, 8)}...</div>
                    </Show>
                    <Show when={report.reported_user_id}>
                      <div>User ID: {report.reported_user_id?.slice(0, 8)}...</div>
                    </Show>
                  </div>
                </div>
              </div>
            )}
          </For>
        </Show>
      </div>

      {/* Detail Modal */}
      <Show when={selectedReport()}>
        <ReportDetailModal
          report={selectedReport()!}
          onClose={() => setSelectedReport(null)}
          onUpdate={() => {
            reports.mutate();
            setSelectedReport(null);
          }}
        />
      </Show>
    </div>
  );
};
```

**Commit:**
```bash
git add client/src/components/admin/ReportsPanel.tsx
git commit -m "feat(moderation): add admin reports panel"
```

---

### Task 15: Report Detail Modal (Client)

**Files:**
- Create: `client/src/components/admin/ReportDetailModal.tsx`

**Purpose:** Detailed view of report with resolve/dismiss actions.

```tsx
import { Component, createSignal, Show } from 'solid-js';
import { invoke } from '@tauri-apps/api/core';
import { Report } from '../../lib/types';

interface ReportDetailModalProps {
  report: Report;
  onClose: () => void;
  onUpdate: () => void;
}

export const ReportDetailModal: Component<ReportDetailModalProps> = (props) => {
  const [resolution, setResolution] = createSignal('');
  const [isProcessing, setIsProcessing] = createSignal(false);
  const [error, setError] = createSignal('');

  const handleResolve = async () => {
    if (!resolution().trim()) {
      setError('Please provide a resolution note');
      return;
    }

    setIsProcessing(true);
    setError('');

    try {
      await invoke('api_request', {
        method: 'PATCH',
        endpoint: `/reports/${props.report.id}/resolve`,
        body: { resolution: resolution() },
      });
      props.onUpdate();
    } catch (err: any) {
      setError(err.message || 'Failed to resolve report');
    } finally {
      setIsProcessing(false);
    }
  };

  const handleDismiss = async () => {
    setIsProcessing(true);
    setError('');

    try {
      await invoke('api_request', {
        method: 'PATCH',
        endpoint: `/reports/${props.report.id}/dismiss`,
      });
      props.onUpdate();
    } catch (err: any) {
      setError(err.message || 'Failed to dismiss report');
    } finally {
      setIsProcessing(false);
    }
  };

  return (
    <div class="fixed inset-0 z-50 flex items-center justify-center bg-black/70">
      <div class="bg-surface-800 rounded-lg shadow-xl w-full max-w-2xl p-6 max-h-[90vh] overflow-y-auto">
        <h2 class="text-xl font-semibold text-white mb-4">Report Details</h2>

        <div class="space-y-4">
          {/* Category & Status */}
          <div class="flex gap-4">
            <div>
              <div class="text-sm text-gray-400 mb-1">Category</div>
              <div class="text-white font-medium">{props.report.category}</div>
            </div>
            <div>
              <div class="text-sm text-gray-400 mb-1">Status</div>
              <div class="text-white font-medium">{props.report.status}</div>
            </div>
          </div>

          {/* Description */}
          <Show when={props.report.description}>
            <div>
              <div class="text-sm text-gray-400 mb-1">Description</div>
              <div class="text-white bg-surface-700 rounded p-3">{props.report.description}</div>
            </div>
          </Show>

          {/* IDs */}
          <div class="grid grid-cols-2 gap-4 text-sm">
            <div>
              <div class="text-gray-400 mb-1">Reporter ID</div>
              <div class="text-white font-mono">{props.report.reporter_id}</div>
            </div>
            <Show when={props.report.reported_user_id}>
              <div>
                <div class="text-gray-400 mb-1">Reported User ID</div>
                <div class="text-white font-mono">{props.report.reported_user_id}</div>
              </div>
            </Show>
            <Show when={props.report.reported_message_id}>
              <div>
                <div class="text-gray-400 mb-1">Reported Message ID</div>
                <div class="text-white font-mono">{props.report.reported_message_id}</div>
              </div>
            </Show>
          </div>

          {/* Timestamps */}
          <div class="text-sm">
            <div class="text-gray-400 mb-1">Created At</div>
            <div class="text-white">{new Date(props.report.created_at).toLocaleString()}</div>
          </div>

          {/* Resolution (if resolved) */}
          <Show when={props.report.status === 'resolved' && props.report.resolution}>
            <div>
              <div class="text-sm text-gray-400 mb-1">Resolution</div>
              <div class="text-white bg-surface-700 rounded p-3">{props.report.resolution}</div>
            </div>
          </Show>

          {/* Action Form (if pending) */}
          <Show when={props.report.status === 'pending' || props.report.status === 'reviewing'}>
            <div class="border-t border-surface-700 pt-4">
              <label class="block text-sm font-medium text-gray-300 mb-2">
                Resolution Notes
              </label>
              <textarea
                value={resolution()}
                onInput={(e) => setResolution(e.target.value)}
                placeholder="Describe the action taken..."
                rows={3}
                class="w-full bg-surface-700 text-white rounded px-3 py-2 border border-surface-600 focus:border-primary-500 focus:outline-none resize-none"
              />
            </div>
          </Show>

          {/* Error */}
          <Show when={error()}>
            <div class="text-red-400 text-sm">{error()}</div>
          </Show>

          {/* Actions */}
          <div class="flex justify-end gap-3 border-t border-surface-700 pt-4">
            <button
              onClick={props.onClose}
              class="px-4 py-2 rounded bg-surface-700 text-white hover:bg-surface-600 transition-colors"
              disabled={isProcessing()}
            >
              Close
            </button>

            <Show when={props.report.status === 'pending' || props.report.status === 'reviewing'}>
              <button
                onClick={handleDismiss}
                class="px-4 py-2 rounded bg-gray-600 text-white hover:bg-gray-700 transition-colors disabled:opacity-50"
                disabled={isProcessing()}
              >
                Dismiss
              </button>
              <button
                onClick={handleResolve}
                class="px-4 py-2 rounded bg-primary-500 text-white hover:bg-primary-600 transition-colors disabled:opacity-50"
                disabled={isProcessing()}
              >
                {isProcessing() ? 'Processing...' : 'Resolve'}
              </button>
            </Show>
          </div>
        </div>
      </div>
    </div>
  );
};
```

**Commit:**
```bash
git add client/src/components/admin/ReportDetailModal.tsx
git commit -m "feat(moderation): add report detail modal"
```

---

### Task 16: CHANGELOG

**Files:**
- Modify: `CHANGELOG.md`

**Purpose:** Document new features.

Add under `[Unreleased]`:

```markdown
### Added
- Content filters with guild-configurable rules (hate speech, discrimination, harassment)
- User reporting system with admin review queue and resolution workflow
- Absolute user blocking with bidirectional message/event filtering
- Guild Safety Settings tab for managing content filters
- Admin Reports Panel for reviewing and resolving user reports
- "Report Message" and "Block User" options in context menus
- Blocked user placeholder in message lists
- WebSocket event filtering to prevent blocked users from seeing each other's activity
- Moderation actions logging for audit trail

### Security
- Server-side content validation with filter pattern matching
- Permission checks on all moderation endpoints (admin-only for reports)
- Bidirectional block enforcement at database and WebSocket layers
```

**Commit:**
```bash
git add CHANGELOG.md
git commit -m "docs: update changelog for moderation features"
```

---

## Verification

### Server Verification

```bash
cd server

# Check compilation
cargo check

# Run tests
cargo test

# Check for unused dependencies
cargo machete

# Verify migration
sqlx migrate run
psql $DATABASE_URL -c "\d reports"
psql $DATABASE_URL -c "\d user_blocks"
psql $DATABASE_URL -c "\d guild_filter_configs"
```

### Client Verification

```bash
cd client

# Type check
bun run typecheck

# Lint
bun run lint

# Build
bun run build
```

### Integration Testing

**Test Scenarios:**

1. **Content Filters:**
   - Enable hate_speech filter with delete_warn action
   - Send message matching pattern → Should be rejected
   - Verify moderation_actions log entry created

2. **User Blocking:**
   - User A blocks User B
   - User B sends message → User A should not see it
   - User B's typing indicator → Should not reach User A
   - Verify bidirectional (B blocks A → same behavior)

3. **User Reporting:**
   - Submit report on message
   - Admin sees report in queue
   - Admin resolves report → Status updates to "resolved"

4. **Guild Safety Settings:**
   - Guild admin enables discrimination filter
   - Change action to shadow_ban
   - Verify config saved in database

---

## Performance Considerations

### Indexes
All critical queries have indexes:
- `user_blocks(blocker_id, blocked_id)` — Block lookups
- `reports(status, guild_id)` — Admin queue filtering
- `guild_filter_configs(guild_id)` — Filter checks

### Caching (Optional Enhancement)
For high-traffic guilds, consider caching:
- Blocked relationships in Redis (TTL: 5 minutes)
- Filter configs in Redis (TTL: 15 minutes, invalidate on update)

### N+1 Prevention
- `is_blocked()` is called once per WebSocket recipient → O(n) per event
- For 100-user guilds with active chat: ~50 QPS per event
- **Future optimization:** Batch block checks or cache in memory

---

## Security Audit Checklist

- [ ] Filter patterns validated server-side (no client-side bypass)
- [ ] Report endpoints check permissions (admin-only for listing)
- [ ] Block relationships enforce `blocker_id != blocked_id` constraint
- [ ] WebSocket filtering prevents information leaks
- [ ] Content filter action limits prevent abuse (delete_warn, shadow_ban, log only)
- [ ] Moderation actions logged for audit trail
- [ ] No personal data in filter pattern strings
- [ ] Rate limiting on report submission (add if not present)

---

## Future Enhancements

### Phase 6 (Next Priority)
- **Auto-Moderation Improvements:**
  - Integration with external moderation APIs (Perspective API, Azure Content Moderator)
  - Machine learning-based classification (false positive reduction)
  - Context-aware filtering (channel-specific rules)

- **Advanced Blocking:**
  - DM blocking (prevent direct messages)
  - Voice call blocking (prevent joining same voice channel)
  - Server-wide block lists (guild-level bans)

- **Admin Tools:**
  - Bulk actions (ban multiple users from reports)
  - Report analytics dashboard (trends, most reported users)
  - Appeal system for false positives

---

## Completion

**Total Tasks:** 16
**Estimated Implementation Time:** 8-12 hours
**Risk Level:** Medium (WebSocket filtering requires careful testing)

**Sign-off:**
- [ ] All migrations run successfully
- [ ] Server tests pass
- [ ] Client builds without errors
- [ ] Integration tests pass (manual verification)
- [ ] CHANGELOG updated
- [ ] Code reviewed (security + architecture concerns)

---

**End of Plan**
