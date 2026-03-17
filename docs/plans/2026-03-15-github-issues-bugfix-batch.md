# GitHub Issues Bugfix Batch — Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Fix all 9 non-blocked open GitHub issues across 3 branches (voice chores, infra fixes, client UI fixes). #293 (test backlog) is deferred.

**Architecture:** Three independent branches, each targeting a specific area. Voice chores are pure code cleanup. Infra fixes address Redis connection config and noisy OIDC logging. Client UI fixes address contrast, elevation state checks, and reports error handling.

**Tech Stack:** Rust (server), Solid.js/TypeScript (client), fred (Redis), CSS custom properties (theming)

---

## Branch 1: `chore/voice-cleanup` (Issues #369, #368, #370)

### Task 1: Update sfu.rs module doc (#369)

**Files:**
- Modify: `server/src/voice/sfu.rs:3`

**Step 1: Update the module doc**

Change line 3 from:
```rust
//! Manages voice rooms and WebRTC peer connections for real-time audio.
```
To:
```rust
//! Manages voice rooms and WebRTC peer connections for real-time audio and video.
```

**Step 2: Verify it compiles**

Run: `SQLX_OFFLINE=true cargo clippy -p vc-server -- -D warnings`
Expected: PASS (no warnings)

**Step 3: Commit**

```bash
git add server/src/voice/sfu.rs
git commit -m "docs(voice): update sfu.rs module doc to include video

Closes #369"
```

---

### Task 2: Guard REMB bitrate cast (#368)

**Files:**
- Modify: `server/src/voice/track.rs:480,524`
- Test: `server/src/voice/track.rs` (existing test module at line 570)

**Step 1: Add non-finite guard in `spawn_subscriber_remb_reader`**

In `server/src/voice/track.rs`, inside `spawn_subscriber_remb_reader` (line 523-524), replace:
```rust
                {
                    let bitrate = remb.bitrate as u64;
```
With:
```rust
                {
                    if !remb.bitrate.is_finite() || remb.bitrate < 0.0 {
                        continue;
                    }
                    let bitrate = remb.bitrate as u64;
```

**Step 2: Add the same guard in `spawn_rtcp_reader`**

In `spawn_rtcp_reader` (line 479-480), replace:
```rust
                {
                    // REMB bitrate is f32 bps; convert to u64 for logging.
                    let bitrate = remb.bitrate as u64;
```
With:
```rust
                {
                    if !remb.bitrate.is_finite() || remb.bitrate < 0.0 {
                        continue;
                    }
                    let bitrate = remb.bitrate as u64;
```

**Step 3: Verify it compiles**

Run: `SQLX_OFFLINE=true cargo clippy -p vc-server -- -D warnings`
Expected: PASS

**Step 4: Commit**

```bash
git add server/src/voice/track.rs
git commit -m "fix(voice): guard REMB bitrate cast against non-finite f32 values

Skip REMB packets with NaN, Infinity, or negative bitrate values
to prevent u64 conversion from producing MAX or 0 values that would
disrupt simulcast layer selection.

Closes #368"
```

---

### Task 3: Remove redundant spawn_rtcp_reader (#370)

**Files:**
- Modify: `server/src/voice/sfu.rs:685-688` (remove call site)
- Modify: `server/src/voice/track.rs:458-499` (remove function)

**Step 1: Remove the call site in sfu.rs**

In `server/src/voice/sfu.rs`, remove lines 685-688:
```rust
                    // Spawn RTCP reader for REMB processing on video tracks.
                    if source_type.is_video() {
                        spawn_rtcp_reader(uid, source_type, layer, receiver.clone());
                    }
```

**Step 2: Remove the `spawn_rtcp_reader` function from track.rs**

Remove the entire function definition at lines 458-499 (the doc comment + function body).

**Step 3: Remove the import if now unused**

Check if `spawn_rtcp_reader` is imported anywhere else. If not, remove the import from `sfu.rs`.

