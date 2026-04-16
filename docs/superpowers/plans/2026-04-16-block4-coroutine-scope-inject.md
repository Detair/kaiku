# Block 4 — `CoroutineScope` Injection into `WebRtcManager` Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Un-ignore `WebRtcManagerTest.voiceIceConnected emits true only when both PCs reach Connected` by making `WebRtcManager`'s internal `stateIn` scope injectable via Hilt. Drive the full 18-test voice suite to zero `@Ignore`s.

**Architecture:** Add a `@VoiceCoroutineScope` qualifier annotation + a new `VoiceCoroutineScopeModule` (`@Provides` in an `object` module) that yields `CoroutineScope(SupervisorJob() + Dispatchers.IO)`. Inject this into `WebRtcManager` as a constructor parameter and replace the inline `CoroutineScope(Dispatchers.IO)` at `WebRtcManager.kt:159`. Tests gain control of the scope via `TestScope`/`backgroundScope`.

**Tech Stack:** Kotlin, Hilt (Dagger 2), Coroutines (`kotlinx-coroutines-core` 1.9.0 + `kotlinx-coroutines-test` 1.9.0), MockK, JUnit 4.

**Spec:** `docs/superpowers/specs/2026-04-16-open-topics-cleanup-design.md` — Block 4.

**Parallelization safe:** Yes — runs in parallel with Blocks 2 and 3 after Block 1 merges. The only dependency is that `main`'s CI is green so Block 4's own CI run is trustworthy.

---

## Pre-flight Check (BLOCKING)

- [ ] **Verify Block 1 has merged**

```bash
cd /home/detair/GIT/detair/kaiku
git fetch origin
git log origin/main --oneline | grep -E "ci-drift|CI drift|RUSTSEC-2026-0099" | head -3
```

Expected: at least one commit referencing the Block 1 fix. Block 4 doesn't strictly *require* Block 1's code changes, but CI must be green on `main` to give an honest signal on Block 4's PR.

- [ ] **Verify Workstream A's code shape still matches assumptions**

```bash
grep -n 'class WebRtcManager\|eglBaseProvider\|@VoiceCoroutineScope' mobile/android/app/src/main/java/io/wolftown/kaiku/data/voice/WebRtcManager.kt | head -5
grep -n 'CoroutineScope(Dispatchers.IO)' mobile/android/app/src/main/java/io/wolftown/kaiku/data/voice/WebRtcManager.kt | head -3
```

Expected:
- `class WebRtcManager @Inject constructor(` at ~line 60
- `private val eglBaseProvider: EglBaseProvider` parameter present at ~line 63 (Workstream A)
- One `CoroutineScope(Dispatchers.IO)` match at ~line 159 — the inline scope this plan replaces
- Zero `@VoiceCoroutineScope` matches (not yet created)

If `eglBaseProvider` is missing, Workstream A has been reverted somehow — **STOP** and escalate. If `@VoiceCoroutineScope` already appears, someone pre-landed Block 4's scaffolding — **STOP** and rebase.

- [ ] **Verify the `@Ignore` is still in place**

```bash
grep -nE '@Ignore|voiceIceConnected emits true' mobile/android/app/src/test/java/io/wolftown/kaiku/data/voice/WebRtcManagerTest.kt | head -4
```

Expected: one `@Ignore(` match near line 345, immediately followed by the `fun \`voiceIceConnected emits true only when both PCs reach Connected\`() = runTest {` test declaration.

- [ ] **Environment for gradle**

```bash
export JAVA_HOME="$HOME/.local/share/jdk/jdk-17.0.18+8"
export ANDROID_HOME="$HOME/.local/share/android-sdk"
export PATH="$JAVA_HOME/bin:$PATH"
```

---

## Worktree Setup (run once after pre-flight passes)

```bash
cd /home/detair/GIT/detair/kaiku
git worktree add .claude/worktrees/voice-scope-inject -b feat/webrtc-scope-inject origin/main
cd .claude/worktrees/voice-scope-inject
```

Working branch: `feat/webrtc-scope-inject`. Working directory for all tasks: `/home/detair/GIT/detair/kaiku/.claude/worktrees/voice-scope-inject`.

---

## File Map

