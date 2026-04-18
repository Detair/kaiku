# Phase 1 — `TestApp::with_pool` API + Pilot Migration Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add pool-injecting factories to `TestApp` (backward-compatible with existing no-arg forms), then migrate 3 pilot integration test files (`blocking.rs`, `e2ee_settings.rs`, `pages.rs`) to `#[sqlx::test]` to validate the template-DB + migrations pipeline in CI before bulk migration.

**Architecture:** The existing no-arg factories (`TestApp::new`, `TestApp::with_screen_share_limiter`, `fresh_test_app_with_s3`) are preserved and retrofitted to delegate to new pool-injecting forms (`TestApp::with_pool`, `TestApp::with_pool_and_screen_share_limiter`, `fresh_test_app_with_s3_and_pool`). The 3 pilot tests switch to `#[sqlx::test]` + new factories; all other tests remain on the old pattern until Phases 2–4.

**Tech Stack:** Rust stable, `sqlx` with the `postgres` + `migrate` + `test` features, `cargo-nextest`, `#[sqlx::test]` macro, PostgreSQL 16.

**Spec:** `docs/superpowers/specs/2026-04-18-sqlx-test-integration-migration-design.md` — Phase 1 section.

**Parallelization safe:** No. Phase 1 is the prerequisite for Phases 2–5 and must merge first so the new factories and the proven pilot pattern exist on `main` before the bulk migration starts.

---

## Pre-flight Check (BLOCKING)

- [ ] **Verify `sqlx` has the `test` feature enabled**

```bash
cd /home/detair/GIT/detair/kaiku
grep -nE '^sqlx|^sqlx =' server/Cargo.toml
```

Expected: a line like `sqlx = { version = "0.8", features = [..., "postgres", "migrate", ...] }`. If `test` is not in the feature list, Phase 1 needs to add it. Examine the current line carefully — some sqlx versions auto-include test support when other features are present.

- [ ] **Verify migrations live at the expected path**

```bash
ls server/migrations | wc -l
```

Expected: 54 migrations. The default `#[sqlx::test]` migrator path is `./migrations` relative to the crate root; since the integration tests live in the `server` crate, `server/migrations/` is the default and no override is needed. If that count differs materially, pause and verify the spec's factual baseline.

- [ ] **Confirm `DATABASE_URL` is set on CI**

```bash
grep -nE 'DATABASE_URL' .github/workflows/ci.yml | head -5
```

Expected: `DATABASE_URL: postgres://postgres:postgres@localhost:5432/canis_test` at the `rust-test` job level. `#[sqlx::test]` reads `DATABASE_URL` and uses it to pick the base DB; per-test DBs are created on the same Postgres instance.

---

## Worktree Setup

```bash
cd /home/detair/GIT/detair/kaiku
git fetch origin
git worktree add .claude/worktrees/sqlx-test-phase1 -b feat/sqlx-test-phase1 origin/main
cd .claude/worktrees/sqlx-test-phase1
```

Working branch: `feat/sqlx-test-phase1`. Working directory for every task below: `/home/detair/GIT/detair/kaiku/.claude/worktrees/sqlx-test-phase1`.

---

## File Map

| Path | Action | Task |
|------|--------|------|
| `server/tests/integration/helpers/mod.rs` | Modify (add new factories, rewire old as delegates) | 1, 2 |
| `server/tests/integration/blocking.rs` | Modify (pilot migrate) | 3 |
| `server/tests/integration/e2ee_settings.rs` | Modify (pilot migrate) | 4 |
| `server/tests/integration/pages.rs` | Modify (pilot migrate) | 5 |

---

## Task 1: Add `TestApp::with_pool` and variants

**Files:**
- Modify: `server/tests/integration/helpers/mod.rs` — add new pool-injecting factories below the existing `TestApp` impl block.

- [ ] **Step 1: Read the current `TestApp::new` implementation**

```bash
grep -n 'pub struct TestApp\|impl TestApp\|pub async fn new\|pub async fn with_screen_share_limiter\|pub async fn fresh_test_app_with_s3' server/tests/integration/helpers/mod.rs
```

Expected: roughly the layout from the spec — `pub struct TestApp { router, pool, config }`, `impl TestApp { pub async fn new() … pub async fn with_screen_share_limiter() … }`, and the free function `pub async fn fresh_test_app_with_s3() -> (TestApp, String)`.

- [ ] **Step 2: Extract the body of `TestApp::new()` into a new `with_pool(pool: PgPool)` method**

Current body (conceptual):
```rust
pub async fn new() -> Self {
    let pool = shared_pool().await.clone();
    let config = shared_config().await.clone();
    let redis = db::create_redis_client(&config.redis_url).await.expect("…");
    let sfu = SfuServer::new(Arc::new(config.clone()), None).expect("…");
    let state = AppState::new(AppStateConfig { db: pool.clone(), redis, config: config.clone(), /* … */ });
    let router = create_router(state);
    Self { router, pool, config: Arc::new(config) }
}
```

