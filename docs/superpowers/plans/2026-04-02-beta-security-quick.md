# Beta Security Quick Fixes — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix Megolm session cache leak on logout and `returnUrl` encoding vulnerability.

**Architecture:** Two independent client-side fixes. Fix 1 clears the crypto manager in Tauri's logout command and resets E2EE frontend signals. Fix 2 adds `encodeURIComponent`/`decodeURIComponent` to the auth redirect flow.

**Tech Stack:** Rust (Tauri), TypeScript/Solid.js (client)

**Spec:** `docs/superpowers/specs/2026-04-02-beta-security-fixes-design.md`

---

## File Map

| File | Responsibility |
|------|---------------|
| `client/src-tauri/src/commands/auth.rs` | Drop CryptoManager on logout |
| `client/src/stores/e2ee.ts` | Add `resetE2EEState()` export |
| `client/src/stores/auth.ts` | Call `resetE2EEState()` on logout |
| `client/src/components/auth/AuthGuard.tsx` | Encode `returnUrl` |
| `client/src/views/Login.tsx` | Decode `returnUrl` before validation |

---

## Task 1: Megolm session cache cleanup on logout

**Files:**
- Modify: `client/src-tauri/src/commands/auth.rs:301` (drop crypto state)
- Modify: `client/src/stores/e2ee.ts:265` (add `resetE2EEState` export)
- Modify: `client/src/stores/auth.ts:409` (call reset after logout)

- [ ] **Step 1: Add `resetE2EEState` to e2ee store**

At `client/src/stores/e2ee.ts`, add a new function before the `e2eeStore` export (before line 244):

```typescript
/** Reset all E2EE state signals to defaults. Called on logout. */
export function resetE2EEState(): void {
  setStatus({ initialized: false, device_id: null, has_identity_keys: false });
  setIsInitializing(false);
  setError(null);
}
```

- [ ] **Step 2: Call `resetE2EEState` in auth logout**

At `client/src/stores/auth.ts`, add the import at the top (alongside other store imports):

```typescript
import { resetE2EEState } from "./e2ee";
```

Then at line 409, after `await tauri.logout();`, add the reset call:

```typescript
    await tauri.logout();
    resetE2EEState();
    setAuthState({
```

- [ ] **Step 3: Drop CryptoManager in Tauri logout**

At `client/src-tauri/src/commands/auth.rs:301`, after the auth state clearing block's closing brace, add:

```rust
    // Clear crypto state — drops Megolm sessions, Olm account, closes SQLite
    *state.crypto.lock().await = None;
```

- [ ] **Step 4: Build and verify**

Run: `cd client/src-tauri && cargo clippy -p vc-client -- -D warnings`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add client/src-tauri/src/commands/auth.rs client/src/stores/e2ee.ts client/src/stores/auth.ts
git commit -m "fix(crypto): clear Megolm session cache and E2EE state on logout

Drop CryptoManager from AppState.crypto on Tauri logout to release
all Olm/Megolm sessions. Reset frontend E2EE signals to defaults
so UI reflects logged-out state."
```

---

## Task 2: `returnUrl` encoding

**Files:**
- Modify: `client/src/components/auth/AuthGuard.tsx:29` (encode)
- Modify: `client/src/views/Login.tsx:82-85` (decode)

- [ ] **Step 1: Encode returnUrl in AuthGuard**

At `client/src/components/auth/AuthGuard.tsx:29`, replace the navigate call:

```typescript
// Before
      navigate(`/login${returnUrl !== "/" ? `?returnUrl=${returnUrl}` : ""}`, {

// After
      navigate(`/login${returnUrl !== "/" ? `?returnUrl=${encodeURIComponent(returnUrl)}` : ""}`, {
```

- [ ] **Step 2: Decode returnUrl in Login**

At `client/src/views/Login.tsx:81-86`, replace the returnUrl handling:

```typescript
// Before (lines 81-86)
      const raw = searchParams.returnUrl;
      const returnUrl = Array.isArray(raw) ? raw[0] : raw;
      const target = returnUrl && returnUrl.startsWith("/") && !returnUrl.startsWith("//")
        ? returnUrl
        : "/";
      navigate(target, { replace: true });

// After
      const raw = searchParams.returnUrl;
      const returnUrl = Array.isArray(raw) ? raw[0] : raw;
      const decoded = returnUrl ? decodeURIComponent(returnUrl) : null;
      const target = decoded && decoded.startsWith("/") && !decoded.startsWith("//")
        ? decoded
        : "/";
      navigate(target, { replace: true });
```

- [ ] **Step 3: Commit**

```bash
git add client/src/components/auth/AuthGuard.tsx client/src/views/Login.tsx
git commit -m "fix(auth): encode returnUrl parameter in auth redirect flow

Wrap returnUrl with encodeURIComponent in AuthGuard to prevent
URL structure breakage from special characters. Decode in Login
before validation."
```

---

## Task 3: Docs — CHANGELOG and checklist

**Files:**
- Modify: `CHANGELOG.md` (add entries under `### Security`)
- Modify: `docs/developer-guide/plans/2026-03-19-beta-checklist.md` (mark 2 items done)

- [ ] **Step 1: Add CHANGELOG entries**

At `CHANGELOG.md`, after the `### Fixed` section in `[Unreleased]`, add a new `### Security` section (if one doesn't exist) or add under `### Fixed`:

```markdown
- E2EE Megolm session cache is now cleared on logout — prevents stale session keys from persisting
- `returnUrl` parameter is now URL-encoded to prevent query string injection
```

- [ ] **Step 2: Mark checklist items done**

In `docs/developer-guide/plans/2026-03-19-beta-checklist.md`:

Line 122: `- [ ] E2EE Megolm session cache never cleared on logout`
→ `- [x] E2EE Megolm session cache never cleared on logout (#PR_NUMBER)`

Line 123: `- [ ] returnUrl injection risk when feature is implemented`
→ `- [x] returnUrl injection risk when feature is implemented (#PR_NUMBER)`

Replace `#PR_NUMBER` with the actual PR number after creating the PR.

- [ ] **Step 3: Commit, push, create PR**

```bash
git add CHANGELOG.md docs/developer-guide/plans/2026-03-19-beta-checklist.md
git commit -m "docs: update CHANGELOG and checklist for security fixes"
git push -u origin fix/beta-security-quick
gh pr create --title "fix: clear E2EE sessions on logout and encode returnUrl" --body "..."
```
