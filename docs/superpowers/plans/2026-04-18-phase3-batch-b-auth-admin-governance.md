# Phase 3 — Batch B: Auth/Admin/Governance Migration Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Migrate 14 auth/admin/governance integration test files to `#[sqlx::test]`. Structurally identical to Phase 2; different file set.

**Architecture:** Same mechanical per-file rewrite as Phase 2, applying the recipe from Phase 1's plan. Tests in this batch include the `setup*` files that currently require cross-process serialization via nextest `[test-groups].setup-state` — per-test DB isolation obsoletes that serialization, but the `setup-state` entry stays in `nextest.toml` until Phase 5 (because un-migrated Batch A/C files may still reference `setup-state` if they're executed in the same run).

**Tech Stack:** Same as Phase 1/2.

**Spec:** `docs/superpowers/specs/2026-04-18-sqlx-test-integration-migration-design.md` — Phases 2–4.
**Recipe source:** `docs/superpowers/plans/2026-04-18-phase1-testapp-api-plus-pilot.md` Task 3 Step 2 — the 6-point mechanical rewrite.
**Worked example:** `docs/superpowers/plans/2026-04-18-phase2-batch-a-voice-chat-media.md` — same pattern, Batch A file set.

**Parallelization safe:** Phase 3 depends on Phase 1. It's independent of Phase 2's file set, so could theoretically open in parallel, but sequential is recommended.

---

## Pre-flight Check (BLOCKING)

- [ ] **Verify Phase 1 has merged** (same check as Phase 2's pre-flight).

Phase 2 need not have merged yet for Phase 3 to start; however, if Phase 2 is open, coordinate branches to avoid accidentally converting the same file twice (this plan's file set is disjoint from Phase 2's, so conflict risk is low).

---

## Worktree Setup

```bash
cd /home/detair/GIT/detair/kaiku
git worktree add .claude/worktrees/sqlx-test-phase3 -b feat/sqlx-test-phase3 origin/main
cd .claude/worktrees/sqlx-test-phase3
```

---

## File Map

| Task | File | Notes |
|------|------|-------|
| 1 | `server/tests/integration/admin_elevation.rs` | |
| 2 | `server/tests/integration/admin_reports.rs` | |
| 3 | `server/tests/integration/auth.rs` | |
| 4 | `server/tests/integration/oidc.rs` | |
| 5 | `server/tests/integration/ratelimit.rs` | |
| 6 | `server/tests/integration/ratelimit_http.rs` | |
| 7 | `server/tests/integration/reports.rs` | |
| 8 | `server/tests/integration/roles_security.rs` | |
| 9 | `server/tests/integration/setup.rs` | |
| 10 | `server/tests/integration/setup_integration.rs` | |
| 11 | `server/tests/integration/setup_http.rs` | Currently in `setup-state` test-group — serialization obsoleted by per-test DB, but leave the nextest entry for Phase 5 |
| 12 | `server/tests/integration/setup_concurrent_http.rs` | Same as above |
| 13 | `server/tests/integration/governance.rs` | |
| 14 | `server/tests/integration/channel_permissions.rs` | |

---

## Migration Recipe

Follow the 6-point recipe from `2026-04-18-phase1-testapp-api-plus-pilot.md` Task 3 Step 2. Summary:

1. `#[tokio::test]` → `#[sqlx::test]`
2. `async fn test_xxx()` → `async fn test_xxx(pool: PgPool)`
3. `TestApp::new().await` → `TestApp::with_pool(pool.clone()).await` (and equivalents for other factory variants).
4. Remove `#[serial]` + unused `serial_test` import.
5. Drop `CleanupGuard` DB-row cleanup calls; keep non-DB cleanup.
6. Ensure `use sqlx::PgPool;` is present.

**Setup-specific caveat:** `setup_http.rs` and `setup_concurrent_http.rs` test the singleton `server_config.setup_complete` row. Under `#[sqlx::test]`, each test gets a fresh DB where `setup_complete` is whatever the migration default initializes it to (typically `false`). Tests should read that as expected behavior. If a test assumes a previous test's mutation persists, it was relying on shared-DB state — carve it out into its own remediation PR per the spec's Risk #2.

---

## Task N: Migrate `<filename>` (applies to Tasks 1–14)

Same 6-step shape as Phase 2's Task template. Repeat per file.

---

## Final Verification

Same as Phase 2's final verification section, with the grep loop updated for this batch's file set:

```bash
for f in admin_elevation admin_reports auth oidc ratelimit ratelimit_http reports roles_security setup setup_integration setup_http setup_concurrent_http governance channel_permissions; do
    c=$(grep -c '#\[tokio::test' "server/tests/integration/$f.rs" 2>/dev/null || echo 0)
    [ "$c" -gt 0 ] && echo "  $f.rs: $c remaining #[tokio::test] — FIX BEFORE PR"
done
```

Expected: no output.

---

## PR

- Title: `test(infra): sqlx::test migration Phase 3 — batch B (auth/admin/governance)`
- Body template: same shape as Phase 2's, swap the file list. Mention the `setup_http`/`setup_concurrent_http` entries still sit under `[test-groups].setup-state` — removal is Phase 5.
- Merge with `gh pr merge <N> --squash` (no `--admin`).
- Post-merge: remove worktree, delete branch, prune.

---

## Success criteria

1. All 14 files in Batch B use `#[sqlx::test]` exclusively.
2. `setup*` tests pass under `#[sqlx::test]` per-test DB isolation without relying on singleton DB state.
3. Overall integration-test failure count strictly non-increasing vs. pre-Phase-3 baseline.

## Notes for the implementer

- **Setup-test fragility**: the setup tests have the strongest dependency on mutable DB state (they're the only ones that mutate the `server_config` singleton). If any setup test breaks under `#[sqlx::test]` because it assumed cross-test state, the fix is either (a) set the expected state explicitly in the test's `@Before`, or (b) carve that test into a separate PR with an explicit `#[sqlx::test(fixtures = …)]` fixture that sets the initial row. Do NOT add retry logic or put it back on `serial_test`.
- **`[test-groups].setup-state` stays in nextest.toml** until Phase 5. Even after these files' tests migrate, the nextest filter `test(setup_http::) + test(setup_concurrent_http::)` is harmless — it just assigns matching tests to a 1-thread group, which is a no-op when each test has its own DB. Phase 5 cleans this up.
