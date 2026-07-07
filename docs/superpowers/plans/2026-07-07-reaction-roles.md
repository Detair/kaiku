# Reaction Roles Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let admins bind an emoji on a message to a guild role so members can grant/revoke that role themselves by reacting, safely and with live WebSocket updates.

**Architecture:** One new table (`reaction_role_bindings`) and one new Tier-1 sub-module under `server/src/guild/` (`reaction_roles.rs` handlers + error + inline types, `queries/reaction_roles.rs` for SQL), mirroring the existing `roles.rs` / `queries/roles.rs` split. Binding creation is gated by `MANAGE_ROLES` plus a role-hierarchy guard (`can_manage_role`) and a "not self-assignable if dangerous" guard that reuses the existing `GuildPermissions::EVERYONE_FORBIDDEN` deny-list. Reaction-time grants hook into `chat/reactions.rs` inside the reaction-write transaction. A new `MemberRolesUpdated` WS event (broadcast on the existing `guild:{id}` Redis channel) is emitted from both the reaction path and — as a latent-gap fix — the existing admin assign/remove handlers.

**Tech Stack:** Rust (axum, sqlx/PostgreSQL, thiserror, utoipa), Valkey/Redis pub-sub (fred), Solid.js + TypeScript client, `#[sqlx::test]` + `TestApp` integration harness, vitest for client.

---

## File Structure

**Server — create:**
- `server/migrations/20260707000000_reaction_role_bindings.sql` — the new table + indexes.
- `server/src/guild/reaction_roles.rs` — `ReactionRoleError`, request/response types, the pure `ensure_role_self_assignable` guard, the three HTTP handlers, and the reaction-time `apply_on_reaction_add` / `apply_on_reaction_remove` service functions.
- `server/src/guild/queries/reaction_roles.rs` — binding CRUD, `member_role_ids`, and transaction-based grant/revoke helpers.
- `server/tests/integration/reaction_roles.rs` — HTTP + reaction-hook integration tests.

**Server — modify:**
- `server/src/guild/mod.rs` — register the module and its routes.
- `server/src/guild/queries/mod.rs` — register the queries sub-module.
- `server/src/ws/events.rs` — add the `MemberRolesUpdated` variant + `broadcast_to_guild` helper.
- `server/src/chat/reactions.rs` — wrap reaction writes in a transaction and call the hook; broadcast the resulting events.
- `server/src/guild/roles.rs` — emit `MemberRolesUpdated` from `assign_role` / `remove_role` (retrofit).
- `server/tests/integration/main.rs` — add `mod reaction_roles;`.

**Client — create:**
- `client/src/stores/__tests__/memberRoles.test.ts` — reducer unit test.

**Client — modify:**
- `client/src/lib/types/events.ts` — add `member_roles_updated` to the event union.
- `client/src/stores/members.ts` — add `applyMemberRoles(guildId, userId, roleIds)` reducer.
- `client/src/stores/websocket/index.ts` — dispatch the new event (both Tauri-listen and switch paths).
- `client/src/components/.../GuildSettingsModal` — new "Reaction Roles" section (final task).

**Docs:**
- `CHANGELOG.md` — user-facing `### Added` entry.

---

## Task 1: Database migration

**Files:**
- Create: `server/migrations/20260707000000_reaction_role_bindings.sql`

- [ ] **Step 1: Write the migration**

```sql
-- Reaction-role bindings: bind an emoji on a message to a self-assignable role.
CREATE TABLE reaction_role_bindings (
    id           UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    guild_id     UUID NOT NULL REFERENCES guilds(id)       ON DELETE CASCADE,
    channel_id   UUID NOT NULL REFERENCES channels(id)     ON DELETE CASCADE,
    message_id   UUID NOT NULL REFERENCES messages(id)     ON DELETE CASCADE,
    -- Emoji key: unicode grapheme ("🎨") or custom emoji ref ("<:name:uuid>"),
    -- the exact string form message_reactions already stores.
    emoji        VARCHAR(128) NOT NULL,
    role_id      UUID NOT NULL REFERENCES guild_roles(id)  ON DELETE CASCADE,
    -- Non-null groups bindings for `unique` (pick-one) behaviour.
    group_key    VARCHAR(64),
    mode         VARCHAR(16) NOT NULL DEFAULT 'toggle',
    created_by   UUID REFERENCES users(id) ON DELETE SET NULL,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT NOW(),

    CONSTRAINT reaction_role_unique_emoji UNIQUE (message_id, emoji),
    CONSTRAINT reaction_role_mode_valid   CHECK (mode IN ('toggle', 'unique'))
);

CREATE INDEX idx_reaction_role_message ON reaction_role_bindings(message_id);
CREATE INDEX idx_reaction_role_guild   ON reaction_role_bindings(guild_id);
```

- [ ] **Step 2: Apply the migration against the dev DB**

Run:
```bash
DATABASE_URL="postgresql://voicechat:voicechat_dev@localhost:5433/voicechat" \
  sqlx migrate run --source server/migrations
```
Expected: `Applied 20260707000000/migrate reaction role bindings`.

- [ ] **Step 3: Commit**

```bash
git add server/migrations/20260707000000_reaction_role_bindings.sql
git commit -m "feat(db): reaction_role_bindings table"
```

---

## Task 2: The self-assignable guard (pure function, TDD)

The security core: a role must be strictly below the actor's highest role (delegated to the existing `can_manage_role`) **and** must not carry any dangerous permission. We reuse `GuildPermissions::EVERYONE_FORBIDDEN` (already the canonical "must never be self-granted" set: MANAGE_ROLES, MANAGE_GUILD, KICK/BAN, MANAGE_CHANNELS, etc.).

**Files:**
- Create: `server/src/guild/reaction_roles.rs`
- Modify: `server/src/guild/mod.rs` (add `pub mod reaction_roles;`)

- [ ] **Step 1: Register the module so the file compiles**

In `server/src/guild/mod.rs`, add to the `pub mod` list (keep alphabetical grouping near `roles`):
```rust
pub mod reaction_roles;
```

- [ ] **Step 2: Write the module skeleton with the guard + its unit test**

Create `server/src/guild/reaction_roles.rs`:
```rust
//! Self-assignable (reaction) role bindings.
//!
//! Admins bind an emoji on a message to a role; members react to grant/revoke
//! it themselves. Creation is gated by `MANAGE_ROLES` + a hierarchy guard +
//! a "not dangerous" guard; reaction-time self-assign is intentionally
//! unprivileged (that is the feature).

use crate::permissions::GuildPermissions;

/// Reject binding a role that carries any permission a member must never be
/// able to self-grant. Reuses the canonical `EVERYONE_FORBIDDEN` deny-list so
/// this stays in lockstep with the @everyone guard.
///
/// Returns `true` if the role is safe to make self-assignable.
#[must_use]
pub fn is_role_self_assignable(role_permissions: GuildPermissions) -> bool {
    !role_permissions.intersects(GuildPermissions::EVERYONE_FORBIDDEN)
}

#[cfg(test)]
mod guard_tests {
    use super::*;

    #[test]
    fn safe_roles_are_self_assignable() {
        let safe = GuildPermissions::SEND_MESSAGES
            | GuildPermissions::VOICE_CONNECT
            | GuildPermissions::ADD_REACTIONS;
        assert!(is_role_self_assignable(safe));
        // A pure cosmetic role (no perms) is fine.
        assert!(is_role_self_assignable(GuildPermissions::empty()));
    }

    #[test]
    fn dangerous_roles_are_not_self_assignable() {
        for perm in [
            GuildPermissions::MANAGE_ROLES,
            GuildPermissions::MANAGE_GUILD,
            GuildPermissions::BAN_MEMBERS,
            GuildPermissions::KICK_MEMBERS,
            GuildPermissions::MANAGE_CHANNELS,
        ] {
            let perms = GuildPermissions::SEND_MESSAGES | perm;
            assert!(
                !is_role_self_assignable(perms),
                "{perm:?} must make a role non-self-assignable"
            );
        }
    }
}
```

- [ ] **Step 3: Run the guard tests — verify they pass**

Run:
```bash
SQLX_OFFLINE=true cargo test -p vc-server guild::reaction_roles::guard_tests -- --nocapture
```
Expected: `test result: ok. 2 passed`.

- [ ] **Step 4: Commit**

```bash
git add server/src/guild/reaction_roles.rs server/src/guild/mod.rs
git commit -m "feat(guild): reaction-role self-assignable guard"
```

---

## Task 3: Request/response types

**Files:**
- Modify: `server/src/guild/reaction_roles.rs`

- [ ] **Step 1: Add the wire types below the guard**

