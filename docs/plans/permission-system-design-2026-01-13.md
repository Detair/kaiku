# Permission System Design

## Overview

A two-tier permission system for the VoiceChat platform, separating system-level administration from guild-level management. Designed with security-first principles including sudo-style elevation, privilege escalation prevention, and hardened emergency procedures.

**Date:** 2026-01-13
**Status:** Design Complete
**Phase:** 3 (Guild Architecture & Security)

---

## Architecture

### Two-Tier Permission Model

```
┌─────────────────────────────────────────────────────────┐
│                    SYSTEM LEVEL                         │
│  ┌─────────────────────────────────────────────────┐   │
│  │  System Admin                                    │   │
│  │  • Assign/remove Guild Owners                   │   │
│  │  • Suspend/delete guilds                        │   │
│  │  • Global user bans                             │   │
│  │  • View all audit logs                          │   │
│  │  • Send system announcements                    │   │
│  │  • NO guild internal settings                   │   │
│  └─────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────┘
                          │
                          ▼
┌─────────────────────────────────────────────────────────┐
│                    GUILD LEVEL                          │
│  ┌────────────────────────────────────────────────┐    │
│  │ Guild Owner (pos 1) ──inherits──▶ Officer      │    │
│  │      │                              │          │    │
│  │      │                              ▼          │    │
│  │      │                         Moderator       │    │
│  │      │                              │          │    │
│  │      │                              ▼          │    │
│  │      └──────────────────────▶  @everyone       │    │
│  └────────────────────────────────────────────────┘    │
│                                                         │
│  Channel Overrides: Allow/Deny per role per channel    │
└─────────────────────────────────────────────────────────┘
```

### Key Principles

- **System Admin operates outside guilds** - Emergency/platform powers only
- **Guild Owner has full control within their guild** - Cannot affect other guilds
- **Hierarchical inheritance** - Higher roles inherit permissions from lower roles
- **Channel overrides** - Can Allow or Deny specific permissions per channel
- **Deny wins** - If both Allow and Deny are set, Deny takes precedence

---

## Role Hierarchy

| Position | Role | Scope | Key Permissions |
|----------|------|-------|-----------------|
| 0 | **Admin** | System | Global permissions, assign Guild Owners, delete/suspend guilds, global bans, view audit logs - cannot change Guild internal settings |
| 1 | **Guild Owner** | Guild | All guild settings, assign new Guild Owner |
| 2 | **Officer** | Guild | Manage channels, kick/timeout, moderate content |
| 3 | **Moderator** | Guild | Delete messages, mute in voice, timeout users |
| 4 | **@everyone** | Guild | Send messages, connect to voice, view channels |

---

## Permission Definitions

### System-Level Permissions

| Permission | Description |
|------------|-------------|
| `system.guilds.view` | View all guilds on platform |
| `system.guilds.suspend` | Suspend a guild (disables access) |
| `system.guilds.delete` | Permanently delete a guild |
| `system.guilds.assign_owner` | Assign/transfer guild ownership |
| `system.users.ban` | Ban user from entire platform |
| `system.users.view` | View all users and their guilds |
| `system.audit.view` | View platform-wide audit log |
| `system.announcements.send` | Create system-wide announcements |

### Guild-Level Permissions

| Category | Permission | @everyone | Mod | Officer | Owner |
|----------|------------|:---------:|:---:|:-------:|:-----:|
| **Content** | `send_messages` | ✓ | ✓ | ✓ | ✓ |
| | `embed_links` | ✓ | ✓ | ✓ | ✓ |
| | `attach_files` | ✓ | ✓ | ✓ | ✓ |
| | `use_emoji` | ✓ | ✓ | ✓ | ✓ |
| | `add_reactions` | ✓ | ✓ | ✓ | ✓ |
| **Voice** | `voice_connect` | ✓ | ✓ | ✓ | ✓ |
| | `voice_speak` | ✓ | ✓ | ✓ | ✓ |
| | `voice_mute_others` | | ✓ | ✓ | ✓ |
| | `voice_deafen_others` | | ✓ | ✓ | ✓ |
| | `voice_move_members` | | | ✓ | ✓ |
| **Moderation** | `manage_messages` | | ✓ | ✓ | ✓ |
| | `timeout_members` | | ✓ | ✓ | ✓ |
| | `kick_members` | | | ✓ | ✓ |
| | `ban_members` | | | ✓ | ✓ |
| **Guild Mgmt** | `manage_channels` | | | ✓ | ✓ |
| | `manage_roles` | | | ✓ | ✓ |
| | `view_audit_log` | | | ✓ | ✓ |
| | `manage_guild` | | | | ✓ |
| | `transfer_ownership` | | | | ✓ |
| **Invites** | `create_invite` | ✓ | ✓ | ✓ | ✓ |
| | `manage_invites` | | | ✓ | ✓ |