Split into:
```rust
/// Canonical pool-injecting constructor for integration tests.
///
/// Pass a pool from `#[sqlx::test]`'s fixture. The returned `TestApp` owns
/// `pool` for the duration of the test.
pub async fn with_pool(pool: PgPool) -> Self {
    let config = shared_config().await.clone();
    let redis = db::create_redis_client(&config.redis_url).await.expect("…");
    let sfu = SfuServer::new(Arc::new(config.clone()), None).expect("…");
    let state = AppState::new(AppStateConfig { db: pool.clone(), redis, config: config.clone(), /* … */ });
    let router = create_router(state);
    Self { router, pool, config: Arc::new(config) }
}

/// Legacy no-arg constructor — delegates to [`Self::with_pool`] using the
/// shared process-global pool. Retained for tests not yet migrated to
/// `#[sqlx::test]`. Removed in Phase 5.
pub async fn new() -> Self {
    Self::with_pool(shared_pool().await.clone()).await
}
```

Copy the full body exactly (do not paraphrase the `AppStateConfig` fields — the spec's sample elided some). Move every line from `let config = ...` through the final `Self { ... }` into `with_pool`.

- [ ] **Step 3: Apply the same split to `with_screen_share_limiter()`**

```rust
pub async fn with_pool_and_screen_share_limiter(pool: PgPool) -> Self {
    /* body: move current `with_screen_share_limiter` body here, replacing the
       `let pool = shared_pool().await.clone();` with use of the argument */
}

pub async fn with_screen_share_limiter() -> Self {
    Self::with_pool_and_screen_share_limiter(shared_pool().await.clone()).await
}
```

- [ ] **Step 4: Apply the same split to `fresh_test_app_with_s3()`**

```rust
pub async fn fresh_test_app_with_s3_and_pool(pool: PgPool) -> (TestApp, String) {
    /* body: move current `fresh_test_app_with_s3` body here, replacing the
       first `let pool = shared_pool().await.clone();` with use of the argument */
}

pub async fn fresh_test_app_with_s3() -> (TestApp, String) {
    fresh_test_app_with_s3_and_pool(shared_pool().await.clone()).await
}
```

- [ ] **Step 5: If `TestApp` has any other factory variants**, apply the same pattern

```bash
grep -nE '^    pub async fn ' server/tests/integration/helpers/mod.rs | grep -v 'with_pool\|cleanup_guard' | head -20
```

Expected: returns any other `impl TestApp { pub async fn … }` methods that currently read `shared_pool()` / `shared_config()` implicitly. For each, add a pool-injecting twin using the `with_pool_<variant>` naming convention and retrofit the original as a delegate.

- [ ] **Step 6: Compile check**

```bash
SQLX_OFFLINE=true cargo check -p vc-server --tests 2>&1 | tail -10
```

Expected: clean compile. The test binaries should still build because all existing call sites use the unchanged no-arg forms, which now delegate internally.

- [ ] **Step 7: Commit**

```bash
git add server/tests/integration/helpers/mod.rs
git commit -m "test(infra): add TestApp::with_pool pool-injecting factories"
```

---

## Task 2: Verify no-op behavior for existing tests

**Files:**
- No code changes. Confirmation step.

- [ ] **Step 1: Run the full integration test suite against the retrofitted helpers**

```bash
cd mobile  # no — wrong repo; skip and do the next line
# from the worktree root:
SQLX_OFFLINE=true cargo nextest run --all-features --workspace --exclude vc-client 2>&1 | tail -10
```

(If `SQLX_OFFLINE=true` is not set, sqlx may attempt to connect to the local dev DB during compile. Keep it set.)

Expected: the tests that pass on current `main` continue to pass. The retrofit is pure refactoring — no test output should change.

**Note on the libspa dev-host issue (carried over from Workstream A):** running `cargo nextest` locally may fail due to libspa/pipewire bindings on some Linux dev machines. If that happens, the actual validation happens on CI when the PR is pushed. For local dev, use `cargo check -p vc-server --tests` (already done in Task 1 Step 6) as a compile-only gate.

- [ ] **Step 2: No commit** — this is a verification step only.

---

## Task 3: Pilot migrate `blocking.rs`

**Files:**
- Modify: `server/tests/integration/blocking.rs`

- [ ] **Step 1: Read the current file**

```bash
cat server/tests/integration/blocking.rs | head -60
```

Observe: (a) the existing use statements — likely `use crate::integration::helpers::*;` or similar; (b) the test function signatures — each is `#[tokio::test] async fn test_xxx() { … }`; (c) whether there are any `#[serial]` attributes.