Append to `server/src/guild/reaction_roles.rs`:
```rust
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use validator::Validate;

/// Body for `POST /api/guilds/{id}/reaction-roles`.
#[derive(Debug, Deserialize, Validate, utoipa::ToSchema)]
pub struct CreateReactionRoleRequest {
    pub channel_id: Uuid,
    pub message_id: Uuid,
    #[validate(length(min = 1, max = 128))]
    pub emoji: String,
    pub role_id: Uuid,
    /// "toggle" (default) or "unique".
    #[serde(default = "default_mode")]
    #[validate(custom(function = "validate_mode"))]
    pub mode: String,
    #[validate(length(max = 64))]
    pub group_key: Option<String>,
}

fn default_mode() -> String {
    "toggle".to_string()
}

fn validate_mode(mode: &str) -> Result<(), validator::ValidationError> {
    if mode == "toggle" || mode == "unique" {
        Ok(())
    } else {
        Err(validator::ValidationError::new("invalid_mode"))
    }
}

/// A reaction-role binding as returned by the API.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct ReactionRoleResponse {
    pub id: Uuid,
    pub guild_id: Uuid,
    pub channel_id: Uuid,
    pub message_id: Uuid,
    pub emoji: String,
    pub role_id: Uuid,
    pub group_key: Option<String>,
    pub mode: String,
    pub created_at: DateTime<Utc>,
}
```

- [ ] **Step 2: Verify it compiles**

Run:
```bash
SQLX_OFFLINE=true cargo check -p vc-server
```
Expected: compiles (warnings about unused types are OK at this stage).

- [ ] **Step 3: Commit**

```bash
git add server/src/guild/reaction_roles.rs
git commit -m "feat(guild): reaction-role request/response types"
```

---

## Task 4: Queries module

All binding CRUD and the transaction-based grant/revoke helpers. The apply helpers take `&mut Transaction` so the `unique`-group swap (assign new + remove sibling roles + clear sibling reactions) is atomic.

**Files:**
- Create: `server/src/guild/queries/reaction_roles.rs`
- Modify: `server/src/guild/queries/mod.rs`

- [ ] **Step 1: Register the sub-module**

In `server/src/guild/queries/mod.rs`, add near the other `pub mod` lines:
```rust
pub mod reaction_roles;
```

- [ ] **Step 2: Write the queries**

Create `server/src/guild/queries/reaction_roles.rs`:
```rust
//! Queries for reaction-role bindings and reaction-time grant/revoke.

use chrono::{DateTime, Utc};
use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

use super::super::reaction_roles::{ReactionRoleError, ReactionRoleResponse};

/// A binding row, in the column order used across this module.
type BindingRow = (
    Uuid,           // id
    Uuid,           // guild_id
    Uuid,           // channel_id
    Uuid,           // message_id
    String,         // emoji
    Uuid,           // role_id
    Option<String>, // group_key
    String,         // mode
    DateTime<Utc>,  // created_at
);

fn to_response(r: BindingRow) -> ReactionRoleResponse {
    ReactionRoleResponse {
        id: r.0,
        guild_id: r.1,
        channel_id: r.2,
        message_id: r.3,
        emoji: r.4,
        role_id: r.5,
        group_key: r.6,
        mode: r.7,
        created_at: r.8,
    }
}

const SELECT_COLS: &str =
    "id, guild_id, channel_id, message_id, emoji, role_id, group_key, mode, created_at";

/// Insert a binding. Returns the created row. Unique-violation on
/// (message_id, emoji) surfaces as `sqlx::Error` for the handler to map.
#[allow(clippy::too_many_arguments)]
pub async fn insert_binding(
    pool: &PgPool,
    guild_id: Uuid,
    channel_id: Uuid,
    message_id: Uuid,
    emoji: &str,
    role_id: Uuid,
    group_key: Option<&str>,
    mode: &str,
    created_by: Uuid,
) -> Result<ReactionRoleResponse, ReactionRoleError> {
    let row: BindingRow = sqlx::query_as(&format!(
        "INSERT INTO reaction_role_bindings
             (guild_id, channel_id, message_id, emoji, role_id, group_key, mode, created_by)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
         RETURNING {SELECT_COLS}"
    ))
    .bind(guild_id)
    .bind(channel_id)
    .bind(message_id)
    .bind(emoji)
    .bind(role_id)
    .bind(group_key)
    .bind(mode)
    .bind(created_by)
    .fetch_one(pool)
    .await?;
    Ok(to_response(row))
}

/// List bindings for a guild, optionally filtered to one message.
pub async fn list_bindings(
    pool: &PgPool,
    guild_id: Uuid,
    message_id: Option<Uuid>,
) -> Result<Vec<ReactionRoleResponse>, ReactionRoleError> {
    let rows: Vec<BindingRow> = sqlx::query_as(&format!(
        "SELECT {SELECT_COLS} FROM reaction_role_bindings
         WHERE guild_id = $1 AND ($2::uuid IS NULL OR message_id = $2)
         ORDER BY created_at ASC"
    ))
    .bind(guild_id)
    .bind(message_id)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(to_response).collect())
}

/// Delete a binding scoped to its guild. Returns rows affected.
pub async fn delete_binding(
    pool: &PgPool,
    guild_id: Uuid,
    binding_id: Uuid,
) -> Result<u64, ReactionRoleError> {
    let res = sqlx::query(
        "DELETE FROM reaction_role_bindings WHERE id = $1 AND guild_id = $2",
    )
    .bind(binding_id)
    .bind(guild_id)
    .execute(pool)
    .await?;
    Ok(res.rows_affected())
}

/// A binding as needed by the reaction-time hook.
pub struct HookBinding {
    pub role_id: Uuid,
    pub mode: String,
    pub group_key: Option<String>,
}

/// Look up the binding for (message_id, emoji), if any.
pub async fn find_binding_for_reaction(
    tx: &mut Transaction<'_, Postgres>,
    message_id: Uuid,
    emoji: &str,
) -> Result<Option<HookBinding>, ReactionRoleError> {
    let row: Option<(Uuid, String, Option<String>)> = sqlx::query_as(
        "SELECT role_id, mode, group_key FROM reaction_role_bindings
         WHERE message_id = $1 AND emoji = $2",
    )
    .bind(message_id)
    .bind(emoji)
    .fetch_optional(&mut **tx)
    .await?;
    Ok(row.map(|(role_id, mode, group_key)| HookBinding {
        role_id,
        mode,
        group_key,
    }))
}

/// Sibling bindings in the same group on the same message (excludes `emoji`).
/// Returns (emoji, role_id) pairs for the `unique` swap.
pub async fn sibling_bindings_in_group(
    tx: &mut Transaction<'_, Postgres>,
    message_id: Uuid,
    group_key: &str,
    exclude_emoji: &str,
) -> Result<Vec<(String, Uuid)>, ReactionRoleError> {
    let rows: Vec<(String, Uuid)> = sqlx::query_as(
        "SELECT emoji, role_id FROM reaction_role_bindings
         WHERE message_id = $1 AND group_key = $2 AND emoji <> $3",
    )
    .bind(message_id)
    .bind(group_key)
    .bind(exclude_emoji)
    .fetch_all(&mut **tx)
    .await?;
    Ok(rows)
}

/// Grant a role to a member inside the transaction (idempotent).
pub async fn tx_assign_role(
    tx: &mut Transaction<'_, Postgres>,
    guild_id: Uuid,
    user_id: Uuid,
    role_id: Uuid,
    assigned_by: Uuid,
) -> Result<(), ReactionRoleError> {
    sqlx::query(
        "INSERT INTO guild_member_roles (guild_id, user_id, role_id, assigned_by)
         VALUES ($1, $2, $3, $4)
         ON CONFLICT (guild_id, user_id, role_id) DO NOTHING",
    )
    .bind(guild_id)
    .bind(user_id)
    .bind(role_id)
    .bind(assigned_by)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// Revoke a role from a member inside the transaction.
pub async fn tx_remove_role(
    tx: &mut Transaction<'_, Postgres>,
    guild_id: Uuid,
    user_id: Uuid,
    role_id: Uuid,
) -> Result<(), ReactionRoleError> {
    sqlx::query(
        "DELETE FROM guild_member_roles WHERE guild_id = $1 AND user_id = $2 AND role_id = $3",
    )
    .bind(guild_id)
    .bind(user_id)
    .bind(role_id)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// Remove a user's stored reaction for an emoji inside the transaction
/// (used to clear the losing reaction on a `unique` swap).
pub async fn tx_remove_user_reaction(
    tx: &mut Transaction<'_, Postgres>,
    message_id: Uuid,
    user_id: Uuid,
    emoji: &str,
) -> Result<(), ReactionRoleError> {
    sqlx::query(
        "DELETE FROM message_reactions WHERE message_id = $1 AND user_id = $2 AND emoji = $3",
    )
    .bind(message_id)
    .bind(user_id)
    .bind(emoji)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// The member's full set of role IDs (for the MemberRolesUpdated payload).
pub async fn tx_member_role_ids(
    tx: &mut Transaction<'_, Postgres>,
    guild_id: Uuid,
    user_id: Uuid,
) -> Result<Vec<Uuid>, ReactionRoleError> {
    let rows: Vec<(Uuid,)> = sqlx::query_as(
        "SELECT role_id FROM guild_member_roles WHERE guild_id = $1 AND user_id = $2",
    )
    .bind(guild_id)
    .bind(user_id)
    .fetch_all(&mut **tx)
    .await?;
    Ok(rows.into_iter().map(|(r,)| r).collect())
}

/// Whether the user is a member of the guild (pool variant, for the hook guard).
pub async fn is_member(
    tx: &mut Transaction<'_, Postgres>,
    guild_id: Uuid,
    user_id: Uuid,
) -> Result<bool, ReactionRoleError> {
    let row: Option<(i32,)> =
        sqlx::query_as("SELECT 1 FROM guild_members WHERE guild_id = $1 AND user_id = $2")
            .bind(guild_id)
            .bind(user_id)
            .fetch_optional(&mut **tx)
            .await?;
    Ok(row.is_some())
}

/// Fetch a role's (position, permissions) for the creation-time guard.
pub async fn fetch_role_guard_state(
    pool: &PgPool,
    guild_id: Uuid,
    role_id: Uuid,
) -> Result<Option<(i32, i64, bool)>, ReactionRoleError> {
    let row: Option<(i32, i64, bool)> = sqlx::query_as(
        "SELECT position, permissions, is_default FROM guild_roles
         WHERE id = $1 AND guild_id = $2",
    )
    .bind(role_id)
    .bind(guild_id)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

/// Whether a message exists in the given channel (creation-time validation).
pub async fn message_in_channel(
    pool: &PgPool,
    message_id: Uuid,
    channel_id: Uuid,
) -> Result<bool, ReactionRoleError> {
    let row: Option<(i32,)> =
        sqlx::query_as("SELECT 1 FROM messages WHERE id = $1 AND channel_id = $2")
            .bind(message_id)
            .bind(channel_id)
            .fetch_optional(pool)
            .await?;
    Ok(row.is_some())
}

/// Whether a channel belongs to the guild (creation-time validation).
pub async fn channel_in_guild(
    pool: &PgPool,
    channel_id: Uuid,
    guild_id: Uuid,
) -> Result<bool, ReactionRoleError> {
    let row: Option<(i32,)> =
        sqlx::query_as("SELECT 1 FROM channels WHERE id = $1 AND guild_id = $2")
            .bind(channel_id)
            .bind(guild_id)
            .fetch_optional(pool)
            .await?;
    Ok(row.is_some())
}
```