### Permission Bitfield

Permissions stored as BIGINT (64 bits) for efficient storage and fast checks:

```rust
pub struct Permissions(u64);

const SEND_MESSAGES:      u64 = 1 << 0;
const EMBED_LINKS:        u64 = 1 << 1;
const ATTACH_FILES:       u64 = 1 << 2;
const USE_EMOJI:          u64 = 1 << 3;
const ADD_REACTIONS:      u64 = 1 << 4;
const VOICE_CONNECT:      u64 = 1 << 5;
const VOICE_SPEAK:        u64 = 1 << 6;
const VOICE_MUTE_OTHERS:  u64 = 1 << 7;
const VOICE_DEAFEN_OTHERS: u64 = 1 << 8;
const VOICE_MOVE_MEMBERS: u64 = 1 << 9;
const MANAGE_MESSAGES:    u64 = 1 << 10;
const TIMEOUT_MEMBERS:    u64 = 1 << 11;
const KICK_MEMBERS:       u64 = 1 << 12;
const BAN_MEMBERS:        u64 = 1 << 13;
const MANAGE_CHANNELS:    u64 = 1 << 14;
const MANAGE_ROLES:       u64 = 1 << 15;
const VIEW_AUDIT_LOG:     u64 = 1 << 16;
const MANAGE_GUILD:       u64 = 1 << 17;
const TRANSFER_OWNERSHIP: u64 = 1 << 18;
const CREATE_INVITE:      u64 = 1 << 19;
const MANAGE_INVITES:     u64 = 1 << 20;
```

---

## Permission Resolution Logic

### System Permissions

```rust
pub fn check_system_permission(
    user_id: Uuid,
    session: &Session,
    required: SystemPermission,
) -> bool {
    // 1. Is user a system admin?
    let is_admin = is_system_admin(user_id);
    if !is_admin {
        return false;
    }

    // 2. Is session elevated? (sudo-style)
    let is_elevated = has_elevated_session(session.id);
    if !is_elevated {
        return false;  // Must elevate first
    }

    // 3. System admins have all system permissions
    true
}
```

### Guild Permissions

```rust
pub fn compute_guild_permissions(
    user_id: Uuid,
    guild: &Guild,
    channel: Option<&Channel>,
    user_roles: &[GuildRole],
) -> Permissions {
    // Guild owner has everything
    if guild.owner_id == user_id {
        return Permissions::all();
    }

    // Start with @everyone
    let mut perms = guild.everyone_permissions;

    // Add role permissions (hierarchical)
    for role in user_roles.iter().sorted_by_key(|r| r.position) {
        perms |= role.permissions;
    }

    // Apply channel overrides
    if let Some(ch) = channel {
        for role in user_roles {
            if let Some(ovr) = ch.get_override(role.id) {
                perms |= ovr.allow;
                perms &= !ovr.deny;
            }
        }
    }

    perms
}
```

---

## Privilege Escalation Prevention

### Attack Vectors and Mitigations