**Step 4: Verify it compiles**

Run: `SQLX_OFFLINE=true cargo clippy -p vc-server -- -D warnings`
Expected: PASS (no unused import warnings)

**Step 5: Commit**

```bash
git add server/src/voice/sfu.rs server/src/voice/track.rs
git commit -m "chore(voice): remove redundant source-side spawn_rtcp_reader

The subscriber-side spawn_subscriber_remb_reader (added in #367) handles
all REMB-driven layer switching. The source-side reader only logged REMB
at trace level, spawning 3 extra tokio tasks per simulcast screen share.

Closes #370"
```

> **Note:** Task 3 depends on Task 2. The REMB guard from Task 2 was applied to `spawn_rtcp_reader` first, then that function is removed in Task 3. The guard in `spawn_subscriber_remb_reader` remains.

---

## Branch 2: `fix/infra-perf` (Issues #390, #387)

### Task 4: Fix 2-second HTTP delay — Redis reconnect config (#390)

**Files:**
- Modify: `server/src/db/mod.rs:91-101`

**Step 1: Configure fred with explicit reconnect policy and performance config**

Replace the `create_redis_client` function in `server/src/db/mod.rs:90-101`:

```rust
/// Create Redis client.
pub async fn create_redis_client(redis_url: &str) -> Result<fred::clients::Client> {
    use fred::prelude::*;

    let config = Config::from_url(redis_url)?;

    // Explicit reconnect policy: constant 100ms delay, unlimited retries.
    // Default is 2000ms which causes a 2-second stall on every request
    // when the connection drops between requests.
    let policy = ReconnectPolicy::new_constant(0, 100);

    let client = Client::new(config, None, None, Some(policy));
    client.connect();
    client.wait_for_connect().await?;

    info!("Connected to Redis");
    Ok(client)
}
```

**Step 2: Verify it compiles**

Run: `SQLX_OFFLINE=true cargo clippy -p vc-server -- -D warnings`
Expected: PASS

**Step 3: Commit**

```bash
git add server/src/db/mod.rs
git commit -m "perf(infra): fix 2-second delay on HTTP requests after first

Configure fred Redis client with 100ms constant reconnect policy instead
of the default 2000ms delay. The default reconnect interval caused every
HTTP request after the first to stall for exactly 2 seconds when the
connection dropped between requests.

Closes #390"
```

---

### Task 5: Suppress OIDC errors when no provider configured (#387)

**Files:**
- Modify: `server/src/db/queries.rs:1871-1880`

**Step 1: Lower log level for RowNotFound in `get_oidc_provider_by_slug`**

The current code logs at `error!` level for ALL failures including `RowNotFound`, which fires on every startup when checking if a legacy OIDC provider exists. Replace lines 1871-1880:

```rust
pub async fn get_oidc_provider_by_slug(pool: &PgPool, slug: &str) -> sqlx::Result<OidcProviderRow> {
    sqlx::query_as::<_, OidcProviderRow>("SELECT * FROM oidc_providers WHERE slug = $1")
        .bind(slug)
        .fetch_one(pool)
        .await
        .map_err(|e| {
            match &e {
                sqlx::Error::RowNotFound => {
                    debug!(slug = %slug, "OIDC provider not found by slug");
                }
                _ => {
                    error!(error = %e, slug = %slug, "Failed to get OIDC provider by slug");
                }
            }
            e
        })
}
```

**Step 2: Verify it compiles**

Run: `SQLX_OFFLINE=true cargo clippy -p vc-server -- -D warnings`
Expected: PASS

**Step 3: Commit**

```bash
git add server/src/db/queries.rs
git commit -m "fix(infra): suppress OIDC errors when no provider configured

Lower log level from ERROR to DEBUG for RowNotFound when looking up OIDC
providers by slug. The seed_from_env check intentionally queries for
'legacy-oidc' to see if it already exists — RowNotFound is expected,
not an error.

Closes #387"
```

