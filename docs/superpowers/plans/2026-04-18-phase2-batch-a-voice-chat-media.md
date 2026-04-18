# Phase 2 — Batch A: Voice/Chat/Media Migration Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Migrate 14 voice/chat/media integration test files to `#[sqlx::test]` using the Phase-1-established `TestApp::with_pool` API.

**Architecture:** Mechanical per-file rewrite applying the Phase 1 migration recipe. Each file is converted in isolation; test count per file ranges from a few to a few dozen. All files in this batch share the same transformation — no per-file custom logic.

**Tech Stack:** Same as Phase 1.

**Spec:** `docs/superpowers/specs/2026-04-18-sqlx-test-integration-migration-design.md` — Phases 2–4 section.
**Recipe source:** `docs/superpowers/plans/2026-04-18-phase1-testapp-api-plus-pilot.md` Task 3 Step 2 — the 6-point mechanical rewrite. **Read that first.**

**Parallelization safe:** Phase 2 depends only on Phase 1 landing on `main`. Phases 3 and 4 can open in parallel with Phase 2 once Phase 1 merges (different files, no conflicts) — but sequential serialization is recommended to keep review load bounded per reviewer.

---

## Pre-flight Check (BLOCKING)

- [ ] **Verify Phase 1 has merged to `main`**

```bash
cd /home/detair/GIT/detair/kaiku
git fetch origin
git log origin/main --oneline | grep 'sqlx-test.*Phase 1\|TestApp::with_pool\|pilot' | head -3
```

Expected: at least one commit mentioning Phase 1 / `TestApp::with_pool` / pilot. If absent, **STOP** and land Phase 1 first.

- [ ] **Verify the new factories exist on `main`**

```bash
git show origin/main:server/tests/integration/helpers/mod.rs | grep -nE 'with_pool|fresh_test_app_with_s3_and_pool' | head -5
```

Expected: matches for `with_pool` and `with_pool_and_screen_share_limiter` (plus `fresh_test_app_with_s3_and_pool`). If none, Phase 1 did not land the factories correctly — escalate.

---

## Worktree Setup

```bash
cd /home/detair/GIT/detair/kaiku
git worktree add .claude/worktrees/sqlx-test-phase2 -b feat/sqlx-test-phase2 origin/main
cd .claude/worktrees/sqlx-test-phase2
```

---

## File Map

14 files, each receiving the Phase 1 recipe. One task per file keeps review granular and per-file failures isolated:

| Task | File |
|------|------|
| 1 | `server/tests/integration/voice_sfu.rs` |
| 2 | `server/tests/integration/voice_mute_enforcement.rs` |
| 3 | `server/tests/integration/voice_rate_limit.rs` |
| 4 | `server/tests/integration/channels_http.rs` |
| 5 | `server/tests/integration/channel_pins.rs` |
| 6 | `server/tests/integration/messages_http.rs` |
| 7 | `server/tests/integration/threads.rs` |
| 8 | `server/tests/integration/dm_http.rs` |
| 9 | `server/tests/integration/media_processing.rs` |
| 10 | `server/tests/integration/upload_limits.rs` |
| 11 | `server/tests/integration/uploads_http.rs` |
| 12 | `server/tests/integration/screenshare.rs` |
| 13 | `server/tests/integration/favorites.rs` |
| 14 | `server/tests/integration/custom_status.rs` |

---

## Migration Recipe (per file, applied in Tasks 1–14)

**For every test function in the target file:**

1. `#[tokio::test]` → `#[sqlx::test]`
2. `async fn test_xxx()` → `async fn test_xxx(pool: PgPool)`
3. `TestApp::new().await` → `TestApp::with_pool(pool.clone()).await`
4. `TestApp::with_screen_share_limiter().await` → `TestApp::with_pool_and_screen_share_limiter(pool.clone()).await`
5. `fresh_test_app_with_s3().await` → `fresh_test_app_with_s3_and_pool(pool.clone()).await`
6. Remove `#[serial]` attributes and `use serial_test::…;` if no other function in the file uses it.
7. `CleanupGuard`: drop DB-row cleanup calls (`guard.delete_user(id)`, `guard.restore_setup_complete(prev)`, etc.). Keep non-DB cleanup (S3 bucket teardown — only relevant for `uploads_http.rs` if it uses S3). If the guard has no remaining actions, delete its declaration.
8. Add `use sqlx::PgPool;` at the top of the file if missing.

---

## Task N: Migrate `<filename>` (applies to Tasks 1–14)

**Files:**
- Modify: `server/tests/integration/<filename>`

- [ ] **Step 1: Read the file and identify every test function**

```bash
grep -nE '#\[(tokio|sqlx)::test|^async fn test_|#\[serial' server/tests/integration/<filename>
```

