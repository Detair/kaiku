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

**Defined in `guild.rs`** (bitflags pattern):
```rust
bitflags! {
    pub struct GuildPermissions: u64 {
        const ADMINISTRATOR      = 1 << 0;   // 0x1 — Bypass all checks
        const MANAGE_GUILD       = 1 << 1;   // 0x2 — Edit guild settings
        const KICK_MEMBERS       = 1 << 2;   // 0x4 — Remove members
        const BAN_MEMBERS        = 1 << 3;   // 0x8 — Ban members
        const CREATE_INVITE      = 1 << 4;   // 0x10 — Create invite codes
        const MANAGE_CHANNELS    = 1 << 5;   // 0x20 — Create/edit/delete channels
        const MANAGE_ROLES       = 1 << 6;   // 0x40 — Edit roles below own highest role
        const VIEW_CHANNELS      = 1 << 7;   // 0x80 — See channels (base permission)
        const SEND_MESSAGES      = 1 << 8;   // 0x100 — Send messages in text channels
        const MANAGE_MESSAGES    = 1 << 9;   // 0x200 — Delete others' messages, pin
        const CONNECT            = 1 << 10;  // 0x400 — Join voice channels
        const SPEAK              = 1 << 11;  // 0x800 — Transmit audio in voice
        const MUTE_MEMBERS       = 1 << 12;  // 0x1000 — Server-mute others in voice
        const DEAFEN_MEMBERS     = 1 << 13;  // 0x2000 — Server-deafen others
        const MOVE_MEMBERS       = 1 << 14;  // 0x4000 — Move users between voice channels
    }
}
```

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

**Computing Effective Permissions**:
```rust
pub async fn compute_guild_permissions(
    pool: &PgPool,
    guild_id: Uuid,
    user_id: Uuid,
) -> Result<GuildPermissions, PermissionError> {
    // 1. Check if user is guild owner → return ADMINISTRATOR
    // 2. Query all roles assigned to user in this guild
    // 3. Bitwise OR all `permissions_bitfield` values
    // 4. If ADMINISTRATOR flag set, return all permissions
    // 5. Return computed bitfield
}
```

**ADMINISTRATOR Bypass**: Users with `ADMINISTRATOR` permission skip all other checks (god mode).

**Channel Overrides** (`channel_overrides`: channel_id, role_id, allow/deny bits; no per-user overrides):
- Resolved only in `compute_guild_permissions` (resolver.rs); all channel checks reach it via `require_channel_access`, `filter_accessible_channels` or `filter_viewable_channels_in_guilds`.
- Order: owner → base (@everyone | assigned roles) → @everyone override (allow masked with `EVERYONE_ALLOWED`, then deny) → role overrides aggregated (OR of denies applied, then OR of allows).
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
use crate::permissions::{compute_guild_permissions, GuildPermissions, PermissionError};

let perms = compute_guild_permissions(&state.db, guild_id, current_user.id).await?;

if !perms.contains(GuildPermissions::KICK_MEMBERS) {
    return Err(PermissionError::InsufficientPermissions);
}

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
- `permissions_bitfield: ADMINISTRATOR`
- Auto-assigned to guild creator

### Testing

**Required Tests**:
- [ ] Compute permissions for user with single role
- [ ] Compute permissions for user with multiple roles (bitwise OR)
- [ ] ADMINISTRATOR bypasses all checks
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