- [ ] **Step 3: Verify it compiles** (the error type it references is added in Task 5; expect an unresolved-import error until then)

Run:
```bash
SQLX_OFFLINE=true cargo check -p vc-server 2>&1 | head -20
```
Expected: only errors about `ReactionRoleError` not existing yet — resolved in Task 5. Do not commit until Task 5 compiles.

---

## Task 5: Error type + HTTP handlers + route wiring

**Files:**
- Modify: `server/src/guild/reaction_roles.rs`
- Modify: `server/src/guild/mod.rs`
- Create: `server/tests/integration/reaction_roles.rs`
- Modify: `server/tests/integration/main.rs`

- [ ] **Step 1: Add the error type + handlers**

Append to `server/src/guild/reaction_roles.rs` (add the imports to the top `use` block as noted inline):
```rust
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use thiserror::Error;

use super::queries::reaction_roles as queries;
use crate::api::AppState;
use crate::auth::AuthUser;
use crate::permissions::{
    can_manage_role, require_guild_permission, PermissionError,
};

#[derive(Debug, Error)]
pub enum ReactionRoleError {
    #[error("Binding not found")]
    NotFound,
    #[error("Not a member of this guild")]
    NotMember,
    #[error("Role not found")]
    RoleNotFound,
    #[error("Message not found in channel")]
    MessageNotFound,
    #[error("Channel not in guild")]
    ChannelNotFound,
    #[error("Role is not self-assignable (carries a privileged permission)")]
    RoleNotSelfAssignable,
    #[error("Cannot bind the @everyone/default role")]
    DefaultRoleNotBindable,
    #[error("A binding for this emoji already exists on the message")]
    DuplicateBinding,
    #[error("{0}")]
    Permission(#[from] PermissionError),
    #[error("Validation failed: {0}")]
    Validation(String),
    #[error("Database error")]
    Database(#[from] sqlx::Error),
}

impl IntoResponse for ReactionRoleError {
    fn into_response(self) -> Response {
        if let Self::Database(db_err) = &self {
            tracing::error!(error = %db_err, "Reaction-role database operation failed");
        }
        let (status, code, message) = match &self {
            Self::NotFound => (StatusCode::NOT_FOUND, "BINDING_NOT_FOUND", self.to_string()),
            Self::NotMember => (StatusCode::FORBIDDEN, "NOT_MEMBER", self.to_string()),
            Self::RoleNotFound => (StatusCode::NOT_FOUND, "ROLE_NOT_FOUND", self.to_string()),
            Self::MessageNotFound => {
                (StatusCode::NOT_FOUND, "MESSAGE_NOT_FOUND", self.to_string())
            }
            Self::ChannelNotFound => {
                (StatusCode::NOT_FOUND, "CHANNEL_NOT_FOUND", self.to_string())
            }
            Self::RoleNotSelfAssignable => (
                StatusCode::FORBIDDEN,
                "ROLE_NOT_SELF_ASSIGNABLE",
                self.to_string(),
            ),
            Self::DefaultRoleNotBindable => (
                StatusCode::BAD_REQUEST,
                "DEFAULT_ROLE_NOT_BINDABLE",
                self.to_string(),
            ),
            Self::DuplicateBinding => {
                (StatusCode::CONFLICT, "DUPLICATE_BINDING", self.to_string())
            }
            Self::Permission(_) => {
                (StatusCode::FORBIDDEN, "PERMISSION_DENIED", self.to_string())
            }
            Self::Validation(_) => {
                (StatusCode::BAD_REQUEST, "VALIDATION_ERROR", self.to_string())
            }
            Self::Database(_) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "INTERNAL_ERROR",
                "Database error".to_string(),
            ),
        };
        (
            status,
            Json(serde_json::json!({ "error": code, "message": message })),
        )
            .into_response()
    }
}

fn map_perm(e: PermissionError) -> ReactionRoleError {
    match e {
        PermissionError::NotGuildMember => ReactionRoleError::NotMember,
        other => ReactionRoleError::Permission(other),
    }
}

/// Query params for listing.
#[derive(Debug, Deserialize)]
pub struct ListQuery {
    pub message_id: Option<Uuid>,
}

/// `GET /api/guilds/{id}/reaction-roles`
#[utoipa::path(
    get,
    path = "/api/guilds/{id}/reaction-roles",
    tag = "reaction-roles",
    params(("id" = Uuid, Path, description = "Guild ID")),
    responses((status = 200, body = Vec<ReactionRoleResponse>)),
    security(("bearer_auth" = []))
)]
#[tracing::instrument(skip(state))]
pub async fn list_reaction_roles(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(guild_id): Path<Uuid>,
    Query(q): Query<ListQuery>,
) -> Result<Json<Vec<ReactionRoleResponse>>, ReactionRoleError> {
    // Membership is enough to view bindings.
    require_guild_permission(&state.db, guild_id, auth.id, crate::permissions::GuildPermissions::empty())
        .await
        .map_err(map_perm)?;
    let bindings = queries::list_bindings(&state.db, guild_id, q.message_id).await?;
    Ok(Json(bindings))
}

/// `POST /api/guilds/{id}/reaction-roles`
#[utoipa::path(
    post,
    path = "/api/guilds/{id}/reaction-roles",
    tag = "reaction-roles",
    params(("id" = Uuid, Path, description = "Guild ID")),
    request_body = CreateReactionRoleRequest,
    responses((status = 200, body = ReactionRoleResponse)),
    security(("bearer_auth" = []))
)]
#[tracing::instrument(skip(state, body))]
pub async fn create_reaction_role(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(guild_id): Path<Uuid>,
    Json(body): Json<CreateReactionRoleRequest>,
) -> Result<Json<ReactionRoleResponse>, ReactionRoleError> {
    body.validate()
        .map_err(|e| ReactionRoleError::Validation(crate::validation::format_validation_errors(&e)))?;

    let ctx = require_guild_permission(
        &state.db,
        guild_id,
        auth.id,
        crate::permissions::GuildPermissions::MANAGE_ROLES,
    )
    .await
    .map_err(map_perm)?;

    // Validate the channel belongs to the guild and the message to the channel.
    if !queries::channel_in_guild(&state.db, body.channel_id, guild_id).await? {
        return Err(ReactionRoleError::ChannelNotFound);
    }
    if !queries::message_in_channel(&state.db, body.message_id, body.channel_id).await? {
        return Err(ReactionRoleError::MessageNotFound);
    }

    // Load the target role's guard state.
    let (position, perms_bits, is_default) =
        queries::fetch_role_guard_state(&state.db, guild_id, body.role_id)
            .await?
            .ok_or(ReactionRoleError::RoleNotFound)?;

    if is_default {
        return Err(ReactionRoleError::DefaultRoleNotBindable);
    }

    // Hierarchy guard: target role must be strictly below the actor's highest.
    let actor_position = if ctx.is_owner {
        -1
    } else {
        ctx.highest_role_position.unwrap_or(i32::MAX)
    };
    can_manage_role(ctx.computed_permissions, actor_position, position, None)?;

    // Dangerous-permission guard.
    let role_perms =
        crate::permissions::GuildPermissions::from_bits_truncate(perms_bits as u64);
    if !is_role_self_assignable(role_perms) {
        return Err(ReactionRoleError::RoleNotSelfAssignable);
    }

    let binding = queries::insert_binding(
        &state.db,
        guild_id,
        body.channel_id,
        body.message_id,
        &body.emoji,
        body.role_id,
        body.group_key.as_deref(),
        &body.mode,
        auth.id,
    )
    .await
    .map_err(|e| match e {
        ReactionRoleError::Database(sqlx::Error::Database(ref db))
            if db.is_unique_violation() =>
        {
            ReactionRoleError::DuplicateBinding
        }
        other => other,
    })?;

    Ok(Json(binding))
}

/// `DELETE /api/guilds/{id}/reaction-roles/{binding_id}`
#[utoipa::path(
    delete,
    path = "/api/guilds/{id}/reaction-roles/{binding_id}",
    tag = "reaction-roles",
    params(
        ("id" = Uuid, Path, description = "Guild ID"),
        ("binding_id" = Uuid, Path, description = "Binding ID")
    ),
    responses((status = 200, description = "Binding removed")),
    security(("bearer_auth" = []))
)]
#[tracing::instrument(skip(state))]
pub async fn delete_reaction_role(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((guild_id, binding_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<serde_json::Value>, ReactionRoleError> {
    require_guild_permission(
        &state.db,
        guild_id,
        auth.id,
        crate::permissions::GuildPermissions::MANAGE_ROLES,
    )
    .await
    .map_err(map_perm)?;

    let affected = queries::delete_binding(&state.db, guild_id, binding_id).await?;
    if affected == 0 {
        return Err(ReactionRoleError::NotFound);
    }
    Ok(Json(serde_json::json!({ "deleted": true, "id": binding_id })))
}
```