| Attack | Mitigation |
|--------|------------|
| User assigns themselves a higher role | **Role hierarchy check**: Can only assign roles *below* your highest role |
| Officer creates role with more perms than they have | **Permission ceiling**: New roles can only have perms you already possess |
| Officer edits existing higher role | **Position lock**: Can only edit roles below your position |
| Kick/ban guild owner | **Owner immunity**: Owner cannot be kicked/banned/demoted by guild members |
| Demote yourself to escape audit | **Self-demotion audit**: All role changes logged, even self-changes |
| Compromise one admin, escalate everywhere | **Guild isolation**: Guild roles have zero effect on other guilds |
| Abuse elevated session indefinitely | **Session timeout**: Elevated sessions expire after 30 min |
| Replay old elevated session token | **Session binding**: Elevation tied to specific session ID, invalidated on logout |

### Enforcement Code

```rust
pub fn can_manage_role(
    actor_perms: Permissions,
    actor_highest_position: i32,
    target_role: &GuildRole,
    new_permissions: Option<Permissions>,
) -> Result<(), PermissionError> {
    // 1. Must have MANAGE_ROLES permission
    if !actor_perms.has(MANAGE_ROLES) {
        return Err(PermissionError::MissingPermission);
    }

    // 2. Cannot edit roles at or above your position
    if target_role.position <= actor_highest_position {
        return Err(PermissionError::RoleHierarchy);
    }

    // 3. Cannot grant permissions you don't have
    if let Some(new_perms) = new_permissions {
        let escalation = new_perms & !actor_perms;
        if escalation.0 != 0 {
            return Err(PermissionError::CannotEscalate);
        }
    }

    Ok(())
}

pub fn can_moderate_member(
    actor: &GuildMember,
    target: &GuildMember,
    guild: &Guild,
) -> Result<(), PermissionError> {
    // Cannot moderate guild owner
    if target.user_id == guild.owner_id {
        return Err(PermissionError::CannotModerateOwner);
    }

    // Cannot moderate someone with higher/equal role
    if target.highest_role_position <= actor.highest_role_position {
        return Err(PermissionError::RoleHierarchy);
    }

    Ok(())
}
```

### Forbidden Permissions for @everyone

```rust
const EVERYONE_FORBIDDEN: Permissions =
    MANAGE_ROLES | MANAGE_GUILD | KICK_MEMBERS | BAN_MEMBERS;

pub fn validate_everyone_permissions(perms: Permissions) -> Result<(), Error> {
    if (perms & EVERYONE_FORBIDDEN).0 != 0 {
        return Err(Error::ForbiddenForEveryone);
    }
    Ok(())
}
```

---

## Security Configuration

### Three-Tier Security Model

```
┌─────────────────────────────────────────────────────────┐
│  TIER 1: ESSENTIAL (Always On)                          │
│  ─────────────────────────────                          │
│  • IP binding for elevated sessions                     │
│  • Rate limit: 3 elevation attempts / 15 min            │
│  • Email notification on elevation                      │
└─────────────────────────────────────────────────────────┘
                          │
                          ▼
┌─────────────────────────────────────────────────────────┐
│  TIER 2: HIGH SECURITY (Admin Toggle)                   │
│  ─────────────────────────────────                      │
│  • Re-auth MFA for destructive actions                  │
│  • Inactivity timeout (5 min no admin action)           │
│                                                         │
│  Toggle: /api/admin/settings                            │
│  { "require_reauth_destructive": true,                  │
│    "inactivity_timeout_minutes": 5 }                    │
└─────────────────────────────────────────────────────────┘
                          │
                          ▼
┌─────────────────────────────────────────────────────────┐
│  TIER 3: MAXIMUM (Admin + Guild Owner Toggle)           │
│  ─────────────────────────────────────────              │
│  System Admin settings:                                 │
│  • Dual admin approval for global bans                  │
│  • WebAuthn required for elevation                      │
│                                                         │
│  Guild Owner settings:                                  │
│  • Dual owner approval for guild deletion               │
│  • WebAuthn required for ownership transfer             │
└─────────────────────────────────────────────────────────┘
```

### Cooling-Off Period

Destructive actions are delayed before execution (configurable: 1-24 hours, default 4h):

