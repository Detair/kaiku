# Permission System Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Implement a two-tier permission system with System Admin (platform) and Guild roles (per-guild), including sudo-style elevation, privilege escalation prevention, and hardened break-glass emergency procedures.

**Architecture:** Separate system permissions from guild permissions. System admins require MFA elevation for privileged actions. Guild permissions use hierarchical inheritance with channel-level overrides. All sensitive actions logged to audit.

**Tech Stack:** Rust/Axum (backend), PostgreSQL (database), sqlx (queries), bitflags (permissions), tower (middleware)

---

## Phase 1: Database Foundation

### Task 1: Permission System Migration

**Files:**
- Create: `server/migrations/20260113000000_permission_system.sql`

**Step 1: Write the migration file**

```sql
-- Permission System Migration
-- Implements two-tier permission model: System Admin + Guild Roles

-- ============================================================================
-- System Admin Tables
-- ============================================================================

-- System-level admin users
CREATE TABLE system_admins (
    user_id UUID PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    granted_by UUID REFERENCES users(id),
    granted_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Elevated admin sessions (sudo-style)
CREATE TABLE elevated_sessions (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    session_id UUID NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    ip_address INET NOT NULL,
    elevated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    expires_at TIMESTAMPTZ NOT NULL,
    reason VARCHAR(255),
    UNIQUE(session_id)
);

CREATE INDEX idx_elevated_sessions_user ON elevated_sessions(user_id);
CREATE INDEX idx_elevated_sessions_expires ON elevated_sessions(expires_at);

-- ============================================================================
-- Guild Role Tables
-- ============================================================================

-- Guild roles (replaces simple roles for guild context)
CREATE TABLE guild_roles (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    guild_id UUID NOT NULL REFERENCES guilds(id) ON DELETE CASCADE,
    name VARCHAR(64) NOT NULL,
    color VARCHAR(7),
    permissions BIGINT NOT NULL DEFAULT 0,
    position INTEGER NOT NULL DEFAULT 0,
    is_default BOOLEAN NOT NULL DEFAULT FALSE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE(guild_id, name)
);

CREATE INDEX idx_guild_roles_guild ON guild_roles(guild_id);
CREATE INDEX idx_guild_roles_position ON guild_roles(guild_id, position);

-- Guild member roles junction
CREATE TABLE guild_member_roles (
    guild_id UUID NOT NULL REFERENCES guilds(id) ON DELETE CASCADE,
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    role_id UUID NOT NULL REFERENCES guild_roles(id) ON DELETE CASCADE,
    assigned_by UUID REFERENCES users(id),
    assigned_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (guild_id, user_id, role_id)
);

CREATE INDEX idx_guild_member_roles_user ON guild_member_roles(user_id);

-- Channel permission overrides
CREATE TABLE channel_overrides (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    channel_id UUID NOT NULL REFERENCES channels(id) ON DELETE CASCADE,
    role_id UUID NOT NULL REFERENCES guild_roles(id) ON DELETE CASCADE,
    allow_permissions BIGINT NOT NULL DEFAULT 0,
    deny_permissions BIGINT NOT NULL DEFAULT 0,
    UNIQUE(channel_id, role_id)
);

CREATE INDEX idx_channel_overrides_channel ON channel_overrides(channel_id);

-- ============================================================================
-- System Settings & Audit
-- ============================================================================

-- System security settings
CREATE TABLE system_settings (
    key VARCHAR(64) PRIMARY KEY,
    value JSONB NOT NULL,
    updated_by UUID REFERENCES users(id),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Insert default settings
INSERT INTO system_settings (key, value) VALUES
    ('security.require_reauth_destructive', 'false'),
    ('security.inactivity_timeout_minutes', 'null'),
    ('security.dual_admin_approval', 'false'),
    ('security.require_webauthn', 'false'),
    ('security.cooling_off_hours', '4'),
    ('break_glass.delay_minutes', '15'),
    ('break_glass.cooldown_hours', '1'),
    ('break_glass.max_per_admin_24h', '1'),
    ('break_glass.max_system_24h', '3'),
    ('break_glass.require_webauthn', 'false'),
    ('break_glass.external_webhook', 'null'),
    ('break_glass.review_due_hours', '48');

-- System audit log
CREATE TABLE system_audit_log (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    actor_id UUID NOT NULL REFERENCES users(id),
    action VARCHAR(64) NOT NULL,
    target_type VARCHAR(32),
    target_id UUID,
    details JSONB,
    ip_address INET,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_system_audit_actor ON system_audit_log(actor_id);
CREATE INDEX idx_system_audit_action ON system_audit_log(action, created_at DESC);
CREATE INDEX idx_system_audit_target ON system_audit_log(target_type, target_id);

-- System announcements
CREATE TABLE system_announcements (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    author_id UUID NOT NULL REFERENCES users(id),
    title VARCHAR(128) NOT NULL,
    content TEXT NOT NULL,
    severity VARCHAR(16) NOT NULL DEFAULT 'info',
    active BOOLEAN NOT NULL DEFAULT TRUE,
    starts_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    ends_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_announcements_active ON system_announcements(active, starts_at, ends_at);

-- ============================================================================
-- Approval & Break-Glass Tables
-- ============================================================================

-- Pending approvals (for dual approval flow)
CREATE TABLE pending_approvals (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    action_type VARCHAR(64) NOT NULL,
    target_type VARCHAR(32) NOT NULL,
    target_id UUID NOT NULL,
    requested_by UUID NOT NULL REFERENCES users(id),
    approved_by UUID REFERENCES users(id),
    status VARCHAR(16) NOT NULL DEFAULT 'pending',
    execute_after TIMESTAMPTZ,
    expires_at TIMESTAMPTZ NOT NULL,
    executed_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_pending_approvals_status ON pending_approvals(status, expires_at);

-- Break-glass requests
CREATE TABLE break_glass_requests (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    admin_id UUID NOT NULL REFERENCES users(id),
    action_type VARCHAR(64) NOT NULL,
    target_type VARCHAR(32) NOT NULL,
    target_id UUID NOT NULL,
    justification TEXT NOT NULL CHECK (length(justification) >= 50),
    incident_ticket VARCHAR(64),
    status VARCHAR(16) NOT NULL DEFAULT 'waiting',
    execute_at TIMESTAMPTZ NOT NULL,
    blocked_by UUID REFERENCES users(id),
    block_reason TEXT,
    executed_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_break_glass_status ON break_glass_requests(status, execute_at);
CREATE INDEX idx_break_glass_admin ON break_glass_requests(admin_id);

-- Break-glass cooldowns (per admin)
CREATE TABLE break_glass_cooldowns (
    admin_id UUID PRIMARY KEY REFERENCES users(id),
    last_used_at TIMESTAMPTZ NOT NULL,
    uses_last_24h INTEGER NOT NULL DEFAULT 1
);

-- Break-glass reviews
CREATE TABLE break_glass_reviews (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    break_glass_id UUID NOT NULL REFERENCES break_glass_requests(id),
    reviewer_id UUID REFERENCES users(id),
    status VARCHAR(16) NOT NULL DEFAULT 'pending',
    notes TEXT,
    due_at TIMESTAMPTZ NOT NULL,
    reviewed_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_bg_reviews_pending ON break_glass_reviews(status, due_at)
    WHERE status = 'pending';

-- ============================================================================
-- Guild Security Settings Extension
-- ============================================================================

ALTER TABLE guilds ADD COLUMN IF NOT EXISTS security_settings JSONB NOT NULL DEFAULT '{
    "require_dual_owner_delete": false,
    "require_webauthn_transfer": false,
    "cooling_off_hours": 4
}';

-- ============================================================================
-- Global User Bans
-- ============================================================================

CREATE TABLE global_bans (
    user_id UUID PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    banned_by UUID NOT NULL REFERENCES users(id),
    reason TEXT NOT NULL,
    expires_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- ============================================================================
-- Guild Suspension
-- ============================================================================

ALTER TABLE guilds ADD COLUMN IF NOT EXISTS suspended_at TIMESTAMPTZ;
ALTER TABLE guilds ADD COLUMN IF NOT EXISTS suspended_by UUID REFERENCES users(id);
ALTER TABLE guilds ADD COLUMN IF NOT EXISTS suspension_reason TEXT;
```