| Path | Action | Task |
|------|--------|------|
| `mobile/android/app/src/main/java/io/wolftown/kaiku/di/VoiceCoroutineScope.kt` | Create | 1 |
| `mobile/android/app/src/main/java/io/wolftown/kaiku/di/VoiceCoroutineScopeModule.kt` | Create | 2 |
| `mobile/android/app/src/main/java/io/wolftown/kaiku/data/voice/WebRtcManager.kt` | Modify (constructor + stateIn) | 3 |
| `mobile/android/app/src/test/java/io/wolftown/kaiku/data/voice/WebRtcManagerTest.kt` | Modify (helper + un-ignore) | 4, 5 |
| `mobile/android/app/src/test/AGENTS.md` | Modify (registry + canonical pattern) | 6 |
| `CHANGELOG.md` | Modify (`### Fixed`) | 7 |

---

## Task 1: Create `@VoiceCoroutineScope` qualifier

**Files:**
- Create: `mobile/android/app/src/main/java/io/wolftown/kaiku/di/VoiceCoroutineScope.kt`

- [ ] **Step 1: Write the qualifier annotation**

```kotlin
package io.wolftown.kaiku.di

import javax.inject.Qualifier

/**
 * Qualifier for the [kotlinx.coroutines.CoroutineScope] used by voice-layer
 * background work (e.g., [WebRtcManager.voiceIceConnected] stateIn collection).
 *
 * Tests inject a `TestScope` / `backgroundScope` via this qualifier so the
 * `TestCoroutineScheduler` drives all emissions synchronously.
 */
@Qualifier
@Retention(AnnotationRetention.BINARY)
annotation class VoiceCoroutineScope
```

- [ ] **Step 2: Verify compile**

```bash
cd mobile/android
./gradlew :app:compileDebugKotlin 2>&1 | tail -5
```

Expected: `BUILD SUCCESSFUL`. The annotation stands alone — no consumers yet.

- [ ] **Step 3: Commit**

```bash
cd /home/detair/GIT/detair/kaiku/.claude/worktrees/voice-scope-inject
git add mobile/android/app/src/main/java/io/wolftown/kaiku/di/VoiceCoroutineScope.kt
git commit -m "feat(client): @VoiceCoroutineScope qualifier for voice-layer scope DI"
```

---

## Task 2: Create `VoiceCoroutineScopeModule` `@Provides` module

**Files:**
- Create: `mobile/android/app/src/main/java/io/wolftown/kaiku/di/VoiceCoroutineScopeModule.kt`

Rationale for a *new module* instead of adding to `VoiceModule`: `VoiceModule` is an `abstract class` (hosts `@Binds`). Hilt requires `@Provides` methods to live in either `object` modules or non-abstract classes. A dedicated `object` keeps the boundary clean and minimizes churn to the existing `@Binds`-style module.

- [ ] **Step 1: Write the module**

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

/**
 * Provides the application-scoped [CoroutineScope] used by voice-layer
 * background collectors (e.g., `WebRtcManager.voiceIceConnected.stateIn(...)`).
 *
 * Kept separate from [VoiceModule] because that module is `abstract` for
 * `@Binds` entries; Hilt requires `@Provides` to live in `object` or
 * non-abstract modules.
 *
 * `SupervisorJob` ensures a child coroutine's failure does not cascade to
 * siblings. `Dispatchers.IO` matches the prior (inline) behavior.
 */
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

- [ ] **Step 2: Verify compile + Hilt codegen**

```bash
cd mobile/android
./gradlew :app:compileDebugKotlin 2>&1 | tail -10
```

Expected: `BUILD SUCCESSFUL`. Hilt KSP generates a `VoiceCoroutineScopeModule_ProvideVoiceCoroutineScopeFactory` class; no errors.

- [ ] **Step 3: Commit**

```bash
cd /home/detair/GIT/detair/kaiku/.claude/worktrees/voice-scope-inject
git add mobile/android/app/src/main/java/io/wolftown/kaiku/di/VoiceCoroutineScopeModule.kt
git commit -m "feat(client): VoiceCoroutineScopeModule @Provides for voice-layer scope"
```

---

## Task 3: Refactor `WebRtcManager` to consume the injected scope

