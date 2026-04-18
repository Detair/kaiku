# Phase 4 — Batch C: Social/Search/Misc Migration Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Migrate 14 social/search/misc integration test files to `#[sqlx::test]`. Last batch before Phase 5's legacy-path deletion.

**Architecture:** Same mechanical per-file rewrite as Phases 2 and 3.

**Spec:** `docs/superpowers/specs/2026-04-18-sqlx-test-integration-migration-design.md` — Phases 2–4.
**Recipe source:** `docs/superpowers/plans/2026-04-18-phase1-testapp-api-plus-pilot.md` Task 3 Step 2.
**Worked examples:** Phase 2 and Phase 3 plans — same pattern, different file sets.

**Parallelization safe:** Same as Phase 3 — depends only on Phase 1. Sequential after Phases 2 and 3 is recommended to keep review load bounded.

---

## Pre-flight Check (BLOCKING)

- [ ] **Verify Phase 1 has merged** (same check as Phases 2/3).

---

## Worktree Setup

```bash
cd /home/detair/GIT/detair/kaiku
git worktree add .claude/worktrees/sqlx-test-phase4 -b feat/sqlx-test-phase4 origin/main
cd .claude/worktrees/sqlx-test-phase4
```

---

## File Map

| Task | File |
|------|------|
| 1 | `server/tests/integration/bot_ecosystem.rs` |
| 2 | `server/tests/integration/bot_intents.rs` |
| 3 | `server/tests/integration/guild_invite.rs` |
| 4 | `server/tests/integration/guild_limits.rs` |
| 5 | `server/tests/integration/mention_permission.rs` |
| 6 | `server/tests/integration/search.rs` |
| 7 | `server/tests/integration/search_http.rs` |
| 8 | `server/tests/integration/global_search_http.rs` |
| 9 | `server/tests/integration/e2ee_keys.rs` |
| 10 | `server/tests/integration/filters_http.rs` |
| 11 | `server/tests/integration/webhooks.rs` |
| 12 | `server/tests/integration/websocket_integration.rs` |
| 13 | `server/tests/integration/workspaces.rs` |
| 14 | `server/tests/integration/connectivity_http.rs` |

---

## Migration Recipe

Same 6-point recipe as Phases 2 and 3. See `2026-04-18-phase1-testapp-api-plus-pilot.md` Task 3 Step 2.

---

## Task N: Migrate `<filename>` (applies to Tasks 1–14)

Same 6-step shape as Phases 2/3. Repeat per file.

---

## Final Verification

```bash
for f in bot_ecosystem bot_intents guild_invite guild_limits mention_permission search search_http global_search_http e2ee_keys filters_http webhooks websocket_integration workspaces connectivity_http; do
    c=$(grep -c '#\[tokio::test' "server/tests/integration/$f.rs" 2>/dev/null || echo 0)
    [ "$c" -gt 0 ] && echo "  $f.rs: $c remaining #[tokio::test] — FIX BEFORE PR"
done
```

Plus the standard `cargo check -p vc-server --tests`, `fmt --check`, `clippy`, `cargo deny` sweeps.

**Post-Phase-4 invariant check:**

```bash
grep -rl '#\[tokio::test' server/tests/integration/ 2>&1 | head -5
```

Expected: **no output**. After Phase 4 merges, every integration test function in the repo should use `#[sqlx::test]`. Any remaining `#[tokio::test]` is a bug — either a missed file in a batch or a test added after the migration started. Either way, must be resolved before Phase 5 opens.

---

## PR

- Title: `test(infra): sqlx::test migration Phase 4 — batch C (social/search/misc)`
- Body template: same as Phase 2's, swap file list; mention that this is the final migration batch and Phase 5 deletes the legacy path next.
- Merge with `gh pr merge <N> --squash` (no `--admin`).
- Post-merge: remove worktree, delete branch, prune.

---

## Success criteria

1. All 14 files in Batch C use `#[sqlx::test]` exclusively.
2. **Invariant**: zero `#[tokio::test]` remains anywhere under `server/tests/integration/` after Phase 4 merges. Verified by the grep above.
3. Overall integration-test failure count strictly non-increasing vs. pre-Phase-4 baseline.

## Notes for the implementer

- **Phase 4 is the "no more `#[tokio::test]`" milestone.** The grep in Final Verification is load-bearing — if it returns anything, fix it before PR.
- **Do not touch `shared_pool` / `SHARED_POOL` / `[test-groups]` in this phase.** Those are Phase 5's job. A test file migrated in Phase 4 will no longer CALL `shared_pool` (because its `TestApp::new()` calls now use `pool.clone()`), but the helper function itself remains on `main` until Phase 5 deletes it.
- **Watch for `use serial_test::…` imports without any matching `#[serial]` attribute** (left-over dead code in files from Phases 2 or 3 where this batch didn't delete them). If you find any while grepping, clean them up opportunistically — it's the only "tidy-up" allowed in Phase 4.
