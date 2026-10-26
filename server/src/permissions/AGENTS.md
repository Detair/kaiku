<!-- Parent: ../../AGENTS.md -->

# Permissions Module

**SECURITY CRITICAL** — Two-tier permission system: platform-level system permissions and guild-level role-based access control (RBAC).

## Purpose

- Define system-level permissions for admin actions
- Define guild-level permission flags for RBAC
- Compute effective permissions for users in guilds
- Role hierarchy and moderation checks
- Permission overrides at channel level (`channel_overrides`, per role)

## Key Files

- `mod.rs` — Re-exports for all permission types and utilities
- `system.rs` — System-level permissions (ADMIN, MANAGE_USERS, etc.)
- `guild.rs` — Guild permission bitflags (MANAGE_GUILD, KICK_MEMBERS, etc.)
- `models.rs` — Role and permission database models
- `queries.rs` — Role CRUD and permission assignment queries
- `resolver.rs` — Permission computation logic (`compute_guild_permissions`, `can_moderate_member`)

## For AI Agents

**SECURITY CRITICAL MODULE**: All permission checks must be server-side. Client-side UI can hide buttons, but server must always validate permissions. Never trust user input claiming permissions.

### System Permissions

**Purpose**: Platform-wide admin actions (not guild-specific).

**Defined in `system.rs`**:
```rust
pub enum SystemPermission {
    Admin,           // Full platform access (manage all guilds, users, settings)
    ManageUsers,     // Ban/unban users globally, view user details
    ManageGuilds,    // Delete any guild, view guild analytics
    ViewAuditLogs,   // Read audit logs for compliance
}
```

**Storage**: `users.system_permissions` (JSON array or bitfield, not yet implemented).

**Usage**:
```rust
if !current_user.has_system_permission(SystemPermission::Admin) {
    return Err(AuthError::Forbidden);
}
```

**Future Implementation**: Add `system_permissions_bitfield` column to `users` table.

### Guild Permissions

**Purpose**: Per-guild role-based access control.

**Defined in `guild.rs`** (bitflags `GuildPermissions: u64`, stored as `i64` via `to_db`/`from_db`). The bit list lives there (e.g. `VIEW_CHANNEL = 1 << 24`); do not duplicate it here. There is no ADMINISTRATOR flag: the guild owner is the only bypass. `EVERYONE_FORBIDDEN` / `EVERYONE_ALLOWED` partition the bits the @everyone role (and its channel overrides) may hold.

**Storage**: `roles.permissions_bitfield` (PostgreSQL `BIGINT` for u64 bitfield).

**Default Role**: "@everyone" role created on guild creation with:
```rust
VIEW_CHANNELS | SEND_MESSAGES | CONNECT | SPEAK
```

### Role Model

**Database Schema** (in `models.rs`):
```rust
pub struct Role {
    pub id: Uuid,
    pub guild_id: Uuid,
    pub name: String,
    pub permissions_bitfield: i64,  // Stored as signed, cast to u64 for bitflags
    pub position: i32,              // Higher = more powerful (for hierarchy)
    pub color: Option<i32>,         // RGB color (0xRRGGBB)
    pub hoist: bool,                // Display separately in member list
    pub mentionable: bool,          // Can be @mentioned
    pub created_at: DateTime,
}
```

**Position Field**: Determines role hierarchy. Owner role always highest position (e.g., 100). "@everyone" always position 0.

**Assignment**: Users can have multiple roles. Effective permissions = OR of all role bitfields.

### Permission Resolution

**Computing Effective Permissions** (`resolver.rs`, pure function, no DB access):
```rust
pub fn compute_guild_permissions(
    user_id: Uuid,
    guild_owner_id: Uuid,
    pending: bool, // screening not accepted: no permissions, overrides ignored
    everyone_permissions: GuildPermissions,
    user_roles: &[GuildRole],
    channel_overrides: Option<&[ChannelOverride]>,
) -> GuildPermissions
```
Handlers normally do not call it directly; they use `get_member_permission_context` / `require_guild_permission` (guild level) or `require_channel_access` / `filter_accessible_channels` / `filter_viewable_channels_in_guilds` (channel level), which load the rows and call it.

**Owner bypass**: the guild owner gets `GuildPermissions::all()`. There is no other bypass.

