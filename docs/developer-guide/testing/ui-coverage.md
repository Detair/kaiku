# UI Test Coverage Tracker

Tracks E2E test coverage for all frontend UI items. Each item maps to a Playwright test.

**Last updated:** 2026-02-13
**Test runner:** `cd client && npx playwright test`

## Status Legend

| Symbol | Meaning |
|--------|---------|
| :white_check_mark: | Covered — test passes without backend |
| :construction: | Test written — requires running backend to pass |
| :wrench: `test.fixme` | Test written — skipped (needs headed browser / WebRTC) |
| :x: | No test coverage |

---

## Authentication (`e2e/auth.spec.ts`)

| UI Item | Status | Test |
|---------|--------|------|
| Login form renders | :white_check_mark: | `should display login form` |
| Login with valid credentials | :construction: | `should login with valid credentials` |
| Login with invalid credentials | :construction: | `should show error for invalid credentials` |
| Login redirects to app | :construction: | `should login with valid credentials` |
| Register form renders | :white_check_mark: | `should display registration form` |
| Register new account | :construction: | `should register a new account` |
| Register validation (password mismatch) | :white_check_mark: | `should show validation errors` |
| Forgot password form renders | :white_check_mark: | `should display forgot password form` |
| Reset password form renders | :white_check_mark: | `should display reset password form` |
| Login ↔ Register link navigation | :white_check_mark: | `should have link to register/login page` |
| Login → Forgot Password link | :white_check_mark: | `should have link to forgot password page` |
| OAuth/OIDC provider buttons | :x: | Requires OIDC provider setup |

## Navigation (`e2e/navigation.spec.ts`)

| UI Item | Status | Test |
|---------|--------|------|
| Sidebar visible after login | :construction: | `should show sidebar after login` |
| Server rail displays home button | :construction: | `should display server rail with home button` |
| Server rail displays guild icons | :construction: | `should display guild icons` |
| Click home button goes to DM view | :construction: | `should navigate to home view` |
| Click guild icon switches guild | :construction: | `should switch guild on click` |
| Channel list visible when guild selected | :construction: | `should show channels when guild selected` |
| Click channel selects it | :construction: | `should select channel on click` |
| User panel visible at bottom | :construction: | `should show user panel` |
| Logout button works | :construction: | `should logout successfully` |

## Messaging (`e2e/messaging.spec.ts`)

| UI Item | Status | Test |
|---------|--------|------|
| Message input visible | :construction: | `should display message input` |
| Send text message | :construction: | `should send and display a message` |
| Message appears in list | :construction: | `should send and display a message` |
| Empty message prevented | :construction: | `should not send empty message` |
| Markdown rendering | :construction: | `should render markdown in messages` |

## Guild Management (`e2e/guild.spec.ts`)

| UI Item | Status | Test |
|---------|--------|------|
| Create guild button visible | :construction: | `should show create guild button` |
| Create guild modal | :construction: | `should create a new guild` |
| Join guild modal | :construction: | `should show join guild modal` |
| Guild settings modal opens | :construction: | `should open guild settings` |
| Edit guild name | :construction: | `should edit guild name` |

## Channel Management (`e2e/channels.spec.ts`)

| UI Item | Status | Test |
|---------|--------|------|
| Channel list renders | :construction: | `should display channel list` |
| Create channel button | :construction: | `should create a text channel` |
| Channel context menu | :construction: | `should show channel context menu` |
| Voice channel shows participants | :wrench: `test.fixme` | `should show voice participants` |

## Friends & DMs (`e2e/friends.spec.ts`)

| UI Item | Status | Test |
|---------|--------|------|
| Friends list renders | :construction: | `should display friends list` |
| Friends tab switching | :construction: | `should switch between tabs` |
| Add friend button | :construction: | `should show add friend form` |
| Send friend request | :construction: | `should send a friend request` |
| DM conversation opens | :construction: | `should open DM conversation` |

## User Settings (`e2e/settings.spec.ts`)

