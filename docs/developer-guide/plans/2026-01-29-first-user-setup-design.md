# First User Setup (Admin Bootstrap) - Design Document

**Date:** 2026-01-29
**Status:** Approved
**Phase:** 4 - Advanced Features

---

## Overview

When a fresh Kaiku server starts with zero users, the first registered user automatically receives system admin permissions. After login, they must complete a mandatory setup wizard to configure the server before the platform is considered production-ready.

## Goals

- **Security:** First user gets admin automatically in a race-condition-free way
- **Proper Configuration:** Mandatory wizard ensures server is configured before use
- **Takeover Prevention:** Setup wizard cannot be re-triggered after completion
- **Good UX:** Clear messaging for first user, streamlined setup flow

## Architecture

### Database Schema

**New Table: `server_config`**

Stores server-level configuration (distinct from `system_settings` which is for security policies).

```sql
CREATE TABLE server_config (
    key VARCHAR(64) PRIMARY KEY,
    value JSONB NOT NULL,
    updated_by UUID REFERENCES users(id) ON DELETE SET NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Initial values
INSERT INTO server_config (key, value) VALUES
    ('setup_complete', 'false'::jsonb),
    ('server_name', '"Kaiku Server"'::jsonb),
    ('registration_policy', '"open"'::jsonb),  -- 'open', 'invite_only', 'closed'
    ('terms_url', 'null'::jsonb),
    ('privacy_url', 'null'::jsonb);

CREATE INDEX idx_server_config_key ON server_config(key);
```

**Existing Table: `system_admins`**

First user gets an entry here automatically during registration.

```sql
-- Already exists from 20260113000001_permission_system.sql
CREATE TABLE system_admins (
    user_id UUID PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    granted_by UUID REFERENCES users(id) ON DELETE SET NULL,
    granted_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
```

### Registration Flow - Race Condition Prevention

**Transaction with Table Lock:**

1. BEGIN TRANSACTION with serializable isolation
2. `SELECT COUNT(*) FROM users FOR UPDATE` — lock users table before checking count
3. If count == 0: `is_first_user = true`
4. Validate username/email uniqueness
5. Hash password
6. `INSERT INTO users`
7. If `is_first_user`: `INSERT INTO system_admins (user_id, granted_by) VALUES (new_user_id, new_user_id)`
8. Create session
9. COMMIT — all or nothing

**Concurrency Guarantee:**

If two users register simultaneously:
- First transaction locks table, completes, gets admin
- Second transaction blocks on `FOR UPDATE`, then sees count=1, does NOT get admin

### API Endpoints

#### 1. `GET /api/setup/status`

**Auth:** None (public)
**Purpose:** Check if server setup is complete

**Response:**
```json
{
  "setup_complete": true
}
```

**Used by:** Registration page to show "You'll be the first admin" message when false.

---

#### 2. `GET /api/setup/wizard`

**Auth:** Required (JWT)
**Authorization:**
- User must be in `system_admins` table
- `setup_complete` must be `false`

**Response on Success (200):**
```json
{
  "current_server_name": "Kaiku Server",
  "current_registration_policy": "open"
}
```

**Response on Failure:**
- 401 if not authenticated
- 403 if not system admin OR setup already complete
- Used by frontend to determine if wizard should be shown

---

#### 3. `POST /api/setup/complete`

**Auth:** Required (JWT)
**Authorization:**
- User must be in `system_admins` table
- `setup_complete` must be `false` (checked with `FOR UPDATE` lock)

**Request Body:**
```json
{
  "server_name": "My Gaming Server",
  "registration_policy": "invite_only",
  "terms_url": "https://example.com/terms",
  "privacy_url": "https://example.com/privacy"
}
```

**Validation:**
- `server_name`: 1-64 characters
- `registration_policy`: must be "open", "invite_only", or "closed"
- `terms_url`, `privacy_url`: valid URLs or null

**Transaction:**
1. Check user is admin
2. Check `setup_complete = false` with `FOR UPDATE` (prevents concurrent completion)
3. Update `server_config` values for all provided fields
4. Set `setup_complete = true` (IRREVERSIBLE)
5. Create audit log entry
6. COMMIT

**Response:**
```json
{
  "message": "Setup complete"
}
```

**Errors:**
- 401 if not authenticated
- 403 if not admin or setup already complete
- 400 if validation fails

---

### Client Flow

#### Login → Setup Check

```typescript
// After successful login
const response = await tauri.login(username, password);
setAuthState({ user: response.user, token: response.access_token });

// Check if setup wizard needed
try {
    const wizardConfig = await tauri.getWizardConfig();  // GET /api/setup/wizard
    if (wizardConfig) {
        setShowSetupWizard(true);  // Show mandatory modal
    } else {
        navigate('/home');
    }
} catch (err) {
    if (err.status === 403) {
        // Setup already complete or not admin
        navigate('/home');
    } else {
        throw err;
    }
}
```

#### Mandatory Setup Wizard

**Component:** `client/src/components/admin/SetupWizard.tsx`

**Characteristics:**
- Full-screen modal overlay
- **No close button** - cannot be dismissed
- **No Escape key handler** - cannot be closed
- **No click-outside-to-close**
- Only "Logout" button available to exit without completing

**Steps:**

**Step 1: Server Name**
- Text input (1-64 chars)
- Pre-filled with current value from config
- Used in page titles, emails, invite links

**Step 2: Registration Policy**
- Radio button group:
  - **Open** - Anyone can register
  - **Invite Only** - Requires invite code (recommended for self-hosted)
  - **Closed** - Registration disabled
- Forces explicit choice (no skip)