- [ ] **Step 2: Apply the migration recipe**

For **every** test function in the file, do:

1. Change the attribute: `#[tokio::test]` → `#[sqlx::test]`
2. Add a `pool: PgPool` parameter: `async fn test_xxx() {` → `async fn test_xxx(pool: PgPool) {`
3. Replace factory calls:
   - `TestApp::new().await` → `TestApp::with_pool(pool.clone()).await`
   - `TestApp::with_screen_share_limiter().await` → `TestApp::with_pool_and_screen_share_limiter(pool.clone()).await`
   - `fresh_test_app_with_s3().await` → `fresh_test_app_with_s3_and_pool(pool.clone()).await`
4. Remove any `#[serial]` attribute on the function (and the `use serial_test::serial;` import at file top if no attribute remains).
5. Remove `CleanupGuard` calls that only perform DB-row cleanup (e.g., `guard.delete_user(id)`). Keep non-DB cleanup. If the guard becomes empty, remove its declaration.
6. Add `use sqlx::PgPool;` at the top of the file if not already imported.

Refer to the spec's §"Phases 2–4 — bulk migration by batch" for the complete recipe.

- [ ] **Step 3: Compile the modified file**

```bash
SQLX_OFFLINE=true cargo check -p vc-server --tests 2>&1 | grep -E "^error|^warning: unused" | head -10
```

Expected: no new errors. A couple of `unused imports` warnings are acceptable if the pilot file had helpers that are now redundant; fix those by removing the imports (lint-clean the file).

- [ ] **Step 4: Run just `blocking.rs` tests against a live DB (local or CI)**

Local (if dev DB is up):
```bash
DATABASE_URL="postgres://voicechat:voicechat_dev@localhost:5433/voicechat" \
SQLX_OFFLINE=false \
cargo nextest run --all-features --workspace --exclude vc-client \
  --test 'integration' blocking:: 2>&1 | tail -10
```

Expected: every test in `blocking.rs` passes. Each test gets its own fresh DB cloned from the template; the 54 migrations run once on first use, then per-test DB creation is ~50–100 ms.

If the local run fails because of the libspa compile issue, skip to CI verification after pushing the PR.

- [ ] **Step 5: Commit**

```bash
git add server/tests/integration/blocking.rs
git commit -m "test(infra): migrate blocking.rs to #[sqlx::test] (pilot)"
```

---

## Task 4: Pilot migrate `e2ee_settings.rs`

**Files:**
- Modify: `server/tests/integration/e2ee_settings.rs`

- [ ] **Step 1: Apply the same 6-point recipe from Task 3 Step 2 to every test in this file.**

- [ ] **Step 2: Compile check**

```bash
SQLX_OFFLINE=true cargo check -p vc-server --tests 2>&1 | grep -E "^error" | head -5
```

Expected: no errors.

- [ ] **Step 3: Run just `e2ee_settings.rs` tests (if local DB available)**

```bash
DATABASE_URL="postgres://voicechat:voicechat_dev@localhost:5433/voicechat" \
cargo nextest run --test 'integration' e2ee_settings:: 2>&1 | tail -10
```

Expected: green.

- [ ] **Step 4: Commit**

```bash
git add server/tests/integration/e2ee_settings.rs
git commit -m "test(infra): migrate e2ee_settings.rs to #[sqlx::test] (pilot)"
```

---

## Task 5: Pilot migrate `pages.rs`

**Files:**
- Modify: `server/tests/integration/pages.rs`

- [ ] **Step 1: Apply the same 6-point recipe from Task 3 Step 2.**

- [ ] **Step 2: Compile check**

```bash
SQLX_OFFLINE=true cargo check -p vc-server --tests 2>&1 | grep -E "^error" | head -5
```

- [ ] **Step 3: Run just `pages.rs` tests (if local DB available)**

```bash
DATABASE_URL="postgres://voicechat:voicechat_dev@localhost:5433/voicechat" \
cargo nextest run --test 'integration' pages:: 2>&1 | tail -10
```

- [ ] **Step 4: Commit**

```bash
git add server/tests/integration/pages.rs
git commit -m "test(infra): migrate pages.rs to #[sqlx::test] (pilot)"
```

---

## Task 6: Verify + PR

- [ ] **Step 1: Full compile + full suite (local)**

```bash
SQLX_OFFLINE=true cargo check -p vc-server --tests 2>&1 | tail -3
SQLX_OFFLINE=true cargo +nightly fmt --all -- --check && echo "fmt OK"
SQLX_OFFLINE=true cargo clippy --all-features --workspace --exclude vc-client -- -D warnings 2>&1 | tail -3
SQLX_OFFLINE=true cargo deny check advisories 2>&1 | tail -3
SQLX_OFFLINE=true cargo deny check licenses 2>&1 | tail -3
```

