# Permission System API Handlers Design

**Date:** 2026-01-18
**Status:** Approved
**Builds on:** `permission-system-implementation-2026-01-13.md` (Tasks 1-5 complete)

## Overview

API handlers for the two-tier permission system: System Admin routes for platform management and Guild Role routes for per-guild access control.

## Design Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Admin route structure | Separate `/api/admin/...` prefix | Cleaner separation, easier to audit |
| Guild role routes | Nested under `/api/guilds/:id/roles` | REST conventions, clear context |
| Elevation requirement | Destructive actions only | Balance security and usability |
| Permission errors | Detailed JSON responses | Better UX and debugging |

---

## Route Structure

### System Admin Routes

Prefix: `/api/admin/...`
Middleware: `require_system_admin`

| Method | Path | Description | Elevation |
|--------|------|-------------|-----------|
| GET | `/users` | List all users (paginated) | No |
| GET | `/guilds` | List all guilds (paginated) | No |
| GET | `/audit-log` | View system audit log | No |
| POST | `/elevate` | Elevate session (requires MFA) | No |
| DELETE | `/elevate` | De-elevate session | No |
| POST | `/users/:id/ban` | Global ban user | **Yes** |
| DELETE | `/users/:id/ban` | Remove global ban | **Yes** |
| POST | `/guilds/:id/suspend` | Suspend guild | **Yes** |
| DELETE | `/guilds/:id/suspend` | Unsuspend guild | **Yes** |
| POST | `/announcements` | Create system announcement | **Yes** |

### Guild Role Routes

Prefix: `/api/guilds/:guild_id/...`
Requires: Guild membership + appropriate permissions

| Method | Path | Description | Permission |
|--------|------|-------------|------------|
| GET | `/roles` | List guild roles | Member |
| POST | `/roles` | Create role | MANAGE_ROLES |
| PATCH | `/roles/:role_id` | Update role | MANAGE_ROLES |
| DELETE | `/roles/:role_id` | Delete role | MANAGE_ROLES |
| POST | `/members/:user_id/roles/:role_id` | Assign role | MANAGE_ROLES |
| DELETE | `/members/:user_id/roles/:role_id` | Remove role | MANAGE_ROLES |

### Channel Override Routes

| Method | Path | Description | Permission |
|--------|------|-------------|------------|
| GET | `/api/channels/:id/overrides` | List overrides | Member |
| PUT | `/api/channels/:id/overrides/:role_id` | Set override | MANAGE_CHANNELS |
| DELETE | `/api/channels/:id/overrides/:role_id` | Remove override | MANAGE_CHANNELS |

---

## Permission Checking

### Single-Query Context Loading

```rust
pub struct MemberPermissionContext {
    pub guild_owner_id: Uuid,
    pub everyone_permissions: GuildPermissions,
    pub member_roles: Vec<GuildRole>,
    pub computed_permissions: GuildPermissions,
    pub highest_role_position: i32,
}

pub async fn get_member_permission_context(
    pool: &PgPool,
    guild_id: Uuid,
    user_id: Uuid,
) -> Result<Option<MemberPermissionContext>, sqlx::Error>
```

### Helper Function

```rust
pub async fn require_guild_permission(
    pool: &PgPool,
    guild_id: Uuid,
    user_id: Uuid,
    required: GuildPermissions,
) -> Result<MemberPermissionContext, PermissionError>
```

### Handler Usage

```rust
let ctx = require_guild_permission(
    &state.db,
    guild_id,
    auth.id,
    GuildPermissions::MANAGE_ROLES
).await?;

// Use ctx.highest_role_position for hierarchy checks
```

### Error Type

Extend existing `PermissionError` in `resolver.rs`:

```rust
pub enum PermissionError {
    // Existing variants
    MissingPermission(GuildPermissions),
    RoleHierarchy { actor_position: i32, target_position: i32 },
    CannotEscalate(GuildPermissions),
    CannotModerateOwner,

    // New variants
    NotGuildMember,
    ElevationRequired,
    NotSystemAdmin,
}
```

---

## Elevation Flow

### Endpoint: `POST /api/admin/elevate`

**Request:**
```json
{
    "mfa_code": "123456",
    "reason": "Investigating spam report #1234"
}
```

**Response (success):**
```json
{
    "elevated": true,
    "expires_at": "2026-01-18T15:30:00Z",
    "session_id": "uuid"
}
```

**Response (MFA not enabled):**
```json
{
    "error": "mfa_required",
    "message": "MFA must be enabled to elevate session"
}
```

### Rules

- Duration: 15 minutes (configurable via `system_settings`)
- Requires MFA enabled on account
- Logs elevation to `system_audit_log`
- Stores IP address for security review
- One elevated session per user session

### Middleware Types

```rust
pub struct SystemAdminUser {
    pub user_id: Uuid,
    pub username: String,
    pub granted_at: DateTime<Utc>,
}

pub struct ElevatedAdmin {
    pub user_id: Uuid,
    pub elevated_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub reason: Option<String>,
}
```

---

## Audit Logging

### Logged Actions

| Action | Target Type | Details |
|--------|-------------|---------|
| `system.session.elevate` | `user` | reason, ip_address |
| `system.session.de_elevate` | `user` | - |
| `system.users.ban` | `user` | reason, expires_at |
| `system.users.unban` | `user` | - |
| `system.guilds.suspend` | `guild` | reason |
| `system.guilds.unsuspend` | `guild` | - |
| `system.announcements.create` | `announcement` | title, severity |

### Audit Log Query

```
GET /api/admin/audit-log?limit=50&offset=0&action=system.users
```

**Response:**
```json
{
    "entries": [
        {
            "id": "uuid",
            "actor_id": "uuid",
            "actor_username": "admin1",
            "action": "system.users.ban",
            "target_type": "user",
            "target_id": "uuid",
            "details": {"reason": "Spam"},
            "ip_address": "192.168.1.1",
            "created_at": "2026-01-18T14:00:00Z"
        }
    ],
    "total": 142
}
```

---

## File Structure

```
server/src/
├── admin/
│   ├── mod.rs              # Router + re-exports
│   ├── handlers.rs         # System admin handlers
│   ├── middleware.rs       # require_system_admin, require_elevated
│   └── types.rs            # Request/response types, AdminError
│
├── guild/
│   ├── mod.rs              # (update router)
│   ├── handlers.rs         # (existing)
│   ├── roles.rs            # NEW: Role management handlers
│   └── types.rs            # (add role request types)
│
├── chat/
│   └── channels.rs         # (add override handlers to existing)
│
└── permissions/
    ├── mod.rs              # (update exports)
    ├── helpers.rs          # NEW: require_guild_permission, MemberPermissionContext
    └── ...                 # (existing files)
```

### Router Integration

```rust
// In api/mod.rs
let admin_routes = admin::router()
    .layer(from_fn_with_state(state.clone(), admin::require_system_admin));

Router::new()
    .nest("/api/admin", admin_routes)
    .merge(protected_routes)
    // ...
```

---

## Estimated Scope

| Component | Lines | Handlers |
|-----------|-------|----------|
| `admin/` module | ~400 | 8 |
| `guild/roles.rs` | ~250 | 5 |
| `chat/` override handlers | ~100 | 3 |
| `permissions/helpers.rs` | ~100 | - |
| Router updates | ~30 | - |
| **Total** | ~880 | 16 |