**Step 2: Run the migration**

```bash
cd /home/detair/GIT/canis/.worktrees/permission-system/server
sqlx migrate run
```

Expected: Migration applies successfully.

**Step 3: Verify tables exist**

```bash
psql -d voicechat -c "\dt" | grep -E "(system_admin|elevated_session|guild_role|channel_override|audit)"
```

Expected: All new tables listed.

**Step 4: Commit**

```bash
git add server/migrations/20260113000000_permission_system.sql
git commit -m "feat(db): Add permission system migration

- System admin and elevated session tables
- Guild roles with hierarchical permissions
- Channel permission overrides
- System settings and audit log
- Break-glass emergency tables
- Pending approvals for dual approval flow

Co-Authored-By: Claude Opus 4.5 <noreply@anthropic.com>"
```

---

## Phase 2: Permission Types

### Task 2: Permission Bitfield Types

**Files:**
- Create: `server/src/permissions/mod.rs`
- Create: `server/src/permissions/guild.rs`
- Create: `server/src/permissions/system.rs`
- Modify: `server/src/lib.rs`

**Step 1: Create permissions module file**

Create `server/src/permissions/mod.rs`:

```rust
//! Permission system types and utilities.
//!
//! Two-tier permission model:
//! - System permissions: Platform-level admin actions
//! - Guild permissions: Per-guild role-based access control

pub mod guild;
pub mod system;

pub use guild::{GuildPermissions, EVERYONE_FORBIDDEN};
pub use system::SystemPermission;
```

**Step 2: Create guild permissions**

Create `server/src/permissions/guild.rs`:

```rust
//! Guild-level permissions using bitfields.

use bitflags::bitflags;
use serde::{Deserialize, Serialize};

bitflags! {
    /// Guild permission flags stored as BIGINT in database.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
    pub struct GuildPermissions: u64 {
        // Content permissions (bits 0-4)
        const SEND_MESSAGES      = 1 << 0;
        const EMBED_LINKS        = 1 << 1;
        const ATTACH_FILES       = 1 << 2;
        const USE_EMOJI          = 1 << 3;
        const ADD_REACTIONS      = 1 << 4;

        // Voice permissions (bits 5-9)
        const VOICE_CONNECT      = 1 << 5;
        const VOICE_SPEAK        = 1 << 6;
        const VOICE_MUTE_OTHERS  = 1 << 7;
        const VOICE_DEAFEN_OTHERS = 1 << 8;
        const VOICE_MOVE_MEMBERS = 1 << 9;

        // Moderation permissions (bits 10-13)
        const MANAGE_MESSAGES    = 1 << 10;
        const TIMEOUT_MEMBERS    = 1 << 11;
        const KICK_MEMBERS       = 1 << 12;
        const BAN_MEMBERS        = 1 << 13;

        // Guild management permissions (bits 14-18)
        const MANAGE_CHANNELS    = 1 << 14;
        const MANAGE_ROLES       = 1 << 15;
        const VIEW_AUDIT_LOG     = 1 << 16;
        const MANAGE_GUILD       = 1 << 17;
        const TRANSFER_OWNERSHIP = 1 << 18;

        // Invite permissions (bits 19-20)
        const CREATE_INVITE      = 1 << 19;
        const MANAGE_INVITES     = 1 << 20;

        // Preset combinations
        const EVERYONE_DEFAULT = Self::SEND_MESSAGES.bits()
            | Self::EMBED_LINKS.bits()
            | Self::ATTACH_FILES.bits()
            | Self::USE_EMOJI.bits()
            | Self::ADD_REACTIONS.bits()
            | Self::VOICE_CONNECT.bits()
            | Self::VOICE_SPEAK.bits()
            | Self::CREATE_INVITE.bits();

        const MODERATOR_DEFAULT = Self::EVERYONE_DEFAULT.bits()
            | Self::MANAGE_MESSAGES.bits()
            | Self::TIMEOUT_MEMBERS.bits()
            | Self::VOICE_MUTE_OTHERS.bits()
            | Self::VOICE_DEAFEN_OTHERS.bits();

        const OFFICER_DEFAULT = Self::MODERATOR_DEFAULT.bits()
            | Self::KICK_MEMBERS.bits()
            | Self::BAN_MEMBERS.bits()
            | Self::MANAGE_CHANNELS.bits()
            | Self::MANAGE_ROLES.bits()
            | Self::VIEW_AUDIT_LOG.bits()
            | Self::MANAGE_INVITES.bits()
            | Self::VOICE_MOVE_MEMBERS.bits();
    }
}

/// Permissions that @everyone role can NEVER have.
pub const EVERYONE_FORBIDDEN: GuildPermissions = GuildPermissions::from_bits_truncate(
    GuildPermissions::MANAGE_ROLES.bits()
        | GuildPermissions::MANAGE_GUILD.bits()
        | GuildPermissions::KICK_MEMBERS.bits()
        | GuildPermissions::BAN_MEMBERS.bits()
        | GuildPermissions::TRANSFER_OWNERSHIP.bits(),
);

impl GuildPermissions {
    /// Check if this permission set contains another.
    #[inline]
    pub fn has(self, permission: GuildPermissions) -> bool {
        self.contains(permission)
    }

    /// Convert from database BIGINT.
    pub fn from_db(value: i64) -> Self {
        Self::from_bits_truncate(value as u64)
    }

    /// Convert to database BIGINT.
    pub fn to_db(self) -> i64 {
        self.bits() as i64
    }

    /// Validate permissions for @everyone role.
    pub fn validate_for_everyone(self) -> Result<(), &'static str> {
        if self.intersects(EVERYONE_FORBIDDEN) {
            return Err("Cannot grant dangerous permissions to @everyone");
        }
        Ok(())
    }
}

impl Default for GuildPermissions {
    fn default() -> Self {
        Self::empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_permission_bits() {
        assert_eq!(GuildPermissions::SEND_MESSAGES.bits(), 1);
        assert_eq!(GuildPermissions::EMBED_LINKS.bits(), 2);
        assert_eq!(GuildPermissions::VOICE_CONNECT.bits(), 32);
    }

    #[test]
    fn test_permission_contains() {
        let perms = GuildPermissions::SEND_MESSAGES | GuildPermissions::EMBED_LINKS;
        assert!(perms.has(GuildPermissions::SEND_MESSAGES));
        assert!(perms.has(GuildPermissions::EMBED_LINKS));
        assert!(!perms.has(GuildPermissions::ATTACH_FILES));
    }

    #[test]
    fn test_everyone_forbidden() {
        let everyone = GuildPermissions::EVERYONE_DEFAULT;
        assert!(everyone.validate_for_everyone().is_ok());

        let bad = GuildPermissions::MANAGE_ROLES;
        assert!(bad.validate_for_everyone().is_err());
    }

    #[test]
    fn test_db_conversion() {
        let perms = GuildPermissions::SEND_MESSAGES | GuildPermissions::KICK_MEMBERS;
        let db_value = perms.to_db();
        let restored = GuildPermissions::from_db(db_value);
        assert_eq!(perms, restored);
    }

    #[test]
    fn test_presets() {
        assert!(GuildPermissions::MODERATOR_DEFAULT.contains(GuildPermissions::SEND_MESSAGES));
        assert!(GuildPermissions::MODERATOR_DEFAULT.contains(GuildPermissions::MANAGE_MESSAGES));
        assert!(GuildPermissions::OFFICER_DEFAULT.contains(GuildPermissions::KICK_MEMBERS));
    }
}
```