```
Owner A requests guild deletion
         │
         ▼
┌─────────────────────────────┐
│ Dual approval required?     │──NO──▶ Apply cooling-off
└──────────────┬──────────────┘        (default: 4h)
               │ YES                         │
               ▼                             │
       Wait for Owner B                      │
               │                             │
               ▼                             │
┌─────────────────────────────┐              │
│ Both approved               │◀─────────────┘
└──────────────┬──────────────┘
               │
               ▼
┌─────────────────────────────┐
│ Cooling-off period starts   │
│ (configurable: 1-24 hours)  │
└──────────────┬──────────────┘
               │
               ▼
┌─────────────────────────────┐
│ During cooling-off:         │
│ • Action visible as pending │
│ • Any approver can CANCEL   │
│ • Notification sent         │
└──────────────┬──────────────┘
               │
               ▼ (timer expires)
┌─────────────────────────────┐
│ Execute action              │
│ Log to audit                │
└─────────────────────────────┘
```

---

## Hardened Break-Glass Emergency

For situations where normal dual-approval flow is blocked but urgent action is needed.

### Security Layers

```
┌─────────────────────────────────────────────────────────┐
│  BREAK-GLASS SECURITY LAYERS                            │
└─────────────────────────────────────────────────────────┘

Layer 1: AUTHENTICATION
├── Must be system admin
├── Must have elevated session
├── Must re-verify MFA (fresh code, not cached)
└── Optional: Require WebAuthn instead of TOTP

Layer 2: RATE LIMITING
├── Max 1 break-glass per admin per 24 hours
├── Max 3 break-glass system-wide per 24 hours
└── 1-hour cooldown between any break-glass actions

Layer 3: DELAY + NOTIFICATION
├── 15-minute delay before execution (short cooling-off)
├── Immediate notification to ALL other system admins
├── During delay: any other admin can BLOCK with reason
└── Override delay only with 2nd admin approval

Layer 4: AUDIT + ACCOUNTABILITY
├── Mandatory justification (min 50 chars)
├── Optional incident ticket reference
├── Full action logged to tamper-evident audit
└── Triggers automatic security review flag

Layer 5: POST-EXECUTION
├── Email summary to all admins + configured addresses
├── Action marked for mandatory review within 48h
└── Break-glass privileges suspended if review fails
```

### Break-Glass Flow

```
Admin requests break-glass
         │
         ▼
┌─────────────────────────────┐
│ Pre-flight checks:          │
│ • Is admin elevated? ✓      │
│ • Cooldown passed? ✓        │
│ • Rate limit OK? ✓          │
└──────────────┬──────────────┘
               │
               ▼
┌─────────────────────────────┐
│ MFA Challenge               │
│ (WebAuthn if configured,    │
│  otherwise fresh TOTP)      │
└──────────────┬──────────────┘
               │ ✓ verified
               ▼
┌─────────────────────────────┐
│ Validate justification      │
│ (min 50 chars, no template) │
└──────────────┬──────────────┘
               │
               ▼
┌─────────────────────────────┐
│ Create pending break-glass  │
│ Status: WAITING             │
│ Execute at: NOW + 15 min    │
└──────────────┬──────────────┘
               │
               ▼
┌─────────────────────────────┐
│ IMMEDIATE NOTIFICATION      │
│ • All system admins         │
│ • External webhook (Slack)  │
│ • Email to security team    │
└──────────────┬──────────────┘
               │
      ┌────────┴────────┐
      │   15 min wait   │
      └────────┬────────┘
               │
      ┌────────▼────────┐
      │ During wait:    │
      │ • Other admin   │◀── POST /admin/break-glass/:id/block
      │   can BLOCK     │    { reason: "..." }
      │ • Requester can │
      │   CANCEL        │
      └────────┬────────┘
               │ (no block received)
               ▼
┌─────────────────────────────┐
│ EXECUTE ACTION              │
│ Log everything              │
│ Flag for 48h review         │
└─────────────────────────────┘
```

### Instant Override (True Emergency)

Skip the 15-min delay only with additional approval:

- 2nd admin approval within 5-min window, OR
- Pre-configured emergency contact confirms via external channel

---

## Database Schema

### Core Permission Tables