**Files:**
- Modify: `mobile/android/app/src/main/java/io/wolftown/kaiku/data/voice/WebRtcManager.kt`

### Step 1: Add the constructor parameter

Current (lines 60-63):

```kotlin
@Singleton
class WebRtcManager @Inject constructor(
    @ApplicationContext private val context: Context,
    private val voiceApi: VoiceApi,
    private val eglBaseProvider: EglBaseProvider
) {
```

Target:

```kotlin
@Singleton
class WebRtcManager @Inject constructor(
    @ApplicationContext private val context: Context,
    private val voiceApi: VoiceApi,
    private val eglBaseProvider: EglBaseProvider,
    @VoiceCoroutineScope private val voiceScope: CoroutineScope,
) {
```

Changes: add trailing comma to line 62, add new parameter line. The comma after `voiceScope: CoroutineScope` is optional but matches Kotlin trailing-comma style; preserve whatever style is most local to the existing file.

Add the import (alphabetical with existing `io.wolftown.kaiku.di.*` imports, or next to `VoiceApi` import):

```kotlin
import io.wolftown.kaiku.di.VoiceCoroutineScope
```

`CoroutineScope` is already imported at line 7 — no new import needed.

- [ ] **Apply the constructor change and add the import.**

### Step 2: Replace the inline `CoroutineScope(Dispatchers.IO)` in `stateIn`

Current (lines 154-162):

```kotlin
val voiceIceConnected: StateFlow<Boolean> =
    combine(publisherIceState, subscriberIceState) { p, s ->
        p == PeerConnection.IceConnectionState.CONNECTED &&
            s == PeerConnection.IceConnectionState.CONNECTED
    }.stateIn(
        CoroutineScope(Dispatchers.IO),
        SharingStarted.Eagerly,
        false
    )
```

Target:

```kotlin
val voiceIceConnected: StateFlow<Boolean> =
    combine(publisherIceState, subscriberIceState) { p, s ->
        p == PeerConnection.IceConnectionState.CONNECTED &&
            s == PeerConnection.IceConnectionState.CONNECTED
    }.stateIn(voiceScope, SharingStarted.Eagerly, false)
```

The three-line `.stateIn(...)` call collapses to a single line per Kotlin style — all three arguments fit on one line now that the scope expression is short. (If your style-check disagrees, keep the multi-line form with `voiceScope` replacing the inline scope expression.)

- [ ] **Apply the stateIn change.**

### Step 3: `dispose()` unchanged

`WebRtcManager.dispose()` at line 371 does **not** change in this PR. The injected `voiceScope` is owned by Hilt's `SingletonComponent` and lives for the application lifetime; `WebRtcManager.dispose()` should not cancel a scope it does not own. (See spec Block 4's "Scope lifecycle decision" section for rationale.)

- [ ] **Verify `dispose()` has not been touched in your diff.**

### Step 4: Verify main-src compile + Hilt graph

```bash
cd mobile/android
./gradlew :app:compileDebugKotlin 2>&1 | tail -10
```

Expected: `BUILD SUCCESSFUL`. Hilt regenerates `WebRtcManager_Factory` with the new 4-arg constructor.

If compile fails with `Dagger MissingBinding: @VoiceCoroutineScope CoroutineScope`, Task 2's module is not registered. Verify `mobile/android/app/src/main/java/io/wolftown/kaiku/KaikuApplication.kt` (or wherever `@HiltAndroidApp` lives) doesn't have a manual module list excluding it — Hilt auto-discovers `@Module`s under `@InstallIn(SingletonComponent::class)` when Gradle is configured normally.

### Step 5: Expect test compile to fail

```bash
./gradlew :app:compileDebugUnitTestKotlin 2>&1 | grep -E "^e:" | head -10
```

Expected: one or more errors in `WebRtcManagerTest.kt` — `newWebRtcManager()` calls the constructor with 3 args but the constructor now requires 4. This is by design; Task 4 fixes it.

### Step 6: Commit

```bash
cd /home/detair/GIT/detair/kaiku/.claude/worktrees/voice-scope-inject
git add mobile/android/app/src/main/java/io/wolftown/kaiku/data/voice/WebRtcManager.kt
git status --short  # confirm only this file staged
git commit -m "refactor(voice): WebRtcManager uses injected @VoiceCoroutineScope"
```