**Step 3: Create system permissions**

Create `server/src/permissions/system.rs`:

```rust
//! System-level permissions for platform admins.

use serde::{Deserialize, Serialize};

/// System-level permission actions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SystemPermission {
    // Guild management
    ViewAllGuilds,
    SuspendGuild,
    DeleteGuild,
    AssignGuildOwner,

    // User management
    ViewAllUsers,
    GlobalBanUser,

    // System
    ViewAuditLog,
    SendAnnouncement,
    ManageSettings,

    // Break-glass
    UseBreakGlass,
    ReviewBreakGlass,
}

impl SystemPermission {
    /// Get the action string for audit logging.
    pub fn action_name(&self) -> &'static str {
        match self {
            Self::ViewAllGuilds => "system.guilds.view",
            Self::SuspendGuild => "system.guilds.suspend",
            Self::DeleteGuild => "system.guilds.delete",
            Self::AssignGuildOwner => "system.guilds.assign_owner",
            Self::ViewAllUsers => "system.users.view",
            Self::GlobalBanUser => "system.users.ban",
            Self::ViewAuditLog => "system.audit.view",
            Self::SendAnnouncement => "system.announcements.send",
            Self::ManageSettings => "system.settings.manage",
            Self::UseBreakGlass => "system.break_glass.use",
            Self::ReviewBreakGlass => "system.break_glass.review",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_action_names() {
        assert_eq!(
            SystemPermission::SuspendGuild.action_name(),
            "system.guilds.suspend"
        );
        assert_eq!(
            SystemPermission::GlobalBanUser.action_name(),
            "system.users.ban"
        );
    }
}
```

**Step 4: Add bitflags dependency**

Check if bitflags is already in Cargo.toml, if not add it:

```bash
cd /home/detair/GIT/canis/.worktrees/permission-system/server
grep -q "bitflags" Cargo.toml || echo 'bitflags = "2.4"' >> Cargo.toml
```

**Step 5: Register module in lib.rs**

Modify `server/src/lib.rs` to add the permissions module:

```rust
// Add near other module declarations
pub mod permissions;
```

**Step 6: Run tests**

```bash
cd /home/detair/GIT/canis/.worktrees/permission-system/server
cargo test permissions --lib
```

Expected: All permission tests pass.

**Step 7: Commit**

```bash
git add server/src/permissions/ server/src/lib.rs server/Cargo.toml
git commit -m "feat(permissions): Add permission bitfield types

- GuildPermissions with bitflags for efficient storage
- SystemPermission enum for platform admin actions
- Default permission presets (everyone, moderator, officer)
- Validation to prevent dangerous @everyone permissions

Co-Authored-By: Claude Opus 4.5 <noreply@anthropic.com>"
```

---

## Phase 3: Database Models

### Task 3: Permission Database Models

**Files:**
- Create: `server/src/permissions/models.rs`
- Modify: `server/src/permissions/mod.rs`

**Step 1: Create database models**

Create `server/src/permissions/models.rs`:

```rust
//! Database models for permission system.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

use super::guild::GuildPermissions;

/// System admin record.
#[derive(Debug, Clone, FromRow, Serialize)]
pub struct SystemAdmin {
    pub user_id: Uuid,
    pub granted_by: Option<Uuid>,
    pub granted_at: DateTime<Utc>,
}

/// Elevated session for sudo-style admin access.
#[derive(Debug, Clone, FromRow, Serialize)]
pub struct ElevatedSession {
    pub id: Uuid,
    pub user_id: Uuid,
    pub session_id: Uuid,
    pub ip_address: String,
    pub elevated_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub reason: Option<String>,
}

/// Guild role with permissions.
#[derive(Debug, Clone, FromRow, Serialize)]
pub struct GuildRole {
    pub id: Uuid,
    pub guild_id: Uuid,
    pub name: String,
    pub color: Option<String>,
    #[sqlx(try_from = "i64")]
    pub permissions: GuildPermissions,
    pub position: i32,
    pub is_default: bool,
    pub created_at: DateTime<Utc>,
}

/// Guild member role assignment.
#[derive(Debug, Clone, FromRow, Serialize)]
pub struct GuildMemberRole {
    pub guild_id: Uuid,
    pub user_id: Uuid,
    pub role_id: Uuid,
    pub assigned_by: Option<Uuid>,
    pub assigned_at: DateTime<Utc>,
}

/// Channel permission override.
#[derive(Debug, Clone, FromRow, Serialize)]
pub struct ChannelOverride {
    pub id: Uuid,
    pub channel_id: Uuid,
    pub role_id: Uuid,
    #[sqlx(try_from = "i64")]
    pub allow_permissions: GuildPermissions,
    #[sqlx(try_from = "i64")]
    pub deny_permissions: GuildPermissions,
}

/// System audit log entry.
#[derive(Debug, Clone, FromRow, Serialize)]
pub struct AuditLogEntry {
    pub id: Uuid,
    pub actor_id: Uuid,
    pub action: String,
    pub target_type: Option<String>,
    pub target_id: Option<Uuid>,
    pub details: Option<serde_json::Value>,
    pub ip_address: Option<String>,
    pub created_at: DateTime<Utc>,
}

/// System announcement.
#[derive(Debug, Clone, FromRow, Serialize)]
pub struct SystemAnnouncement {
    pub id: Uuid,
    pub author_id: Uuid,
    pub title: String,
    pub content: String,
    pub severity: String,
    pub active: bool,
    pub starts_at: DateTime<Utc>,
    pub ends_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

/// Pending approval for dual-approval actions.
#[derive(Debug, Clone, FromRow, Serialize)]
pub struct PendingApproval {
    pub id: Uuid,
    pub action_type: String,
    pub target_type: String,
    pub target_id: Uuid,
    pub requested_by: Uuid,
    pub approved_by: Option<Uuid>,
    pub status: String,
    pub execute_after: Option<DateTime<Utc>>,
    pub expires_at: DateTime<Utc>,
    pub executed_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

/// Break-glass emergency request.
#[derive(Debug, Clone, FromRow, Serialize)]
pub struct BreakGlassRequest {
    pub id: Uuid,
    pub admin_id: Uuid,
    pub action_type: String,
    pub target_type: String,
    pub target_id: Uuid,
    pub justification: String,
    pub incident_ticket: Option<String>,
    pub status: String,
    pub execute_at: DateTime<Utc>,
    pub blocked_by: Option<Uuid>,
    pub block_reason: Option<String>,
    pub executed_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

/// Global user ban.
#[derive(Debug, Clone, FromRow, Serialize)]
pub struct GlobalBan {
    pub user_id: Uuid,
    pub banned_by: Uuid,
    pub reason: String,
    pub expires_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

// Implement TryFrom for GuildPermissions to work with sqlx
impl TryFrom<i64> for GuildPermissions {
    type Error = std::convert::Infallible;

    fn try_from(value: i64) -> Result<Self, Self::Error> {
        Ok(GuildPermissions::from_db(value))
    }
}

/// Request types for API
#[derive(Debug, Deserialize)]
pub struct CreateGuildRoleRequest {
    pub name: String,
    pub color: Option<String>,
    pub permissions: Option<u64>,
    pub position: Option<i32>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateGuildRoleRequest {
    pub name: Option<String>,
    pub color: Option<String>,
    pub permissions: Option<u64>,
    pub position: Option<i32>,
}

#[derive(Debug, Deserialize)]
pub struct SetChannelOverrideRequest {
    pub allow: Option<u64>,
    pub deny: Option<u64>,
}

#[derive(Debug, Deserialize)]
pub struct ElevateSessionRequest {
    pub mfa_code: String,
    pub reason: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct BreakGlassRequestBody {
    pub action_type: String,
    pub target_type: String,
    pub target_id: Uuid,
    pub justification: String,
    pub incident_ticket: Option<String>,
}
```

**Step 2: Update mod.rs to export models**

Modify `server/src/permissions/mod.rs`:

```rust
//! Permission system types and utilities.
//!
//! Two-tier permission model:
//! - System permissions: Platform-level admin actions
//! - Guild permissions: Per-guild role-based access control

pub mod guild;
pub mod models;
pub mod system;

pub use guild::{GuildPermissions, EVERYONE_FORBIDDEN};
pub use models::*;
pub use system::SystemPermission;
```

**Step 3: Build to verify**

```bash
cd /home/detair/GIT/canis/.worktrees/permission-system/server
cargo build --lib
```

Expected: Builds successfully.

**Step 4: Commit**

```bash
git add server/src/permissions/
git commit -m "feat(permissions): Add database models

- SystemAdmin, ElevatedSession for admin access
- GuildRole, GuildMemberRole for role management
- ChannelOverride for channel-specific permissions
- AuditLogEntry, PendingApproval, BreakGlassRequest
- API request types for role and permission management

Co-Authored-By: Claude Opus 4.5 <noreply@anthropic.com>"
```

---

## Phase 4: Permission Resolution Logic

### Task 4: Permission Computation

**Files:**
- Create: `server/src/permissions/resolver.rs`
- Modify: `server/src/permissions/mod.rs`

**Step 1: Create permission resolver**

Create `server/src/permissions/resolver.rs`:

```rust
//! Permission resolution logic.
//!
//! Computes effective permissions for a user in a guild/channel context.

use uuid::Uuid;

use super::guild::GuildPermissions;
use super::models::{ChannelOverride, GuildRole};

/// Compute guild permissions for a user.
///
/// Resolution order:
/// 1. Guild owner has all permissions
/// 2. Start with @everyone role permissions
/// 3. Add permissions from assigned roles (by position)
/// 4. Apply channel overrides if channel context provided
pub fn compute_guild_permissions(
    user_id: Uuid,
    guild_owner_id: Uuid,
    everyone_permissions: GuildPermissions,
    user_roles: &[GuildRole],
    channel_overrides: Option<&[ChannelOverride]>,
) -> GuildPermissions {
    // Guild owner has everything
    if guild_owner_id == user_id {
        return GuildPermissions::all();
    }

    // Start with @everyone permissions
    let mut perms = everyone_permissions;

    // Add role permissions (sorted by position, lower number = higher rank)
    let mut sorted_roles: Vec<_> = user_roles.iter().collect();
    sorted_roles.sort_by_key(|r| r.position);

    for role in sorted_roles {
        perms |= role.permissions;
    }

    // Apply channel overrides if provided
    if let Some(overrides) = channel_overrides {
        for role in user_roles {
            if let Some(ovr) = overrides.iter().find(|o| o.role_id == role.id) {
                perms |= ovr.allow_permissions;
                perms &= !ovr.deny_permissions; // Deny wins
            }
        }
    }

    perms
}

/// Check if a user can manage a target role.
///
/// Rules:
/// 1. Must have MANAGE_ROLES permission
/// 2. Cannot edit roles at or above your position
/// 3. Cannot grant permissions you don't have
pub fn can_manage_role(
    actor_permissions: GuildPermissions,
    actor_highest_position: i32,
    target_role_position: i32,
    new_permissions: Option<GuildPermissions>,
) -> Result<(), PermissionError> {
    // Must have MANAGE_ROLES
    if !actor_permissions.has(GuildPermissions::MANAGE_ROLES) {
        return Err(PermissionError::MissingPermission(
            GuildPermissions::MANAGE_ROLES,
        ));
    }

    // Cannot edit roles at or above your position (lower number = higher rank)
    if target_role_position <= actor_highest_position {
        return Err(PermissionError::RoleHierarchy {
            actor_position: actor_highest_position,
            target_position: target_role_position,
        });
    }

    // Cannot grant permissions you don't have
    if let Some(new_perms) = new_permissions {
        let escalation = new_perms & !actor_permissions;
        if !escalation.is_empty() {
            return Err(PermissionError::CannotEscalate(escalation));
        }
    }

    Ok(())
}

/// Check if a user can moderate a target member.
///
/// Rules:
/// 1. Cannot moderate guild owner
/// 2. Cannot moderate someone with higher/equal role
pub fn can_moderate_member(
    actor_highest_position: i32,
    target_highest_position: i32,
    target_is_owner: bool,
) -> Result<(), PermissionError> {
    // Cannot moderate guild owner
    if target_is_owner {
        return Err(PermissionError::CannotModerateOwner);
    }

    // Cannot moderate someone with higher/equal role (lower number = higher rank)
    if target_highest_position <= actor_highest_position {
        return Err(PermissionError::RoleHierarchy {
            actor_position: actor_highest_position,
            target_position: target_highest_position,
        });
    }

    Ok(())
}

/// Permission check errors.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PermissionError {
    /// User lacks required permission.
    MissingPermission(GuildPermissions),

    /// Role hierarchy violation.
    RoleHierarchy {
        actor_position: i32,
        target_position: i32,
    },

    /// Attempted to grant permissions not held.
    CannotEscalate(GuildPermissions),

    /// Attempted to moderate guild owner.
    CannotModerateOwner,
}

impl std::fmt::Display for PermissionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingPermission(p) => write!(f, "Missing permission: {:?}", p),
            Self::RoleHierarchy {
                actor_position,
                target_position,
            } => write!(
                f,
                "Cannot modify role at position {} (your position: {})",
                target_position, actor_position
            ),
            Self::CannotEscalate(p) => {
                write!(f, "Cannot grant permissions you don't have: {:?}", p)
            }
            Self::CannotModerateOwner => write!(f, "Cannot moderate guild owner"),
        }
    }
}

impl std::error::Error for PermissionError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_owner_has_all_permissions() {
        let owner_id = Uuid::new_v4();
        let perms = compute_guild_permissions(
            owner_id,
            owner_id,
            GuildPermissions::empty(),
            &[],
            None,
        );
        assert_eq!(perms, GuildPermissions::all());
    }

    #[test]
    fn test_everyone_permissions_applied() {
        let user_id = Uuid::new_v4();
        let owner_id = Uuid::new_v4();
        let everyone = GuildPermissions::SEND_MESSAGES | GuildPermissions::VOICE_CONNECT;

        let perms = compute_guild_permissions(user_id, owner_id, everyone, &[], None);

        assert!(perms.has(GuildPermissions::SEND_MESSAGES));
        assert!(perms.has(GuildPermissions::VOICE_CONNECT));
        assert!(!perms.has(GuildPermissions::KICK_MEMBERS));
    }

    #[test]
    fn test_role_permissions_combined() {
        let user_id = Uuid::new_v4();
        let owner_id = Uuid::new_v4();
        let everyone = GuildPermissions::SEND_MESSAGES;

        let mod_role = GuildRole {
            id: Uuid::new_v4(),
            guild_id: Uuid::new_v4(),
            name: "Moderator".to_string(),
            color: None,
            permissions: GuildPermissions::MANAGE_MESSAGES | GuildPermissions::TIMEOUT_MEMBERS,
            position: 100,
            is_default: false,
            created_at: chrono::Utc::now(),
        };

        let perms = compute_guild_permissions(
            user_id,
            owner_id,
            everyone,
            &[mod_role],
            None,
        );

        assert!(perms.has(GuildPermissions::SEND_MESSAGES)); // from everyone
        assert!(perms.has(GuildPermissions::MANAGE_MESSAGES)); // from role
        assert!(perms.has(GuildPermissions::TIMEOUT_MEMBERS)); // from role
    }

    #[test]
    fn test_channel_override_deny_wins() {
        let user_id = Uuid::new_v4();
        let owner_id = Uuid::new_v4();
        let role_id = Uuid::new_v4();
        let channel_id = Uuid::new_v4();

        let everyone = GuildPermissions::SEND_MESSAGES | GuildPermissions::EMBED_LINKS;

        let role = GuildRole {
            id: role_id,
            guild_id: Uuid::new_v4(),
            name: "Member".to_string(),
            color: None,
            permissions: GuildPermissions::empty(),
            position: 999,
            is_default: true,
            created_at: chrono::Utc::now(),
        };

        let override_entry = ChannelOverride {
            id: Uuid::new_v4(),
            channel_id,
            role_id,
            allow_permissions: GuildPermissions::ATTACH_FILES,
            deny_permissions: GuildPermissions::SEND_MESSAGES,
        };

        let perms = compute_guild_permissions(
            user_id,
            owner_id,
            everyone,
            &[role],
            Some(&[override_entry]),
        );

        assert!(!perms.has(GuildPermissions::SEND_MESSAGES)); // denied by override
        assert!(perms.has(GuildPermissions::EMBED_LINKS)); // still from everyone
        assert!(perms.has(GuildPermissions::ATTACH_FILES)); // allowed by override
    }

    #[test]
    fn test_can_manage_role_hierarchy() {
        let perms = GuildPermissions::MANAGE_ROLES | GuildPermissions::KICK_MEMBERS;

        // Can manage lower role
        assert!(can_manage_role(perms, 50, 100, None).is_ok());

        // Cannot manage equal position
        assert!(can_manage_role(perms, 50, 50, None).is_err());

        // Cannot manage higher role
        assert!(can_manage_role(perms, 50, 10, None).is_err());
    }

    #[test]
    fn test_cannot_escalate_permissions() {
        let actor_perms = GuildPermissions::MANAGE_ROLES | GuildPermissions::KICK_MEMBERS;
        let new_perms = GuildPermissions::KICK_MEMBERS | GuildPermissions::BAN_MEMBERS;

        // Actor doesn't have BAN_MEMBERS, so this should fail
        let result = can_manage_role(actor_perms, 50, 100, Some(new_perms));
        assert!(matches!(result, Err(PermissionError::CannotEscalate(_))));
    }

    #[test]
    fn test_cannot_moderate_owner() {
        let result = can_moderate_member(50, 1, true);
        assert!(matches!(result, Err(PermissionError::CannotModerateOwner)));
    }
}
```

