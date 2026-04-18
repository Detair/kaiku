# Phase 5 — Legacy Path Deletion Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Delete the shared-pool legacy path now that every integration test uses `#[sqlx::test]`. Removes `SHARED_POOL`/`shared_pool`/`SHARED_CONFIG`/`shared_config`, the legacy `TestApp` no-arg factories, the `[test-groups].setup-state` nextest entry, all remaining `#[serial]` attributes and `serial_test` imports under `server/tests/`, and `CleanupGuard`'s DB-row cleanup methods. Updates contributor-facing docs. Ships one CHANGELOG `### Changed` entry.

**Architecture:** Pure deletion + documentation refresh. No functional change to what tests assert — only infrastructure that is now provably unused.

**Tech Stack:** Same as Phases 1–4.

**Spec:** `docs/superpowers/specs/2026-04-18-sqlx-test-integration-migration-design.md` — Phase 5 section.

**Parallelization safe:** No. Phase 5 requires Phases 1–4 all merged, verified by a grep invariant below.

---

## Pre-flight Check (BLOCKING)

- [ ] **Verify zero `#[tokio::test]` remains in integration tests**

```bash
cd /home/detair/GIT/detair/kaiku
git fetch origin
git show origin/main --stat | head -1  # just to confirm we're on latest origin/main
grep -rln '#\[tokio::test' server/tests/integration/ 2>&1 | head -5
```

Expected: no output from the grep. If any file still has `#[tokio::test]`, **STOP** — a previous batch did not complete. Identify the file, open a remediation PR to migrate it, then proceed.

- [ ] **Verify the Phase 1 new factories exist on `main`** (sanity check — should always be true at this point)

```bash
git show origin/main:server/tests/integration/helpers/mod.rs | grep -c 'with_pool'
```

