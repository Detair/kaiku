# CleanupGuard Test Flake Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Eliminate the CI flake where Rust integration tests intermittently time out at nextest's 120s `slow-timeout` because `CleanupGuard::drop` blocks indefinitely on a hanging cleanup runtime.

**Architecture:** Two layers shipped in one PR. Layer 1 is a stopgap that bounds `CleanupGuard::drop` with a 30-second polling join, then detaches if cleanup hasn't completed. Layer 2 introduces an explicit `cleanup(self)` async method that runs cleanup on the test's existing tokio runtime, and migrates ~136 test sites across 16 files to call it. The Drop fallback stays in place during migration; after migration completes and CI is green for ~2 weeks, a follow-up commit replaces the fallback's eprintln with `panic!()` to enforce the contract permanently.

**Tech Stack:** Rust, tokio, sqlx, cargo-nextest

**Spec:** `docs/superpowers/specs/2026-04-11-security-audit-followups-design.md` (Topic 4)

**Branch:** `fix/cleanup-guard-test-flake`

---

## File structure

| File | Role | Touched in |
|---|---|---|
| `server/tests/integration/helpers/mod.rs` | Defines `CleanupGuard`, the broken Drop, and the new `cleanup()` method | Tasks 1, 2 |
| `server/tests/integration/guild_limits.rs` | Contains 2 known-flaky tests; 10 guard usages | Task 3 |
| `server/tests/integration/custom_status.rs` | Contains 1 known-flaky test; 6 guard usages | Task 4 |
| `server/tests/integration/workspaces.rs` | 24 guard usages (largest file) | Task 5 |
| `server/tests/integration/filters_http.rs` | 17 guard usages | Task 6 |
| `server/tests/integration/governance.rs` | 12 guard usages | Task 7 |
| `server/tests/integration/webhooks.rs` | 10 guard usages | Task 8 |
| `server/tests/integration/channel_pins.rs` | 10 guard usages | Task 9 |
| `server/tests/integration/connectivity_http.rs` | 8 guard usages | Task 10 |
| `server/tests/integration/setup_http.rs` | 6 guard usages | Task 11 |
| `server/tests/integration/messages_http.rs` | 6 guard usages | Task 11 |
| `server/tests/integration/media_processing.rs` | 6 guard usages | Task 11 |
| `server/tests/integration/bot_intents.rs` | 6 guard usages | Task 12 |
| `server/tests/integration/dm_http.rs` | 5 guard usages | Task 12 |
| `server/tests/integration/channels_http.rs` | 5 guard usages | Task 12 |
| `server/tests/integration/uploads_http.rs` | 3 guard usages | Task 13 |
| `server/tests/integration/setup_concurrent_http.rs` | 2 guard usages (already #[ignore]'d, no migration needed but check anyway) | Task 13 |

**Total: 136 cleanup_guard usages across 16 files.**

The plan groups the 16 files into 11 migration tasks (tasks 3-13). Tasks are sized so each commit migrates a related set of tests in 5-15 minutes.

---

## Task 1: Layer 1 — Bounded join in `CleanupGuard::drop`

**Files:**
- Modify: `server/tests/integration/helpers/mod.rs:163-185`

**Goal:** Replace the unconditional `.join().expect()` with a 30-second polling join. If cleanup completes within 30s, join normally. Otherwise, detach the cleanup thread and let the test process continue. Stops the 120s nextest timeout from being triggered by hanging cleanup.

- [ ] **Step 1: Read the current Drop impl**

Run: `sed -n '163,185p' server/tests/integration/helpers/mod.rs`

Expected: see the current `impl Drop for CleanupGuard` which spawns a thread, builds a `tokio::runtime::Builder::new_current_thread()`, calls `runtime.block_on(...)`, and ends with `.join().expect("Cleanup thread panicked");`. Confirm the line range is still 163-185 (file may have shifted).

- [ ] **Step 2: Establish a baseline**

Run: `SQLX_OFFLINE=true cargo nextest run -p vc-server --lib 2>&1 | tail -5`

Expected: see a "Summary" line with PASS counts. Record the number — for example, "300 tests passed". You'll compare against this after Task 1 to verify nothing regressed.

(We use `--lib` only, not full integration tests, because integration tests need a running DB and we don't want to chase that here. The Drop change is in test-helpers code paths but doesn't affect lib tests' compilation.)

- [ ] **Step 3: Replace the Drop impl**

Edit `server/tests/integration/helpers/mod.rs`. Replace the existing `impl Drop for CleanupGuard { ... }` block (lines 163-185) with this new version:

```rust
impl Drop for CleanupGuard {
    fn drop(&mut self) {
        let actions = std::mem::take(&mut self.actions);
        if actions.is_empty() {
            return;
        }

        let pool = self.pool.clone();
        let handle = std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("Failed to create cleanup runtime");
            runtime.block_on(async move {
                for action in actions {
                    action(pool.clone()).await;
                }
            });
        });

        // Bounded join: poll for up to 30s, then detach if still running.
        // Prevents flaky cleanup hangs from triggering nextest's 120s slow-timeout.
        // Removed once the explicit-cleanup migration completes (Task 14+).
        let timeout = std::time::Duration::from_secs(30);
        let start = std::time::Instant::now();
        loop {
            if handle.is_finished() {
                let _ = handle.join();
                return;
            }
            if start.elapsed() > timeout {
                eprintln!(
                    "warning: CleanupGuard did not complete within {timeout:?}, \
                     detaching thread to allow test to finish"
                );
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
    }
}
```

- [ ] **Step 4: Compile check**

Run: `SQLX_OFFLINE=true cargo build --tests -p vc-server 2>&1 | tail -10`

Expected: clean build, no warnings.

If clippy warnings appear (e.g., `clippy::needless_continue`), address them. If errors appear, double-check the edit matched the existing structure.

- [ ] **Step 5: Re-run baseline tests**

Run: `SQLX_OFFLINE=true cargo nextest run -p vc-server --lib 2>&1 | tail -5`

Expected: same pass count as Step 2. If regressed, investigate before proceeding.

- [ ] **Step 6: Commit**

```bash
git add server/tests/integration/helpers/mod.rs
git commit -m "fix(test): bound CleanupGuard::drop join to 30s

CleanupGuard::drop currently calls .join() with no timeout. When test
cleanup hangs (DB pool contention, slow DELETE, lock wait, etc.), the
test process holds until nextest's 120s slow-timeout kills it.

Add a 30-second polling join: if cleanup completes within 30s, join
normally. Otherwise, log to stderr and return — the cleanup thread
continues detached but the test process is no longer blocked.

This is a stopgap. The proper fix (explicit .cleanup().await) is in
follow-up commits.

Refs #509 (same anti-pattern, different test set)"
```

---

## Task 2: Layer 2 — Add `cleanup(self)` method and Drop fallback warning

**Files:**
- Modify: `server/tests/integration/helpers/mod.rs` (add new method on `CleanupGuard`, update Drop fallback)

**Goal:** Add an explicit `cleanup(self)` async method that runs cleanup actions on the test's existing tokio runtime. Update the Drop impl to also log a "dropped with N pending actions" warning so unmigrated tests are visible during the migration.

- [ ] **Step 1: Add the `cleanup` method**

Find the `impl CleanupGuard { ... }` block (just above the `impl Drop`). Add this new method at the end of that block, after the existing `restore_config_defaults` method:

```rust
    /// Run all registered cleanup actions on the caller's tokio runtime
    /// and consume the guard.
    ///
    /// Tests MUST call this at the end of the test body. Forgetting it
    /// triggers a runtime warning from the Drop fallback (during the
    /// migration) and eventually a panic (after the migration completes).
    pub async fn cleanup(mut self) {
        let actions = std::mem::take(&mut self.actions);
        for action in actions {
            action(self.pool.clone()).await;
        }
        // Drop runs after this returns, but `actions` is now empty so
        // Drop is a no-op.
    }
```

- [ ] **Step 2: Update the Drop fallback to warn on pending actions**

In the `impl Drop for CleanupGuard` block from Task 1, add an `eprintln!` warning **before** the existing thread-spawn fallback. The full updated block looks like this (changes marked with comments):

```rust
impl Drop for CleanupGuard {
    fn drop(&mut self) {
        let actions = std::mem::take(&mut self.actions);
        if actions.is_empty() {
            return;
        }

        // NEW: warn that the test forgot .cleanup().await — this is a
        // bug, but during migration we still try to clean up so we
        // don't leak DB rows.
        eprintln!(
            "warning: CleanupGuard dropped with {} pending actions — \
             test forgot to call .cleanup().await",
            actions.len()
        );

        let pool = self.pool.clone();
        let handle = std::thread::spawn(move || {
            // ... unchanged from Task 1 ...
        });

        // ... unchanged bounded-join logic from Task 1 ...
    }
}
```

- [ ] **Step 3: Compile check**

Run: `SQLX_OFFLINE=true cargo build --tests -p vc-server 2>&1 | tail -10`

Expected: clean build.

- [ ] **Step 4: Verify compilation**

The helpers/mod.rs file is integration-test infrastructure that needs DATABASE_URL to actually exercise. We don't have a unit-test smoke check here — Tasks 3-13 will exercise the new method against real tests.

Run: `SQLX_OFFLINE=true cargo build --tests -p vc-server 2>&1 | tail -3`

Expected: clean.

- [ ] **Step 5: Commit**

```bash
git add server/tests/integration/helpers/mod.rs
git commit -m "feat(test): add CleanupGuard::cleanup() explicit method

Tests should call .cleanup().await at the end of the body instead of
relying on Drop. The new method consumes self, runs all cleanup actions
on the caller's tokio runtime, and leaves Drop as a no-op.

The Drop fallback now also prints a 'dropped with N pending actions'
warning so the migration progress is visible. After all tests are
migrated and CI is green for ~2 weeks, the warning will be replaced
with a panic in a follow-up commit.

Refs spec docs/superpowers/specs/2026-04-11-security-audit-followups-design.md"
```

---

## Tasks 3-13: Migrate test files to `.cleanup().await`

The migration is purely mechanical. For every test that has `let mut guard = app.cleanup_guard();` (or `let mut guard = CleanupGuard::new(pool.clone());`), add `guard.cleanup().await;` immediately before the test function returns.

**The pattern:**

```rust
// Before:
#[tokio::test]
async fn test_something() {
    let app = TestApp::new().await;
    let (user_id, _) = create_test_user(&app.pool).await;
    let mut guard = app.cleanup_guard();
    guard.delete_user(user_id);

    // ... assertions ...
}

// After:
#[tokio::test]
async fn test_something() {
    let app = TestApp::new().await;
    let (user_id, _) = create_test_user(&app.pool).await;
    let mut guard = app.cleanup_guard();
    guard.delete_user(user_id);

    // ... assertions ...

    guard.cleanup().await;
}
```

**Important caveats for every migration task:**

1. **Place `guard.cleanup().await;` after the LAST assertion**, not before. If an assertion fails, the test panics and the Drop fallback handles cleanup. We want the happy path to call cleanup explicitly.

2. **If the test has early returns** (e.g., `return;` inside an `if`), insert `guard.cleanup().await;` before each return. Or refactor to a single exit point (`break 'outer;`). The Drop fallback handles the panic path; you're only fixing the happy path.

3. **If the test moves `guard` into a closure** (e.g., `tokio::spawn(async move { guard.cleanup().await })`), the migration is non-trivial. Flag it in the commit message and handle it case-by-case. For Tasks 3-13, expect the simple case unless the file has unusual structure.

4. **Don't touch tests that don't use `CleanupGuard`** — they're out of scope.

5. **Don't add `cleanup().await` to tests inside `#[ignore]` blocks** unless they would pass in a clean environment. The setup_concurrent_http tests are already `#[ignore]`'d (per issue #509); migrate them anyway since the guard pattern is the same.

### Per-task verification template

For every migration task (3-13), the steps are:

- [ ] **Step A: Read the test file**

Run: `cat server/tests/integration/<file>.rs | grep -nE "fn test_|cleanup_guard|CleanupGuard::new|guard\.cleanup"`

This gives a quick overview of which test functions touch the guard.

- [ ] **Step B: Edit the file**

For each test function that creates a guard, add `guard.cleanup().await;` before the closing `}` of the function (or before each early return).

Use the Edit tool with the exact `let mut guard = ...` line as the anchor, plus the surrounding context, to make the edits unambiguous.

- [ ] **Step C: Compile check**

Run: `SQLX_OFFLINE=true cargo build --tests -p vc-server 2>&1 | tail -5`

Expected: clean. If errors, the most likely issue is a borrow problem (e.g., the test reads from `guard.pool` after `cleanup()` consumes it). Fix by moving the `pool` clone before the cleanup.

- [ ] **Step D: Run the file's tests**

Run: `SQLX_OFFLINE=true cargo nextest run -p vc-server --test integration <module-name> 2>&1 | tail -10`

Where `<module-name>` is the file basename without extension (e.g., `guild_limits`).

Expected: same pass/fail count as before the migration. We don't need DATABASE_URL set for this — the failures will be `Connection refused`, not "dropped with pending actions". The compile success is what matters here.

- [ ] **Step E: Commit**

```bash
git add server/tests/integration/<file>.rs
git commit -m "test: migrate <file> to explicit CleanupGuard::cleanup().await"
```

### Task 3: Migrate `guild_limits.rs` (priority: 2 known-flaky tests)

10 guard usages. Files: `guild_limits.rs`. Follow the per-task template above. The known-flaky tests in this file are `test_globally_banned_user_cannot_join_via_discovery` and `test_channel_limit` — give them extra attention.

### Task 4: Migrate `custom_status.rs` (priority: 1 known-flaky test)

6 guard usages. The known-flaky test is `test_custom_status_with_expiry_persists`. Note this file uses the `CleanupGuard::new(pool.clone())` pattern, not `app.cleanup_guard()`.

### Task 5: Migrate `workspaces.rs` (largest file)

24 guard usages. Use Edit's `replace_all` cautiously — every `let mut guard = ...` is followed by a different test body, so each function needs a separate edit for the closing-brace insertion.

### Task 6: Migrate `filters_http.rs`

17 guard usages.

### Task 7: Migrate `governance.rs`

12 guard usages.

### Task 8: Migrate `webhooks.rs`

10 guard usages.

### Task 9: Migrate `channel_pins.rs`

10 guard usages.

### Task 10: Migrate `connectivity_http.rs`

8 guard usages.

### Task 11: Migrate the 6-usage files (one commit each)

Three files: `setup_http.rs`, `messages_http.rs`, `media_processing.rs`. Migrate each in a separate commit (so review can be per-file).

### Task 12: Migrate the 5-usage files (one commit each)

Three files: `bot_intents.rs`, `dm_http.rs`, `channels_http.rs`.

### Task 13: Migrate the small files

Two files: `uploads_http.rs` (3 usages), `setup_concurrent_http.rs` (2 usages — already `#[ignore]`'d, but migrate for consistency). One commit each.

---

## Task 14: Final verification

**Files:** none (verification only)

- [ ] **Step 1: Compile entire test suite**

Run: `SQLX_OFFLINE=true cargo build --tests --workspace 2>&1 | tail -10`

Expected: clean build.

- [ ] **Step 2: Run lib tests**

Run: `SQLX_OFFLINE=true cargo nextest run -p vc-server --lib 2>&1 | tail -5`

Expected: same baseline pass count as Task 1 Step 2.

- [ ] **Step 3: Static check for unmigrated tests**

Run: `grep -rn "let mut guard = \(app\.cleanup_guard\|CleanupGuard::new\)" server/tests/integration/ | wc -l`

Record the count. Then:

Run: `grep -rn "guard\.cleanup()\.await" server/tests/integration/ | wc -l`

Expected: roughly equal counts. If the `cleanup().await` count is significantly lower, some tests were missed. Re-grep per file to find them.

- [ ] **Step 4: Verify CI gate stays green**

Run: `cargo fmt --check 2>&1 | tail -3`
Run: `SQLX_OFFLINE=true cargo clippy -p vc-server --tests -- -D warnings 2>&1 | tail -10`

Both must be clean.

- [ ] **Step 5: Push and create PR**

```bash
git push -u origin fix/cleanup-guard-test-flake
gh pr create --title "fix(test): eliminate CleanupGuard CI flake (#900/900 timeout)" --body "$(cat <<'EOF'
## Summary

Eliminates the recurring CI flake where Rust integration tests intermittently time out at nextest's 120s slow-timeout. Root cause: CleanupGuard::drop spawns a thread + tokio runtime + .join() with no timeout. When cleanup hangs, the test process holds for 120s and gets killed.

## Layers

**Layer 1 (commit 1)** — Bounded join in CleanupGuard::drop. 30-second polling join, then detach. Stopgap that immediately removes the failure mode.

**Layer 2 (commits 2-13)** — Add explicit cleanup(self).await method, migrate ~136 test sites across 16 files. Drop fallback now also prints a "dropped with N pending actions" warning so unmigrated tests are visible.

**Layer 3 (deferred)** — After ~2 weeks of clean main-branch CI, a follow-up commit will replace the Drop fallback's eprintln with panic!() to enforce the contract permanently. Tracked in a separate issue (link in this PR description).

## Test plan

- [x] cargo build --tests --workspace clean
- [x] cargo nextest run -p vc-server --lib clean
- [x] cargo fmt --check clean
- [x] cargo clippy -p vc-server --tests -- -D warnings clean
- [ ] CI Rust Tests passes 3 consecutive runs (verify in checks)
- [ ] Local stress test (20 nextest runs back-to-back) produces zero "did not complete within" warnings — the implementer should run this before merging

## Refs

- Spec: docs/superpowers/specs/2026-04-11-security-audit-followups-design.md (Topic 4)
- Related: #509 (same anti-pattern in setup tests)

🤖 Generated with [Claude Code](https://claude.com/claude-code)
EOF
)"
```

- [ ] **Step 6: Local stress test (recommended before merge)**

Run: `for i in {1..20}; do echo "=== run $i ==="; SQLX_OFFLINE=true cargo nextest run -p vc-server --lib 2>&1 | tail -3; done`

(Skip the integration tests since they need DB. The lib tests still exercise the helpers/mod.rs compilation paths.)

If you have a local DB available, also run integration tests:

Run: `for i in {1..20}; do echo "=== run $i ==="; cargo nextest run -p vc-server --test integration 2>&1 | grep -E "Summary|did not complete"; done`

Expected: zero "did not complete within" warnings across 20 runs. If ANY warning appears, pause and investigate the failing test before merging.

- [ ] **Step 7: Wait for CI**

Watch the PR's CI checks. The Rust Tests job should pass on the first run. If it doesn't, investigate the specific failure (which test? did it print a warning? check the slow-timeout pattern).

- [ ] **Step 8: After merge — file follow-up issue**

Create a tracking issue for the Layer 3 panic activation:

```bash
gh issue create --title "Follow-up: replace CleanupGuard Drop eprintln with panic!()" --body "After ~2 weeks of clean main-branch CI (no 'dropped with N pending actions' warnings), replace the eprintln in server/tests/integration/helpers/mod.rs::Drop with panic!() to permanently enforce the .cleanup().await contract.

Verify before flipping:
- grep main branch CI logs for 'dropped with N pending actions' over the last 2 weeks
- if zero hits, ship the panic
- if any hits, fix the specific test first

Spec: docs/superpowers/specs/2026-04-11-security-audit-followups-design.md (Topic 4 Layer 3)"
```

---

## Done criteria

- [ ] Layer 1 stopgap committed
- [ ] Layer 2 cleanup() method committed
- [ ] All 16 migration files committed (Tasks 3-13)
- [ ] Final verification (Task 14) passed
- [ ] PR opened
- [ ] CI Rust Tests green
- [ ] Follow-up issue filed for Layer 3 panic activation