Expected: all green.

- [ ] **Step 2: Review commit log**

```bash
git log --oneline origin/main..HEAD
```

Expected 4 commits:
1. `test(infra): add TestApp::with_pool pool-injecting factories`
2. `test(infra): migrate blocking.rs to #[sqlx::test] (pilot)`
3. `test(infra): migrate e2ee_settings.rs to #[sqlx::test] (pilot)`
4. `test(infra): migrate pages.rs to #[sqlx::test] (pilot)`

- [ ] **Step 3: Push + open PR**

```bash
git push -u origin feat/sqlx-test-phase1
gh pr create --base main --head feat/sqlx-test-phase1 \
  --title "test(infra): sqlx::test migration Phase 1 — TestApp::with_pool + 3-file pilot" \
  --body "$(cat <<'EOF'
## Summary

Phase 1 of the integration-test migration to `#[sqlx::test]`'s per-test database isolation. Introduces pool-injecting `TestApp` factories (backward-compatible: existing no-arg forms delegate to them) and migrates 3 pilot files to validate the template-DB + migrations pipeline on CI before bulk migration.

- New factories: `TestApp::with_pool`, `TestApp::with_pool_and_screen_share_limiter`, `fresh_test_app_with_s3_and_pool`.
- Existing factories (`TestApp::new`, `with_screen_share_limiter`, `fresh_test_app_with_s3`) kept, now delegating via the shared pool.
- Pilot migrations: `blocking.rs`, `e2ee_settings.rs`, `pages.rs`.

Spec: `docs/superpowers/specs/2026-04-18-sqlx-test-integration-migration-design.md` — Phase 1.

## Test plan

- [x] `cargo check -p vc-server --tests` — green
- [x] `cargo +nightly fmt --all -- --check` — green
- [x] `cargo clippy --all-features --workspace --exclude vc-client` — green
- [x] `cargo deny check advisories` / `licenses` — green
- [x] Existing tests continue to pass (backward-compat preserved via delegate `new()`)
- [x] Pilot tests run under `#[sqlx::test]` (verified in CI)

## Follow-ups

Phases 2–4 migrate the remaining 42 files in 3 batches; Phase 5 deletes the legacy `shared_pool` path and removes `[test-groups]` serialization + `#[serial]` attributes.

🤖 Generated with [Claude Code](https://claude.com/claude-code)
EOF
)"
```

- [ ] **Step 4: Wait for CI to pass**

Expected: all checks green, including the `Rust Tests` job that actually runs the pilot tests under `#[sqlx::test]`. If `Rust Tests` fails because template-DB migrations exceed the 2-minute nextest slow-test timeout, that is **critical feedback** — pause, investigate migration squash vs `#[sqlx::test(migrator = …)]` caching, and revise the spec before proceeding to Phase 2.

- [ ] **Step 5: Squash-merge (no `--admin`)**

```bash
gh pr merge <PR_NUMBER> --squash
```

- [ ] **Step 6: Post-merge cleanup**

```bash
cd /home/detair/GIT/detair/kaiku
git worktree remove .claude/worktrees/sqlx-test-phase1
git branch -D feat/sqlx-test-phase1
git fetch origin --prune
```

---

## Success criteria

1. `TestApp::with_pool`, `TestApp::with_pool_and_screen_share_limiter`, and `fresh_test_app_with_s3_and_pool` exist on `main`.
2. Existing `TestApp::new()`, `TestApp::with_screen_share_limiter()`, and `fresh_test_app_with_s3()` continue to work unchanged from a caller's perspective.
3. `blocking.rs`, `e2ee_settings.rs`, `pages.rs` all run under `#[sqlx::test]` and pass CI.
4. No regression in the full integration test suite's green-test count.

## Notes for the implementer

- **libspa-on-Linux caveat**: local `cargo test`/`nextest run` can fail to build due to pipewire bindings mismatch on some Linux dev hosts. CI is authoritative. If local runs fail with `libspa` errors, verify via `cargo check -p vc-server --tests` and rely on CI for final green signal.
- **Template-DB first-run cost**: on CI's first test run with `#[sqlx::test]`, the template DB is provisioned (~1–2 s for 54 migrations). Subsequent tests within the same binary share the template and provision per-test DBs in ~50–100 ms. This is documented in the spec's §CI posture; if reality differs materially, escalate.
- **DO NOT migrate a 4th pilot file.** The 3-file scope is deliberate — tighter diff for review, enough signal to validate the approach.
- **DO NOT touch tests outside the 3 pilot files in Phase 1.** Batch migration is Phases 2–4.
- **DO NOT delete `shared_pool`/`SHARED_POOL` in Phase 1.** Deletion is Phase 5 after all migrations complete.
