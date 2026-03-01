# Implementation Plan: Permission System E2E Tests

**Date:** 2026-01-19
**Goal:** Prevent regressions in the Permission System UI and Logic by implementing automated End-to-End tests using Playwright.

---

## 1. Prerequisites & Environment

The tests will run against the **browser version** of the client (`bun run dev`), communicating with a **local running backend** (`cargo run`).

*   **Test Directory:** `client/e2e/`
*   **New File:** `client/e2e/permissions.spec.ts`
*   **Required Seed Data:** The tests assume the standard dev seed data (`scripts/create-test-users.sh`) is present:
    *   User: `admin` (Owner of default guild)
    *   User: `alice` (Member)
    *   User: `bob` (Member)

## 2. Test Scenarios

### 2.1. Role Management (Owner Flow)
**Objective:** Verify that a guild owner can create, edit, and delete roles.

**Steps:**
1.  **Login** as `admin`.
2.  **Open Guild Settings**: Click the dropdown arrow next to the guild name -> "Server Settings".
3.  **Navigate to Roles**: Select the "Roles" tab.
4.  **Create Role**:
    *   Click "New Role".
    *   Enter name "Test Officer".
    *   Select color Blue.
    *   Check "Manage Messages" and "Kick Members".
    *   Click "Save Changes".
5.  **Verify**:
    *   "Test Officer" appears in the role list.
    *   Permissions count shows "2 permissions".

### 2.2. @everyone Security Constraints
**Objective:** Verify that the UI prevents assigning dangerous permissions to the `@everyone` role.

**Steps:**
1.  **Edit @everyone**: In Roles tab, click "Edit" next to `@everyone`.
2.  **Verify UI**:
    *   Ensure "Administrator", "Ban Members", "Manage Server" checkboxes are **hidden** or **disabled** (as per `permissionConstants.ts` logic).
3.  **Attempt Modification**:
    *   Enable "Send Messages" (allowed).
    *   Save.
4.  **Verify**: Change persists.

### 2.3. Member Role Assignment
**Objective:** Verify that a member can be assigned a role.

**Steps:**
1.  **Close Settings** (if open).
2.  **Open Member List**: Click "Members" tab in the main UI.
3.  **Assign Role**:
    *   Find `alice`.
    *   Click "Manage" (or context menu).
    *   Select "Assign Role" -> "Test Officer".
4.  **Verify**:
    *   "Test Officer" badge appears next to `alice`'s name.

### 2.4. Channel Permission Overrides
**Objective:** Verify channel-specific permission overrides.

**Steps:**
1.  **Context Menu**: Right-click the `#general` channel.
2.  **Edit Channel**: Select "Edit Channel".
3.  **Permissions Tab**: Go to "Permissions" tab.
4.  **Add Override**:
    *   Click "Add Role".
    *   Select "Test Officer".
5.  **Configure**:
    *   Set "Send Messages" to **Deny**.
    *   Set "Read Messages" to **Allow**.
    *   Click "Save".
6.  **Verify**:
    *   UI shows "Test Officer: 1 allowed, 1 denied".

## 3. Implementation Details

**File:** `client/e2e/permissions.spec.ts`

```typescript
import { test, expect } from '@playwright/test';

test.describe('Permission System', () => {
  test.beforeEach(async ({ page }) => {
    // Login as admin before each test
    await page.goto('/login');
    await page.fill('input[name="username"]', 'admin');
    await page.fill('input[name="password"]', 'admin123');
    await page.click('button[type="submit"]');
    await expect(page.locator('.guild-nav')).toBeVisible();
  });

  test('should create a new role with permissions', async ({ page }) => {
    // Open Guild Menu
    await page.click('[data-testid="guild-header"]');
    await page.click('[data-testid="settings-trigger"]');
    
    // Go to Roles
    await page.click('text=Roles');
    
    // Create Role
    await page.click('text=New Role');
    await page.fill('input[placeholder="Enter role name..."]', 'E2E Role');
    
    // Toggle Permissions
    await page.click('text=Manage Messages'); // Toggle on
    
    // Save
    await page.click('text=Create Role');
    
    // Verify
    await expect(page.locator('text=E2E Role')).toBeVisible();
  });

  test('should prevent dangerous permissions for @everyone', async ({ page }) => {
    await page.click('[data-testid="guild-header"]');
    await page.click('[data-testid="settings-trigger"]');
    await page.click('text=Roles');
    
    // Edit @everyone
    // Note: Assuming @everyone is the last one or identifiable
    await page.click('text=@everyone');
    
    // Check for absence of Ban Members checkbox
    await expect(page.locator('text=Ban Members')).toBeHidden();
  });
});
```

## 4. Execution Strategy

1.  **Start Backend**: `cargo run` in `server/` terminal.
2.  **Run Tests**: `cd client && npx playwright test permissions` (this will spin up vite dev server automatically).

## 5. Success Criteria

- All tests pass in CI/local environment.
- Tests accurately reflect the Security/UI constraints defined in `docs/plans/2026-01-18-permission-ui-design.md`.