---

## Task 4: Update `newWebRtcManager()` helper to accept a scope

**Files:**
- Modify: `mobile/android/app/src/test/java/io/wolftown/kaiku/data/voice/WebRtcManagerTest.kt:45-49`

- [ ] **Step 1: Read the current helper**

```bash
sed -n '40,50p' mobile/android/app/src/test/java/io/wolftown/kaiku/data/voice/WebRtcManagerTest.kt
```

Expected: the helper as captured in pre-flight:

```kotlin
/**
 * Construct a [WebRtcManager] with mocks suitable for tests that exercise
 * signaling/state behavior without needing a real EGL or native PeerConnection.
 * The provider's `create()` is never invoked unless the test reads `.eglBase`.
 */
private fun newWebRtcManager(): WebRtcManager = WebRtcManager(
    context = mockk<Context>(relaxed = true),
    voiceApi = mockk<VoiceApi>(relaxed = true),
    eglBaseProvider = mockk<EglBaseProvider>(relaxed = true),
)
```

- [ ] **Step 2: Change the helper signature to accept a `voiceScope` with a sensible default**

```kotlin
/**
 * Construct a [WebRtcManager] with mocks suitable for tests that exercise
 * signaling/state behavior without needing a real EGL or native PeerConnection.
 * The provider's `create()` is never invoked unless the test reads `.eglBase`.
 *
 * [voiceScope] defaults to an `UnconfinedTestDispatcher`-backed [TestScope],
 * which runs continuations synchronously on the calling thread. Tests that
 * need scheduler control (i.e., `runCurrent()` / `advanceUntilIdle()` must
 * drive the stateIn combine) should pass `backgroundScope` from a `runTest`
 * block instead.
 */
@OptIn(ExperimentalCoroutinesApi::class)
private fun newWebRtcManager(
    voiceScope: CoroutineScope = TestScope(UnconfinedTestDispatcher()),
): WebRtcManager = WebRtcManager(
    context = mockk<Context>(relaxed = true),
    voiceApi = mockk<VoiceApi>(relaxed = true),
    eglBaseProvider = mockk<EglBaseProvider>(relaxed = true),
    voiceScope = voiceScope,
)
```

Add the necessary imports (sorted into the existing alphabetical import block near the top of the file):

```kotlin
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.test.TestScope
import kotlinx.coroutines.test.UnconfinedTestDispatcher
```

`kotlinx.coroutines.ExperimentalCoroutinesApi` is already imported for existing `@OptIn` uses — check `grep -n "ExperimentalCoroutinesApi" mobile/android/app/src/test/java/io/wolftown/kaiku/data/voice/WebRtcManagerTest.kt`. Reuse if present; add if missing.

- [ ] **Step 3: Verify test-src compile now passes**

```bash
cd mobile/android
./gradlew :app:compileDebugUnitTestKotlin 2>&1 | tail -5
```

Expected: `BUILD SUCCESSFUL`. The 5 existing callers of `newWebRtcManager()` still compile because the new parameter has a default.

- [ ] **Step 4: Verify 5 previously-passing tests still pass under the new `UnconfinedTestDispatcher` default**

```bash
./gradlew :app:testDebugUnitTest --tests 'io.wolftown.kaiku.data.voice.WebRtcManagerTest' --rerun-tasks 2>&1 | tail -10
```

Expected: 18 tests, 1 skipped (the still-`@Ignore`d scheduler test), 0 failed. If any previously-passing test now fails, the `UnconfinedTestDispatcher` default is causing unexpected scheduling — downgrade the default to a vanilla `CoroutineScope(EmptyCoroutineContext)` *or* switch to `StandardTestDispatcher()` and re-run. Ordering-sensitive tests may prefer Standard.

- [ ] **Step 5: Commit**

```bash
cd /home/detair/GIT/detair/kaiku/.claude/worktrees/voice-scope-inject
git add mobile/android/app/src/test/java/io/wolftown/kaiku/data/voice/WebRtcManagerTest.kt
git commit -m "test(client): newWebRtcManager helper accepts injected voiceScope"
```