| UI Item | Status | Test |
|---------|--------|------|
| Settings modal opens | :construction: | `should open settings modal` |
| Account tab | :construction: | `should display account settings` |
| Appearance tab | :construction: | `should switch to appearance tab` |
| Audio tab | :construction: | `should switch to audio tab` |
| Notifications tab | :construction: | `should switch to notifications tab` |
| Privacy tab | :construction: | `should switch to privacy tab` |
| Security tab | :construction: | `should switch to security tab` |
| Change display name | :construction: | `should update display name` |

## Voice (`e2e/voice.spec.ts`)

| UI Item | Status | Test |
|---------|--------|------|
| Voice channel join | :wrench: `test.fixme` | `should join voice channel` |
| Voice controls render | :wrench: `test.fixme` | `should show voice controls` |
| Mute toggle | :wrench: `test.fixme` | `should toggle mute` |
| Disconnect button | :wrench: `test.fixme` | `should disconnect from voice` |
| Screen share button | :x: | Requires WebRTC mock |
| Webcam button | :x: | Requires media device mock |

## Admin (`e2e/admin.spec.ts`)

| UI Item | Status | Test |
|---------|--------|------|
| Admin dashboard accessible | :construction: | `should access admin dashboard` |
| Admin sidebar panels | :construction: | `should display admin panels` |
| Users panel renders | :construction: | `should show users panel` |
| Guilds panel renders | :construction: | `should show guilds panel` |
| Audit log panel renders | :construction: | `should show audit log panel` |
| Non-admin blocked | :construction: | `should block non-admin access` |

## Search (`e2e/search.spec.ts`)

| UI Item | Status | Test |
|---------|--------|------|
| Search panel opens | :construction: | `should open search panel` |
| Search input functional | :construction: | `should accept search query` |
| Search results display | :construction: | `should display search results` |

## Permissions (`e2e/permissions.spec.ts`) — Pre-existing

| UI Item | Status | Test |
|---------|--------|------|
| Create role | :construction: | `should create a new role with permissions` |
| Edit role | :construction: | `should edit an existing role` |
| @everyone security | :construction: | `should not display dangerous permissions` |
| Member role assignment | :construction: | `should assign a role to a member` |
| Channel permission overrides | :construction: | `should add a role override` |

---

## Coverage Summary

| Area | Total Items | Passing | Needs Backend | Not Covered |
|------|------------|---------|---------------|-------------|
| Auth | 12 | 7 | 4 | 1 |
| Navigation | 9 | 0 | 9 | 0 |
| Messaging | 5 | 0 | 5 | 0 |
| Guild Mgmt | 5 | 0 | 5 | 0 |
| Channels | 4 | 0 | 4 | 0 |
| Friends/DMs | 5 | 0 | 5 | 0 |
| Settings | 8 | 0 | 8 | 0 |
| Voice | 6 | 0 | 0 (4 fixme) | 2 |
| Admin | 6 | 0 | 6 | 0 |
| Search | 3 | 0 | 3 | 0 |
| Permissions | 5 | 0 | 5 | 0 |
| **Total** | **68** | **7** | **58** | **3** |

## First Test Run Results (2026-02-13)

**Environment:** No backend running, Chromium headless
**Command:** `cd client && npx playwright test`

| Result | Count |
|--------|-------|
| Passed | 8 |
| Failed | ~60 |
| Total | ~68 |

**Passing tests (no backend required):**
- `auth > Login > should display login form`
- `auth > Login > should have link to register page`
- `auth > Login > should have link to forgot password page`
- `auth > Registration > should display registration form`
- `auth > Registration > should show validation errors`
- `auth > Registration > should have link to login page`
- `auth > Password Recovery > should display forgot password form`
- `auth > Password Recovery > should display reset password form`

**Failure pattern:** All remaining tests require a running backend for login. The `login()` helper waits for `aside` (sidebar) to appear after form submission, which times out at 15s without a backend.

**Next step:** Run with backend + seed data (`scripts/create-test-users.sh`) to validate the full suite.

## Not Coverable Without Infrastructure

These items require external services or device mocks that aren't practical in basic E2E:

- OAuth/OIDC login flows (needs OIDC provider)
- Screen share (needs WebRTC + display capture)
- Webcam (needs media device mock)
- Password reset email flow (needs SMTP)