Record: number of tests, whether `#[serial]` is used, whether the file already has any `#[sqlx::test]` (shouldn't — but verify).

- [ ] **Step 2: Apply the migration recipe (above) to every test**

Use the Edit tool, `sed`, or your editor. For each test, apply steps 1–8 mechanically. Do NOT change test body logic — only the attribute, signature, and factory calls.

- [ ] **Step 3: Remove unused `CleanupGuard` state + shared_pool references**

```bash
grep -n 'shared_pool\|CleanupGuard\|cleanup_guard\|delete_user(' server/tests/integration/<filename>
```

For each match: if it's a DB-row cleanup call (e.g., `guard.delete_user(id)`), delete it (sqlx::test drops the DB). If it's S3 or otherwise non-DB, keep it. If the file declared a guard only to delete rows, remove the `let mut guard = …;` line.

- [ ] **Step 4: Compile check**

```bash
SQLX_OFFLINE=true cargo check -p vc-server --tests 2>&1 | grep -E "^error|^warning: unused" | head -10
```

Expected: no new errors. Fix any "unused import" warning by deleting the unused import from the file.

- [ ] **Step 5: (Optional) Run just this file's tests against a live DB**

If a local DB is available:
```bash
DATABASE_URL="postgres://voicechat:voicechat_dev@localhost:5433/voicechat" \
cargo nextest run --test 'integration' <module_prefix>:: 2>&1 | tail -10
```

Expected: every test in the file passes. If the local run is blocked by libspa, defer to CI verification after push.

- [ ] **Step 6: Commit**

```bash
git add server/tests/integration/<filename>
git commit -m "test(infra): migrate <filename> to #[sqlx::test]"
```

**Repeat Steps 1–6 for every file in the Task N table above.**

---

## Final Verification (before opening PR)

- [ ] **Full compile + lint + deny + fmt**

```bash
SQLX_OFFLINE=true cargo check -p vc-server --tests 2>&1 | tail -3
SQLX_OFFLINE=true cargo +nightly fmt --all -- --check && echo "fmt OK"
SQLX_OFFLINE=true cargo clippy --all-features --workspace --exclude vc-client -- -D warnings 2>&1 | tail -3
SQLX_OFFLINE=true cargo deny check advisories 2>&1 | tail -3
SQLX_OFFLINE=true cargo deny check licenses 2>&1 | tail -3
```

Expected: all green.

- [ ] **Grep sweep for remaining `#[tokio::test]` in this batch**

```bash
for f in voice_sfu voice_mute_enforcement voice_rate_limit channels_http channel_pins messages_http threads dm_http media_processing upload_limits uploads_http screenshare favorites custom_status; do
    c=$(grep -c '#\[tokio::test' "server/tests/integration/$f.rs" 2>/dev/null || echo 0)
    [ "$c" -gt 0 ] && echo "  $f.rs: $c remaining #[tokio::test] — FIX BEFORE PR"
done
```

Expected: no output (every `#[tokio::test]` in these 14 files has been converted). Any output is a MUST-FIX before opening the PR.

- [ ] **Commit log sanity**

```bash
git log --oneline origin/main..HEAD
```

Expected: 14 commits, each matching `test(infra): migrate <filename> to #[sqlx::test]`.

- [ ] **Push + open PR**

```bash
git push -u origin feat/sqlx-test-phase2
gh pr create --base main --head feat/sqlx-test-phase2 \
  --title "test(infra): sqlx::test migration Phase 2 — batch A (voice/chat/media)" \
  --body "$(cat <<'EOF'
## Summary

Phase 2 of the integration-test migration to `#[sqlx::test]`. Converts 14 voice/chat/media test files from the shared-pool pattern to per-test database isolation.

Spec: `docs/superpowers/specs/2026-04-18-sqlx-test-integration-migration-design.md` — Phases 2–4.
Phase 1: established `TestApp::with_pool` API and pilot-validated the approach on 3 files.

Files migrated:
- voice: `voice_sfu.rs`, `voice_mute_enforcement.rs`, `voice_rate_limit.rs`
- chat: `channels_http.rs`, `channel_pins.rs`, `messages_http.rs`, `threads.rs`, `dm_http.rs`
- media: `media_processing.rs`, `upload_limits.rs`, `uploads_http.rs`, `screenshare.rs`
- misc: `favorites.rs`, `custom_status.rs`

## Test plan

- [x] `cargo check -p vc-server --tests` — green
- [x] `cargo +nightly fmt --all -- --check` — green
- [x] `cargo clippy --all-features --workspace --exclude vc-client` — green
- [x] `cargo deny check advisories` / `licenses` — green
- [x] No `#[tokio::test]` remaining in these 14 files (verified via grep)
- [x] Full suite passes on CI with no regressions

## Follow-ups

Phases 3 (batch B: auth/admin/governance, 14 files) and 4 (batch C: social/search/misc, 14 files) still to ship. Phase 5 will delete `shared_pool`, remove `[test-groups]`, and prune `#[serial]` attributes after all tests migrate.

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
git worktree remove .claude/worktrees/sqlx-test-phase2
git branch -D feat/sqlx-test-phase2
git fetch origin --prune
```

---

## Success criteria

1. All 14 files in this batch use `#[sqlx::test]` exclusively; zero `#[tokio::test]` remaining in them.
2. All tests in these files pass on CI under `-j 4` parallelism.
3. Overall integration-test failure count is strictly non-increasing vs. pre-Phase-2 baseline.
4. No new `#[serial]` or `use serial_test` additions anywhere in the migrated files.

## Notes for the implementer

- **If any test in a migrated file fails for a reason that isn't the mechanical conversion** (e.g., it depended on cross-test state that per-test DB isolation now exposes), STOP and carve that file out into its own PR with an explicit diagnosis. Don't block the rest of the batch.
- **No cross-file changes** in this phase. `TestApp` helpers stay exactly as Phase 1 left them. If you feel the urge to tweak a helper, resist — that's Phase 5.
- **Commit per-file**, not per-test. Test-level commits bloat history; file-level commits give the right granularity for review and for bisect.