**Step 3: Optional Links**
- Terms of Service URL (optional)
- Privacy Policy URL (optional)
- Can leave blank and skip

**Completion:**
- Calls `POST /api/setup/complete` with all values
- On success: closes wizard, redirects to `/home`
- On error: shows error message, allows retry

### Wizard Interruption Handling

**Scenario:** User completes registration, sees wizard, closes browser mid-setup.

**Behavior:**
- `setup_complete` remains `false`
- Next login → `GET /api/setup/wizard` returns 200 → wizard appears again
- No timeout or expiry — setup stays incomplete indefinitely
- Admin can repeat this cycle (logout, login, see wizard) until completion

**Rationale:** Server should not be "production ready" until properly configured. Better to nag the admin than have an improperly configured server.

### Security Guarantees

#### 1. Only First User Gets Admin
- Table lock in registration transaction prevents race conditions
- Two concurrent registrations: first one locks table, gets admin; second one waits, does not get admin

#### 2. Setup Wizard Cannot Be Re-Triggered
- Once `setup_complete = true`, it's permanent (no UPDATE back to false in code)
- `GET /api/setup/wizard` returns 403 after completion, even for admins
- `POST /api/setup/complete` returns 403 after completion
- Frontend never renders wizard when setup complete

#### 3. Non-Admin Cannot Complete Setup
- Both endpoints check `user_id IN (SELECT user_id FROM system_admins)`
- Regular users get 403 even if they know the endpoint

#### 4. Atomic Completion
- `POST /api/setup/complete` uses transaction with `FOR UPDATE` on `setup_complete` row
- Prevents concurrent completion attempts
- All config updates + `setup_complete` flag + audit log happen atomically

#### 5. No Takeover Vector
- Attacker cannot:
  - Re-trigger wizard (blocked by `setup_complete` check)
  - Complete wizard without being system admin
  - Race first user registration (table lock prevents it)
  - Bypass wizard by calling API directly (checks in place)

### Error Handling

**Registration Failures:**
- Username exists → `AuthError::UserAlreadyExists`
- Email exists → `AuthError::UserAlreadyExists`
- Password hash fails → `AuthError::PasswordHash`
- Transaction fails → rollback, return database error

**Setup Wizard Failures:**
- Not authenticated → 401 Unauthorized
- Not system admin → 403 Forbidden
- Setup already complete → 403 Forbidden with message "Setup already completed"
- Validation fails → 400 Bad Request with validation errors
- Database error → 500 Internal Server Error

**Client Error Handling:**
- 401/403 on wizard check → redirect to `/home` (setup not needed)
- 500 on wizard check → show error toast, retry
- Validation errors on submit → highlight invalid fields
- Network errors on submit → show error, allow retry

### Testing

**Unit Tests:**

1. **First user gets admin:**
   - Register user when count=0 → check `system_admins` table has entry

2. **Second user does NOT get admin:**
   - Register two users sequentially → only first has admin entry

3. **Setup status endpoint:**
   - Before completion → returns `{ setup_complete: false }`
   - After completion → returns `{ setup_complete: true }`

4. **Wizard access control:**
   - Non-admin calls wizard endpoint → 403
   - Admin calls when setup complete → 403
   - Admin calls when setup incomplete → 200 with config

5. **Setup completion:**
   - Valid request → updates config, sets complete flag, returns success
   - Already complete → 403
   - Invalid data → 400 with validation errors

**Integration Tests:**

6. **Concurrent registration:**
   - Simulate two simultaneous registrations
   - Verify only one gets admin
   - Verify both users created successfully

7. **Setup wizard flow:**
   - Register first user
   - Login → verify wizard appears
   - Complete wizard → verify `setup_complete` set to true
   - Login again → verify wizard does NOT appear

8. **Wizard interruption:**
   - Start wizard, close browser
   - Login again → wizard re-appears
   - Complete wizard → no longer appears

### Migration

**File:** `server/migrations/20260129000000_first_user_setup.sql`

Creates `server_config` table with initial values.

**Backwards Compatibility:**

For existing servers with users:
- Migration runs, creates `server_config` with `setup_complete = true`
- No wizard shown to existing admins
- System functions normally

### Audit Trail

**Events Logged:**

1. **User registration (first user):**
   ```json
   {
     "action": "user.register.first_admin",
     "actor_id": "<user_id>",
     "details": { "username": "admin", "auto_granted_admin": true }
   }
   ```

2. **Setup completion:**
   ```json
   {
     "action": "setup.complete",
     "actor_id": "<user_id>",
     "target_type": "server",
     "details": {
       "server_name": "My Gaming Server",
       "registration_policy": "invite_only"
     }
   }
   ```

### Future Enhancements

**Out of Scope for Initial Implementation:**

- Server logo upload in wizard
- Default theme selection
- Auto-create first guild for admin
- Email configuration in wizard (SMTP settings)
- Announcement to all users when setup completes

These can be added in future iterations if needed.

---

## Implementation Checklist

- [ ] Create migration: `20260129000000_first_user_setup.sql`
- [ ] Add `server_config` queries to `server/src/db/queries.rs`
- [ ] Modify `register()` handler with transaction + admin grant logic
- [ ] Create `server/src/api/setup.rs` with three endpoints
- [ ] Wire routes in `server/src/api/mod.rs`
- [ ] Create `SetupWizard.tsx` component with mandatory modal
- [ ] Add setup check to login flow in `auth.ts` store
- [ ] Add Tauri commands for wizard endpoints
- [ ] Write unit tests for registration and setup endpoints
- [ ] Write integration test for concurrent registration
- [ ] Update CHANGELOG.md with feature entry
- [ ] Update roadmap to mark feature as complete