---

## Task 5: Un-`@Ignore` the scheduler test and drive it with `backgroundScope`

**Files:**
- Modify: `mobile/android/app/src/test/java/io/wolftown/kaiku/data/voice/WebRtcManagerTest.kt:343-360`

- [ ] **Step 1: Confirm the current @Ignore block**

```bash
sed -n '343,360p' mobile/android/app/src/test/java/io/wolftown/kaiku/data/voice/WebRtcManagerTest.kt
```

Expected: the `@Ignore(...)` annotation followed by `fun \`voiceIceConnected emits true only when both PCs reach Connected\`() = runTest {`.

- [ ] **Step 2: Remove the `@Ignore` and pass `backgroundScope` into `newWebRtcManager`**

Current:

```kotlin
@OptIn(ExperimentalCoroutinesApi::class)
@Test
@Ignore(
    "voiceIceConnected uses stateIn(CoroutineScope(Dispatchers.IO), Eagerly) " +
        "whose re-emissions can't be driven by TestCoroutineScheduler. Unblock by " +
        "injecting the CoroutineScope into WebRtcManager (separate workstream)."
)
fun `voiceIceConnected emits true only when both PCs reach Connected`() = runTest {
    val webRtcManager = newWebRtcManager()

    // ... existing body unchanged
}
```

Target:

```kotlin
@OptIn(ExperimentalCoroutinesApi::class)
@Test
fun `voiceIceConnected emits true only when both PCs reach Connected`() = runTest {
    val webRtcManager = newWebRtcManager(voiceScope = backgroundScope)

    // ... existing body unchanged
}
```

`backgroundScope` is a property of `TestScope` (the receiver inside `runTest { ... }`) that gives a `CoroutineScope` backed by the same `TestCoroutineScheduler`. Using it means `runCurrent()` now actually drains the combine re-emission triggered by `setPublisherIceStateForTest`/`setSubscriberIceStateForTest`.

- [ ] **Step 3: Remove the `import org.junit.Ignore` line if no other `@Ignore` usages remain**

```bash
grep -c '@Ignore' mobile/android/app/src/test/java/io/wolftown/kaiku/data/voice/WebRtcManagerTest.kt
```

