<!-- Parent: ../AGENTS.md -->
# Admin Module

## Purpose

System administration module for platform-wide management operations. Implements a two-tier privilege model:

1. **Base Admin** - Read-only operations and session management
2. **Elevated Admin** - Destructive operations (requires MFA re-verification)

All operations are logged to the system audit log for compliance and security tracing.

## Key Files

- `mod.rs` - Router setup with middleware layers, public exports
- `users.rs` - User listing, details, ban/unban, delete, bulk ban, CSV export
- `guilds.rs` - Guild listing, details, suspend/unsuspend, delete, bulk suspend, CSV export, page limits
- `audit.rs` - Audit log query handler with filters
- `system.rs` - Status, stats, elevation, announcements, auth settings, OIDC providers
- `shared.rs` - Cross-cutting DTOs (`PaginationParams`, `PaginatedResponse`, `DeleteResponse`)
- `middleware.rs` - Authorization middleware (`require_system_admin`, `require_elevated`)
- `types.rs` - Request/response types and error definitions

## API Endpoints

### Base Admin Routes (require `SystemAdminUser`)

| Method | Path | Handler | Description |
|--------|------|---------|-------------|
| GET | `/health` | inline | Health check |
| GET | `/users` | `list_users` | Paginated user list with ban status |
| GET | `/guilds` | `list_guilds` | Paginated guild list with member counts |
| GET | `/audit-log` | `get_audit_log` | System audit log with action filtering |
| POST | `/elevate` | `elevate_session` | Elevate session (requires MFA) |
| DELETE | `/elevate` | `de_elevate_session` | De-elevate session |

### Elevated Routes (require `ElevatedAdmin`)

| Method | Path | Handler | Description |
|--------|------|---------|-------------|
| POST | `/users/:id/ban` | `ban_user` | Global ban a user |
| DELETE | `/users/:id/ban` | `unban_user` | Remove global ban |
| POST | `/guilds/:id/suspend` | `suspend_guild` | Suspend a guild |
| DELETE | `/guilds/:id/suspend` | `unsuspend_guild` | Unsuspend a guild |
| POST | `/announcements` | `create_announcement` | Create system announcement |

## For AI Agents

### Security Model

**Two-tier privilege escalation:**
```
User → JWT Auth → SystemAdminUser (full session) → step-up grant `admin.elevate` → ElevatedAdmin
```

- `SystemAdminUser` is extracted from `system_admins` table via `require_system_admin` middleware, which also requires a `full` session (C27: `/api/admin/*` is in the full row; probation sessions get 403 `FULL_SESSION_REQUIRED`, non-admins `NOT_ADMIN`). `/api/admin/status` is outside the guard.
- `ElevatedAdmin` requires active `elevated_sessions` entry (15-minute TTL) for the request's own session
- Elevation consumes an `admin.elevate` step-up grant (key phone or passkey, F7); no password or TOTP
- All destructive operations require elevation
- Ban, bulk ban and deletion lock the admin's and the subject's account rows first (B.7 rule 1), before the ban row and the sessions

### Critical Security Paths

- `middleware.rs:require_system_admin` - Verifies admin status from database and a full session
- `middleware.rs:require_elevated` - Checks for active elevated session
- `system.rs:elevate_session` - consumes the `admin.elevate` step-up grant

### Audit Logging

All admin actions are logged via `write_audit_log()`:
- `admin.session.elevated` - Session elevation
- `admin.session.de_elevated` - Session de-elevation
- `admin.users.ban` / `admin.users.unban` - User bans
- `admin.guilds.suspend` / `admin.guilds.unsuspend` - Guild suspensions
- `admin.announcements.create` - Announcements

### Database Tables

- `system_admins` - Admin user registry
- `elevated_sessions` - Active elevated sessions (FK to `sessions`)
- `global_bans` - Platform-wide user bans
- `system_audit_log` - Audit trail
- `system_announcements` - Platform announcements

### Error Handling

`AdminError` enum provides structured errors:
- `NotAdmin` (403) - User lacks admin privileges
- `ElevationRequired` (403) - Operation requires elevated session
- `MfaRequired` (400) - MFA must be enabled to elevate
- `InvalidMfaCode` (401) - TOTP verification failed
- `NotFound` (404) - Resource not found
- `Validation` (400) - Request validation failed

### Testing Considerations

- Test elevation flow with mock MFA (controlled secret)
- Verify elevated session expiry (15 minutes)
- Test audit log capture for all operations
- Verify self-ban prevention (`user_id == admin.user_id`)

## Dependencies

- `crate::auth::mfa_crypto` - MFA secret decryption
- `crate::permissions::queries` - Admin status, audit logging, elevated sessions
- `totp_rs` - TOTP verification
- `sqlx` - Database queries
