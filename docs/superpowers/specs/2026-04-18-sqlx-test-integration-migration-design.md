# `#[sqlx::test]` Migration for Integration Tests

**Date:** 2026-04-18
**Status:** Draft
**Goal:** Migrate all ~406 `#[tokio::test]` integration tests under `server/tests/integration/` to `#[sqlx::test]`'s per-test isolated-database pattern, eliminating the cross-process Postgres deadlock flakes that have plagued CI for 2+ years under the shared-pool + `CleanupGuard` model.

## Context

Integration tests currently share a single `canis_test` database through a static `SHARED_POOL` (`server/tests/integration/helpers/mod.rs:58`). `cargo nextest run -j 4` runs four test processes concurrently against that one DB. Tests insert/delete rows in `users`, `guild_members`, `channels`, `messages`, etc.; Postgres's implicit `FOR KEY SHARE` foreign-key-check locks are taken in inconsistent order by concurrent processes, producing sporadic `40P01` deadlocks during fixture setup.

Four distinct deadlock failures observed in the current working week alone, across four unrelated PRs:
- `setup::test_first_user_detection_works` — deadlock on `guild_members` tuple (PR #531)
- `search_http::test_guild_search_xss_content_returned_verbatim` — deadlock on `users` `FOR KEY SHARE` (PRs #540, #539 twice)
- `uploads_http::test_upload_requires_auth` — TIMEOUT, likely related (PR #529)

The team has responded with point fixes for 2+ years:
- `#193` serialized bot ecosystem tests
- `#194` serialized channel permissions tests
- `#280` serialized guild joins in production code
- `#508` unblocked a flaky test
- `#509/#511` added nextest `[test-groups].setup-state` for cross-process serialization of setup-state tests
- `#515` migrated to explicit `CleanupGuard` cleanup
- `a21506a` marked one specific concurrent-setup race test as ignored

Each fix addresses only the test class that flaked most recently; the underlying shared-DB concurrency model is untouched. New flakes keep appearing in new test classes. This spec is a root-cause rewrite: replace the shared-DB model with per-test isolation via sqlx's `#[sqlx::test]` macro.

`#[sqlx::test]` is already used extensively in unit tests under `server/src/` (e.g., `server/src/chat/messages.rs` has 6 examples). This spec extends the pattern to integration tests.

## Scope

Target: 45 integration test files and ~406 test functions under `server/tests/integration/`. In scope: the `TestApp` constructor API, the `SHARED_POOL`/`SHARED_CONFIG` `OnceCell`s, the `[test-groups]` nextest entries, the `#[serial]` attributes on test functions, and the `CleanupGuard` DB-cleanup methods. Out of scope (deliberate):

- **Unit tests under `server/src/`** — already use `#[sqlx::test]`; nothing to migrate.
- **Tauri / Android / client-side tests** — unrelated subsystem; their own concurrency story.
- **Production code changes** — the deadlocks are an artefact of test concurrency, not a production bug. Production traffic serialises through single connections per request; it does not hit the interleavings this spec fixes. No production-side audit is warranted by this workstream.
- **Retry-on-deadlock config** in nextest — the structural fix obsoletes retry as a flake-mitigation strategy; no point adding it.

## Approach

Phased across 5 PRs. Each PR lands independently with CI green; no `--admin` overrides. Backward compatibility preserved within the migration window: existing `TestApp::new()` callers keep working until Phase 5 deletes them.

### Phase 1 — new `TestApp` API + pilot migration (1 PR)

Add pool-injecting factories alongside the existing ones:

```rust
impl TestApp {
    /// New canonical constructor: accepts an externally-provided pool
    /// (typically from `#[sqlx::test]`'s fixture). All fresh integration
    /// tests should use this form.
    pub async fn with_pool(pool: PgPool) -> Self { /* move TestApp::new body here */ }

    /// Screen-share-enabled variant, pool-injecting form.
    pub async fn with_pool_and_screen_share_limiter(pool: PgPool) -> Self { /* … */ }

    // Existing no-arg wrappers kept as convenience during migration:
    pub async fn new() -> Self {
        Self::with_pool(shared_pool().await.clone()).await
    }
    pub async fn with_screen_share_limiter() -> Self {
        Self::with_pool_and_screen_share_limiter(shared_pool().await.clone()).await
    }
}

/// S3 variant, pool-injecting form.
pub async fn fresh_test_app_with_s3_and_pool(pool: PgPool) -> (TestApp, String) { /* … */ }
// `fresh_test_app_with_s3()` (no-arg) kept as convenience during migration.
```

Pilot migration: 3 low-risk files — `blocking.rs`, `e2ee_settings.rs`, `pages.rs` — are converted to `#[sqlx::test]` form as part of Phase 1 to validate the template-DB + migrations pipeline end-to-end in CI before committing to bulk migration.

**Phase 1 success criteria:**
- New factories exist and compile.
- Old factories still exist and delegate to the new ones.
- The 3 pilot files pass under `#[sqlx::test]` in CI.
- Template DB creation + migrations execute within a reasonable bound on CI (verified: cold startup < 10s, per-test overhead < 200ms).

### Phases 2–4 — bulk migration by batch (3 PRs)

Remaining 42 files split into 3 batches of approximately 14 each, grouped by domain coherence so reviewers see related tests together:

- **Batch A — voice/chat/media**: `voice_sfu.rs`, `channels_http.rs`, `messages_http.rs`, `media_processing.rs`, `screenshare.rs`, `threads.rs`, `upload_limits.rs`, `uploads_http.rs`, plus any others in the same domain.
- **Batch B — auth/admin/governance**: `admin_elevation.rs`, `admin_reports.rs`, `oidc.rs`, `ratelimit.rs`, `reports.rs`, `roles_security.rs`, `setup.rs`, `setup_integration.rs`, plus related.
- **Batch C — social/search/misc**: `bot_ecosystem.rs`, `channel_permissions.rs`, `guild_invite.rs`, `mention_permission.rs`, `search.rs`, `search_http.rs`, plus remaining.

Per-batch mechanical rewrite per test function:

1. `#[tokio::test]` → `#[sqlx::test]`
2. Function signature gains `pool: PgPool` parameter
3. Any `TestApp::new().await` → `TestApp::with_pool(pool.clone()).await`
4. Any `TestApp::with_screen_share_limiter().await` → `TestApp::with_pool_and_screen_share_limiter(pool.clone()).await`
5. Any `fresh_test_app_with_s3().await` → `fresh_test_app_with_s3_and_pool(pool.clone()).await`
6. `CleanupGuard`: drop DB-row cleanup calls that become redundant under per-test DB isolation (e.g., `guard.delete_user(id)`); keep S3-bucket cleanup and any future non-DB cleanup methods. For tests that use `CleanupGuard` solely for DB rows, delete the guard entirely.
7. `#[serial(setup)]` (or any `#[serial]`) attributes: remove unconditionally. Cross-test DB isolation is now absolute.

Each batch PR is small enough for a subagent to execute end-to-end with spec-compliance + code-quality review between phases.

**Batch success criteria:**
- All migrated tests green under `cargo nextest run -j 4`.
- Full-suite failure count strictly monotonically non-increasing across the series of PRs.
- No previously-passing test regresses.

### Phase 5 — delete the legacy path (1 PR)

After Batches A, B, C merge, Phase 5 removes the old patterns:

1. Delete `SHARED_POOL` (`OnceCell<PgPool>`), `shared_pool()`, `SHARED_CONFIG`, `shared_config()`, and the no-arg `TestApp::new()` / `with_screen_share_limiter()` / `fresh_test_app_with_s3()` wrappers.
2. Remove the `[test-groups]` `setup-state` entry from `.config/nextest.toml`. Remove the `[[profile.default.overrides]]` that assigns tests to it.
3. Grep remaining `#[serial]` attributes and `use serial_test` imports across `server/tests/`; delete them.
4. `CleanupGuard` audit: delete DB-row cleanup methods (`delete_user`, `restore_setup_complete`, `delete_dm_channel`, `delete_guild`, `delete_connection_data`). Keep the type and any non-DB cleanup methods (S3 bucket cleanup, etc.). If no non-DB uses remain, delete the type entirely.
5. Update `docs/developer-guide/testing/` (and any `AGENTS.md` under `server/`) to name `#[sqlx::test]` as the canonical integration-test setup and remove references to `shared_pool`, `SHARED_POOL`, `[test-groups]`, etc.
6. `CHANGELOG.md` `### Changed` entry: *"Integration tests now use `#[sqlx::test]`'s per-test database isolation; contributor-facing documentation of the new pattern is in `docs/developer-guide/testing/`."*

**Phase 5 success criteria:**
- `grep -rn 'shared_pool\|SHARED_POOL\|shared_config\|SHARED_CONFIG' server/tests/` returns empty.
- `grep -rn '#\[serial\]\|use serial_test' server/tests/` returns empty.
- `.config/nextest.toml` no longer contains `[test-groups]`.
- All 4 observed deadlock-flake test functions run cleanly under `-j 4` across 10 consecutive CI runs on `main`.
- Zero deadlock flakes on `main` CI for 1 week post-merge.

## File map

**Modified (Phase 1):**
- `server/tests/integration/helpers/mod.rs` — add `with_pool`, `with_pool_and_screen_share_limiter`, `fresh_test_app_with_s3_and_pool`; refactor existing no-arg forms to delegate.
- `server/tests/integration/blocking.rs`, `e2ee_settings.rs`, `pages.rs` — pilot migration.

**Modified (Phases 2–4):** 39 files under `server/tests/integration/`, one batch per phase.

**Modified (Phase 5):**
- `server/tests/integration/helpers/mod.rs` — delete `SHARED_POOL`, `shared_pool`, `SHARED_CONFIG`, `shared_config`, legacy no-arg `TestApp` factories, and DB-cleanup methods on `CleanupGuard`.
- `.config/nextest.toml` — remove `[test-groups]` block and its override.
- `docs/developer-guide/testing/*.md` — document `#[sqlx::test]` as canonical.
- `server/AGENTS.md`, `server/tests/AGENTS.md` (if it exists) — update test-infra guidance.
- `CHANGELOG.md` — `### Changed` entry.
- Any file currently importing `serial_test` — drop the import + attributes.

**Total LOC estimate:** ~500–800 lines touched across all phases (mostly mechanical per-test edits of ~1–2 lines each plus the helper refactor and Phase 5 deletions).

## CI posture

- sqlx::test uses a template-database optimization: on first use per binary, it creates `canis_test_template`, runs the 54 migrations, and subsequently per-test database creation is `CREATE DATABASE ... TEMPLATE canis_test_template` (~50–100 ms). Initial migration cost amortises across the whole test run.
- Rough CI wall-time impact: +1–2 s cold start for template + 406 × ~100 ms per-test = ~40 s extra per integration test binary. Offset against eliminated deadlock reruns (each currently costs ~10–30 min of human+machine time), CI throughput improves on a moderate timescale.
- `-j 4` parallelism is preserved. `[test-groups]` serialization is removed (no longer needed).

## Risks

| # | Risk | Probability | Mitigation |
|---|------|-------------|-----------|
| 1 | Template DB + 54 migrations exceeds sqlx::test's startup budget and CI stalls | Low | Validate in Phase 1's pilot before committing to bulk. If slow, investigate migration squash or `sqlx::migrate!` caching. |
| 2 | A test silently depends on cross-test state (e.g., data from test A leaks to test B) | Medium | Per-test DB isolation will surface this as a failure; roll the specific test into its own carve-out PR with an explicit `#[sqlx::test(fixtures = …)]` fixture. Do not block the batch on it. |
| 3 | `CleanupGuard`'s S3 bucket cleanup pattern doesn't cleanly combine with `#[sqlx::test]`'s per-test DB | Low | S3 cleanup is orthogonal to DB — `CleanupGuard` continues to work. Phase 5 audits that the guard's type survives with only non-DB methods. |
| 4 | A subagent batch produces a large hard-to-review diff | Medium | Batch size caps at ~13 files. If review burden exceeds tolerance, split the batch. |
| 5 | During Phase 2–4, a test flakes for unrelated reasons (e.g., CI runner infra) | Low | Retry the job per the existing workflow. If a specific test flakes repeatedly, escalate as a separate concern; do not lump it into this workstream. |
| 6 | A production test (e.g., `setup_integration`) depends on the singleton `server_config.setup_complete` row — `#[sqlx::test]` per-test DB resets that row between tests, breaking the test's premise | Medium | These tests already tolerated isolation under `setup-state` nextest group serialization; per-test DB should be a strict improvement. Verify during Batch B (where `setup*` lives). |

## Success criteria (workstream-level)

1. All 406 integration test functions run under `#[sqlx::test]`.
2. `shared_pool`, `SHARED_POOL`, `shared_config`, `SHARED_CONFIG`, and all no-arg `TestApp` factories are deleted from `server/tests/integration/helpers/mod.rs`.
3. `.config/nextest.toml` no longer contains `[test-groups]`.
4. No `#[serial]` attribute or `use serial_test` import remains under `server/tests/`.
5. The 3 observed deadlock-flake test functions (`test_first_user_detection_works`, `test_guild_search_xss_content_returned_verbatim`, `test_upload_requires_auth`) run cleanly under `cargo nextest run -j 4` for 10 consecutive CI runs on `main`.
6. Zero `40P01` (deadlock_detected) occurrences in `main` CI logs for 1 week after Phase 5 merges.

## Out of scope

- **Production code changes.** The deadlocks are artefacts of test-only concurrency; production traffic serialises per-request and does not hit these interleavings.
- **Retry-on-deadlock configuration.** Obsoleted by the structural fix.
- **Unit tests under `server/src/`.** Already use `#[sqlx::test]`.
- **Android, Tauri, or frontend tests.** Different subsystems.
- **Redis / Valkey fixture isolation.** Integration tests share one Redis via `config.redis_url`. Out of scope here; Redis contention has not been observed as a source of flakes. If it becomes one, treat as a separate workstream.
- **S3 / RustFS bucket-per-test isolation.** Only 2 files use S3 and they already create unique bucket names. Current CleanupGuard-based S3 teardown is retained.

## CHANGELOG entries

- **Phase 5 only** (the earlier phases are refactoring with no user-visible change; per CLAUDE.md, CHANGELOG is updated only for user-visible changes. This one gets an entry because the new integration-test pattern is a contributor-facing convention worth noting):

  Under `### Changed`:
  > Integration tests now use `#[sqlx::test]`'s per-test database isolation; the shared-pool model that produced sporadic Postgres deadlocks in CI is retired. Contributor-facing documentation is in `docs/developer-guide/testing/`.