**Channel Overrides** (`channel_overrides`: channel_id, role_id, allow/deny bits; no per-user overrides; the loaders set `ChannelOverride::everyone` from the role's `is_default`):
- Resolved only in `compute_guild_permissions` (resolver.rs); all channel checks reach it via `require_channel_access`, `filter_accessible_channels` or `filter_viewable_channels_in_guilds`.
- Order: owner → pending member (empty, before any override) → base (@everyone | assigned roles) → @everyone override (allow masked with `EVERYONE_ALLOWED`, then deny) → role overrides aggregated (OR of denies applied, then OR of allows).
- A role allow beats an @everyone deny and another role's deny of the same bit; a role deny beats an @everyone allow.

### Moderation Checks

**Role Hierarchy** (`can_moderate_member`):
```rust
pub async fn can_moderate_member(
    pool: &PgPool,
    guild_id: Uuid,
    moderator_id: Uuid,
    target_id: Uuid,
) -> Result<bool, PermissionError> {
    // 1. Cannot moderate yourself
    // 2. Cannot moderate guild owner (unless you are owner)
    // 3. Get highest role position for both users
    // 4. Moderator's highest role must be > target's highest role
    // 5. Return true if hierarchy satisfied
}
```

**Use Cases**:
- Kicking members: Check `KICK_MEMBERS` permission + `can_moderate_member`
- Editing roles: Check `MANAGE_ROLES` permission + target role position < moderator's highest role
- Banning: Check `BAN_MEMBERS` permission + `can_moderate_member`

**Role Editing Hierarchy**:
```rust
pub async fn can_manage_role(
    pool: &PgPool,
    guild_id: Uuid,
    user_id: Uuid,
    target_role_id: Uuid,
) -> Result<bool, PermissionError> {
    // 1. User needs MANAGE_ROLES permission
    // 2. User's highest role position > target role position
    // 3. Cannot edit @everyone role (position 0, special case)
}
```

### Permission Checking Pattern

**In Handlers**:
```rust
use crate::permissions::{require_guild_permission, GuildPermissions};

let ctx = require_guild_permission(&state.db, guild_id, current_user.id, GuildPermissions::KICK_MEMBERS)
    .await?;

if !can_moderate_member(&state.db, guild_id, current_user.id, target_user_id).await? {
    return Err(PermissionError::CannotModerateTarget);
}
```

**Error Conversion**:
```rust
impl From<PermissionError> for (StatusCode, Json<ErrorResponse>) {
    fn from(err: PermissionError) -> Self {
        (StatusCode::FORBIDDEN, Json(ErrorResponse { error: err.to_string() }))
    }
}
```

### Role Management Queries

**Creating Roles** (`queries.rs`):
```rust
pub async fn create_role(
    pool: &PgPool,
    guild_id: Uuid,
    name: &str,
    permissions_bitfield: i64,
    position: i32,
) -> Result<Role, sqlx::Error>
```

**Assigning Roles**:
```rust
pub async fn assign_role_to_member(
    pool: &PgPool,
    guild_id: Uuid,
    user_id: Uuid,
    role_id: Uuid,
) -> Result<(), sqlx::Error>
```

**Listing User Roles**:
```rust
pub async fn get_user_roles(
    pool: &PgPool,
    guild_id: Uuid,
    user_id: Uuid,
) -> Result<Vec<Role>, sqlx::Error>
```

### Default Roles

**@everyone**:
- Created automatically with every guild
- `position: 0`
- `permissions_bitfield: VIEW_CHANNELS | SEND_MESSAGES | CONNECT | SPEAK`
- Cannot be deleted or have position changed

**Owner Role** (future):
- Separate from guild owner flag
- `position: 100` (highest)
- `permissions_bitfield: GuildPermissions::all()`
- Auto-assigned to guild creator

### Testing

**Required Tests**:
- [ ] Compute permissions for user with single role
- [ ] Compute permissions for user with multiple roles (bitwise OR)
- [x] Guild owner bypasses all checks
- [ ] Role hierarchy: higher position can moderate lower position
- [ ] Role hierarchy: equal positions cannot moderate each other
- [ ] Cannot moderate guild owner (unless you are owner)
- [ ] Permission check fails for user without required permission
- [x] Channel overrides: @everyone override, then aggregated role overrides (role allow wins)

### Common Security Mistakes

**DO NOT**:
- Check permissions client-side only (server must always validate)
- Use `contains()` check without computing permissions first
- Skip role hierarchy check for moderation actions (security bug)
- Allow users to assign roles higher than their own position
- Trust permission bitfield from client input (always compute server-side)

**DO**:
- Always call `compute_guild_permissions()` for permission checks
- Cache computed permissions per request (not across requests)
- Use `can_moderate_member()` for all moderation actions
- Verify user is guild member before computing permissions
- Log permission denials for audit trail (future)

### Future Enhancements

**Per-user channel overrides**: not implemented; if added, apply them after the role overrides (deny, then allow) inside `compute_guild_permissions`.

**Audit Logs**:
- Track permission changes (role created, permissions modified, role assigned/removed)
- Store in `audit_logs` table with `action_type`, `actor_id`, `target_id`, `changes_json`

**Permission Templates**:
- Predefined role sets (Admin, Moderator, Member, Guest)
- Quick setup for new guilds
