# UI Visibility & Contrast Fixes Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Fix text contrast failures, admin panel elevation bugs, and OIDC logging across client and server.

**Architecture:** Six independent tasks touching CSS tokens, component classes, admin panel logic, and server logging. Each is self-contained and testable.

**Tech Stack:** Solid.js (TSX), UnoCSS (tokens/config), Rust (server logging)

---

## Task 1: Fix accent text contrast on buttons

**Files:**
- Modify: `client/src/components/voice/ScreenShareQualityPicker.tsx:152`
- Modify: `client/src/components/auth/SessionExpiredModal.tsx:55`

**Steps:**

1. In `ScreenShareQualityPicker.tsx` line 152, replace:
   ```
   bg-primary text-white
   ```
   with:
   ```
   bg-accent-primary text-on-accent
   ```
   Also update `hover:bg-primary/90` to `hover:bg-accent-primary/90`.

2. In `SessionExpiredModal.tsx` line 55, replace:
   ```
   bg-primary text-white
   ```
   with:
   ```
   bg-accent-primary text-on-accent
   ```
   Also update `hover:bg-primary/90` to `hover:bg-accent-primary/90`.

3. Run `bun run test:run` — verify no test regressions.

4. Commit: `fix(client): use text-on-accent for accent background buttons`

---

## Task 2: Add status text tokens and fix status button contrast

**Files:**
- Modify: `client/src/styles/themes.css` — add `--color-text-on-success`, `--color-text-on-danger` to each theme
- Modify: `client/uno.config.ts` — map new tokens + safelist
- Modify: `client/src/components/admin/UsersPanel.tsx:706` and other `bg-status-success text-white` / `bg-status-error text-white` buttons
- Modify: `client/src/components/admin/GuildsPanel.tsx:764` and similar

**Steps:**

1. In `themes.css`, add to each theme block after `--color-accent-warning`:
   ```css
   --color-text-on-success: #2E3440;
   --color-text-on-danger: #ECEFF4;
   ```
   Values per theme:
   - `focused-hybrid`: success=`#2E3440` (dark), danger=`#ECEFF4` (light)
   - `solarized-dark`: success=`#fdf6e3`, danger=`#fdf6e3`
   - `solarized-light`: success=`#fdf6e3`, danger=`#fdf6e3`
   - `pixel-cozy`: success=`#2c2418`, danger=`#f5e6d0`
   Also add to the `:root` fallback block.

2. In `uno.config.ts`, add to the `text` color section (around line 43):
   ```ts
   "on-success": "var(--color-text-on-success)",
   "on-danger": "var(--color-text-on-danger)",
   ```
   Add `"text-on-success"` and `"text-on-danger"` to the safelist array.

3. In `UsersPanel.tsx`, replace `bg-status-success text-white` with `bg-status-success text-on-success` (line 706 — Unban button). Replace `bg-status-error text-white` with `bg-status-error text-on-danger` (lines 346, 778, 842, 918 — ban/delete buttons).

4. In `GuildsPanel.tsx`, same replacements: `bg-status-success text-white` → `text-on-success` (line 764), `bg-status-error text-white` → `text-on-danger` (lines 351, 836, 900, 975).

5. Run `bun run test:run && bun run build`.

6. Commit: `fix(client): add text-on-success/danger tokens and fix status button contrast`

---

## Task 3: Replace hardcoded Tailwind colors in CommandCenterPanel

**Files:**
- Modify: `client/src/components/admin/CommandCenterPanel.tsx:280-323`

**Steps:**

1. In `CommandCenterPanel.tsx`, find the `LevelBadge` component (lines 280-301). Replace hardcoded Tailwind colors with theme tokens:
   - `bg-blue-500/20 text-blue-400` → `bg-accent-primary/20 text-accent-primary`
   - `bg-green-500/20 text-green-400` → `bg-status-success/20 text-status-success`
   - `bg-yellow-500/20 text-yellow-400` → `bg-status-warning/20 text-status-warning`
   - `bg-red-500/20 text-red-400` → `bg-status-error/20 text-status-error`

2. Find the `StatusBadge` component (lines 307-323). Same replacements for HTTP status color coding.

3. Run `bun run build` to verify.

4. Commit: `fix(client): replace hardcoded Tailwind colors with theme tokens in Command Center`

---

## Task 4: Fix OIDC startup logging level

**Files:**
- Modify: `server/src/db/queries.rs:1854-1896`

**Steps:**

1. In `queries.rs`, the OIDC query functions use `error!()` for DB failures. These are legitimate errors — keep as-is.

2. Check `server/src/main.rs:311` — `warn!("Failed to load OIDC providers")`. This fires when the DB query fails during startup. Change from `warn!` to `info!` since OIDC is optional:
   ```rust
   tracing::info!(error = %e, "OIDC providers not loaded (optional)");
   ```

3. Run `SQLX_OFFLINE=true cargo clippy -p vc-server -- -D warnings`.

4. Commit: `fix(infra): downgrade OIDC startup warning to info when providers fail to load`

---

## Task 5: Fix Admin Settings elevation flash

**Files:**
- Modify: `client/src/components/admin/AdminSettings.tsx:102-105`

**Steps:**

1. In `AdminSettings.tsx`, change the `onMount` (lines 102-105) from:
   ```tsx
   onMount(() => {
     checkAdminStatus();
     loadSettings();
   });
   ```
   to:
   ```tsx
   onMount(() => {
     if (!adminState.isElevated) {
       checkAdminStatus();
     }
     loadSettings();
   });
   ```
   This skips the async re-fetch if elevation was already confirmed by another panel, preventing the flash.

2. Run `bun run test:run`.

3. Commit: `fix(client): skip redundant elevation check in AdminSettings when already elevated`

---

## Task 6: Fix Reports panel elevation guard

**Files:**
- Modify: `client/src/components/admin/ReportsPanel.tsx:114-117`

**Steps:**

1. In `ReportsPanel.tsx`, change the `onMount` (lines 114-117) from:
   ```tsx
   onMount(() => {
     loadReports();
     loadStats();
   });
   ```
   to:
   ```tsx
   onMount(() => {
     if (adminState.isElevated) {
       loadReports();
       loadStats();
     }
   });

   createEffect(() => {
     if (adminState.isElevated) {
       loadReports();
       loadStats();
     }
   });
   ```
   Add `createEffect` import if not already present. The effect re-triggers when `isElevated` becomes true after elevation.

2. Run `bun run test:run`.

3. Commit: `fix(client): guard reports loading behind elevation check`