---

## Branch 3: `fix/client-ui-issues` (Issues #386, #389, #388)

### Task 6: Add `text-on-accent` token and fix contrast (#386 items 1-2)

**Files:**
- Modify: `client/src/styles/themes.css` (all 4 themes + fallback)
- Modify: All files using `bg-accent-primary text-white` pattern

**Step 1: Add `--color-text-on-accent` token to each theme**

In `client/src/styles/themes.css`:

For `focused-hybrid` (after line 51 `--color-accent-primary`):
```css
  --color-text-on-accent: #2E3440;
```

For `solarized-dark` (after line 85 `--color-accent-primary`):
```css
  --color-text-on-accent: #fdf6e3;
```

For `solarized-light` (after line 111 `--color-accent-primary`):
```css
  --color-text-on-accent: #fdf6e3;
```

For `pixel-cozy` (after line 151 `--color-accent-primary`):
```css
  --color-text-on-accent: #2c2418;
```

For fallback `:root` (after line 181 `--color-accent-primary`):
```css
  --color-text-on-accent: #2E3440;
```

**Step 2: Search and replace `bg-accent-primary text-white`**

Replace all instances of `bg-accent-primary text-white` with `bg-accent-primary text-on-accent` across the client source. Use grep to find all instances first, then replace.

Key files identified:
- `client/src/components/home/modules/CollapsibleModule.tsx:47`
- `client/src/components/home/modules/UnreadModule.tsx:359,397`
- `client/src/components/layout/ServerRail.tsx:146`
- `client/src/views/AdminDashboard.tsx:253`
- `client/src/views/InviteJoin.tsx:67`
- `client/src/components/admin/AdminSettings.tsx:432,682`
- `client/src/components/admin/ReportsPanel.tsx:435`
- `client/src/components/admin/AdminQuickModal.tsx:147`
- And any others found by grep

**Step 3: Verify the client builds**

Run: `cd client && bun run build`
Expected: PASS

**Step 4: Commit**

```bash
git add client/src/styles/themes.css client/src/
git commit -m "fix(client): add text-on-accent token to fix contrast on accent backgrounds

Introduces --color-text-on-accent CSS token per theme, using dark text
for bright accents (focused-hybrid, pixel-cozy) and light text for dark
accents (solarized). Replaces all bg-accent-primary text-white instances
with bg-accent-primary text-on-accent.

Ref #386"
```

---

### Task 7: Fix admin settings elevation check (#389)

**Files:**
- Modify: `client/src/components/admin/AdminSettings.tsx:272`

**Step 1: Verify the current behavior**

Read `AdminSettings.tsx` around line 272. The `<Show when={!adminState.isElevated}>` block correctly shows the warning only when NOT elevated. The issue title says "shows 'needs elevation' when already elevated" — investigate if the `isElevated` state isn't being updated correctly, or if there's a timing issue.

Check `client/src/stores/admin.ts` to verify `checkAdminStatus()` is called when the AdminSettings panel mounts.

**Step 2: Ensure AdminSettings checks elevation on mount**

If `AdminSettings` doesn't call `checkAdminStatus()` on mount, add it:

```typescript
onMount(() => {
  checkAdminStatus();
  // ... existing onMount logic
});
```

**Step 3: Verify client builds**

Run: `cd client && bun run build`
Expected: PASS

**Step 4: Commit**

```bash
git add client/src/components/admin/AdminSettings.tsx
git commit -m "fix(client): admin settings checks elevation state on mount

Ensures checkAdminStatus() is called when AdminSettings mounts so the
elevation warning is hidden when session is already elevated.

Closes #389"
```

---

### Task 8: Fix admin reports page error handling (#388)

**Files:**
- Modify: `client/src/components/admin/ReportsPanel.tsx:70-87`

**Step 1: Add error state and user-visible error handling to `loadReports`**