**Step 2: Update mod.rs to export resolver**

Modify `server/src/permissions/mod.rs`:

```rust
//! Permission system types and utilities.
//!
//! Two-tier permission model:
//! - System permissions: Platform-level admin actions
//! - Guild permissions: Per-guild role-based access control

pub mod guild;
pub mod models;
pub mod resolver;
pub mod system;

pub use guild::{GuildPermissions, EVERYONE_FORBIDDEN};
pub use models::*;
pub use resolver::{
    can_manage_role, can_moderate_member, compute_guild_permissions, PermissionError,
};
pub use system::SystemPermission;
```

**Step 3: Run tests**

```bash
cd /home/detair/GIT/canis/.worktrees/permission-system/server
cargo test permissions --lib
```

Expected: All tests pass.

**Step 4: Commit**

```bash
git add server/src/permissions/
git commit -m "feat(permissions): Add permission resolution logic

- compute_guild_permissions with role inheritance
- Channel override support (deny wins)
- can_manage_role with hierarchy checks
- can_moderate_member with owner protection
- Privilege escalation prevention

Co-Authored-By: Claude Opus 4.5 <noreply@anthropic.com>"
```

---

## Phase 5: System Admin Service

### Task 5: System Admin Queries

**Files:**
- Create: `server/src/permissions/queries.rs`
- Modify: `server/src/permissions/mod.rs`

**Step 1: Create database queries**

Create `server/src/permissions/queries.rs`:

```rust
//! Database queries for permission system.

use chrono::{Duration, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use super::models::*;
use super::guild::GuildPermissions;

// ============================================================================
// System Admin Queries
// ============================================================================

/// Check if user is a system admin.
pub async fn is_system_admin(pool: &PgPool, user_id: Uuid) -> sqlx::Result<bool> {
    let result = sqlx::query_scalar!(
        r#"SELECT EXISTS(SELECT 1 FROM system_admins WHERE user_id = $1) as "exists!""#,
        user_id
    )
    .fetch_one(pool)
    .await?;

    Ok(result)
}

/// Get system admin record.
pub async fn get_system_admin(pool: &PgPool, user_id: Uuid) -> sqlx::Result<Option<SystemAdmin>> {
    sqlx::query_as!(
        SystemAdmin,
        r#"SELECT user_id, granted_by, granted_at FROM system_admins WHERE user_id = $1"#,
        user_id
    )
    .fetch_optional(pool)
    .await
}

/// List all system admins.
pub async fn list_system_admins(pool: &PgPool) -> sqlx::Result<Vec<SystemAdmin>> {
    sqlx::query_as!(
        SystemAdmin,
        r#"SELECT user_id, granted_by, granted_at FROM system_admins ORDER BY granted_at"#
    )
    .fetch_all(pool)
    .await
}

/// Grant system admin privileges.
pub async fn grant_system_admin(
    pool: &PgPool,
    user_id: Uuid,
    granted_by: Uuid,
) -> sqlx::Result<SystemAdmin> {
    sqlx::query_as!(
        SystemAdmin,
        r#"INSERT INTO system_admins (user_id, granted_by)
           VALUES ($1, $2)
           RETURNING user_id, granted_by, granted_at"#,
        user_id,
        granted_by
    )
    .fetch_one(pool)
    .await
}

/// Revoke system admin privileges.
pub async fn revoke_system_admin(pool: &PgPool, user_id: Uuid) -> sqlx::Result<bool> {
    let result = sqlx::query!(
        r#"DELETE FROM system_admins WHERE user_id = $1"#,
        user_id
    )
    .execute(pool)
    .await?;

    Ok(result.rows_affected() > 0)
}

// ============================================================================
// Elevated Session Queries
// ============================================================================

/// Get active elevated session for a session ID.
pub async fn get_elevated_session(
    pool: &PgPool,
    session_id: Uuid,
) -> sqlx::Result<Option<ElevatedSession>> {
    sqlx::query_as!(
        ElevatedSession,
        r#"SELECT id, user_id, session_id, ip_address::text as "ip_address!",
                  elevated_at, expires_at, reason
           FROM elevated_sessions
           WHERE session_id = $1 AND expires_at > NOW()"#,
        session_id
    )
    .fetch_optional(pool)
    .await
}

/// Create elevated session.
pub async fn create_elevated_session(
    pool: &PgPool,
    user_id: Uuid,
    session_id: Uuid,
    ip_address: &str,
    duration_minutes: i64,
    reason: Option<&str>,
) -> sqlx::Result<ElevatedSession> {
    let expires_at = Utc::now() + Duration::minutes(duration_minutes);

    sqlx::query_as!(
        ElevatedSession,
        r#"INSERT INTO elevated_sessions (user_id, session_id, ip_address, expires_at, reason)
           VALUES ($1, $2, $3::inet, $4, $5)
           ON CONFLICT (session_id) DO UPDATE SET
               elevated_at = NOW(),
               expires_at = $4,
               reason = $5
           RETURNING id, user_id, session_id, ip_address::text as "ip_address!",
                     elevated_at, expires_at, reason"#,
        user_id,
        session_id,
        ip_address,
        expires_at,
        reason
    )
    .fetch_one(pool)
    .await
}

/// Delete elevated session (de-elevate).
pub async fn delete_elevated_session(pool: &PgPool, session_id: Uuid) -> sqlx::Result<bool> {
    let result = sqlx::query!(
        r#"DELETE FROM elevated_sessions WHERE session_id = $1"#,
        session_id
    )
    .execute(pool)
    .await?;

    Ok(result.rows_affected() > 0)
}

/// Check if session is elevated and not expired.
pub async fn is_session_elevated(pool: &PgPool, session_id: Uuid) -> sqlx::Result<bool> {
    let result = sqlx::query_scalar!(
        r#"SELECT EXISTS(
            SELECT 1 FROM elevated_sessions
            WHERE session_id = $1 AND expires_at > NOW()
        ) as "exists!""#,
        session_id
    )
    .fetch_one(pool)
    .await?;

    Ok(result)
}

// ============================================================================
// Guild Role Queries
// ============================================================================

/// Get all roles for a guild.
pub async fn get_guild_roles(pool: &PgPool, guild_id: Uuid) -> sqlx::Result<Vec<GuildRole>> {
    sqlx::query_as!(
        GuildRole,
        r#"SELECT id, guild_id, name, color, permissions, position, is_default, created_at
           FROM guild_roles
           WHERE guild_id = $1
           ORDER BY position ASC"#,
        guild_id
    )
    .fetch_all(pool)
    .await
}

/// Get a specific guild role.
pub async fn get_guild_role(pool: &PgPool, role_id: Uuid) -> sqlx::Result<Option<GuildRole>> {
    sqlx::query_as!(
        GuildRole,
        r#"SELECT id, guild_id, name, color, permissions, position, is_default, created_at
           FROM guild_roles
           WHERE id = $1"#,
        role_id
    )
    .fetch_optional(pool)
    .await
}

/// Get the @everyone role for a guild.
pub async fn get_everyone_role(pool: &PgPool, guild_id: Uuid) -> sqlx::Result<Option<GuildRole>> {
    sqlx::query_as!(
        GuildRole,
        r#"SELECT id, guild_id, name, color, permissions, position, is_default, created_at
           FROM guild_roles
           WHERE guild_id = $1 AND is_default = true"#,
        guild_id
    )
    .fetch_optional(pool)
    .await
}

/// Create a guild role.
pub async fn create_guild_role(
    pool: &PgPool,
    guild_id: Uuid,
    name: &str,
    color: Option<&str>,
    permissions: GuildPermissions,
    position: i32,
    is_default: bool,
) -> sqlx::Result<GuildRole> {
    sqlx::query_as!(
        GuildRole,
        r#"INSERT INTO guild_roles (guild_id, name, color, permissions, position, is_default)
           VALUES ($1, $2, $3, $4, $5, $6)
           RETURNING id, guild_id, name, color, permissions, position, is_default, created_at"#,
        guild_id,
        name,
        color,
        permissions.to_db(),
        position,
        is_default
    )
    .fetch_one(pool)
    .await
}

/// Update a guild role.
pub async fn update_guild_role(
    pool: &PgPool,
    role_id: Uuid,
    name: Option<&str>,
    color: Option<&str>,
    permissions: Option<GuildPermissions>,
    position: Option<i32>,
) -> sqlx::Result<Option<GuildRole>> {
    sqlx::query_as!(
        GuildRole,
        r#"UPDATE guild_roles SET
               name = COALESCE($2, name),
               color = COALESCE($3, color),
               permissions = COALESCE($4, permissions),
               position = COALESCE($5, position)
           WHERE id = $1
           RETURNING id, guild_id, name, color, permissions, position, is_default, created_at"#,
        role_id,
        name,
        color,
        permissions.map(|p| p.to_db()),
        position
    )
    .fetch_optional(pool)
    .await
}

/// Delete a guild role.
pub async fn delete_guild_role(pool: &PgPool, role_id: Uuid) -> sqlx::Result<bool> {
    let result = sqlx::query!(
        r#"DELETE FROM guild_roles WHERE id = $1 AND is_default = false"#,
        role_id
    )
    .execute(pool)
    .await?;

    Ok(result.rows_affected() > 0)
}

/// Create default roles for a new guild.
pub async fn create_default_roles(pool: &PgPool, guild_id: Uuid) -> sqlx::Result<()> {
    // @everyone
    create_guild_role(
        pool,
        guild_id,
        "@everyone",
        None,
        GuildPermissions::EVERYONE_DEFAULT,
        999,
        true,
    )
    .await?;

    // Moderator
    create_guild_role(
        pool,
        guild_id,
        "Moderator",
        Some("#3498db"),
        GuildPermissions::MODERATOR_DEFAULT,
        100,
        false,
    )
    .await?;

    // Officer
    create_guild_role(
        pool,
        guild_id,
        "Officer",
        Some("#e74c3c"),
        GuildPermissions::OFFICER_DEFAULT,
        50,
        false,
    )
    .await?;

    Ok(())
}

// ============================================================================
// Guild Member Role Queries
// ============================================================================

/// Get all roles for a guild member.
pub async fn get_member_roles(
    pool: &PgPool,
    guild_id: Uuid,
    user_id: Uuid,
) -> sqlx::Result<Vec<GuildRole>> {
    sqlx::query_as!(
        GuildRole,
        r#"SELECT r.id, r.guild_id, r.name, r.color, r.permissions, r.position, r.is_default, r.created_at
           FROM guild_roles r
           JOIN guild_member_roles mr ON r.id = mr.role_id
           WHERE mr.guild_id = $1 AND mr.user_id = $2
           ORDER BY r.position ASC"#,
        guild_id,
        user_id
    )
    .fetch_all(pool)
    .await
}

/// Assign a role to a guild member.
pub async fn assign_member_role(
    pool: &PgPool,
    guild_id: Uuid,
    user_id: Uuid,
    role_id: Uuid,
    assigned_by: Option<Uuid>,
) -> sqlx::Result<()> {
    sqlx::query!(
        r#"INSERT INTO guild_member_roles (guild_id, user_id, role_id, assigned_by)
           VALUES ($1, $2, $3, $4)
           ON CONFLICT (guild_id, user_id, role_id) DO NOTHING"#,
        guild_id,
        user_id,
        role_id,
        assigned_by
    )
    .execute(pool)
    .await?;

    Ok(())
}

/// Remove a role from a guild member.
pub async fn remove_member_role(
    pool: &PgPool,
    guild_id: Uuid,
    user_id: Uuid,
    role_id: Uuid,
) -> sqlx::Result<bool> {
    let result = sqlx::query!(
        r#"DELETE FROM guild_member_roles
           WHERE guild_id = $1 AND user_id = $2 AND role_id = $3"#,
        guild_id,
        user_id,
        role_id
    )
    .execute(pool)
    .await?;

    Ok(result.rows_affected() > 0)
}

/// Get highest (lowest position number) role for a member.
pub async fn get_member_highest_role(
    pool: &PgPool,
    guild_id: Uuid,
    user_id: Uuid,
) -> sqlx::Result<Option<i32>> {
    let result = sqlx::query_scalar!(
        r#"SELECT MIN(r.position) as position
           FROM guild_roles r
           JOIN guild_member_roles mr ON r.id = mr.role_id
           WHERE mr.guild_id = $1 AND mr.user_id = $2"#,
        guild_id,
        user_id
    )
    .fetch_one(pool)
    .await?;

    Ok(result)
}

// ============================================================================
// Channel Override Queries
// ============================================================================

/// Get all overrides for a channel.
pub async fn get_channel_overrides(
    pool: &PgPool,
    channel_id: Uuid,
) -> sqlx::Result<Vec<ChannelOverride>> {
    sqlx::query_as!(
        ChannelOverride,
        r#"SELECT id, channel_id, role_id, allow_permissions, deny_permissions
           FROM channel_overrides
           WHERE channel_id = $1"#,
        channel_id
    )
    .fetch_all(pool)
    .await
}

/// Set channel override for a role.
pub async fn set_channel_override(
    pool: &PgPool,
    channel_id: Uuid,
    role_id: Uuid,
    allow: GuildPermissions,
    deny: GuildPermissions,
) -> sqlx::Result<ChannelOverride> {
    sqlx::query_as!(
        ChannelOverride,
        r#"INSERT INTO channel_overrides (channel_id, role_id, allow_permissions, deny_permissions)
           VALUES ($1, $2, $3, $4)
           ON CONFLICT (channel_id, role_id) DO UPDATE SET
               allow_permissions = $3,
               deny_permissions = $4
           RETURNING id, channel_id, role_id, allow_permissions, deny_permissions"#,
        channel_id,
        role_id,
        allow.to_db(),
        deny.to_db()
    )
    .fetch_one(pool)
    .await
}

/// Delete channel override.
pub async fn delete_channel_override(
    pool: &PgPool,
    channel_id: Uuid,
    role_id: Uuid,
) -> sqlx::Result<bool> {
    let result = sqlx::query!(
        r#"DELETE FROM channel_overrides WHERE channel_id = $1 AND role_id = $2"#,
        channel_id,
        role_id
    )
    .execute(pool)
    .await?;

    Ok(result.rows_affected() > 0)
}

// ============================================================================
// Audit Log Queries
// ============================================================================

/// Write an audit log entry.
pub async fn write_audit_log(
    pool: &PgPool,
    actor_id: Uuid,
    action: &str,
    target_type: Option<&str>,
    target_id: Option<Uuid>,
    details: Option<serde_json::Value>,
    ip_address: Option<&str>,
) -> sqlx::Result<AuditLogEntry> {
    sqlx::query_as!(
        AuditLogEntry,
        r#"INSERT INTO system_audit_log (actor_id, action, target_type, target_id, details, ip_address)
           VALUES ($1, $2, $3, $4, $5, $6::inet)
           RETURNING id, actor_id, action, target_type, target_id, details,
                     ip_address::text as ip_address, created_at"#,
        actor_id,
        action,
        target_type,
        target_id,
        details,
        ip_address
    )
    .fetch_one(pool)
    .await
}

/// Get audit log entries with pagination.
pub async fn get_audit_log(
    pool: &PgPool,
    limit: i64,
    offset: i64,
    action_filter: Option<&str>,
) -> sqlx::Result<Vec<AuditLogEntry>> {
    sqlx::query_as!(
        AuditLogEntry,
        r#"SELECT id, actor_id, action, target_type, target_id, details,
                  ip_address::text as ip_address, created_at
           FROM system_audit_log
           WHERE ($3::text IS NULL OR action LIKE $3 || '%')
           ORDER BY created_at DESC
           LIMIT $1 OFFSET $2"#,
        limit,
        offset,
        action_filter
    )
    .fetch_all(pool)
    .await
}
```