```sql
-- System-level admin users (separate from guild roles)
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
    expires_at TIMESTAMPTZ NOT NULL,  -- 30 min from elevation
    reason VARCHAR(255),
    UNIQUE(session_id)
);

-- Guild roles (replaces simple roles table)
CREATE TABLE guild_roles (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    guild_id UUID NOT NULL REFERENCES guilds(id) ON DELETE CASCADE,
    name VARCHAR(64) NOT NULL,
    color VARCHAR(7),
    permissions BIGINT NOT NULL DEFAULT 0,  -- bitfield
    position INTEGER NOT NULL DEFAULT 0,
    is_default BOOLEAN NOT NULL DEFAULT FALSE,  -- @everyone
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE(guild_id, name)
);

-- Channel permission overrides
CREATE TABLE channel_overrides (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    channel_id UUID NOT NULL REFERENCES channels(id) ON DELETE CASCADE,
    role_id UUID NOT NULL REFERENCES guild_roles(id) ON DELETE CASCADE,
    allow BIGINT NOT NULL DEFAULT 0,
    deny BIGINT NOT NULL DEFAULT 0,
    UNIQUE(channel_id, role_id)
);

-- Guild member roles junction
CREATE TABLE guild_member_roles (
    guild_id UUID NOT NULL REFERENCES guilds(id) ON DELETE CASCADE,
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    role_id UUID NOT NULL REFERENCES guild_roles(id) ON DELETE CASCADE,
    assigned_by UUID REFERENCES users(id),
    assigned_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (guild_id, user_id, role_id)
);
```

### Settings and Audit Tables

```sql
-- System security settings
CREATE TABLE system_settings (
    key VARCHAR(64) PRIMARY KEY,
    value JSONB NOT NULL,
    updated_by UUID REFERENCES users(id),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Default settings
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

-- Guild security settings
ALTER TABLE guilds ADD COLUMN security_settings JSONB NOT NULL DEFAULT '{
    "require_dual_owner_delete": false,
    "require_webauthn_transfer": false,
    "cooling_off_hours": 4
}';

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

-- System announcements
CREATE TABLE system_announcements (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    author_id UUID NOT NULL REFERENCES users(id),
    title VARCHAR(128) NOT NULL,
    content TEXT NOT NULL,
    severity VARCHAR(16) NOT NULL DEFAULT 'info',  -- info, warning, critical
    active BOOLEAN NOT NULL DEFAULT TRUE,
    starts_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    ends_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
```

### Approval and Break-Glass Tables

```sql
-- Pending approvals (for dual approval flow)
CREATE TABLE pending_approvals (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    action_type VARCHAR(64) NOT NULL,
    target_type VARCHAR(32) NOT NULL,
    target_id UUID NOT NULL,
    requested_by UUID NOT NULL REFERENCES users(id),
    approved_by UUID REFERENCES users(id),
    status VARCHAR(16) NOT NULL DEFAULT 'pending',
        -- pending, approved, rejected, cancelled, expired, executed
    execute_after TIMESTAMPTZ,  -- cooling-off period
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
        -- waiting, blocked, cancelled, executed, expired
    execute_at TIMESTAMPTZ NOT NULL,
    blocked_by UUID REFERENCES users(id),
    block_reason TEXT,
    executed_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_break_glass_status ON break_glass_requests(status, execute_at);

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
        -- pending, approved, flagged
    notes TEXT,
    due_at TIMESTAMPTZ NOT NULL,
    reviewed_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_bg_reviews_pending ON break_glass_reviews(status, due_at)
    WHERE status = 'pending';
```

---

## API Endpoints

### System Admin APIs