The reports routes are behind elevated middleware. When not elevated (or on any API error), `loadReports()` silently catches the error (console.error only). Add a toast and error state:

```typescript
const [loadError, setLoadError] = createSignal<string | null>(null);

const loadReports = async () => {
  setIsLoading(true);
  setLoadError(null);
  try {
    const offset = (page() - 1) * PAGE_SIZE;
    const result = await tauri.adminListReports(
      PAGE_SIZE,
      offset,
      statusFilter() || undefined,
      categoryFilter() || undefined,
    );
    setReports(result.items);
    setTotal(result.total);
  } catch (err) {
    console.error("[Admin] Failed to load reports:", err);
    setLoadError("Failed to load reports. Ensure your session is elevated.");
    showToast({
      type: "error",
      title: "Failed to load reports",
      duration: 8000,
    });
  } finally {
    setIsLoading(false);
  }
};
```

**Step 2: Show error state in the UI**

After the loading spinner `<Show>` block (around line 254), add an error state:

```tsx
<Show when={loadError()}>
  <div class="flex flex-col items-center justify-center p-12 gap-3">
    <p class="text-sm text-status-danger">{loadError()}</p>
    <button
      onClick={loadReports}
      class="px-3 py-1.5 text-xs rounded bg-surface-highlight text-text-primary hover:bg-surface-layer2"
    >
      Retry
    </button>
  </div>
</Show>
```

**Step 3: Verify client builds**

Run: `cd client && bun run build`
Expected: PASS

**Step 4: Commit**

```bash
git add client/src/components/admin/ReportsPanel.tsx
git commit -m "fix(client): admin reports page shows error state instead of silent failure

Add toast notification and inline error state with retry button when
report loading fails. Previously only logged to console, leaving users
with a blank 'No reports found' message on API errors.

Closes #388"
```

---

### Task 9: Fix Command Center "Top Routes" contrast (#386 item 4)

**Files:**
- Modify: `client/src/components/admin/CommandCenterPanel.tsx:629,641`

**Step 1: Fix active sort button contrast**

The active sort buttons use `bg-accent-primary/20 text-accent-primary`. On focused-hybrid theme, `text-accent-primary` (#88c0d0) is a bright blue that's hard to read on the semi-transparent blue background. Change to `text-text-primary` for better readability.

Replace `text-accent-primary` with `text-on-accent` isn't appropriate here since the background is semi-transparent. Instead, use `text-text-primary`:

```typescript
"bg-accent-primary/20 text-text-primary": routeSort() === "latency",
```
and:
```typescript
"bg-accent-primary/20 text-text-primary": routeSort() === "errors",
```

**Step 2: Verify client builds**

Run: `cd client && bun run build`
Expected: PASS

**Step 3: Commit**

```bash
git add client/src/components/admin/CommandCenterPanel.tsx
git commit -m "fix(client): improve Top Routes sort button contrast in Command Center

Use text-text-primary instead of text-accent-primary on active sort
buttons so text is readable on the semi-transparent accent background.

Ref #386"
```

---

## Execution Order

1. **Branch `chore/voice-cleanup`**: Tasks 1 → 2 → 3 (sequential — Task 3 removes code modified in Task 2)
2. **Branch `fix/infra-perf`**: Tasks 4, 5 (independent, can run in parallel)
3. **Branch `fix/client-ui-issues`**: Tasks 6, 7, 8, 9 (independent, can run in parallel)

All three branches are independent and can be worked on in parallel using worktrees.

## Post-Implementation

After all branches are complete:
1. Run full test suite: `cargo test` and `cd client && bun run test:run`
2. Run linters: `SQLX_OFFLINE=true cargo clippy -- -D warnings` and `cargo fmt --check`
3. Create PRs for each branch, referencing the closed issues
4. Close #386 items 3 (icons) and 5 (OIDC = #387) via their respective PRs