**Step 2: Update mod.rs to export queries**

Modify `server/src/permissions/mod.rs`:

```rust
//! Permission system types and utilities.
//!
//! Two-tier permission model:
//! - System permissions: Platform-level admin actions
//! - Guild permissions: Per-guild role-based access control

pub mod guild;
pub mod models;
pub mod queries;
pub mod resolver;
pub mod system;

pub use guild::{GuildPermissions, EVERYONE_FORBIDDEN};
pub use models::*;
pub use queries::*;
pub use resolver::{
    can_manage_role, can_moderate_member, compute_guild_permissions, PermissionError,
};
pub use system::SystemPermission;
```

**Step 3: Build to verify**

```bash
cd /home/detair/GIT/canis/.worktrees/permission-system/server
cargo build --lib
```

Expected: Builds successfully.

**Step 4: Commit**

```bash
git add server/src/permissions/
git commit -m "feat(permissions): Add database queries

- System admin CRUD operations
- Elevated session management
- Guild role queries with default creation
- Member role assignment
- Channel override management
- Audit log writing and retrieval

Co-Authored-By: Claude Opus 4.5 <noreply@anthropic.com>"
```

---

## Summary & Next Steps

This plan covers the core permission system foundation:

1. **Task 1**: Database migration (all tables)
2. **Task 2**: Permission bitfield types
3. **Task 3**: Database models
4. **Task 4**: Permission resolution logic
5. **Task 5**: Database queries

**Remaining work (future tasks):**
- Task 6-8: API handlers for system admin
- Task 9-11: API handlers for guild roles
- Task 12-14: Break-glass emergency system
- Task 15+: Frontend components

---

## Execution Checklist

Before starting:
- [ ] Worktree at `/home/detair/GIT/canis/.worktrees/permission-system`
- [ ] Branch: `feature/permission-system`
- [ ] Database running locally

After each task:
- [ ] Tests pass (or build succeeds if no tests yet)
- [ ] Committed with descriptive message