```
# Elevation (sudo-style)
POST   /api/admin/elevate              Elevate session (requires MFA)
POST   /api/admin/de-elevate           Drop elevated privileges
GET    /api/admin/session-status       Check if currently elevated

# Guild management (requires elevation)
GET    /api/admin/guilds               List all guilds
POST   /api/admin/guilds/:id/suspend   Suspend guild
POST   /api/admin/guilds/:id/unsuspend Unsuspend guild
DELETE /api/admin/guilds/:id           Delete guild
POST   /api/admin/guilds/:id/owner     Assign new owner

# User management (requires elevation)
GET    /api/admin/users                List all users
POST   /api/admin/users/:id/ban        Global ban
DELETE /api/admin/users/:id/ban        Remove global ban

# Announcements (requires elevation)
GET    /api/admin/announcements        List announcements
POST   /api/admin/announcements        Create announcement
PATCH  /api/admin/announcements/:id    Update announcement
DELETE /api/admin/announcements/:id    Delete announcement

# Audit log (requires elevation)
GET    /api/admin/audit                View system audit log

# Settings (requires elevation)
GET    /api/admin/settings             Get system settings
PATCH  /api/admin/settings             Update system settings

# Break-glass (requires elevation + MFA)
POST   /api/admin/break-glass          Request break-glass action
GET    /api/admin/break-glass          List pending break-glass requests
POST   /api/admin/break-glass/:id/block   Block a break-glass request
POST   /api/admin/break-glass/:id/cancel  Cancel own break-glass request
GET    /api/admin/break-glass/log      View break-glass history
GET    /api/admin/break-glass/reviews  List pending reviews
POST   /api/admin/break-glass/reviews/:id  Submit review
```

### Guild Role APIs

```
# Roles
GET    /api/guilds/:id/roles               List guild roles
POST   /api/guilds/:id/roles               Create role
PATCH  /api/guilds/:id/roles/:rid          Update role
DELETE /api/guilds/:id/roles/:rid          Delete role
POST   /api/guilds/:id/roles/reorder       Reorder roles

# Member roles
GET    /api/guilds/:id/members/:uid/roles      Get member's roles
PUT    /api/guilds/:id/members/:uid/roles      Set member's roles
POST   /api/guilds/:id/members/:uid/roles/:rid Add role to member
DELETE /api/guilds/:id/members/:uid/roles/:rid Remove role from member

# Channel overrides
GET    /api/channels/:id/overrides             List channel overrides
PUT    /api/channels/:id/overrides/:rid        Set override for role
DELETE /api/channels/:id/overrides/:rid        Remove override

# Guild security settings (owner only)
GET    /api/guilds/:id/settings                Get guild settings
PATCH  /api/guilds/:id/settings                Update guild settings

# Approvals
GET    /api/approvals/pending                  List pending actions (in cooling-off)
POST   /api/approvals/:id/approve              Approve pending action
POST   /api/approvals/:id/reject               Reject pending action
POST   /api/approvals/:id/cancel               Cancel during cooling-off
```

---

## Implementation Notes

### Default Roles for New Guilds

When a guild is created, automatically create:

1. **@everyone** (is_default=true, position=999)
   - Permissions: `SEND_MESSAGES | EMBED_LINKS | ATTACH_FILES | USE_EMOJI | ADD_REACTIONS | VOICE_CONNECT | VOICE_SPEAK | CREATE_INVITE`

2. **Moderator** (position=100)
   - Permissions: @everyone + `MANAGE_MESSAGES | TIMEOUT_MEMBERS | VOICE_MUTE_OTHERS | VOICE_DEAFEN_OTHERS`

3. **Officer** (position=50)
   - Permissions: Moderator + `KICK_MEMBERS | BAN_MEMBERS | MANAGE_CHANNELS | MANAGE_ROLES | VIEW_AUDIT_LOG | MANAGE_INVITES | VOICE_MOVE_MEMBERS`

Guild owner implicitly has all permissions (checked by `guild.owner_id == user_id`).

### Audit Log Events

All permission-related actions should be logged:

- `role.create`, `role.update`, `role.delete`
- `role.assign`, `role.remove`
- `channel_override.set`, `channel_override.remove`
- `member.kick`, `member.ban`, `member.timeout`
- `guild.settings_update`
- `admin.elevate`, `admin.de_elevate`
- `admin.guild_suspend`, `admin.guild_delete`
- `admin.user_ban`
- `break_glass.request`, `break_glass.block`, `break_glass.execute`

---

## Next Steps

1. Create database migration for new tables
2. Implement permission bitfield types in Rust
3. Add permission check middleware to API routes
4. Build elevation flow with MFA
5. Create guild role management UI
6. Implement channel override UI
7. Add admin dashboard for system management
8. Write comprehensive tests for permission logic