- [ ] **Step 2: Wire the routes**

In `server/src/guild/mod.rs` `router()`, add after the role routes block:
```rust
        // Reaction-role routes
        .route(
            "/{id}/reaction-roles",
            get(reaction_roles::list_reaction_roles).post(reaction_roles::create_reaction_role),
        )
        .route(
            "/{id}/reaction-roles/{binding_id}",
            delete(reaction_roles::delete_reaction_role),
        )
```

- [ ] **Step 3: Verify the whole workspace compiles**

Run:
```bash
SQLX_OFFLINE=true cargo check -p vc-server
```
Expected: compiles cleanly (Task 4's `ReactionRoleError` import now resolves).

- [ ] **Step 4: Write the HTTP integration tests**

Create `server/tests/integration/reaction_roles.rs`:
```rust
//! Integration tests for reaction-role bindings.
//!
//! Run with: `cargo test --test integration reaction_roles`

use axum::body::Body;
use axum::http::{Method, StatusCode};
use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;
use vc_server::permissions::GuildPermissions;

use crate::helpers::{
    add_guild_member, body_to_json, create_channel, create_guild_with_default_role,
    create_test_user, insert_message, TestApp,
};

/// Insert a non-default role with the given permissions/position; return its id.
async fn insert_role(
    pool: &PgPool,
    guild_id: Uuid,
    perms: GuildPermissions,
    position: i32,
) -> Uuid {
    let id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO guild_roles (id, guild_id, name, permissions, position, is_default)
         VALUES ($1, $2, 'role', $3, $4, false)",
    )
    .bind(id)
    .bind(guild_id)
    .bind(perms.bits() as i64)
    .bind(position)
    .execute(pool)
    .await
    .expect("insert role");
    id
}

#[sqlx::test]
async fn owner_can_create_binding_for_safe_role(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (owner, _) = create_test_user(&pool).await;
    // Owner's @everyone can be empty; owner bypasses hierarchy (position -1).
    let guild = create_guild_with_default_role(&pool, owner, GuildPermissions::empty()).await;
    let channel = create_channel(&pool, guild, "general").await;
    let msg = insert_message(&pool, channel, owner, "react here").await;
    let role = insert_role(&pool, guild, GuildPermissions::SEND_MESSAGES, 5).await;

    let token = vc_server_token(&app, owner);
    let req = TestApp::request(Method::POST, &format!("/api/guilds/{guild}/reaction-roles"))
        .header("Authorization", format!("Bearer {token}"))
        .header("Content-Type", "application/json")
        .body(Body::from(
            json!({
                "channel_id": channel,
                "message_id": msg,
                "emoji": "🎨",
                "role_id": role,
                "mode": "toggle"
            })
            .to_string(),
        ))
        .unwrap();
    let res = app.oneshot(req).await;
    assert_eq!(res.status(), StatusCode::OK);
    let body = body_to_json(res).await;
    assert_eq!(body["emoji"], "🎨");
    assert_eq!(body["role_id"], role.to_string());
}

#[sqlx::test]
async fn cannot_bind_dangerous_role(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (owner, _) = create_test_user(&pool).await;
    let guild = create_guild_with_default_role(&pool, owner, GuildPermissions::empty()).await;
    let channel = create_channel(&pool, guild, "general").await;
    let msg = insert_message(&pool, channel, owner, "react").await;
    let role = insert_role(&pool, guild, GuildPermissions::BAN_MEMBERS, 5).await;

    let token = vc_server_token(&app, owner);
    let req = TestApp::request(Method::POST, &format!("/api/guilds/{guild}/reaction-roles"))
        .header("Authorization", format!("Bearer {token}"))
        .header("Content-Type", "application/json")
        .body(Body::from(
            json!({ "channel_id": channel, "message_id": msg, "emoji": "🔨", "role_id": role })
                .to_string(),
        ))
        .unwrap();
    let res = app.oneshot(req).await;
    assert_eq!(res.status(), StatusCode::FORBIDDEN);
    let body = body_to_json(res).await;
    assert_eq!(body["error"], "ROLE_NOT_SELF_ASSIGNABLE");
}

#[sqlx::test]
async fn non_manager_cannot_create_binding(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (owner, _) = create_test_user(&pool).await;
    let (member, _) = create_test_user(&pool).await;
    let guild = create_guild_with_default_role(&pool, owner, GuildPermissions::empty()).await;
    add_guild_member(&pool, guild, member).await;
    let channel = create_channel(&pool, guild, "general").await;
    let msg = insert_message(&pool, channel, owner, "react").await;
    let role = insert_role(&pool, guild, GuildPermissions::SEND_MESSAGES, 5).await;

    let token = vc_server_token(&app, member);
    let req = TestApp::request(Method::POST, &format!("/api/guilds/{guild}/reaction-roles"))
        .header("Authorization", format!("Bearer {token}"))
        .header("Content-Type", "application/json")
        .body(Body::from(
            json!({ "channel_id": channel, "message_id": msg, "emoji": "🎨", "role_id": role })
                .to_string(),
        ))
        .unwrap();
    let res = app.oneshot(req).await;
    assert_eq!(res.status(), StatusCode::FORBIDDEN);
}

/// Helper: mint an access token for a user via the app config.
fn vc_server_token(app: &TestApp, user_id: Uuid) -> String {
    crate::helpers::generate_access_token(&app.config, user_id)
}
```

- [ ] **Step 5: Register the test module**

In `server/tests/integration/main.rs`, add (alphabetically, near `mod ratelimit;`):
```rust
mod reaction_roles;
```

- [ ] **Step 6: Run the HTTP tests**

Run (requires dev Postgres + Redis up):
```bash
SQLX_OFFLINE=true DATABASE_URL="postgresql://voicechat:voicechat_dev@localhost:5433/voicechat" \
  cargo test -p vc-server --test integration reaction_roles -- --nocapture
```
Expected: `owner_can_create_binding_for_safe_role`, `cannot_bind_dangerous_role`, `non_manager_cannot_create_binding` all pass.

- [ ] **Step 7: Regenerate the offline sqlx cache and commit**

Run:
```bash
cargo sqlx prepare --workspace -- --tests
git add server/src/guild/ server/tests/integration/ .sqlx/
git commit -m "feat(guild): reaction-role binding endpoints"
```

---

## Task 6: MemberRolesUpdated WS event + guild broadcast helper

**Files:**
- Modify: `server/src/ws/events.rs`

- [ ] **Step 1: Add the event variant**

In `server/src/ws/events.rs`, add to the `ServerEvent` enum near `GuildEmojiUpdated`:
```rust
    /// A member's role set changed (self-assign or admin assign/remove).
    MemberRolesUpdated {
        /// Guild the member belongs to.
        guild_id: Uuid,
        /// The member whose roles changed.
        user_id: Uuid,
        /// The member's full role-ID set after the change (idempotent).
        role_ids: Vec<Uuid>,
    },
```

- [ ] **Step 2: Add a generic guild broadcast helper**

After `broadcast_to_channel`, add:
```rust
/// Broadcast a server event to all of a guild's subscribers via Redis.
#[tracing::instrument(skip(redis, event), fields(guild_id = %guild_id))]
pub async fn broadcast_to_guild(
    redis: &Client,
    guild_id: Uuid,
    event: &ServerEvent,
) -> Result<(), Error> {
    let payload = serde_json::to_string(event)
        .map_err(|e| Error::new(ErrorKind::Parse, format!("JSON error: {e}")))?;
    redis
        .publish::<(), _, _>(channels::guild_events(guild_id), payload)
        .await?;
    Ok(())
}
```

- [ ] **Step 3: Verify compile + re-export**

Confirm `broadcast_to_guild` is reachable via `crate::ws::broadcast_to_guild` (the module re-exports `broadcast_to_channel`; add `broadcast_to_guild` to the same `pub use` in `server/src/ws/mod.rs` if channel is re-exported there).

Run:
```bash
SQLX_OFFLINE=true cargo check -p vc-server
```
Expected: compiles.

- [ ] **Step 4: Commit**

```bash
git add server/src/ws/
git commit -m "feat(ws): MemberRolesUpdated event + broadcast_to_guild"
```

---

## Task 7: Reaction-time hook

The hook runs inside the reaction-write transaction. On add: if a binding exists, grant the role; for `unique`, remove sibling roles + clear sibling reactions. On remove: revoke the role. Returns the data the caller needs to broadcast.

**Files:**
- Modify: `server/src/guild/reaction_roles.rs` (add the service functions)
- Modify: `server/src/chat/reactions.rs` (transactional + call hook + broadcast)

- [ ] **Step 1: Add the hook service functions**

Append to `server/src/guild/reaction_roles.rs`:
```rust
use sqlx::{Postgres, Transaction};

/// Outcome of a reaction-role hook: what to broadcast after commit.
pub struct HookOutcome {
    /// The member's full role set after the change.
    pub role_ids: Vec<Uuid>,
    /// Sibling emojis whose reaction rows were cleared (unique swap) — the
    /// caller broadcasts a ReactionRemove for each so other clients update.
    pub cleared_emojis: Vec<String>,
}

/// Apply reaction-role effects for an added reaction, inside `tx`.
/// Returns `None` if there is no binding for (message_id, emoji) or the user
/// is not a guild member.
pub async fn apply_on_reaction_add(
    tx: &mut Transaction<'_, Postgres>,
    guild_id: Uuid,
    message_id: Uuid,
    user_id: Uuid,
    emoji: &str,
) -> Result<Option<HookOutcome>, ReactionRoleError> {
    let Some(binding) = queries::find_binding_for_reaction(tx, message_id, emoji).await? else {
        return Ok(None);
    };
    if !queries::is_member(tx, guild_id, user_id).await? {
        return Ok(None);
    }

    queries::tx_assign_role(tx, guild_id, user_id, binding.role_id, user_id).await?;

    let mut cleared_emojis = Vec::new();
    if binding.mode == "unique" {
        if let Some(group) = binding.group_key.as_deref() {
            let siblings =
                queries::sibling_bindings_in_group(tx, message_id, group, emoji).await?;
            for (sib_emoji, sib_role) in siblings {
                queries::tx_remove_role(tx, guild_id, user_id, sib_role).await?;
                queries::tx_remove_user_reaction(tx, message_id, user_id, &sib_emoji).await?;
                cleared_emojis.push(sib_emoji);
            }
        }
    }

    let role_ids = queries::tx_member_role_ids(tx, guild_id, user_id).await?;
    Ok(Some(HookOutcome {
        role_ids,
        cleared_emojis,
    }))
}

/// Apply reaction-role effects for a removed reaction, inside `tx`.
pub async fn apply_on_reaction_remove(
    tx: &mut Transaction<'_, Postgres>,
    guild_id: Uuid,
    message_id: Uuid,
    user_id: Uuid,
    emoji: &str,
) -> Result<Option<HookOutcome>, ReactionRoleError> {
    let Some(binding) = queries::find_binding_for_reaction(tx, message_id, emoji).await? else {
        return Ok(None);
    };
    if !queries::is_member(tx, guild_id, user_id).await? {
        return Ok(None);
    }
    queries::tx_remove_role(tx, guild_id, user_id, binding.role_id).await?;
    let role_ids = queries::tx_member_role_ids(tx, guild_id, user_id).await?;
    Ok(Some(HookOutcome {
        role_ids,
        cleared_emojis: Vec::new(),
    }))
}
```

- [ ] **Step 2: Make `add_reaction` transactional and call the hook**

In `server/src/chat/reactions.rs`, replace the `add_reaction` body from the `// Insert reaction` block through the end so the insert, hook, and count run in one transaction. Full replacement of the section starting at `// Insert reaction (ignore if already exists)`:
```rust
    // Insert reaction + apply reaction-role effects atomically.
    let mut tx = state.db.begin().await?;

    let result = sqlx::query(
        r"
        INSERT INTO message_reactions (message_id, user_id, emoji)
        VALUES ($1, $2, $3)
        ON CONFLICT (message_id, user_id, emoji) DO NOTHING
        ",
    )
    .bind(message_id)
    .bind(auth_user.id)
    .bind(&req.emoji)
    .execute(&mut *tx)
    .await?;

    // Reaction-role hook (only for guild channels, only when newly inserted).
    let mut hook_outcome = None;
    if result.rows_affected() > 0 {
        let channel = db::find_channel_by_id(&state.db, channel_id)
            .await?
            .ok_or(ReactionsError::ChannelNotFound)?;
        if let Some(guild_id) = channel.guild_id {
            hook_outcome = crate::guild::reaction_roles::apply_on_reaction_add(
                &mut tx,
                guild_id,
                message_id,
                auth_user.id,
                &req.emoji,
            )
            .await
            .map_err(|e| {
                tracing::error!(error = %e, "reaction-role hook (add) failed");
                ReactionsError::Database(sqlx::Error::Protocol("reaction-role hook".into()))
            })?
            .map(|o| (guild_id, o));
        }
    }

    // Get updated count inside the same transaction.
    let count: (i64,) = sqlx::query_as(
        r"
        SELECT COUNT(*) FROM message_reactions
        WHERE message_id = $1 AND emoji = $2
        ",
    )
    .bind(message_id)
    .bind(&req.emoji)
    .fetch_one(&mut *tx)
    .await?;

    tx.commit().await?;

    // Broadcast after commit.
    if result.rows_affected() > 0 {
        if let Err(e) = broadcast_to_channel(
            &state.redis,
            channel_id,
            &ServerEvent::ReactionAdd {
                channel_id,
                message_id,
                user_id: auth_user.id,
                emoji: req.emoji.clone(),
            },
        )
        .await
        {
            tracing::warn!("Failed to broadcast reaction_add event: {}", e);
        }
    }

    if let Some((guild_id, outcome)) = hook_outcome {
        // Roles changed → members/admins update live.
        if let Err(e) = crate::ws::broadcast_to_guild(
            &state.redis,
            guild_id,
            &ServerEvent::MemberRolesUpdated {
                guild_id,
                user_id: auth_user.id,
                role_ids: outcome.role_ids,
            },
        )
        .await
        {
            tracing::warn!("Failed to broadcast member_roles_updated: {}", e);
        }
        // Cleared sibling reactions (unique swap) → tell clients to drop them.
        for cleared in outcome.cleared_emojis {
            if let Err(e) = broadcast_to_channel(
                &state.redis,
                channel_id,
                &ServerEvent::ReactionRemove {
                    channel_id,
                    message_id,
                    user_id: auth_user.id,
                    emoji: cleared,
                },
            )
            .await
            {
                tracing::warn!("Failed to broadcast cleared reaction: {}", e);
            }
        }
    }

    let status = if result.rows_affected() > 0 {
        StatusCode::CREATED
    } else {
        StatusCode::OK
    };

    Ok((
        status,
        Json(ReactionResponse {
            emoji: req.emoji,
            count: count.0,
            me: true,
        }),
    ))
```

Note: the existing `add_reaction` fetches the channel earlier for the existence check; keep that earlier fetch too (it validates the channel before the transaction). The in-transaction re-fetch above is only reached on a fresh insert.

- [ ] **Step 3: Make `remove_reaction` transactional and call the hook**

In `server/src/chat/reactions.rs`, replace the DELETE-through-broadcast section of `remove_reaction`:
```rust
    let mut tx = state.db.begin().await?;
    let result = sqlx::query(
        r"
        DELETE FROM message_reactions
        WHERE message_id = $1 AND user_id = $2 AND emoji = $3
        ",
    )
    .bind(message_id)
    .bind(auth_user.id)
    .bind(&emoji)
    .execute(&mut *tx)
    .await?;

    let mut hook_outcome = None;
    if result.rows_affected() > 0 {
        if let Some(guild_id) = db::find_channel_by_id(&state.db, channel_id)
            .await?
            .and_then(|c| c.guild_id)
        {
            hook_outcome = crate::guild::reaction_roles::apply_on_reaction_remove(
                &mut tx,
                guild_id,
                message_id,
                auth_user.id,
                &emoji,
            )
            .await
            .map_err(|e| {
                tracing::error!(error = %e, "reaction-role hook (remove) failed");
                ReactionsError::Database(sqlx::Error::Protocol("reaction-role hook".into()))
            })?
            .map(|o| (guild_id, o));
        }
    }

    tx.commit().await?;

    if result.rows_affected() > 0 {
        if let Err(e) = broadcast_to_channel(
            &state.redis,
            channel_id,
            &ServerEvent::ReactionRemove {
                channel_id,
                message_id,
                user_id: auth_user.id,
                emoji: emoji.clone(),
            },
        )
        .await
        {
            tracing::warn!("Failed to broadcast reaction_remove event: {}", e);
        }
    }

    if let Some((guild_id, outcome)) = hook_outcome {
        if let Err(e) = crate::ws::broadcast_to_guild(
            &state.redis,
            guild_id,
            &ServerEvent::MemberRolesUpdated {
                guild_id,
                user_id: auth_user.id,
                role_ids: outcome.role_ids,
            },
        )
        .await
        {
            tracing::warn!("Failed to broadcast member_roles_updated: {}", e);
        }
    }

    Ok(StatusCode::NO_CONTENT)
```

- [ ] **Step 4: Write the reaction-hook integration tests**

Append to `server/tests/integration/reaction_roles.rs`:
```rust
/// Assert a member has (or lacks) a role.
async fn has_role(pool: &PgPool, guild_id: Uuid, user_id: Uuid, role_id: Uuid) -> bool {
    let row: Option<(i32,)> = sqlx::query_as(
        "SELECT 1 FROM guild_member_roles WHERE guild_id = $1 AND user_id = $2 AND role_id = $3",
    )
    .bind(guild_id)
    .bind(user_id)
    .bind(role_id)
    .fetch_optional(pool)
    .await
    .unwrap();
    row.is_some()
}

async fn insert_binding_row(
    pool: &PgPool,
    guild_id: Uuid,
    channel_id: Uuid,
    message_id: Uuid,
    emoji: &str,
    role_id: Uuid,
    mode: &str,
    group_key: Option<&str>,
) {
    sqlx::query(
        "INSERT INTO reaction_role_bindings
             (guild_id, channel_id, message_id, emoji, role_id, mode, group_key)
         VALUES ($1, $2, $3, $4, $5, $6, $7)",
    )
    .bind(guild_id)
    .bind(channel_id)
    .bind(message_id)
    .bind(emoji)
    .bind(role_id)
    .bind(mode)
    .bind(group_key)
    .execute(pool)
    .await
    .expect("insert binding");
}

#[sqlx::test]
async fn toggle_reaction_grants_and_revokes_role(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (owner, _) = create_test_user(&pool).await;
    // @everyone needs ADD_REACTIONS so the member may react.
    let guild = create_guild_with_default_role(&pool, owner, GuildPermissions::ADD_REACTIONS | GuildPermissions::VIEW_CHANNEL).await;
    let (member, _) = create_test_user(&pool).await;
    add_guild_member(&pool, guild, member).await;
    let channel = create_channel(&pool, guild, "general").await;
    let msg = insert_message(&pool, channel, owner, "react").await;
    let role = insert_role(&pool, guild, GuildPermissions::SEND_MESSAGES, 5).await;
    insert_binding_row(&pool, guild, channel, msg, "🎨", role, "toggle", None).await;

    let token = vc_server_token(&app, member);

    // React → role granted.
    let put = TestApp::request(
        Method::PUT,
        &format!("/api/channels/{channel}/messages/{msg}/reactions"),
    )
    .header("Authorization", format!("Bearer {token}"))
    .header("Content-Type", "application/json")
    .body(Body::from(json!({ "emoji": "🎨" }).to_string()))
    .unwrap();
    assert_eq!(app.oneshot(put).await.status(), StatusCode::CREATED);
    assert!(has_role(&pool, guild, member, role).await, "role granted on react");

    // Un-react → role revoked.
    let del = TestApp::request(
        Method::DELETE,
        &format!("/api/channels/{channel}/messages/{msg}/reactions/🎨"),
    )
    .header("Authorization", format!("Bearer {token}"))
    .body(Body::empty())
    .unwrap();
    assert_eq!(app.oneshot(del).await.status(), StatusCode::NO_CONTENT);
    assert!(!has_role(&pool, guild, member, role).await, "role revoked on un-react");
}

#[sqlx::test]
async fn unique_group_swaps_roles(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (owner, _) = create_test_user(&pool).await;
    let guild = create_guild_with_default_role(&pool, owner, GuildPermissions::ADD_REACTIONS | GuildPermissions::VIEW_CHANNEL).await;
    let (member, _) = create_test_user(&pool).await;
    add_guild_member(&pool, guild, member).await;
    let channel = create_channel(&pool, guild, "general").await;
    let msg = insert_message(&pool, channel, owner, "pick a color").await;
    let red = insert_role(&pool, guild, GuildPermissions::empty(), 5).await;
    let blue = insert_role(&pool, guild, GuildPermissions::empty(), 6).await;
    insert_binding_row(&pool, guild, channel, msg, "🔴", red, "unique", Some("color")).await;
    insert_binding_row(&pool, guild, channel, msg, "🔵", blue, "unique", Some("color")).await;

    let token = vc_server_token(&app, member);
    let react = |emoji: &str| {
        TestApp::request(
            Method::PUT,
            &format!("/api/channels/{channel}/messages/{msg}/reactions"),
        )
        .header("Authorization", format!("Bearer {token}"))
        .header("Content-Type", "application/json")
        .body(Body::from(json!({ "emoji": emoji }).to_string()))
        .unwrap()
    };

    app.oneshot(react("🔴")).await;
    assert!(has_role(&pool, guild, member, red).await);

    app.oneshot(react("🔵")).await;
    assert!(has_role(&pool, guild, member, blue).await, "new group role granted");
    assert!(!has_role(&pool, guild, member, red).await, "old group role revoked");

    // The losing reaction row was cleared.
    let red_react: Option<(i32,)> = sqlx::query_as(
        "SELECT 1 FROM message_reactions WHERE message_id = $1 AND user_id = $2 AND emoji = '🔴'",
    )
    .bind(msg)
    .bind(member)
    .fetch_optional(&pool)
    .await
    .unwrap();
    assert!(red_react.is_none(), "losing reaction cleared");
}

#[sqlx::test]
async fn non_member_reaction_does_not_grant(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (owner, _) = create_test_user(&pool).await;
    let guild = create_guild_with_default_role(&pool, owner, GuildPermissions::ADD_REACTIONS | GuildPermissions::VIEW_CHANNEL).await;
    let channel = create_channel(&pool, guild, "general").await;
    let msg = insert_message(&pool, channel, owner, "react").await;
    let role = insert_role(&pool, guild, GuildPermissions::empty(), 5).await;
    insert_binding_row(&pool, guild, channel, msg, "🎨", role, "toggle", None).await;

    // A user who is NOT a guild member but can still hit the endpoint check:
    // the hook must skip role grant. (VIEW_CHANNEL for @everyone lets non-guild
    // reads through in some setups; the hook's is_member guard is the backstop.)
    let (outsider, _) = create_test_user(&pool).await;
    let token = vc_server_token(&app, outsider);
    let put = TestApp::request(
        Method::PUT,
        &format!("/api/channels/{channel}/messages/{msg}/reactions"),
    )
    .header("Authorization", format!("Bearer {token}"))
    .header("Content-Type", "application/json")
    .body(Body::from(json!({ "emoji": "🎨" }).to_string()))
    .unwrap();
    let _ = app.oneshot(put).await;
    assert!(!has_role(&pool, guild, outsider, role).await, "non-member never granted");
}
```

- [ ] **Step 5: Run all reaction-role tests**

Run:
```bash
SQLX_OFFLINE=true DATABASE_URL="postgresql://voicechat:voicechat_dev@localhost:5433/voicechat" \
  cargo test -p vc-server --test integration reaction_roles -- --nocapture
```
Expected: all six tests pass. If `non_member_reaction_does_not_grant` fails at the HTTP layer (403 before the hook), assert on `has_role` only — the guarantee under test is "no grant", regardless of HTTP status.

- [ ] **Step 6: Regenerate offline cache and commit**

```bash
cargo sqlx prepare --workspace -- --tests
git add server/src/guild/reaction_roles.rs server/src/chat/reactions.rs server/tests/integration/reaction_roles.rs .sqlx/
git commit -m "feat(chat): grant reaction-role on react (toggle + unique)"
```

---

## Task 8: Retrofit admin assign/remove to emit MemberRolesUpdated

Closes the latent gap: admin role changes were silent over WS.

**Files:**
- Modify: `server/src/guild/roles.rs`

- [ ] **Step 1: Add a helper to fetch a member's role IDs (pool variant)**

In `server/src/guild/queries/roles.rs`, append:
```rust
/// Fetch a member's full role-ID set (for MemberRolesUpdated broadcasts).
pub async fn member_role_ids(
    pool: &PgPool,
    guild_id: Uuid,
    user_id: Uuid,
) -> Result<Vec<Uuid>, RoleError> {
    let rows: Vec<(Uuid,)> = sqlx::query_as(
        "SELECT role_id FROM guild_member_roles WHERE guild_id = $1 AND user_id = $2",
    )
    .bind(guild_id)
    .bind(user_id)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(|(r,)| r).collect())
}
```

- [ ] **Step 2: Emit the event from `assign_role`**

In `server/src/guild/roles.rs` `assign_role`, after the successful `queries::assign_role_to_member(...)` call and before returning `Ok(Json(...))`, add:
```rust
    let role_ids = queries::member_role_ids(&state.db, guild_id, user_id).await?;
    if let Err(e) = crate::ws::broadcast_to_guild(
        &state.redis,
        guild_id,
        &crate::ws::ServerEvent::MemberRolesUpdated {
            guild_id,
            user_id,
            role_ids,
        },
    )
    .await
    {
        tracing::warn!("Failed to broadcast member_roles_updated (assign): {}", e);
    }
```

- [ ] **Step 3: Emit the event from `remove_role`**

In `remove_role`, after the `rows_affected == 0` check (i.e., when a row was actually removed) and before `Ok(Json(...))`, add the same block (identical code — repeated intentionally so the handler reads top-to-bottom):
```rust
    let role_ids = queries::member_role_ids(&state.db, guild_id, user_id).await?;
    if let Err(e) = crate::ws::broadcast_to_guild(
        &state.redis,
        guild_id,
        &crate::ws::ServerEvent::MemberRolesUpdated {
            guild_id,
            user_id,
            role_ids,
        },
    )
    .await
    {
        tracing::warn!("Failed to broadcast member_roles_updated (remove): {}", e);
    }
```

- [ ] **Step 4: Verify `state.redis` and `ServerEvent` are in scope**

`assign_role`/`remove_role` already take `State(state)`. Confirm `crate::ws::ServerEvent` is the correct path (Task 6 exposes it). Run:
```bash
SQLX_OFFLINE=true cargo check -p vc-server
```
Expected: compiles.

- [ ] **Step 5: Add a regression test that admin-assign emits (via role_ids side effect)**

Since WS broadcast requires Redis, assert the DB effect is correct (role assigned) which already has coverage; add a lightweight test that `member_role_ids` returns the assigned role. Append to `server/tests/integration/reaction_roles.rs`:
```rust
#[sqlx::test]
async fn admin_assign_role_persists_and_is_listable(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (owner, _) = create_test_user(&pool).await;
    let guild = create_guild_with_default_role(&pool, owner, GuildPermissions::empty()).await;
    let (member, _) = create_test_user(&pool).await;
    add_guild_member(&pool, guild, member).await;
    let role = insert_role(&pool, guild, GuildPermissions::SEND_MESSAGES, 5).await;

    let token = vc_server_token(&app, owner);
    let req = TestApp::request(
        Method::POST,
        &format!("/api/guilds/{guild}/members/{member}/roles/{role}"),
    )
    .header("Authorization", format!("Bearer {token}"))
    .body(Body::empty())
    .unwrap();
    assert_eq!(app.oneshot(req).await.status(), StatusCode::OK);
    assert!(has_role(&pool, guild, member, role).await);
}
```

- [ ] **Step 6: Run + commit**

```bash
SQLX_OFFLINE=true DATABASE_URL="postgresql://voicechat:voicechat_dev@localhost:5433/voicechat" \
  cargo test -p vc-server --test integration reaction_roles -- --nocapture
cargo sqlx prepare --workspace -- --tests
git add server/src/guild/ server/tests/integration/reaction_roles.rs .sqlx/
git commit -m "feat(guild): emit MemberRolesUpdated on admin role assign/remove"
```

---

## Task 9: Client — event type, dispatch, members reducer + test

**Files:**
- Modify: `client/src/lib/types/events.ts`
- Modify: `client/src/stores/members.ts`
- Modify: `client/src/stores/websocket/index.ts`
- Create: `client/src/stores/__tests__/memberRoles.test.ts`

- [ ] **Step 1: Extend the client member type with role_ids**

In `client/src/lib/types/guild.ts`, add an optional field to `GuildMember` (the member list only carries it once a MemberRolesUpdated arrives or the list is fetched with roles):
```typescript
export interface GuildMember {
  user_id: string;
  username: string;
  display_name: string;
  avatar_url: string | null;
  nickname: string | null;
  joined_at: string;
  status: "online" | "idle" | "offline";
  last_seen_at: string | null;
  role_ids?: string[];
}
```

- [ ] **Step 2: Add the event to the union**

In `client/src/lib/types/events.ts`, add to the `ServerEvent` union (near the `guild_emoji_updated` entry):
```typescript
  | {
      type: "member_roles_updated";
      guild_id: string;
      user_id: string;
      role_ids: string[];
    }
```

- [ ] **Step 3: Write the reducer test first (TDD)**

Create `client/src/stores/__tests__/memberRoles.test.ts`:
```typescript
import { describe, it, expect, beforeEach } from "vitest";
import { applyMemberRoles, getMember } from "@/stores/members";
import { guildsState, setGuildsState } from "@/stores/guilds";

const GUILD = "11111111-1111-1111-1111-111111111111";
const USER = "22222222-2222-2222-2222-222222222222";

describe("applyMemberRoles", () => {
  beforeEach(() => {
    setGuildsState("members", GUILD, [
      {
        user_id: USER,
        username: "alice",
        display_name: "Alice",
        avatar_url: null,
        nickname: null,
        joined_at: "2026-01-01T00:00:00Z",
        status: "online",
        last_seen_at: null,
      },
    ]);
  });

  it("updates the cached member's role_ids", () => {
    applyMemberRoles(GUILD, USER, ["role-a", "role-b"]);
    expect(getMember(GUILD, USER)?.role_ids).toEqual(["role-a", "role-b"]);
  });

  it("ignores unknown guilds without throwing", () => {
    expect(() => applyMemberRoles("no-such-guild", USER, ["x"])).not.toThrow();
  });

  it("ignores unknown members without throwing", () => {
    applyMemberRoles(GUILD, "no-such-user", ["x"]);
    expect(guildsState.members[GUILD][0].role_ids).toBeUndefined();
  });
});
```

- [ ] **Step 4: Run the test — verify it fails (no `applyMemberRoles`)**

Run:
```bash
cd client && bun run test:run -- memberRoles
```
Expected: FAIL — `applyMemberRoles` is not exported.

- [ ] **Step 5: Implement the reducer**

In `client/src/stores/members.ts`, add:
```typescript
/**
 * Replace a cached member's full role set (from a MemberRolesUpdated event).
 * Idempotent: the server always sends the complete set.
 */
export function applyMemberRoles(
  guildId: string,
  userId: string,
  roleIds: string[],
): void {
  const members = guildsState.members[guildId];
  if (!members) return;
  const memberIndex = members.findIndex((m) => m.user_id === userId);
  if (memberIndex === -1) return;
  setGuildsState("members", guildId, memberIndex, (prev) => ({
    ...prev,
    role_ids: roleIds,
  }));
}
```

- [ ] **Step 6: Run the test — verify it passes**

Run:
```bash
cd client && bun run test:run -- memberRoles
```
Expected: PASS (3 tests).

- [ ] **Step 7: Wire dispatch in the websocket store**

In `client/src/stores/websocket/index.ts`:

(a) Add a case to the event `switch` (near `case "guild_emoji_updated"` — match the exact surrounding style):
```typescript
    case "member_roles_updated": {
      const { applyMemberRoles } = await import("@/stores/members");
      applyMemberRoles(event.guild_id, event.user_id, event.role_ids);
      break;
    }
```

(b) Add the Tauri-listen path (near the `ws:guild_emoji_updated` listener at ~line 532):
```typescript
      listen<{ guild_id: string; user_id: string; role_ids: string[] }>(
        "ws:member_roles_updated",
        (event) => {
          import("@/stores/members").then(({ applyMemberRoles }) =>
            applyMemberRoles(
              event.payload.guild_id,
              event.payload.user_id,
              event.payload.role_ids,
            ),
          );
        },
      );
```
(Match how the surrounding listeners are registered — if they are inside an array of unlisten handles, push this one the same way.)

- [ ] **Step 8: Typecheck + full client test run**

Run:
```bash
cd client && bun run build && bun run test:run -- memberRoles
```
Expected: build succeeds (no TS errors), tests pass.

- [ ] **Step 9: Commit**

```bash
git add client/src/lib/types/ client/src/stores/members.ts client/src/stores/websocket/index.ts client/src/stores/__tests__/memberRoles.test.ts
git commit -m "feat(client): handle MemberRolesUpdated (live role chips)"
```

---

## Task 10: Client — Reaction Roles admin section

Add a "Reaction Roles" management section to the guild settings modal: list existing bindings, create a binding (channel + message id + emoji + role + mode + optional group), delete a binding. Only roles the actor can manage and that pass the self-assignable check are offered (the server enforces this; the UI filters for UX).

**Files:**
- Locate and modify the guild settings modal (find with the command below).

- [ ] **Step 1: Locate the guild settings modal and an existing roles section to mirror**

Run:
```bash
rg -l "GuildSettings" client/src/components | head
rg -l "roles" client/src/components/**/GuildSettings* 2>/dev/null | head
```
Identify the modal component and the API client module used for guild role calls (e.g. `client/src/lib/api/*` or a `tauri.ts` wrapper).

- [ ] **Step 2: Add API client functions**

In the client's guild API module (same file that has `getGuildRoles`/`createRole`), add:
```typescript
export interface ReactionRole {
  id: string;
  guild_id: string;
  channel_id: string;
  message_id: string;
  emoji: string;
  role_id: string;
  group_key: string | null;
  mode: string;
  created_at: string;
}

export async function listReactionRoles(guildId: string): Promise<ReactionRole[]> {
  return apiGet(`/api/guilds/${guildId}/reaction-roles`);
}

export async function createReactionRole(
  guildId: string,
  body: {
    channel_id: string;
    message_id: string;
    emoji: string;
    role_id: string;
    mode: string;
    group_key?: string;
  },
): Promise<ReactionRole> {
  return apiPost(`/api/guilds/${guildId}/reaction-roles`, body);
}

export async function deleteReactionRole(
  guildId: string,
  bindingId: string,
): Promise<void> {
  return apiDelete(`/api/guilds/${guildId}/reaction-roles/${bindingId}`);
}
```
Use whatever the existing `apiGet`/`apiPost`/`apiDelete` (or `invoke`) primitives are named in that module — mirror the sibling role functions exactly.

- [ ] **Step 3: Build the section component**

Create a `ReactionRolesSection` Solid component beside the other guild-settings sections. It:
- Calls `listReactionRoles(guildId)` in a `createResource` on mount.
- Renders each binding as a row: emoji, role name (resolve via the roles list already loaded in the modal), mode, group, and a delete button calling `deleteReactionRole` then refetch.
- Provides a create form: a channel `<select>` (guild channels), a message-id text input (with helper text "paste a message ID"), the existing emoji picker, a role `<select>` filtered to non-default roles the actor can manage, a mode `<select>` (`toggle`/`unique`), and an optional group-key text input (shown when mode is `unique`). On submit, calls `createReactionRole` then refetch; surface server error codes (`ROLE_NOT_SELF_ASSIGNABLE`, `DUPLICATE_BINDING`, `ROLE_HIERARCHY`) as friendly messages.

Follow the exact styling/utility-class conventions of the neighbouring section (respect the CLAUDE.md UI contrast rules — `text-text-primary` for labels, `bg-accent-primary/20` for selected states; never `text-accent-primary` on an accent background).

- [ ] **Step 4: Mount the section in the modal**

Add the `ReactionRolesSection` to the modal's section list/nav under the "Roles" area, gated on the viewer having `MANAGE_ROLES` (mirror how the existing Roles tab is gated).

- [ ] **Step 5: Build + smoke the client**

Run:
```bash
cd client && bun run build
```
Expected: builds with no TS errors. (Live UI verification happens in the deploy/functionality-test phase.)

- [ ] **Step 6: Commit**

```bash
git add client/src
git commit -m "feat(client): reaction-roles admin section in guild settings"
```

---

## Task 11: Changelog, full gate, and PR

**Files:**
- Modify: `CHANGELOG.md`

- [ ] **Step 1: Add a changelog entry**

Under `[Unreleased] → ### Added` in `CHANGELOG.md`:
```markdown
- **Reaction roles**: admins can bind an emoji on a message to a role, and members grant/revoke that role themselves by reacting. Supports `toggle` and pick-one `unique` groups. Role changes now broadcast live over WebSocket.
```

- [ ] **Step 2: Run the full pre-push gate**

Run:
```bash
cargo fmt --all
SQLX_OFFLINE=true cargo clippy -- -D warnings
SQLX_OFFLINE=true DATABASE_URL="postgresql://voicechat:voicechat_dev@localhost:5433/voicechat" \
  cargo test -p vc-server --test integration reaction_roles
cd client && bun run test:run && bun run build
```
Expected: fmt clean, clippy clean, all tests pass, client builds.

- [ ] **Step 3: Commit + push the feature branch + open PR**

```bash
git add CHANGELOG.md
git commit -m "docs(changelog): reaction roles"
git push -u origin feature/reaction-roles
gh pr create --title "feat: reaction roles" --body "<summary + test plan>"
```

---

## Self-Review (completed during planning)

- **Spec coverage:** data model (Task 1), permissions/safety hierarchy + dangerous guard (Tasks 2, 5), reaction-time toggle/unique behaviour + single-transaction swap (Task 7), WS event + admin retrofit (Tasks 6, 8), API surface GET/POST/DELETE (Task 5), client event handling + admin UI (Tasks 9, 10), edge cases: non-member skip (Task 7 test), cascades (covered by FK `ON DELETE CASCADE` in Task 1 — no code), duplicate emoji (Task 5 `DuplicateBinding`). All spec sections map to a task.
- **Placeholder scan:** no TBD/TODO; every code step has concrete code. Task 10 is deliberately described at component level (UI, following existing modal patterns) rather than pinned to exact class names, because the target file must be located first (Step 1) — this is a locate-then-mirror instruction, not a placeholder.
- **Type consistency:** `ReactionRoleError`, `ReactionRoleResponse`, `HookOutcome`, `HookBinding` are defined once and referenced consistently; `applyMemberRoles(guildId, userId, roleIds)` signature matches between test (Task 9 Step 3), implementation (Step 5), and dispatch (Step 7); `MemberRolesUpdated { guild_id, user_id, role_ids }` matches across server variant (Task 6), emit sites (Tasks 7, 8), and client union (Task 9).
