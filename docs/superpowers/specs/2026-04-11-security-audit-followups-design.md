# Security Audit Follow-Ups

**Date:** 2026-04-11
**Status:** Approved
**Goal:** Address the four remaining items from the post-#512 security audit (PR #513 deferred them) as four independent sub-projects.

## Context

The codebase consistency refactor (#512) was followed by a security audit that produced 4 distinct findings beyond what PR #513 fixed:

1. **17 client devtime advisories** — transitive `bun audit` findings in build tooling (vite, picomatch, rollup, lodash-es, etc.)
2. **Second advisory source missing** — only `cargo deny` checks Rust advisories; no equivalent for npm beyond `bun audit` running locally
3. **`scap` git fork dependency** — pinned to `Detair/scap@4d1304e9` because upstream `scap 0.1.0-beta.1` shipped a Linux-breaking Frame enum restructure
4. **CI flaky test #900/900** — Rust integration tests intermittently time out at the `slow-timeout = 120s` threshold; three different tests have hit this (`test_globally_banned_user_cannot_join_via_discovery`, `test_custom_status_with_expiry_persists`, `test_channel_limit`)

## Approach

The 4 topics are truly independent — different files, different risks, different external dependencies. Treat them as 4 sub-projects: one design doc (this), four implementation plans, four PRs.

Topic 4 (CI flake) is the highest-impact and gets the most design depth — it's actively blocking work via admin-merges. Topics 1-3 are lower-stakes and get tighter treatment. Implementation order is whatever the user prefers; nothing here blocks anything else.

---

## Topic 1: Devtime advisories

**Branch:** `fix/client-devtime-advisories`

### Background

`bun audit` reports 17 vulnerabilities in client transitive dependencies. None affect production runtime — the vulnerable packages are dev-server tooling (vite 7.x), build tools (rollup, picomatch), and lint plumbing (flatted via eslint). One transitive (`lodash-es` via `mermaid → dagre-d3-es`) is technically runtime, but only reachable if mermaid is loaded, which depends on whether mermaid is lazy-loaded in the client.

PR #513 attempted to fix these via `package.json` overrides. The override approach worked locally on the main branch but produced divergent `node_modules` when applied in a git worktree (missing `esbuild`, `@ampproject/remapping`, and ~38 other packages), which broke `vite-plugin-solid`'s JSX transformation in 4 test files. The hypothesis is that bun's worktree resolution behavior is the bug, not the overrides themselves.

### Plan A — retry overrides on a regular branch

Create the fix branch directly off main without using `git worktree add`. Apply the same `package.json` overrides as PR #513:

```json
"overrides": {
  "picomatch": "^4.0.4",
  "rollup": "^4.60.1",
  "flatted": "^3.4.2",
  "brace-expansion": "^1.1.13",
  "lodash-es": "^4.17.24",
  "defu": "^6.1.5",
  "vite": "8.0.5"
}
```

Run the full client gate and require all four to pass before commit:

```sh
cd client
rm -rf node_modules
bun install
bun audit         # must report 0 vulnerabilities
bun run test:run  # must pass 577/577
bun run build     # must succeed
```

If all four pass: commit, push, open PR, ship.

### Plan B — fallback if overrides still misbehave

If `bun install` produces a divergent `node_modules` even on a regular branch, fall back to lockstep direct dep updates:

- Update `vite` to a version where the dev-server fixes are present (>=8.0.5, but verify no JSX regression — vite 8.0.8 broke `vite-plugin-solid`)
- Update `vitest` and `vite-plugin-solid` to versions that don't pin old vite
- Update `unocss` to a version whose `unconfig` dep uses patched `defu`
- Update `mermaid` to a version whose `dagre-d3-es` uses patched `lodash-es` (or accept the risk)
- Run the same four gates

### Side investigation — mermaid lazy-loading

Independent of A/B: check whether `mermaid` is statically imported or lazy-loaded. The relevant question is "does `import('mermaid')` happen at app startup or only when a markdown code block has `language-mermaid`?"

If statically imported: the `lodash-es` runtime exposure is real (every page load executes mermaid's code). Propose a follow-up to lazy-load mermaid via dynamic `import()`.

If lazy-loaded: runtime exposure is negligible (only triggered by user-supplied content that intentionally renders a diagram). No additional action needed.

### Acceptance boundary for documented exceptions (Plan B only)

If Plan B can't reach 0 vulnerabilities, only these exceptions are acceptable:

- `lodash-es` via `mermaid › dagre-d3-es` — **only** if mermaid is confirmed lazy-loaded (side investigation result is "yes"). The runtime exposure in that case is `_.template` code injection only when a markdown code block invokes mermaid rendering, with content that has already passed dompurify sanitization. Acceptable risk.
- Anything else in the dev-server / build-tool category (vite, rollup, picomatch, etc.) — **only** if the implementer has documented in the PR description: (1) why the upstream chain hasn't moved, (2) what the dev-time exploitation path looks like, (3) how a developer would notice an exploit. If any of these can't be answered, the exception isn't acceptable and Plan B fails — escalate.

Any exception must be added to a SECURITY.md or in-code comment with a tracking link to the upstream issue (so future audits can re-check whether the exception is still needed).

### Success criteria

- `bun audit` reports 0 vulnerabilities (Plan A) or ≤2 documented exceptions per the boundary above (Plan B)
- `bun run test:run` passes 577/577
- `bun run build` succeeds
- Mermaid load mode is documented in the PR description

---

## Topic 2: osv-scanner CI job

**Branch:** `feat/osv-scanner-ci`

### Background

The original audit recommendation suggested installing `cargo-audit` alongside `cargo-deny` for "two independent advisory sources." This is wrong: both tools read the same RustSec advisory database. A real second source needs to come from a different database.

The cleanest second source is **OSV** (osv.dev) — an open-source vulnerability database maintained by Google that aggregates from multiple ecosystems (RustSec, npm, PyPI, etc.) and is independent from GitHub Advisory Database (GHSA) and RustSec.

GitHub Dependabot was rejected in favor of osv-scanner because the user wants a non-PR-based approach (no automated dependency PRs cluttering the queue).

### Plan

Add a new CI job that runs `osv-scanner` against the workspace.

**File:** `.github/workflows/osv-scan.yml` (or extend existing CI workflow)

**Job shape:**

```yaml
name: OSV Scanner

on:
  push:
    branches: [main]
  pull_request:
  schedule:
    - cron: "0 6 * * 1"  # Mondays 06:00 UTC

permissions:
  contents: read
  security-events: write  # for SARIF upload

jobs:
  osv-scan:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - name: Run osv-scanner
        uses: google/osv-scanner-action/osv-scanner-action@v2
        with:
          scan-args: |-
            --recursive
            --skip-git
            --format=sarif
            --output=osv-results.sarif
            ./
        # Do NOT set continue-on-error: we want the job to fail when the
        # scanner finds vulnerabilities at or above the configured severity.
        # The scanner exit code is the gate; SARIF upload below is for
        # historical tracking in GitHub Security tab, not for gating.
      - name: Upload SARIF
        if: always()  # Run even if scanner failed, so the SARIF lands in Security tab
        uses: github/codeql-action/upload-sarif@v3
        with:
          sarif_file: osv-results.sarif
```

**Failure semantics (be explicit):**
- The osv-scanner exit code is the gate. Non-zero exit fails the CI job, blocks the PR.
- SARIF upload runs on `if: always()` so failed scans still publish results to the Security tab for tracking — but the upload itself does not gate.
- No `continue-on-error` anywhere in this job. If the scanner says fail, we fail.
- Severity threshold: scan-args should pin a threshold once the implementer has tested both modes; the plan phase should choose between (a) exit non-zero on any vulnerability of any severity, or (b) exit non-zero only on HIGH+. Recommendation is (b) HIGH+ for the PR gate so MEDIUM/LOW finds become tracked-but-non-blocking via the Security tab.

**Behavior:**
- Runs on every PR and every push to main
- Weekly cron catches newly-published advisories on quiet branches
- SARIF output uploads to GitHub Code Scanning for centralized tracking

**Why not extend `cargo deny`:** osv-scanner uses a different DB and supports both Rust and JS in one pass. cargo-deny only covers Rust.

### Success criteria

- New CI job runs on PRs and main
- Scheduled weekly runs visible in Actions UI
- Job reports 0 HIGH+ vulnerabilities on main (after Topic 1 lands)
- SARIF results visible in GitHub Security tab

---

## Topic 3: scap fork cleanup

**Branch:** `chore/scap-upstream-recheck` (only if upstream is now buildable; otherwise no branch — just upstream PR work)

### Background

`client/src-tauri/Cargo.toml` pins `scap` to `Detair/scap@fix/linux-frame-enum` (commit `4d1304e9`). The fork exists because upstream `scap 0.1.0-beta.1` shipped a Frame enum restructure (`Frame::Video(VideoFrame::*)`) without updating the Linux pipewire backend, breaking the Linux build. Detair's fork adapts the Linux backend plus a Windows typo fix.

Upstream PR `CapSoftware/scap#178` is open since 2025-10-26 with a broader scope (sequence numbers + SystemTime + latest-buffer grab) but has not been merged.

The audit finding documented this in PR #513. This topic is about actually dropping the git dep.

### Plan

**Step 1: Test if upstream main HEAD now builds as a `vc-client` dependency on Linux.**

The right test is "does upstream main work AS A DEP of vc-client", not "does upstream main compile standalone." The previous breakage was a Frame enum change in the `scap` API surface that only manifests when a downstream consumer (vc-client) tries to use the API. A standalone `cargo check` on scap won't catch this.

```sh
# Get upstream main HEAD SHA
gh api repos/CapSoftware/scap/branches/main --jq '.commit.sha'

# Edit client/src-tauri/Cargo.toml temporarily:
# scap = { git = "https://github.com/CapSoftware/scap.git", rev = "<sha>" }

# Build vc-client against upstream
cargo check -p vc-client
# (Run on Linux specifically — that's where the previous break happened.
# CI builds across all 3 OS, but the fast local check is Linux.)
```

If `cargo check -p vc-client` succeeds, the change is real. Run the full `cargo build -p vc-client` to confirm linking, then proceed to Step 2a.

**Step 2a: If it builds (cleanup possible).**

Update `client/src-tauri/Cargo.toml`:

```toml
# Before:
scap = { git = "https://github.com/Detair/scap.git", branch = "fix/linux-frame-enum" }

# After:
scap = { git = "https://github.com/CapSoftware/scap.git", rev = "<upstream-main-sha>" }
```

Update the doc comment to remove the "Detair fork" rationale and replace it with "pinned to upstream main pending next release." Drop the upstream PR #178 reference. Run the full Tauri build to verify. Commit, push, open PR, ship.

**Step 2b: If it doesn't build (no cleanup possible yet).**

No change to `main` branch. Instead:

1. Open a narrow upstream PR to `CapSoftware/scap` containing just Detair's targeted fix (linux/mod.rs Frame enum adaptation + win/mod.rs typo). Reference our use case.
2. Update our `Cargo.toml` doc comment to point at the new PR number alongside #178.
3. Wait. When the new PR or #178 merges and ships, repeat Step 1.

### Success criteria

- Either `Cargo.toml` no longer references `Detair/scap` (best case, Step 2a) **or** there is a tracked upstream PR with our targeted fix submitted (acceptable case, Step 2b)
- Tauri client still builds on Linux/macOS/Windows (cross-platform regression check on the new dep)

---

## Topic 4: CI #900/900 flake — `CleanupGuard` redesign

**Branch:** `fix/cleanup-guard-test-flake`

### Background

`server/tests/integration/helpers/mod.rs:163-185` defines `Drop for CleanupGuard`, which spawns a fresh OS thread, builds a new single-threaded tokio runtime, blocks on cleanup actions, and joins the thread with **no timeout**. When cleanup hangs (DB pool contention, lock wait, slow DELETE), the join blocks the test process indefinitely. nextest's `slow-timeout = 120s` then kills the test, reporting it as the "last test" because nextest reports completion in finish-time order.

Three different tests have hit this in recent CI:

| Test | First seen | File |
|---|---|---|
| `test_globally_banned_user_cannot_join_via_discovery` | PR #512 first run | `guild_limits.rs` |
| `test_custom_status_with_expiry_persists` | PR #513 first run | `custom_status.rs` |
| `test_channel_limit` | PR #513 rerun | `guild_limits.rs` |

All three use `CleanupGuard`. Closed issue #509 documents the exact same anti-pattern in setup tests, with the same root cause analysis. The fix recommendation in #509 was to make cleanup synchronous via explicit `await` rather than relying on Drop.

### Layer 1: Stopgap — bounded join in `CleanupGuard::drop`

**File:** `server/tests/integration/helpers/mod.rs:163-185`

Replace the unconditional `.join()` with a 30-second polling loop. If cleanup completes within 30s, join normally. Otherwise, log to stderr and return — the cleanup thread continues detached, but the test no longer hangs the test process.

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
        // Removed once explicit-cleanup migration completes (Layer 2).
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

**Effort:** 1 file, ~15 lines. Self-contained.

**Risk:** Detached threads continue accessing the DB pool until the test process exits. Cleanup actions are idempotent (`DELETE … WHERE id = …`, `UPDATE … WHERE key = …`), so concurrent execution is safe.

### Layer 2: Proper fix — explicit cleanup with runtime enforcement

Convert `CleanupGuard` to require explicit `cleanup().await` instead of relying on Drop. The enforcement strategy is **runtime**, not compile-time, because Rust's `#[must_use]` attribute on structs only fires when the value is unused at the call site — it does NOT catch a guard that is bound to a variable and then dropped at end-of-scope (which is exactly the test pattern). A binding like `let mut guard = app.cleanup_guard();` counts as "used" for `#[must_use]` purposes even if `cleanup()` is never called.

**Enforcement approach: iterative migration via runtime feedback, then panic.**

1. Layer 1's Drop fallback already prints a warning to stderr when actions remain at drop time. This is the migration signal.
2. Add `cleanup(self)` method (consumes `self`, runs actions, leaves Drop with empty actions vec).
3. Migrate test files iteratively. Each migration commit removes the Drop fallback warnings from a specific test file.
4. After all migrations complete and CI shows zero "did not complete within" warnings AND zero "dropped with N pending actions" warnings for ~2 weeks of normal main branch activity, **swap eprintln to panic in a separate follow-up commit**. From that point on, any test that forgets `cleanup().await` fails immediately.

The migration is detected via runtime instrumentation, not compile-time enforcement. This is slower but actually works.

**File:** Same `helpers/mod.rs`, plus every test file using `CleanupGuard`.

**Change to `CleanupGuard` (Layer 2 commit, on top of Layer 1):**

```rust
pub struct CleanupGuard {
    pool: PgPool,
    actions: Vec<CleanupAction>,
}

impl CleanupGuard {
    // ... existing constructors and add() methods unchanged ...

    /// Run all registered cleanup actions on the test's existing tokio runtime.
    /// Tests MUST call this at the end of the test body. Forgetting it triggers
    /// a runtime warning from the Drop fallback (during migration), then a
    /// panic (after migration completes).
    pub async fn cleanup(mut self) {
        let actions = std::mem::take(&mut self.actions);
        for action in actions {
            action(self.pool.clone()).await;
        }
        // Drop is now a no-op for this guard (actions is empty).
    }
}

// Drop fallback (from Layer 1) is updated to also detect "dropped with
// pending actions" — that's the migration signal:
impl Drop for CleanupGuard {
    fn drop(&mut self) {
        let actions = std::mem::take(&mut self.actions);
        if actions.is_empty() {
            return;  // Test called cleanup().await — happy path
        }

        // Test forgot cleanup().await — this is a bug, but during migration
        // we still try to clean up so we don't leak DB rows.
        eprintln!(
            "warning: CleanupGuard dropped with {} pending actions in test — \
             test forgot to call .cleanup().await. Falling back to thread+runtime spawn.",
            actions.len()
        );

        // ... Layer 1's bounded-join cleanup runs here ...
    }
}
```

**Migration pattern:**

```rust
// Before:
let mut guard = app.cleanup_guard();
guard.add(...);
// ... test body ...
}  // drop runs here, prints warning, runs cleanup via thread+runtime

// After:
let mut guard = app.cleanup_guard();
guard.add(...);
// ... test body ...
guard.cleanup().await;
}  // drop runs here, sees empty actions, no-op
```

**Migration mechanics:**

1. Run the full test suite with the Layer 2 changes in place. Capture stderr.
2. Grep stderr for "dropped with N pending actions" — this lists every unmigrated test.
3. Iterate by file. Pick `guild_limits.rs` first (2 of 3 known-flaky tests live here), add `guard.cleanup().await` before each test function returns, re-run `cargo nextest run -p vc-server --test integration guild_limits`, verify the warning is gone.
4. Repeat for `custom_status.rs` (the third known-flaky test).
5. Repeat for remaining files until no warnings remain.
6. **After ~2 weeks of clean main branch CI**, ship a follow-up commit that swaps the eprintln in Drop for `panic!("CleanupGuard dropped with pending actions — call .cleanup().await")`. This makes the contract permanent.

**Why not compile-time enforcement?**

Rust's `#[must_use]` on a struct fires when the value is unused at the call site (e.g., `app.cleanup_guard();` with no binding). It does NOT fire when the struct is bound to a variable and goes out of scope. Since every test in this codebase binds the guard (`let mut guard = ...`), `#[must_use]` would produce zero warnings for unmigrated tests. A custom clippy lint could catch the pattern, but writing one is more work than the runtime approach.

**Estimated scope:** ~50 test functions across ~20 files. Each touch is 1 line. Total LOC change: ~50.

### Layer 3: Root cause investigation — deferred

Not in this PR. Tracked as a follow-up issue. Once Layer 2 lands and CI is green for ~2 weeks with zero "did not complete within" warnings, the question of "why does cleanup occasionally take >30s" becomes a quality concern, not a correctness issue.

Investigation hooks to add when needed:

- Per-action timing logs in `cleanup()` to identify which DELETE is slow
- Snapshot `pg_stat_activity` and `pg_locks` when cleanup exceeds 5s
- Check if `delete_user` cascades through FKs that contend with other tests
- Review pool sizing under parallel test load (`max_connections = 20` may be tight at 4 nextest workers × 5 connections per test)

This is intentionally vague — if we keep getting "did not complete" warnings post-migration, that's the signal to start investigating. If we don't, no work is needed.

### Single-PR shape

| Commit | Layer | Files | Estimated LOC |
|---|---|---|---|
| 1 | Stopgap (bounded join in Drop) | `helpers/mod.rs` | ~15 |
| 2 | Add `cleanup(self)` method, update Drop fallback to warn on pending actions | `helpers/mod.rs` | ~20 |
| 3 | Migrate `guild_limits.rs` (priority — 2 known-flaky tests) | `guild_limits.rs` | ~10 |
| 4 | Migrate `custom_status.rs` (priority — 1 known-flaky test) | `custom_status.rs` | ~3 |
| 5..N | Migrate remaining test files (one commit per file or small group) | various | ~50 total |

**CI verification gates per commit:**

- `cargo clippy --tests -p vc-server -- -D warnings` clean
- Each migration commit: `cargo nextest run -p vc-server --test integration <file>` passes
- Final commit: full nextest run (lib + integration) passes 3 consecutive times

**Stress-test recommendation:** After all commits, run locally:

```sh
for i in {1..20}; do cargo nextest run -p vc-server --test integration; done
```

If any single run hits a "did not complete within" warning, pause and investigate before merging. If no warnings appear in 20 runs, the fix is solid.

### Success criteria

- CI Rust Tests passes 3 consecutive runs on the PR without timeouts
- Local stress test (20 runs) produces zero "did not complete within" warnings
- `cargo nextest run -p vc-server --test integration` produces zero "dropped with N pending actions" warnings (i.e. every test calls `.cleanup().await` before returning)
- The 3 known-flaky tests can be re-run individually 10× without timing out
- Follow-up tracked: a separate issue is filed (referenced from this PR description) to swap the eprintln in Drop for `panic!()` after ~2 weeks of clean main-branch CI

---

## Implementation order

The 4 topics are independent. Recommended order based on impact:

1. **Topic 4 (CI flake)** — highest impact, blocks every PR's CI
2. **Topic 1 (devtime advisories)** — clears `bun audit` to 0, unblocks adding bun audit to CI
3. **Topic 2 (osv-scanner CI)** — depends on Topic 1 being green; otherwise the new job fails day 1
4. **Topic 3 (scap fork)** — lowest impact, can be done anytime

Each topic gets its own implementation plan and PR.
