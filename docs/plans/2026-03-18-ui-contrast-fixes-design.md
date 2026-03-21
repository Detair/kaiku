# UI Visibility & Contrast Fixes Design

**Issue:** #386
**Goal:** Fix text contrast on accent/status backgrounds, admin panel elevation bugs, and OIDC startup logging.

## Items

### 1. Replace `text-white` with `text-on-accent` on accent backgrounds
Two files use the broken `bg-primary text-white` pattern. Replace with `bg-accent-primary text-on-accent` (token already exists per theme).

Files: `ScreenShareQualityPicker.tsx`, `SessionExpiredModal.tsx`

### 2. Fix status badge contrast
Add `--color-text-on-success` and `--color-text-on-warning` tokens to `themes.css`. Map in `uno.config.ts`. Update admin badges from `text-white` to `text-on-success`.

Files: `themes.css`, `uno.config.ts`, `UsersPanel.tsx`, `GuildsPanel.tsx`

### 3. Icons — deferred
Transparent versions exist as untracked files. Asset management task, not code fix.

### 4. Command Center "Top Routes" contrast
Find the blue label in the Command Center component and apply proper contrast token.

### 5. OIDC error logging
`oidc.rs` doesn't log errors on empty config. Grep for the exact log message in the full server codebase and fix if found.

### 6. Admin Settings elevation flash
Check `adminState.elevationExpiresAt` before calling `checkAdminStatus()` on mount. If already known-elevated, skip the async re-fetch to avoid the flash.

File: `AdminSettings.tsx`

### 7. Reports panel elevation guard
Guard `loadReports()`/`loadStats()` behind `adminState.isElevated`. Add reactive effect to load when elevation becomes true.

File: `ReportsPanel.tsx`