Expected: 0 (after Step 2's deletion). If 0, remove the import:

```bash
# In your editor, delete the line: import org.junit.Ignore
# Or via sed:
sed -i '/^import org.junit.Ignore$/d' mobile/android/app/src/test/java/io/wolftown/kaiku/data/voice/WebRtcManagerTest.kt
```

- [ ] **Step 4: Run the scheduler test**

```bash
cd mobile/android
./gradlew :app:testDebugUnitTest --tests 'io.wolftown.kaiku.data.voice.WebRtcManagerTest.voiceIceConnected emits true only when both PCs reach Connected' --rerun-tasks 2>&1 | tail -10
```

Expected: **1 test, 0 skipped, 0 failed.**

If the test fails with `AssertionError` on `assertTrue(webRtcManager.voiceIceConnected.value)`, the `stateIn` may still need one extra `runCurrent()` after setting the second ICE state — add it if needed (the original test already has `runCurrent()` between each `setXxxIceStateForTest` and its assertion; no new calls should be required).

- [ ] **Step 5: Run the full `WebRtcManagerTest` to confirm no regressions**

```bash
./gradlew :app:testDebugUnitTest --tests 'io.wolftown.kaiku.data.voice.WebRtcManagerTest' --rerun-tasks 2>&1 | tail -10
python3 <<'PY'
import xml.etree.ElementTree as ET
r = ET.parse('app/build/test-results/testDebugUnitTest/TEST-io.wolftown.kaiku.data.voice.WebRtcManagerTest.xml').getroot()
print(f"tests={r.get('tests')} skipped={r.get('skipped')} failures={r.get('failures')} errors={r.get('errors')}")
PY
```

Expected: `tests=18 skipped=0 failures=0 errors=0`.

- [ ] **Step 6: Verify no `@Ignore` remains anywhere in voice tests**

```bash
grep -rn '@Ignore' mobile/android/app/src/test/java/io/wolftown/kaiku/data/voice/
```

Expected: empty output.

- [ ] **Step 7: Commit**

```bash
cd /home/detair/GIT/detair/kaiku/.claude/worktrees/voice-scope-inject
git add mobile/android/app/src/test/java/io/wolftown/kaiku/data/voice/WebRtcManagerTest.kt
git commit -m "test(client): un-ignore voiceIceConnected test; inject backgroundScope"
```

---

## Task 6: Update `mobile/android/app/src/test/AGENTS.md`

**Files:**
- Modify: `mobile/android/app/src/test/AGENTS.md`

Workstream A introduced this file with a "Currently `@Ignore`d tests" section listing one entry, and a "`CoroutineScope(Dispatchers.IO)` + `StandardTestDispatcher` caveat" section proposing three workarounds of which option 2 was "inject the scope." This task makes option 2 canonical (now that it's been applied) and updates the registry.

- [ ] **Step 1: Update the "Currently `@Ignore`d tests" section**

Find the section:

```bash
grep -nE '^## Currently @Ignore|^- `WebRtcManagerTest' mobile/android/app/src/test/AGENTS.md
```

Replace the content of that section — currently lists the `voiceIceConnected` test — with a single-line statement:

```markdown
## Currently `@Ignore`d tests

None.

If you add a new `@Ignore`, append it here with a one-line reason and link
an issue or ticket.
```

- [ ] **Step 2: Update the `CoroutineScope(Dispatchers.IO)` + `StandardTestDispatcher` caveat section**

Find the section:

```bash
grep -nE '^## `CoroutineScope|option 2|Inject the scope' mobile/android/app/src/test/AGENTS.md
```

Replace the three-option list with a single canonical recommendation pointing at `@VoiceCoroutineScope`:

```markdown
## `CoroutineScope(Dispatchers.IO)` + `StandardTestDispatcher` caveat

Production code that wraps a flow in `stateIn(CoroutineScope(Dispatchers.IO), Eagerly, …)`
or launches collectors on its own `Dispatchers.IO` scope cannot be driven by
`TestCoroutineScheduler`. `runCurrent()` / `advanceUntilIdle()` only drain the
test dispatcher, so synchronous assertions on `.value` right after mutating the
source flow may read stale state.

**Canonical fix: inject the scope via Hilt with a dedicated qualifier.**

```kotlin
// Production — in a Hilt module
@Module @InstallIn(SingletonComponent::class)
object MyFeatureScopeModule {
    @Provides @Singleton @MyFeatureScope
    fun provideScope(): CoroutineScope =
        CoroutineScope(SupervisorJob() + Dispatchers.IO)
}

// Production — in the consumer
class MyFeature @Inject constructor(
    @MyFeatureScope private val scope: CoroutineScope,
) {
    val state: StateFlow<Boolean> = combine(...).stateIn(scope, Eagerly, false)
}

// Test
@Test fun test() = runTest {
    val feature = MyFeature(scope = backgroundScope)
    // TestCoroutineScheduler now drives the stateIn combine.
}
```

`WebRtcManager`'s `voiceIceConnected` uses this pattern via `@VoiceCoroutineScope`
(`mobile/android/app/src/main/java/io/wolftown/kaiku/di/VoiceCoroutineScope.kt`).

**Fallbacks (avoid unless the injection path is genuinely blocked):**

- `@VisibleForTesting internal var scope` setter — requires making the
  downstream `StateFlow` `by lazy` and introduces a test-only code path.
- `Dispatchers.Unconfined` for `stateIn` — sidesteps scheduling but runs
  continuations on the emitter's thread, with subtle correctness risks.
```

- [ ] **Step 3: Commit**

```bash
cd /home/detair/GIT/detair/kaiku/.claude/worktrees/voice-scope-inject
git add mobile/android/app/src/test/AGENTS.md
git commit -m "docs(client): update AGENTS.md — no @Ignore'd voice tests; canonical scope inject"
```

---

## Task 7: CHANGELOG entry

**Files:**
- Modify: `CHANGELOG.md` — top-most `### Fixed` under `## [Unreleased]`.

- [ ] **Step 1: Locate the insertion point**

```bash
grep -nE '^## \[Unreleased\]|^### Fixed' CHANGELOG.md | head -5
```

Find the first `### Fixed` that falls inside `## [Unreleased]`.

- [ ] **Step 2: Append a new bullet at the top of that `### Fixed` block**

```markdown
- Android: voice-layer `CoroutineScope` is now DI-provided, making the full `WebRtcManager` test suite runnable under `TestCoroutineScheduler` and unblocking CI coverage for dual-PC ICE state transitions
```

(Do not introduce a new `## [Unreleased]` section — use the existing one.)

- [ ] **Step 3: Commit**

```bash
cd /home/detair/GIT/detair/kaiku/.claude/worktrees/voice-scope-inject
git add CHANGELOG.md
git commit -m "docs(client): CHANGELOG entry for voice-layer scope injection"
```

---

## Final Verification (before opening PR)

- [ ] **Compile + full unit test suite**

```bash
cd mobile/android
./gradlew :app:compileDebugKotlin 2>&1 | grep -E "BUILD (SUCCESSFUL|FAILED)" | tail -2
./gradlew :app:testDebugUnitTest --rerun-tasks 2>&1 | tail -3
python3 <<'PY'
import xml.etree.ElementTree as ET, glob
total=skips=fails=errors=0
by_cls = []
for f in sorted(glob.glob('app/build/test-results/testDebugUnitTest/*.xml')):
    r = ET.parse(f).getroot()
    t = int(r.get('tests',0)); s = int(r.get('skipped',0)); fl = int(r.get('failures',0)); e = int(r.get('errors',0))
    total += t; skips += s; fails += fl; errors += e
    if fl or e: by_cls.append((r.get('name'), fl+e))
print(f"TOTAL tests={total} skipped={skips} failures={fails} errors={errors}")
for n, c in by_cls: print(f"  {n}: {c}")
PY
```

Expected:
- `BUILD SUCCESSFUL` on compile.
- `tests=175` (Workstream A baseline 174 + 1 un-skipped = same 174 if backed by the same set; more likely 174 with 1 fewer skipped).
- `skipped=0` (dropped from 1).
- `failures=13` — the Block 3 allowlist (`AuthStateTest×2, AuthFlowTest×5, MessageFlowTest×5, QrLoginFlowTest×1`). No new classes.

If any failure class appears outside the allowlist, **STOP** — Block 4 has introduced a regression. Likely suspect: the `UnconfinedTestDispatcher` default in `newWebRtcManager()` reordered emissions for a previously-passing test. Diagnose by switching the default to a vanilla `CoroutineScope` and re-running.

- [ ] **Grep for residual anti-patterns**

```bash
grep -rn '@Ignore' mobile/android/app/src/test/java/io/wolftown/kaiku/data/voice/
grep -rn 'CoroutineScope(Dispatchers.IO)' mobile/android/app/src/main/java/io/wolftown/kaiku/data/voice/
```

Expected: both empty. The first confirms no voice tests are ignored; the second confirms `WebRtcManager` no longer constructs its own scope.

- [ ] **Commit log review**

```bash
cd /home/detair/GIT/detair/kaiku/.claude/worktrees/voice-scope-inject
git log --oneline origin/main..HEAD
```

Expected 7 commits in this order:

1. `feat(client): @VoiceCoroutineScope qualifier for voice-layer scope DI`
2. `feat(client): VoiceCoroutineScopeModule @Provides for voice-layer scope`
3. `refactor(voice): WebRtcManager uses injected @VoiceCoroutineScope`
4. `test(client): newWebRtcManager helper accepts injected voiceScope`
5. `test(client): un-ignore voiceIceConnected test; inject backgroundScope`
6. `docs(client): update AGENTS.md — no @Ignore'd voice tests; canonical scope inject`
7. `docs(client): CHANGELOG entry for voice-layer scope injection`

- [ ] **Push and open PR**

```bash
git push -u origin feat/webrtc-scope-inject
gh pr create --base main --head feat/webrtc-scope-inject \
  --title "feat(voice): inject @VoiceCoroutineScope into WebRtcManager" \
  --body "$(cat <<'EOF'
## Summary

Block 4 of Phase 2.5 open-topics cleanup. Un-ignores `WebRtcManagerTest.voiceIceConnected emits true only when both PCs reach Connected` (the last `@Ignore` in the Android voice-test suite) by making `WebRtcManager`'s internal `stateIn` scope injectable via a `@VoiceCoroutineScope` Hilt qualifier.

- New qualifier: `io.wolftown.kaiku.di.VoiceCoroutineScope`
- New module: `VoiceCoroutineScopeModule` with `@Provides @Singleton` for `CoroutineScope(SupervisorJob() + Dispatchers.IO)`
- `WebRtcManager` gains a 4th constructor parameter; inline `CoroutineScope(Dispatchers.IO)` at `WebRtcManager.kt:159` is replaced
- Test helper `newWebRtcManager()` accepts an optional `voiceScope` (default `TestScope(UnconfinedTestDispatcher())`); the previously-`@Ignore`d test passes `backgroundScope`
- AGENTS.md updated: `@Ignore` registry now "None"; scope-injection becomes the canonical pattern

Spec: `docs/superpowers/specs/2026-04-16-open-topics-cleanup-design.md` — Block 4.

## Test plan

- [x] `./gradlew :app:compileDebugKotlin` — BUILD SUCCESSFUL
- [x] `./gradlew :app:testDebugUnitTest --tests WebRtcManagerTest` — 18 tests, 0 skipped, 0 failed
- [x] Full `:app:testDebugUnitTest` — 13 remaining failures, all in Block 3's pre-existing allowlist (`AuthStateTest`, `AuthFlowTest`, `MessageFlowTest`, `QrLoginFlowTest`)
- [x] `grep -rn '@Ignore' mobile/android/app/src/test/java/io/wolftown/kaiku/data/voice/` — empty

## Scope lifecycle

The injected `CoroutineScope` is owned by Hilt's `SingletonComponent`. `WebRtcManager.dispose()` does **not** cancel it — `SupervisorJob` keeps sibling children alive, and the scope may gain future consumers. Scope-cancellation semantics on app shutdown are a separate concern deferred per spec.

🤖 Generated with [Claude Code](https://claude.com/claude-code)
EOF
)"
```

- [ ] **Wait for CI green and merge**

```bash
gh pr checks <PR_NUMBER> --watch
gh pr merge <PR_NUMBER> --squash
```

---

## Post-merge cleanup

```bash
cd /home/detair/GIT/detair/kaiku
git worktree remove .claude/worktrees/voice-scope-inject
git branch -D feat/webrtc-scope-inject
git push origin --delete feat/webrtc-scope-inject
git fetch --prune
```

---

## Notes for the implementer

- **Do not batch Tasks 1 and 2 into Task 3's refactor.** The qualifier + provider are standalone and compile-clean on their own; keeping them as separate commits produces a readable history where each commit is an atomically-reviewable DI addition.
- **The `UnconfinedTestDispatcher` default in `newWebRtcManager`** is a conservative choice. It runs continuations synchronously on the calling thread, which matches the prior behavior of the pure-data tests that never touched the scope at all. If any of the 5 previously-passing tests start failing after Task 4, swap to `StandardTestDispatcher()` and re-run; they may have picked up an implicit ordering assumption.
- **Hilt auto-discovery of the new module.** Both `VoiceCoroutineScopeModule` (new) and `VoiceModule` (existing) are `@InstallIn(SingletonComponent::class)`. Gradle's Hilt plugin picks both up automatically — no manual registration in `KaikuApplication` needed.
- **If Task 5's scheduler test fails after un-ignoring** with an `AssertionError`, the most likely cause is that `runTest { backgroundScope }` is not what you think. `backgroundScope` is the `CoroutineScope` property of the `TestScope` *receiver* inside `runTest { … }` — it's already bound to the test's `TestCoroutineScheduler`. If you see "backgroundScope not resolved," make sure the test lambda is a `TestScope.() -> Unit` (which `runTest` is), and that `kotlinx-coroutines-test:1.9.0` is on the classpath (verified during spec writing).
- **Do not add scope cancellation to `dispose()`** even if it feels "tidy." Spec Block 4 explicitly defers that to a follow-up workstream. Landing it here couples this small injection PR to a broader lifecycle-audit concern that isn't ready to ship.