Expected: ≥ 3 matches (from Phase 1's additions).

- [ ] **Verify no non-test code depends on the helpers' legacy factories**

```bash
grep -rnE 'shared_pool|shared_config|TestApp::new\(' server/src/ 2>&1 | head -5
grep -rnE 'shared_pool|shared_config|TestApp::new\(' server/benches/ 2>&1 | head -5
```

Expected: no matches (helpers are test-only; nothing outside `tests/` should reference them). If anything turns up, escalate — Phase 5 must not break a non-test caller.

---

## Worktree Setup

```bash
cd /home/detair/GIT/detair/kaiku
git worktree add .claude/worktrees/sqlx-test-phase5 -b chore/sqlx-test-phase5-cleanup origin/main
cd .claude/worktrees/sqlx-test-phase5
```

---

## File Map

| Path | Action | Task |
|------|--------|------|
| `server/tests/integration/helpers/mod.rs` | Delete `SHARED_POOL`, `shared_pool`, `SHARED_CONFIG`, `shared_config`, legacy no-arg factories, `CleanupGuard` DB-row methods | 1, 2 |
| `.config/nextest.toml` | Delete `[test-groups]` block + its `[[profile.default.overrides]]` | 3 |
| Any file importing `serial_test` under `server/tests/` | Remove imports + any stray `#[serial]` | 4 |
| `docs/developer-guide/testing/*.md` (if exists) | Update to document `#[sqlx::test]` as canonical | 5 |
| `server/AGENTS.md`, `server/tests/AGENTS.md` (if exists) | Same | 5 |
| `CHANGELOG.md` | Add `### Changed` entry under `[Unreleased]` | 6 |

---

## Task 1: Delete `SHARED_POOL`, `SHARED_CONFIG`, and related helpers

**Files:**
- Modify: `server/tests/integration/helpers/mod.rs`

- [ ] **Step 1: Locate the targets**

```bash
grep -nE 'SHARED_POOL|SHARED_CONFIG|shared_pool\(\)|shared_config\(\)|pub async fn new\(\)|pub async fn with_screen_share_limiter\(\)|pub async fn fresh_test_app_with_s3\(\)' server/tests/integration/helpers/mod.rs
```

Expected: matches for:
- `static SHARED_POOL: OnceCell<PgPool> = OnceCell::const_new();`
- `static SHARED_CONFIG: OnceCell<Config> = OnceCell::const_new();`
- `pub async fn shared_pool() -> &'static PgPool { … }`
- `pub async fn shared_config() -> &'static Config { … }`
- `pub async fn new() -> Self { Self::with_pool(shared_pool().await.clone()).await }` (under `impl TestApp`)
- `pub async fn with_screen_share_limiter() -> Self { … }` (the legacy no-arg delegate)
- `pub async fn fresh_test_app_with_s3() -> (TestApp, String) { … }` (the legacy no-arg delegate)

- [ ] **Step 2: Delete them**

Remove each of the items listed in Step 1. Also remove any `use` statements that are now unused (commonly `use tokio::sync::OnceCell;` if nothing else in the file uses it).

Keep:
- `with_pool`, `with_pool_and_screen_share_limiter`, `fresh_test_app_with_s3_and_pool`
- All test-fixture builder helpers (`create_test_user`, `create_guild`, `insert_message`, …)
- `CleanupGuard` struct itself (DB-row methods are removed in Task 2)

- [ ] **Step 3: Compile check**

```bash
SQLX_OFFLINE=true cargo check -p vc-server --tests 2>&1 | grep -E "^error" | head -10
```

Expected: no errors. Every test in the integration crate should now use the new `with_pool` factories — confirming the deletion is safe.

If errors appear like "cannot find function `shared_pool` in this scope" in some test file, that file was not properly migrated in Phases 2–4. STOP, migrate that file per the recipe, then retry.

- [ ] **Step 4: Commit**

```bash
git add server/tests/integration/helpers/mod.rs
git commit -m "test(infra): delete SHARED_POOL + legacy no-arg TestApp factories"
```

---

## Task 2: Prune `CleanupGuard` DB-row cleanup methods

**Files:**
- Modify: `server/tests/integration/helpers/mod.rs`

- [ ] **Step 1: Audit `CleanupGuard`'s methods**

```bash
awk '/^impl CleanupGuard \{/,/^\}/' server/tests/integration/helpers/mod.rs | grep -E '^\s*pub fn [a-z]'
```

Expected: a list of method signatures — `delete_user`, `delete_guild`, `delete_dm_channel`, `delete_connection_data`, `restore_setup_complete`, and possibly others. Also any non-DB methods (e.g., S3 bucket cleanup).

- [ ] **Step 2: Classify each method as "DB-row" or "non-DB"**

DB-row methods (example list — verify against actual code):
- `delete_user` — row deletion, now redundant (sqlx::test drops DB)
- `delete_guild` — row deletion, redundant
- `delete_dm_channel` — row deletion, redundant
- `delete_connection_data` — row deletion, redundant
- `restore_setup_complete` — singleton row mutation, redundant

Non-DB methods (keep):
- Any method that operates on S3 buckets, Redis keys, or external resources.

If `CleanupGuard` has zero non-DB methods after audit, delete the struct entirely along with the `type CleanupAction = …;` typedef and the `impl Drop for CleanupGuard` block.

- [ ] **Step 3: Delete DB-row methods (and the generic `add_action` / `register` if it's only reachable via DB-row helpers)**

Keep only what non-DB methods need.

- [ ] **Step 4: If `CleanupGuard` becomes empty, delete it fully**

```bash
grep -n 'CleanupGuard\|cleanup_guard' server/tests/integration/
```

Expected after full deletion: no matches outside of the deletion itself. If any test file still references `cleanup_guard()` or `CleanupGuard::new(...)` (carried over from Phase 2–4 batches), remove those calls — they were orphans the batches missed.

- [ ] **Step 5: Compile check**

```bash
SQLX_OFFLINE=true cargo check -p vc-server --tests 2>&1 | grep -E "^error" | head -5
```

Expected: clean.

- [ ] **Step 6: Commit**

```bash
git add server/tests/integration/helpers/mod.rs
git add server/tests/integration/  # catch any orphan CleanupGuard calls in test files
git commit -m "test(infra): prune CleanupGuard DB-row cleanup methods"
```

---

## Task 3: Remove `[test-groups].setup-state` from nextest config

**Files:**
- Modify: `.config/nextest.toml`

- [ ] **Step 1: Read the current config**

```bash
cat .config/nextest.toml
```

Expected: a `[test-groups] setup-state = { max-threads = 1 }` block and a `[[profile.default.overrides]]` that filters `setup_http::` and `setup_concurrent_http::` into that group.

- [ ] **Step 2: Delete the `[test-groups]` block and the related override**

Keep `[profile.default]` (with `slow-timeout`, `fail-fast`). Remove:
- The `[test-groups]` section and any members under it.
- The `[[profile.default.overrides]]` that assigns tests to `setup-state`.

If, after the deletion, no override remains, the file reduces to only `[profile.default]` plus its settings — that's correct.

- [ ] **Step 3: Sanity check nextest still parses the file**

```bash
cargo nextest list --help 2>&1 | head -1
cargo nextest list 2>&1 | head -5
```

Expected: nextest lists tests without a parse error on `.config/nextest.toml`. If it errors on the file, the deletion was over-zealous — restore `[profile.default]` as needed.

- [ ] **Step 4: Commit**

```bash
git add .config/nextest.toml
git commit -m "chore(infra): remove [test-groups].setup-state nextest entry"
```

---

## Task 4: Remove remaining `serial_test` imports and `#[serial]` attrs

**Files:**
- Modify: any file under `server/tests/` that still imports `serial_test` or applies `#[serial]`.

- [ ] **Step 1: Find stragglers**

```bash
grep -rnE 'use serial_test|#\[serial' server/tests/ 2>&1 | head -20
```

Expected: this should mostly be empty (each phase's recipe removes `#[serial]` on migrated tests). Any remaining hits are (a) files where the import was left as dead code after the attribute was removed, or (b) tests that the batches somehow missed.

- [ ] **Step 2: Delete each straggler**

For a dead `use serial_test::…;`: delete the line.
For any residual `#[serial]` attribute: delete it.

- [ ] **Step 3: Check that `serial_test` is no longer needed as a dev-dependency**

```bash
grep -nE 'serial_test|serial-test' server/Cargo.toml server/Cargo.toml.orig 2>/dev/null
grep -rnE 'use serial_test' server/ 2>&1 | head -5
```

Expected: no matches in `server/src/` or `server/tests/`. If `server/Cargo.toml` declares `serial_test` as a dev-dep, remove that declaration too.

- [ ] **Step 4: Compile check**

```bash
SQLX_OFFLINE=true cargo check -p vc-server --tests 2>&1 | grep -E "^error" | head -5
```

- [ ] **Step 5: Commit**

```bash
git add -A server/tests/ server/Cargo.toml
git commit -m "chore(infra): remove serial_test dep and residual #[serial] attributes"
```

---

## Task 5: Update contributor docs

**Files:**
- Modify: `docs/developer-guide/testing/*.md` (any file documenting integration test patterns)
- Modify: `server/AGENTS.md` and `server/tests/AGENTS.md` if they exist
- Possibly modify: `server/src/AGENTS.md` if it references the integration-test pattern

- [ ] **Step 1: Locate doc references to the old pattern**

```bash
grep -rn 'shared_pool\|SHARED_POOL\|shared_config\|TestApp::new()\|CleanupGuard\|\[test-groups\]\|setup-state\|#\[serial\]' docs/ server/AGENTS.md server/tests/ 2>/dev/null | grep '\.md:' | head -20
```

Expected: a handful of matches in developer-guide docs and AGENTS files.

- [ ] **Step 2: Update each doc to point at `#[sqlx::test]`**

Concretely, change sentences like:
- "Integration tests use `TestApp::new()` with a shared pool." → "Integration tests use `#[sqlx::test]`; each test receives a fresh `PgPool` via `TestApp::with_pool(pool)`."
- "Use `CleanupGuard` to delete rows at test end." → (delete the sentence if DB-row cleanup was the only use case; otherwise narrow it to non-DB cleanup).
- "Setup-state tests are serialized via nextest `[test-groups]`." → (delete the sentence; per-test DB makes serialization unnecessary).

- [ ] **Step 3: Add a short "Integration test patterns" section to `server/tests/AGENTS.md`** (create the file if it doesn't exist) describing the canonical pattern:

```markdown
## Integration test pattern

Each integration test gets its own isolated PostgreSQL database via `#[sqlx::test]`:

```rust
#[sqlx::test]
async fn test_something(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    // … test body
}
```

The pool is freshly created from a template DB that has all migrations applied; the test's DB is dropped automatically after the test returns. There is no shared state between tests.

For tests that need S3, use `fresh_test_app_with_s3_and_pool(pool)` — it returns a `TestApp` plus a unique bucket name, with the bucket's teardown deferred to a `CleanupGuard`.

Do NOT use `#[tokio::test]` for integration tests. Do NOT use `#[serial]` — per-test DB isolation is absolute.
```

- [ ] **Step 4: Commit**

```bash
git add docs/ server/AGENTS.md server/tests/ 2>/dev/null  # commit whatever paths exist
git commit -m "docs(infra): document #[sqlx::test] as canonical integration-test pattern"
```

---

## Task 6: CHANGELOG entry

**Files:**
- Modify: `CHANGELOG.md`

- [ ] **Step 1: Locate the `[Unreleased]` → `### Changed` section**

```bash
grep -nE '^## \[Unreleased\]|^### Changed' CHANGELOG.md | head -5
```

- [ ] **Step 2: Add the entry**

Append (or prepend, matching the repo's convention) under the topmost `### Changed` block within `[Unreleased]`:

```markdown
- Integration tests now use `#[sqlx::test]`'s per-test database isolation; the shared-pool model that produced sporadic Postgres deadlocks in CI is retired. Contributor-facing documentation is in `docs/developer-guide/testing/` and `server/tests/AGENTS.md`.
```

If `[Unreleased]` has no `### Changed` subsection yet, add one (alphabetical with the other `### …` headings per Keep-a-Changelog).

- [ ] **Step 3: Commit**

```bash
git add CHANGELOG.md
git commit -m "docs(infra): CHANGELOG entry for sqlx::test integration-test migration"
```

---

## Final Verification (before opening PR)

- [ ] **Invariant greps**

```bash
grep -rn 'shared_pool\|SHARED_POOL\|shared_config\|SHARED_CONFIG' server/tests/ 2>&1 | head -5
grep -rn '#\[serial\]\|use serial_test' server/tests/ 2>&1 | head -5
grep -c '\[test-groups\]' .config/nextest.toml
```

Expected:
- First grep: no output.
- Second grep: no output.
- Third grep: `0`.

- [ ] **Full check-fmt-clippy-deny sweep**

```bash
SQLX_OFFLINE=true cargo check -p vc-server --tests 2>&1 | tail -3
cargo +nightly fmt --all -- --check && echo "fmt OK"
SQLX_OFFLINE=true cargo clippy --all-features --workspace --exclude vc-client -- -D warnings 2>&1 | tail -3
cargo deny check advisories 2>&1 | tail -3
cargo deny check licenses 2>&1 | tail -3
```

Expected: all green.

- [ ] **Commit log**

```bash
git log --oneline origin/main..HEAD
```

Expected 6 commits:
1. `test(infra): delete SHARED_POOL + legacy no-arg TestApp factories`
2. `test(infra): prune CleanupGuard DB-row cleanup methods`
3. `chore(infra): remove [test-groups].setup-state nextest entry`
4. `chore(infra): remove serial_test dep and residual #[serial] attributes`
5. `docs(infra): document #[sqlx::test] as canonical integration-test pattern`
6. `docs(infra): CHANGELOG entry for sqlx::test integration-test migration`

- [ ] **Push + open PR**

```bash
git push -u origin chore/sqlx-test-phase5-cleanup
gh pr create --base main --head chore/sqlx-test-phase5-cleanup \
  --title "chore(infra): sqlx::test migration Phase 5 — delete legacy path + update docs" \
  --body "$(cat <<'EOF'
## Summary

Final phase of the integration-test migration to `#[sqlx::test]`. Deletes the shared-pool legacy path now that every integration test uses per-test DB isolation.

Changes:
- Delete `SHARED_POOL`/`shared_pool`, `SHARED_CONFIG`/`shared_config`, legacy `TestApp::new()`/`with_screen_share_limiter()`/`fresh_test_app_with_s3()` no-arg wrappers.
- Prune `CleanupGuard` DB-row cleanup methods; keep only non-DB cleanup (e.g., S3).
- Remove `[test-groups].setup-state` from `.config/nextest.toml` — per-test DB obsoletes cross-process serialization.
- Drop `serial_test` dev-dep + any residual `#[serial]` attributes / imports under `server/tests/`.
- Document `#[sqlx::test]` as the canonical integration-test pattern in `server/tests/AGENTS.md` and `docs/developer-guide/testing/`.
- CHANGELOG `### Changed` entry.

Spec: `docs/superpowers/specs/2026-04-18-sqlx-test-integration-migration-design.md` — Phase 5.

## Test plan

- [x] `grep -rn 'shared_pool\|SHARED_POOL\|shared_config\|SHARED_CONFIG' server/tests/` returns empty
- [x] `grep -rn '#\[serial\]\|use serial_test' server/tests/` returns empty
- [x] `.config/nextest.toml` no longer has `[test-groups]`
- [x] Full integration test suite passes on CI under `-j 4` parallelism
- [x] fmt / clippy / deny sweep green

## Expected post-merge outcome

- The 3 observed deadlock-flake test functions (`test_first_user_detection_works`, `test_guild_search_xss_content_returned_verbatim`, `test_upload_requires_auth`) run cleanly under `-j 4`.
- Zero `40P01` (deadlock_detected) occurrences in `main` CI logs going forward. Verified against 10 consecutive runs before declaring the workstream complete.

🤖 Generated with [Claude Code](https://claude.com/claude-code)
EOF
)"
```

- [ ] **Wait for CI green and squash-merge (no `--admin`)**

```bash
gh pr checks <PR_NUMBER> --watch
gh pr merge <PR_NUMBER> --squash
```

- [ ] **Post-merge cleanup**

```bash
cd /home/detair/GIT/detair/kaiku
git worktree remove .claude/worktrees/sqlx-test-phase5
git branch -D chore/sqlx-test-phase5-cleanup
git fetch origin --prune
```

---

## Success criteria

1. `grep -rn 'shared_pool\|SHARED_POOL\|shared_config\|SHARED_CONFIG' server/tests/` returns empty.
2. `grep -rn '#\[serial\]\|use serial_test' server/tests/` returns empty.
3. `.config/nextest.toml` no longer contains `[test-groups]`.
4. `server/tests/AGENTS.md` documents `#[sqlx::test]` as canonical with a copy-pasteable example.
5. `CHANGELOG.md` `### Changed` has the Phase 5 entry under `[Unreleased]`.
6. Full integration test suite passes on CI under `-j 4` for the PR.

## Post-merge workstream validation (tracked separately from this PR)

- [ ] Observe `main` CI for 1 week post-merge.
- [ ] Zero `40P01` deadlock failures in that window.
- [ ] The 3 flagged test functions run cleanly under `-j 4` across 10 consecutive `main` runs.

If any of those validations fail, open a remediation issue — do NOT revert this PR. The structural fix should be correct; any residual flake indicates a different root cause (e.g., a different shared resource like Redis) that was latent under the old shared-pool pattern.

## Notes for the implementer

- **Phase 5 is a deletion PR.** If you find yourself writing a new helper or adding new behavior, you've wandered out of scope. Undo and ask.
- **Don't leave `CleanupGuard` alive if it has no non-DB users.** If the audit in Task 2 Step 2 finds zero non-DB methods, delete the entire struct. YAGNI.
- **Don't split this into smaller PRs.** The 6 commits tell a coherent deletion story; splitting fragments reviewer context.
- **If CI goes red on deletion**, the most likely cause is an orphaned reference from a batch that "migrated" a test but left a helper call behind. Grep for the specific symbol (`shared_pool`, `CleanupGuard::new`, etc.), fix in-place, commit as an amendment to Task 1 or 2 rather than a new commit.
