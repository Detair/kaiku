# Phase 2.5 — Open Topics Cleanup

**Date:** 2026-04-16
**Status:** Draft
**Goal:** Close the open items left behind by Phase 1's admin-merged PRs (#529–#534) and by Workstream A's deliberate scope carve-outs: restore honest-green CI on `main`, land the four remaining Phase 1 PRs without admin override, classify the 13 pre-existing Android test failures into actionable clusters, and un-ignore the last voice test by injecting `WebRtcManager`'s internal `CoroutineScope`.

## Context

Phase 1 shipped five voice/screen-share PRs (#529–#533) and Phase 2 Workstream A shipped #534 (Android test infrastructure). Both phases reached `main` via `gh pr merge --admin` because `main`'s CI had two pre-existing drift failures that predate the Phase 1 branches and affected every PR that branched from any recent commit of `main`:

- **`Rust Lint (fmt)`** — `server/src/ws/handlers.rs:73` has a docstring that nightly `rustfmt` wants reformatted under `wrap_comments=true` (an unstable feature in the project's `rustfmt.toml`). Contributors running `make fmt` or `cargo fmt` on stable silently pass because stable rustfmt ignores unstable features; CI runs nightly for the fmt check and flags the drift.
- **`License Compliance` (advisories)** — `rustls-webpki 0.103.10` is pinned by transitive deps, and `RUSTSEC-2026-0099` (name-constraint bypass for wildcard certs) was published against this version. Fix is a `cargo update -p rustls-webpki` bumping to `≥0.103.12`.

`Rust Lint (clippy)` fails as a cascade of the fmt workflow's cancellation; fixing fmt should unblock clippy automatically.

Workstream A (#534) also left two intentional scope carve-outs:

- **13 pre-existing Android unit test failures** in `AuthStateTest` (×2), `AuthFlowTest` (×5), `MessageFlowTest` (×5), `QrLoginFlowTest` (×1). Three of the four classes are integration tests, suggesting a shared fixture/component is the likely root cause — but that is an untested hypothesis.
- **1 `@Ignore`d voice test** — `WebRtcManagerTest.voiceIceConnected emits true only when both PCs reach Connected`. `WebRtcManager.voiceIceConnected` uses `stateIn(CoroutineScope(Dispatchers.IO), Eagerly, false)` with an *inline* anonymous scope. `TestCoroutineScheduler` cannot drive that scope, so `runCurrent()` never flushes the combine re-emission, and the assertion reads stale state.

Finally, PRs #529 (server security), #530 (web ICE buffering), #531 (Tauri RTP protocol), #532 (Tauri VP8 decode) from Phase 1 remain open with the same CI drift blocking them.

This workstream closes all of the above.

## Scope

Four sub-workstreams, each shipping as its own PR (or set of PRs for Block 2). Block 1 is the sequencing prerequisite; Blocks 3 and 4 parallelize with Block 2 once Block 1 lands.

**Out of scope (deliberate):**

- **Phase 2 Workstreams B, C, D, E, F, G** — commissioned separately by the user with their own specs (B.2 error UX, G Tauri test gaps, etc.).
- **Actually fixing production bugs in auth/messaging/qr-login** if Block 3's triage uncovers them — those are commissioned as follow-up workstreams off the triage report, not landed under Block 3.
- **Scope lifecycle refactor** — injecting `CoroutineScope` (Block 4) does not introduce scope-cancellation on `WebRtcManager.dispose()`. That is a separate concern that would require auditing all scope consumers; deferred.

## Block 1 — CI drift fix

**Goal:** Turn `main`'s CI back to honest-green and prevent the Makefile-fmt trap that allowed the drift to accumulate.

**Changes (single PR, four commits):**

1. **Fix the fmt drift.** Run `cargo +nightly fmt --all`. Confirmed single-file delta in `server/src/ws/handlers.rs:73` — a docstring the nightly `wrap_comments=true` feature wants rewrapped.
2. **Patch the security advisory.** Run `cargo update -p rustls-webpki` to bump from `0.103.10` → `≥0.103.12`. Resolves `RUSTSEC-2026-0099`. `Cargo.lock` delta only; the bump is semver-patch and MSRV-compatible with the existing `rustls 0.23.x` + `hyper-rustls 0.27.x` graph.
3. **Route `Makefile` fmt targets through nightly.** `Makefile:139` changes `cargo fmt --all` → `cargo +nightly fmt --all`; `Makefile:143` changes `cargo fmt --all -- --check` → `cargo +nightly fmt --all -- --check`. Contributors running `make fmt` / `make fmt-check` now reproduce CI's check locally.
4. **Document the nightly requirement.** Short paragraph in `docs/developer-guide/development/standards.md` flagging that fmt requires nightly rustfmt and noting `rustup toolchain install nightly -c rustfmt` as the install command.

**Commits:**

1. `fix(infra): apply cargo +nightly fmt to handlers.rs docstring`
2. `chore(infra): cargo update -p rustls-webpki for RUSTSEC-2026-0099`
3. `chore(infra): route Makefile fmt targets through nightly toolchain`
4. `docs(infra): note nightly rustfmt requirement in standards.md`

(CLAUDE.md allowed commit types are `feat`, `fix`, `docs`, `refactor`, `test`, `chore`, `perf`, `style`; allowed scopes include `infra` which covers tooling/CI/Makefile changes.)

**Success criteria:**

- `Rust Lint (fmt)`, `Rust Lint (clippy)`, `License Compliance` — all green on the PR and on `main` post-merge.
- `cargo deny check advisories` locally returns 0 matches for `RUSTSEC-2026-0099`.
- `make fmt-check` on a contributor machine with nightly installed returns no diff.
- `make fmt-check` on a contributor machine *without* nightly produces a clear error pointing to the standards.md note (rustup returns a missing-toolchain message; acceptable).

**Risks:**

- **`rustls-webpki` bump breaks another transitive consumer.** Low probability (patch bump). Mitigated by running `cargo test` in the workspace before pushing.
- **More fmt drift discovered once nightly fmt runs.** Confirmed via `cargo +nightly fmt --check` that the only diff is `handlers.rs:73` — no other drift hidden. If that changes between now and PR creation, expand scope of commit 1 accordingly.
- **No pre-commit hook.** Deliberate — drift rate is ~1 file per quarter; not worth the tooling overhead. Re-evaluate if drift recurs after this PR.

## Block 2 — Clean-merge Phase 1 PRs #529–#532

**Goal:** Land the four remaining Phase 1 PRs on a green-CI `main` without admin override. Restores real CI signal for future PRs.

**Hard dependency:** Block 1 must merge first.

**Sequence and rationale:**

1. **#529 — `fix(voice): server security — self-mute, rate limiting, screen share slot leak`.** Rust server. Independent. Security-sensitive; goes first to reduce exposure time.
2. **#530 — `fix(client): buffer ICE candidates until remote description is set`.** Web client. Independent of #529 and Tauri PRs. Can merge any time after #529.
3. **#531 — `fix(voice): Tauri RTP protocol — per-session seq/ts, VP8 payload_type`.** Tauri client. Must precede #532 (content dependency: #532's VP8 decode path relies on #531's RTP protocol fixes).
4. **#532 — `feat(voice): native VP8 decode for remote screen shares in Tauri`.** Tauri client. After #531.

**Per-PR shepherding protocol:**

Worktrees are already set up in `.claude/worktrees/{voice-server-security,web-voice-ice-buffering,tauri-voice-rtp-protocol,tauri-vp8-decode}`. For each PR in sequence:

1. `cd <worktree>`
2. `git fetch origin && git rebase origin/main` (new `main` includes Block 1 fixes + any previously-landed PRs in this sequence)
3. Resolve any drift conflicts — most likely in `Cargo.lock` (due to Block 1's `rustls-webpki` bump interacting with the PR's own dep deltas). For text-file drift in `server/src/ws/handlers.rs` or similar, rerun `cargo +nightly fmt --all` after rebasing.
4. Run local gates per CLAUDE.md: `cargo test` + `bun run test:run` (if the PR touches frontend) + `SQLX_OFFLINE=true cargo clippy -- -D warnings` (if server).
5. `git push --force-with-lease`.
6. Wait for CI green (no admin override).
7. `gh pr merge <n> --squash`.
8. Post-merge cleanup: `git worktree remove <worktree> && git branch -D <branch> && git push origin --delete <branch>`.

**No new code.** We are not the owners of these PRs. Block 2's sole deliverable is that all four squash-land with honest green CI.

**Commits:** Block 2 contributes zero commits of its own; it's purely a merge-shepherding activity. PR bodies are authored by the PR owners.

**Success criteria:**

- All four PRs merged to `main`.
- Final merge commits' CI status on `main` is green.
- All four feature worktrees removed; all four remote branches deleted; `git branch --list` shows no `[gone]` entries for these branches.

**Risks:**

- **#529 content introduces new `handlers.rs` drift.** Possible — #529 touches server code likely including WebSocket handlers. Mitigated by rerunning `cargo +nightly fmt --all` during rebase and committing any additional fmt delta.
- **`Cargo.lock` merge conflicts.** Possible — Block 1 bumps `rustls-webpki` and #529 might transitively pull different versions. Standard resolution: accept main's version, rerun `cargo build` to regenerate `Cargo.lock` cleanly.
- **Test regressions under the fixed toolchain.** If Block 1's nightly rustfmt pass surfaces an issue clippy doesn't catch, #529's Rust Tests could fail in ways that didn't before. Mitigated by running `cargo test` locally during rebase. If regressions appear, surface to PR owner; do not silently fix.

## Block 3 — Test-failure triage spike

**Goal:** Convert "13 unknown failures" into "clustered root causes with actionable next steps" within a bounded investigation budget.

**Budget:** 90 minutes wall-clock for investigation. Additionally, up to 30 minutes for inline fix commits if clusters resolve to S-rated quick fixes — for a 120-minute total worst case. The 30-minute fix budget is *additive* to the 90-minute investigation budget, not carved out of it. If investigation alone exceeds 90 minutes, the PR ships as a classification document only and any fix work is commissioned as a follow-up.

**Investigation method:**

1. Run `./gradlew :app:testDebugUnitTest` with full stack traces captured to a scratch file.
2. For each of the 13 failing tests, record: fully-qualified test name, assertion/exception message, first 5 frames of the stack trace, test file path.
3. Cluster by root cause signal:
   - Shared fixture failure? (e.g., `@Before` consistently throws the same error across all tests in a class)
   - Shared infrastructure failure? (e.g., a Hilt test component, mocked HTTP server, datastore fixture used by all three integration-test classes)
   - Independent real bugs in production code under test?
4. For each cluster, document: (i) root-cause hypothesis backed by evidence, (ii) estimated fix size (S/M/L: <30 min, <4h, >4h), (iii) domain owner (auth, messaging, qr-login, data-local), (iv) recommended approach (inline fix, dedicated workstream, permanent `@Ignore` with tracker link).

**PR deliverable:** `docs/developer-guide/testing/2026-04-16-android-test-failure-triage.md` containing:

- Per-class failure tables with the data from step 2.
- A cluster index: 1–3 clusters, each with the step-4 attributes.
- For each cluster rated "S": fix commits in the same PR.
- For each cluster rated "M" or "L": a one-paragraph spec proposal that could seed a follow-up `docs/superpowers/specs/YYYY-MM-DD-<cluster>-design.md`.

**Commits (single PR):**

1. `docs(client): triage 13 pre-existing Android unit test failures`
2. (Zero or more) `test(client): <cluster fix>` — only for S-rated clusters.

**Success criteria:**

- Triage doc committed.
- Every one of the 13 failing tests appears in the doc's per-class table.
- At least one cluster has a clear fix path (even if rated M/L and deferred).
- If any S-rated fixes land inline, those tests pass in the PR's CI run.
- No new test failures introduced by the PR.

**Risks:**

- **No clustering emerges — 13 genuinely independent bugs.** Possible but unlikely given 3 integration-test classes failing together. Mitigation: the doc still ships as "no shared root cause" and each failure gets its own L-rated proposal. The spike's deliverable is classification, not fix-everything.
- **Investigation overruns the 90-minute budget.** Acknowledged risk. If the budget is exceeded, the spike MUST stop; partial findings ship as a `status: incomplete` section of the doc, and a follow-up spike gets commissioned. Do not let investigation become a rabbit hole.

## Block 4 — `CoroutineScope` injection into `WebRtcManager`

**Goal:** Un-ignore `WebRtcManagerTest.voiceIceConnected emits true only when both PCs reach Connected` (the only remaining `@Ignore` in the Android voice-test suite) by making `WebRtcManager`'s `stateIn` scope injectable.

### Changes

**1. New qualifier annotation:** `mobile/android/app/src/main/java/io/wolftown/kaiku/di/VoiceCoroutineScope.kt`

```kotlin
package io.wolftown.kaiku.di

import javax.inject.Qualifier

/**
 * Qualifier for the [CoroutineScope] used by voice-layer background work
 * (e.g., [WebRtcManager.voiceIceConnected] stateIn collection).
 *
 * Tests inject a [kotlinx.coroutines.test.TestScope] via this qualifier so the
 * test scheduler drives all emissions.
 */
@Qualifier
@Retention(AnnotationRetention.BINARY)
annotation class VoiceCoroutineScope
```

**2. New Hilt provider module** (`mobile/android/app/src/main/java/io/wolftown/kaiku/di/VoiceCoroutineScopeModule.kt`):

```kotlin
package io.wolftown.kaiku.di

import dagger.Module
import dagger.Provides
import dagger.hilt.InstallIn
import dagger.hilt.components.SingletonComponent
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import javax.inject.Singleton

@Module
@InstallIn(SingletonComponent::class)
object VoiceCoroutineScopeModule {
    @Provides
    @Singleton
    @VoiceCoroutineScope
    fun provideVoiceCoroutineScope(): CoroutineScope =
        CoroutineScope(SupervisorJob() + Dispatchers.IO)
}
```

New `object` module (distinct from `VoiceModule` which is an `abstract class` for `@Binds`). Hilt requires `@Provides` methods to live in either `object` or concrete classes.

**3. `WebRtcManager` constructor addition:**

```kotlin
@Singleton
class WebRtcManager @Inject constructor(
    @ApplicationContext private val context: Context,
    private val voiceApi: VoiceApi,
    private val eglBaseProvider: EglBaseProvider,
    @VoiceCoroutineScope private val voiceScope: CoroutineScope,
) {
    ...
    val voiceIceConnected: StateFlow<Boolean> =
        combine(publisherIceState, subscriberIceState) { p, s ->
            p == PeerConnection.IceConnectionState.CONNECTED &&
                s == PeerConnection.IceConnectionState.CONNECTED
        }.stateIn(voiceScope, SharingStarted.Eagerly, false)
}
```

The inline `CoroutineScope(Dispatchers.IO)` at `WebRtcManager.kt:159` is replaced by the injected `voiceScope`.

### Scope lifecycle decision

`WebRtcManager.dispose()` does **not** cancel `voiceScope`. Rationale:

- The scope is provided at `SingletonComponent` level; its lifetime is the application lifetime.
- `SupervisorJob` means a child coroutine's failure does not cancel the scope itself.
- The scope may gain additional consumers in future workstreams; a singleton's `dispose()` unilaterally cancelling a DI-owned resource is the wrong default.
- If a future workstream needs scope-cancellation-on-dispose, it can explicitly own a scoped child via `coroutineScope { ... }` or `launch` on a local `Job`.

Tests provide their own scope (see test update below), so scope leakage is not a JVM-unit-test concern.

### Test update

**`WebRtcManagerTest.kt`:**

- The existing `newWebRtcManager()` helper gains a 4th parameter `voiceScope`:
  ```kotlin
  private fun newWebRtcManager(
      voiceScope: CoroutineScope = TestScope(UnconfinedTestDispatcher()),
  ): WebRtcManager = WebRtcManager(
      context = mockk<Context>(relaxed = true),
      voiceApi = mockk<VoiceApi>(relaxed = true),
      eglBaseProvider = mockk<EglBaseProvider>(relaxed = true),
      voiceScope = voiceScope,
  )
  ```
  Default is `TestScope(UnconfinedTestDispatcher())` so tests that don't care about scheduler control get synchronous emission.
- For `voiceIceConnected emits true only when both PCs reach Connected`, the `runTest` block passes `this.backgroundScope` (the `TestScope`-provided `CoroutineScope` backed by the same scheduler as `runTest`):
  ```kotlin
  @OptIn(ExperimentalCoroutinesApi::class)
  @Test
  fun `voiceIceConnected emits true only when both PCs reach Connected`() = runTest {
      val webRtcManager = newWebRtcManager(voiceScope = backgroundScope)
      // ... existing test body unchanged; runCurrent() now actually drives the combine
  }
  ```
- The `@Ignore` annotation and its import (`org.junit.Ignore`) are removed.

**`mobile/android/app/src/test/AGENTS.md` update (from Workstream A):**

- "Currently `@Ignore`d tests" section changes from one entry to "None".
- "`CoroutineScope(Dispatchers.IO)` + `StandardTestDispatcher` caveat" section's workaround #2 ("Inject the scope") becomes the canonical pattern with a reference to `@VoiceCoroutineScope`.

### Commits (single PR)

1. `feat(client): @VoiceCoroutineScope qualifier + Hilt provider`
2. `refactor(voice): WebRtcManager uses injected @VoiceCoroutineScope`
3. `test(client): un-ignore voiceIceConnected test; inject TestScope`
4. `docs(client): update AGENTS.md — no more @Ignore`d voice tests`
5. `docs(client): CHANGELOG entry`

### Success criteria

- `./gradlew :app:testDebugUnitTest --tests 'io.wolftown.kaiku.data.voice.WebRtcManagerTest'` reports **18 tests, 0 skipped, 0 failed**.
- `grep -rn '@Ignore' mobile/android/app/src/test/java/io/wolftown/kaiku/data/voice/` returns no matches.
- `AGENTS.md` "Currently `@Ignore`d tests" section reads `None`.
- Full `testDebugUnitTest` still has failures only in the Block 3 allowlist (`AuthStateTest`, `AuthFlowTest`, `MessageFlowTest`, `QrLoginFlowTest`).

### Risks

- **New Hilt graph cycle.** Low probability — `CoroutineScope` has no dependencies beyond JDK types. Hilt codegen catches cycles at compile time.
- **`backgroundScope` is missing on older `kotlinx-coroutines-test`.** `backgroundScope` is available from 1.7.0+; the project uses `1.9.0` per `build.gradle.kts:140`. Safe.
- **Test flakiness from `UnconfinedTestDispatcher` default.** `UnconfinedTestDispatcher` runs continuations eagerly on the current thread — fine for the existing tests that don't care about scheduling. The scheduler-sensitive test explicitly overrides with `backgroundScope` (`StandardTestDispatcher`-equivalent). No regression risk for the 5 other un-ignored tests.

## Sequencing & dependencies

```
Block 1 (PR A: CI drift fix) — hard gate
   │
   ├──► Block 2 PR1 (#529 server security)
   │       └──► Block 2 PR2 (#530 web ICE)
   │              └──► Block 2 PR3 (#531 Tauri RTP)
   │                     └──► Block 2 PR4 (#532 Tauri VP8)
   │
   ├──► Block 3 (PR C: test triage) — parallelizable with Block 2
   │
   └──► Block 4 (PR D: CoroutineScope inject) — parallelizable with Block 2 & 3
```

- Block 1 is the only hard prerequisite. It must merge before Block 2's first PR and should merge before Blocks 3 and 4's PRs open so they inherit green-CI baseline.
- Block 2 serializes among its four PRs (#529 → #530 → #531 → #532). #530 actually has no content dependency on #529 and could race, but serializing reduces CI-rebasing churn and keeps merge history legible.
- Blocks 3 and 4 are fully independent and can open in parallel.

## Success criteria (workstream-level)

1. `main`'s CI is green on three consecutive commits without admin override.
2. All four Phase 1 PRs (#529, #530, #531, #532) merged.
3. `./gradlew :app:testDebugUnitTest --tests 'io.wolftown.kaiku.data.voice.*'` reports **zero** `@Ignore`'d tests and zero failures.
4. `docs/developer-guide/testing/2026-04-16-android-test-failure-triage.md` exists and classifies all 13 pre-existing failures.
5. Workstream-A and Phase-2.5 worktrees + branches cleaned up (`.claude/worktrees/` contains only active work).

## CHANGELOG entries

Per CLAUDE.md, user-relevant changes go under `## [Unreleased]` in `CHANGELOG.md`. Not every block warrants one:

- **Block 1:** `### Security` — "Upgrade `rustls-webpki` to patch RUSTSEC-2026-0099 (name-constraint bypass for wildcard certs)." Fmt / Makefile changes are tooling; not user-visible.
- **Block 2:** Each of the four PRs already carries its own CHANGELOG entry from its original author; no new entries from this workstream.
- **Block 3:** None unless S-rated fixes land (those get their own entry per cluster).
- **Block 4:** `### Fixed` — "Android: voice-layer `CoroutineScope` is now DI-provided, making the full `WebRtcManager` test suite runnable under `TestCoroutineScheduler` and unblocking CI coverage for dual-PC ICE state transitions."

## Out of scope

- **Phase 2 Workstreams B/C/D/E/F/G** — separate specs, user-commissioned.
- **Production fixes for any auth/messaging/qr-login bugs surfaced by Block 3's triage** — those are commissioned as follow-up workstreams, not landed inside Block 3.
- **Scope-cancellation semantics on `WebRtcManager.dispose()`** — explicitly deferred; Block 4 leaves the scope owned by Hilt's `SingletonComponent`.
- **Robolectric adoption, instrumented `androidTest/` coverage, or any expansion of the Android test pyramid** — carries forward Workstream A's decision to focus on JVM unit tests only.
- **Pre-commit hooks or other automated fmt enforcement beyond the Makefile fix** — re-evaluate if drift recurs after Block 1.
- **Cleaning up `[gone]` branches and stale worktrees not related to the four blocks** — trivial git housekeeping, out of design scope.
