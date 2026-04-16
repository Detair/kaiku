# Android Test Infrastructure Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Introduce an `EglBaseProvider` DI seam so Android JVM unit tests can construct `WebRtcManager` without invoking native EGL14; un-`@Ignore` six voice tests; fix two `VoiceViewModelTest` failures; add a new `VoiceRepositoryTest` covering dual-PC signaling; document the test-infra conventions.

**Architecture:** Small refactor of `WebRtcManager` to accept an injected `EglBaseProvider` interface with a Hilt-bound default implementation. `EglBase` construction moves behind `Lazy<EglBase>` so tests that never touch video rendering pay no cost, and production `dispose()` uses `isInitialized()` to avoid triggering lazy init purely to release. A second axis of the fix is adding `mockk-agent-jvm` to the test classpath, which lets MockK proxy final/abstract Android classes like `EglBase`.

**Tech Stack:** Kotlin, Hilt (DI), MockK + mockk-agent-jvm, Turbine, kotlinx-coroutines-test, JUnit 4, `stream-webrtc-android` 1.3.0.

**Spec:** `docs/superpowers/specs/2026-04-16-android-test-infra-design.md`

**Parallelization safe:** One PR, tasks executed serially. No cross-PR dependencies among Phase 2 workstreams. **But this plan depends on Phase 1 PR E (`feat/android-publisher-pc`, GitHub PR #533) having merged first** — every Pre-Execution Verified Fact and every code snippet assumes the dual-PC shape introduced by PR E (`publisherPc`/`subscriberPc`, `voiceIceConnected` StateFlow, `VoicePublisherOffer`/`VoiceSubscriberAnswer` events, `VoiceIceCandidate.pcType`, 6 `@Ignore`d tests, etc.).

---

## Pre-flight Check (BLOCKING — run before creating the worktree)

- [ ] **Verify PR E has merged to `main`**

```bash
cd /home/detair/GIT/detair/kaiku
git fetch origin
git log origin/main --oneline --grep='#533' | head -3
```

Expected: at least one commit whose subject ends with `(#533)` — GitHub's squash-merge appends the PR number, so this match is unambiguous regardless of how the PR's title may have been edited. If the grep returns nothing, PR E has not merged yet — **STOP**. Options:
- **Wait** for PR E to merge, then restart this plan.
- **Branch from PR E explicitly** (risky — the squash-merge will change hashes): `git worktree add .claude/worktrees/android-test-infra -b fix/android-test-infra feat/android-publisher-pc` and be prepared to rebase onto `main` after PR E merges.

The recommended path is to wait. The rest of this plan assumes the merge has happened.

- [ ] **Verify the dual-PC code shape exists on `main`**

```bash
grep -n 'subscriberPc\|publisherPc\|voiceIceConnected\|handlePublisherAnswer' \
  mobile/android/app/src/main/java/io/wolftown/kaiku/data/voice/WebRtcManager.kt | head -6
grep -n '@Ignore' \
  mobile/android/app/src/test/java/io/wolftown/kaiku/data/voice/WebRtcManagerTest.kt | head -6
```

Expected: at least one match for each pattern in `WebRtcManager.kt`, and exactly 6 `@Ignore` matches in `WebRtcManagerTest.kt`. If any search returns zero matches, **STOP and escalate** — the merge may have been squashed in a way that lost changes, or an intervening PR may have altered the shape.

---

## Worktree Setup (run once after pre-flight passes)

```bash
cd /home/detair/GIT/detair/kaiku
git worktree add .claude/worktrees/android-test-infra -b fix/android-test-infra
cd .claude/worktrees/android-test-infra
```

Working branch: `fix/android-test-infra`, based on `main` (which now includes PR E's changes). Working directory for all tasks below: `/home/detair/GIT/detair/kaiku/.claude/worktrees/android-test-infra`.

**Environment for gradle commands** (one-time export; all subsequent gradle invocations inherit):

```bash
export JAVA_HOME="$HOME/.local/share/jdk/jdk-17.0.18+8"
export ANDROID_HOME="$HOME/.local/share/android-sdk"
export PATH="$JAVA_HOME/bin:$PATH"
```

---

## Pre-Execution Verified Facts

Every fact below was confirmed against the current `android-publisher-pc` worktree (which contains the Phase 1 E-branch). The implementer can rely on these without re-checking:

- **Hilt module for voice bindings:** `mobile/android/app/src/main/java/io/wolftown/kaiku/di/VoiceModule.kt` — `abstract class` with `@Module @InstallIn(SingletonComponent::class)` already containing one `@Binds @Singleton` for `VoiceApi`. The new `EglBaseProvider` binding goes here alongside.
- **`WebRtcManager` class header:** `mobile/android/app/src/main/java/io/wolftown/kaiku/data/voice/WebRtcManager.kt:60-63`:
  ```kotlin
  @Singleton
  class WebRtcManager @Inject constructor(
      @ApplicationContext private val context: Context,
      private val voiceApi: VoiceApi
  ) {
  ```
- **Eager `EglBase` field init:** `WebRtcManager.kt:110` — `val eglBase: EglBase = EglBase.create()`.
- **`dispose()` eglBase release:** `WebRtcManager.kt:361-374` — currently has try/catch wrapping `eglBase.release()`.
- **Test deps already present** (`mobile/android/app/build.gradle.kts:135-142`): `junit:junit:4.13.2`, `io.mockk:mockk:1.13.14`, `app.cash.turbine:turbine:1.2.0`, `org.jetbrains.kotlinx:kotlinx-coroutines-test:1.9.0`, `io.ktor:ktor-client-mock:3.1.0`, `com.squareup.okhttp3:mockwebserver:4.12.0`, `org.jetbrains.kotlin:kotlin-test:2.1.0`. The new addition: `mockk-agent-jvm:1.13.14` (same version as existing `mockk`).
- **Test file exists with 6 `@Ignore`:** `mobile/android/app/src/test/java/io/wolftown/kaiku/data/voice/WebRtcManagerTest.kt` — each of the 6 `@Ignore`s has the rationale comment "EglBase.create() dispatches via native EGL14 JNI that mockkStatic cannot intercept…".
- **`VoiceViewModelTest` failing lines:** `mobile/android/app/src/test/java/io/wolftown/kaiku/ui/voice/VoiceViewModelTest.kt:79-80` — `val mockEglBase = mockk<EglBase>(relaxed = true)` then `every { webRtcManager.eglBase } returns mockEglBase`.
- **`VoiceRepository` structure:** private fields include `eventCollectionJob`, `serviceEventJob`, `iceConnectedJob`. `handleServerEvent` is a `private fun` that dispatches on `ServerEvent` sealed class.
- **`voiceIceConnected` contract:** `StateFlow<Boolean>` exposed from `WebRtcManager`; emits true only when both publisher and subscriber PC ICE states are `CONNECTED`.
- **`FakeRtcPc`/native classes:** none needed — test never touches real `PeerConnection` or `EglBase` instances; mocks at the interface level suffice.

---

## File Map

| Worktree | Files Modified/Created |
|----------|------------------------|
| `.claude/worktrees/android-test-infra` | `mobile/android/app/build.gradle.kts`, `mobile/android/app/src/main/java/io/wolftown/kaiku/data/voice/EglBaseProvider.kt` (new), `mobile/android/app/src/main/java/io/wolftown/kaiku/di/VoiceModule.kt`, `mobile/android/app/src/main/java/io/wolftown/kaiku/data/voice/WebRtcManager.kt`, `mobile/android/app/src/test/java/io/wolftown/kaiku/data/voice/WebRtcManagerTest.kt`, `mobile/android/app/src/test/java/io/wolftown/kaiku/data/repository/VoiceRepositoryTest.kt` (new), `mobile/android/app/src/test/AGENTS.md` (new), `CHANGELOG.md` |

---

## Task 1: Add `mockk-agent-jvm` to Android test deps

**Files:**
- Modify: `mobile/android/app/build.gradle.kts:137` (add line after existing mockk dep)

- [ ] **Step 1: Confirm current state**

```bash
grep -n 'mockk\|mockk-agent' mobile/android/app/build.gradle.kts
```
Expected: one line `testImplementation("io.mockk:mockk:1.13.14")` at line 137, zero `mockk-agent` lines.

- [ ] **Step 2: Add the dep**

Insert a new `testImplementation` line immediately after line 137, matching the existing MockK version:

```kotlin
    testImplementation("io.mockk:mockk:1.13.14")
    testImplementation("io.mockk:mockk-agent-jvm:1.13.14")  // Final/abstract class mocking (e.g., EglBase)
```

- [ ] **Step 3: Run the full test suite and confirm targeted fixes**

First, the full-suite summary:
```bash
./gradlew :app:testDebugUnitTest 2>&1 | grep -E "tests? completed|BUILD (SUCCESS|FAIL)|FAILED$" | head -30
```

Then a focused run on `VoiceViewModelTest` so the two specific flips are visible by name:
```bash
./gradlew :app:testDebugUnitTest --tests 'io.wolftown.kaiku.ui.voice.VoiceViewModelTest' 2>&1 | tail -40
```

Expected outcome changes from baseline:
- **`VoiceViewModelTest > isConnected state reflects repository`** — previously FAILED with `MockKException: Can't instantiate proxy for class org.webrtc.EglBase`; now PASSES.
- **`VoiceViewModelTest > screenShares state reflects repository`** — same pattern; now PASSES.
- The 6 `@Ignore`d `WebRtcManagerTest` cases remain skipped (still `@Ignore`d; Task 5 will address).
- The ~12 pre-existing `AuthStateTest` / `AuthFlowTest` / `MessageFlowTest` / `QrLoginFlowTest` failures are unchanged — out of scope.

If more than 3 previously-passing tests start failing due to the agent, **STOP and escalate to the user** per the spec's Risk #2 threshold. Otherwise proceed.

- [ ] **Step 4: Commit**

```bash
git add mobile/android/app/build.gradle.kts
git commit -m "chore(client): add mockk-agent-jvm to Android test deps"
```

---

## Task 2: Create `EglBaseProvider` interface + default impl

**Files:**
- Create: `mobile/android/app/src/main/java/io/wolftown/kaiku/data/voice/EglBaseProvider.kt`

- [ ] **Step 1: Create the file**

```kotlin
package io.wolftown.kaiku.data.voice

import org.webrtc.EglBase
import javax.inject.Inject
import javax.inject.Singleton

/**
 * Indirection over [EglBase.create] so tests can substitute the EGL stack
 * without invoking native EGL14 JNI.
 *
 * Production code MUST inject this rather than calling [EglBase.create] directly.
 * In tests, provide a `mockk<EglBaseProvider>(relaxed = true)` via the
 * [WebRtcManager] constructor; the `create()` return is only read if the test
 * actually touches `webRtcManager.eglBase`.
 */
interface EglBaseProvider {
    fun create(): EglBase
}

@Singleton
class DefaultEglBaseProvider @Inject constructor() : EglBaseProvider {
    override fun create(): EglBase = EglBase.create()
}
```

- [ ] **Step 2: Verify it compiles**

```bash
./gradlew :app:compileDebugKotlin 2>&1 | tail -5
```

Expected: `BUILD SUCCESSFUL`. The new file stands alone — no consumer yet — so no other file needs to change at this step.

- [ ] **Step 3: Commit**

```bash
git add mobile/android/app/src/main/java/io/wolftown/kaiku/data/voice/EglBaseProvider.kt
git commit -m "feat(client): EglBaseProvider interface for DI-based testability"
```

---

## Task 3: Bind `EglBaseProvider` in `VoiceModule`

**Files:**
- Modify: `mobile/android/app/src/main/java/io/wolftown/kaiku/di/VoiceModule.kt`

- [ ] **Step 1: Add the binding**

Replace the body of `VoiceModule` to include a second `@Binds` method for `EglBaseProvider`. Exact target state:

```kotlin
package io.wolftown.kaiku.di

import dagger.Binds
import dagger.Module
import dagger.hilt.InstallIn
import dagger.hilt.components.SingletonComponent
import io.wolftown.kaiku.data.api.VoiceApi
import io.wolftown.kaiku.data.api.VoiceApiImpl
import io.wolftown.kaiku.data.voice.DefaultEglBaseProvider
import io.wolftown.kaiku.data.voice.EglBaseProvider
import javax.inject.Singleton

@Module
@InstallIn(SingletonComponent::class)
abstract class VoiceModule {

    @Binds
    @Singleton
    abstract fun bindVoiceApi(impl: VoiceApiImpl): VoiceApi

    @Binds
    @Singleton
    abstract fun bindEglBaseProvider(impl: DefaultEglBaseProvider): EglBaseProvider
}
```

- [ ] **Step 2: Verify it compiles**

```bash
./gradlew :app:compileDebugKotlin 2>&1 | tail -5
```

Expected: `BUILD SUCCESSFUL`. Hilt codegen may emit a new factory; that's fine.

- [ ] **Step 3: Commit**

```bash
git add mobile/android/app/src/main/java/io/wolftown/kaiku/di/VoiceModule.kt
git commit -m "feat(client): bind EglBaseProvider in VoiceModule"
```

---

## Task 4: Refactor `WebRtcManager` to use injected provider with lazy init

**Files:**
- Modify: `mobile/android/app/src/main/java/io/wolftown/kaiku/data/voice/WebRtcManager.kt`

- [ ] **Step 1: Update the constructor (line 60-63)**

Add `eglBaseProvider: EglBaseProvider` as the third constructor parameter. After the change, lines 60-64 should read:

```kotlin
@Singleton
class WebRtcManager @Inject constructor(
    @ApplicationContext private val context: Context,
    private val voiceApi: VoiceApi,
    private val eglBaseProvider: EglBaseProvider,
) {
```

- [ ] **Step 2: Replace the eager `eglBase` field (line 110)**

Currently:
```kotlin
val eglBase: EglBase = EglBase.create()
```

Replace with a private `Lazy<EglBase>` and a public `get()`-style accessor:

```kotlin
/**
 * Lazy so tests that never touch video rendering never trigger EGL init,
 * and so production startup defers the native cost until first use.
 *
 * Exposed via the `eglBase` accessor below; dispose() uses `_eglBase.isInitialized()`
 * to avoid re-triggering init purely to release.
 */
private val _eglBase: Lazy<EglBase> = lazy { eglBaseProvider.create() }

/** The EGL stack used by SurfaceViewRenderer for video rendering. */
val eglBase: EglBase get() = _eglBase.value
```

Keep the existing `val eglBase: EglBase` *name* for external callers (e.g., `VoiceViewModel` reads `webRtcManager.eglBase`). The type is unchanged.

- [ ] **Step 3: Update `dispose()` (lines 361-374)**

Currently:
```kotlin
fun dispose() {
    closePeerConnections()
    factory?.dispose()
    factory = null
    audioDeviceModule?.release()
    audioDeviceModule = null
    try {
        eglBase.release()
    } catch (_: IllegalStateException) {
        // EglBase may already be released
    } catch (e: Exception) {
        logger.log(Level.WARNING, "Unexpected error releasing EglBase", e)
    }
    logger.info("WebRtcManager disposed")
}
```

Change to only release if the lazy is initialized:

```kotlin
fun dispose() {
    closePeerConnections()
    factory?.dispose()
    factory = null
    audioDeviceModule?.release()
    audioDeviceModule = null
    if (_eglBase.isInitialized()) {
        try {
            _eglBase.value.release()
        } catch (_: IllegalStateException) {
            // EglBase may already be released
        } catch (e: Exception) {
            logger.log(Level.WARNING, "Unexpected error releasing EglBase", e)
        }
    }
    logger.info("WebRtcManager disposed")
}
```

- [ ] **Step 4: Verify compile**

```bash
./gradlew :app:compileDebugKotlin 2>&1 | tail -5
```

Expected: `BUILD SUCCESSFUL`. Hilt re-generates the `WebRtcManager_Factory` to include the new `EglBaseProvider` param.

- [ ] **Step 5: Verify existing tests still behave as expected**

```bash
./gradlew :app:compileDebugUnitTestKotlin 2>&1 | grep -E "^e:" | head -10
```

Expected errors:
- `WebRtcManagerTest.kt` — existing `addIceCandidate buffers up to MAX_PENDING_CANDIDATES then drops` and the other 5 `@Ignore`d tests now fail **compilation** because they call `WebRtcManager(context = ..., voiceApi = ...)` with two args but the constructor now takes three.

This is expected and good — it means the refactor is enforcing the new signature. Task 5 updates these tests.

If any test file OTHER than `WebRtcManagerTest.kt` has a compile error, **STOP and escalate**. Likely production-code consumers of `WebRtcManager` would break compile too, but since `WebRtcManager` is `@Inject`-constructed, all production call sites get the new param injected automatically by Hilt. No non-test file should need updating.

- [ ] **Step 6: Commit**

```bash
git add mobile/android/app/src/main/java/io/wolftown/kaiku/data/voice/WebRtcManager.kt
git commit -m "refactor(client): WebRtcManager uses injected EglBaseProvider"
```

---

## Task 5: Un-`@Ignore` voice tests; inject mock provider

**Files:**
- Modify: `mobile/android/app/src/test/java/io/wolftown/kaiku/data/voice/WebRtcManagerTest.kt`

- [ ] **Step 1: Read the current file to confirm structure**

```bash
grep -n "@Ignore\|mockkStatic\|unmockkStatic\|EglBaseProvider\|tearDownEglBaseMock" \
  mobile/android/app/src/test/java/io/wolftown/kaiku/data/voice/WebRtcManagerTest.kt
```

Expected: 6 `@Ignore` annotations, 6 `mockkStatic(EglBase::class)` calls (one per `@Ignore`d test), 1 `unmockkStatic(EglBase::class)` call inside `@After tearDownEglBaseMock`, zero `EglBaseProvider` references.

- [ ] **Step 2: Remove the `@After tearDownEglBaseMock`**

Find the `@After` block (around line 251-263):
```kotlin
@After
fun tearDownEglBaseMock() {
    // Safe to call even if mockkStatic wasn't invoked this test.
    try {
        unmockkStatic(EglBase::class)
    } catch (_: Throwable) {
        // Ignore — nothing was mocked.
    }
}
```

Delete the entire block. Also delete the now-unused imports:
- `import io.mockk.mockkStatic`
- `import io.mockk.unmockkStatic`
- `import org.junit.After`
- `import org.webrtc.EglBase`

Keep: `import io.mockk.every`, `import io.mockk.mockk`, and add `import io.wolftown.kaiku.data.voice.EglBaseProvider` (same package — may be omissible, but include for clarity).

- [ ] **Step 3: Add a test helper `newWebRtcManager()` at the top of the test class**

Insert near the top of the class body, right after the `@After` removal:

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

- [ ] **Step 4: Rewrite the 6 `@Ignore`d tests to use the helper**

For each of the 6 `@Ignore`d tests, apply the exact same transformation:

1. Delete the `@Ignore("EglBase.create() dispatches via native EGL14 JNI…")` line.
2. Delete the `mockkStatic(EglBase::class)` + `every { EglBase.create() } returns mockk(relaxed = true)` lines at the top of the test body.
3. Replace the manual construction:
   ```kotlin
   val webRtcManager = WebRtcManager(
       context = mockk<Context>(relaxed = true),
       voiceApi = mockk<VoiceApi>(relaxed = true)
   )
   ```
   with:
   ```kotlin
   val webRtcManager = newWebRtcManager()
   ```
4. Leave the rest of the test body (assertions, `addIceCandidate` calls, `voiceIceConnected.value` reads) unchanged.

The 6 tests to update (exact method names):
- `addIceCandidate buffers up to MAX_PENDING_CANDIDATES then drops`
- `addIceCandidate buffers publisher candidates up to cap then drops`
- `addIceCandidate routes by pcType to publisher vs subscriber buffer`
- `addIceCandidate with unknown pcType is no-op`
- `voiceIceConnected starts false before any PC connects`
- `voiceIceConnected emits true only when both PCs reach Connected` (uses `runTest`; transformation is the same)

- [ ] **Step 5: Run the test suite filtered to this class**

```bash
./gradlew :app:testDebugUnitTest --tests 'io.wolftown.kaiku.data.voice.WebRtcManagerTest' 2>&1 | tail -15
```

Expected: **15 tests completed, 0 failed, 0 skipped.** (9 pre-existing pure-data tests + 6 previously-@Ignore'd tests now passing.)

If any test still fails, read the failure carefully. Most likely cause: a residual `mockkStatic` reference missed during Step 4, or an import error. Fix and re-run. Do not revert the @Ignore removals.

- [ ] **Step 6: Also run the VoiceViewModel tests to confirm no regression**

```bash
./gradlew :app:testDebugUnitTest --tests 'io.wolftown.kaiku.ui.voice.VoiceViewModelTest' 2>&1 | tail -10
```

Expected: all tests pass. Task 1 already made the two previously-failing ones pass; Task 4's constructor change is transparent to the view-model tests because they mock `WebRtcManager` entirely.

- [ ] **Step 7: Commit**

```bash
git add mobile/android/app/src/test/java/io/wolftown/kaiku/data/voice/WebRtcManagerTest.kt
git commit -m "test(client): un-ignore voice tests; inject mock EglBaseProvider"
```

---

## Task 6: New `VoiceRepositoryTest` covering dual-PC signaling

**Files:**
- Create: `mobile/android/app/src/test/java/io/wolftown/kaiku/data/repository/VoiceRepositoryTest.kt`

- [ ] **Step 1: Read `VoiceRepository` to confirm field visibility and public surface**

```bash
grep -n "private\|fun joinChannel\|fun leaveChannel\|fun handleServerEvent\|iceConnectedJob\|_isConnected\|_currentChannelId" \
  mobile/android/app/src/main/java/io/wolftown/kaiku/data/repository/VoiceRepository.kt | head -30
```

Key points to confirm:
- `joinChannel(channelId: String)` is `suspend` and `public`.
- `leaveChannel()` is `suspend` and `public`.
- `handleServerEvent(channelId: String, event: ServerEvent)` is `private fun`.
- `_isConnected: MutableStateFlow<Boolean>` is private with public `isConnected: StateFlow<Boolean>` accessor.
- `_currentChannelId: MutableStateFlow<String?>` is private with public `currentChannelId: StateFlow<String?>`.

Because `handleServerEvent` is private, the test cannot call it directly. The tests must drive it by:
1. Making the test's mock `KaikuWebSocket.events` emit a `ServerEvent`, and
2. Calling `joinChannel()` to start the event-collection job.

This matches the existing `VoiceViewModelTest` pattern.

- [ ] **Step 2: Create the test file**

File: `mobile/android/app/src/test/java/io/wolftown/kaiku/data/repository/VoiceRepositoryTest.kt`.

```kotlin
package io.wolftown.kaiku.data.repository

import android.content.Context
import app.cash.turbine.test
import io.mockk.coEvery
import io.mockk.coVerify
import io.mockk.every
import io.mockk.mockk
import io.mockk.slot
import io.mockk.verify
import io.wolftown.kaiku.data.voice.AudioRouteManager
import io.wolftown.kaiku.data.voice.VoiceServiceEvents
import io.wolftown.kaiku.data.voice.WebRtcManager
import io.wolftown.kaiku.data.ws.ClientEvent
import io.wolftown.kaiku.data.ws.KaikuWebSocket
import io.wolftown.kaiku.data.ws.ServerEvent
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.test.StandardTestDispatcher
import kotlinx.coroutines.test.advanceUntilIdle
import kotlinx.coroutines.test.resetMain
import kotlinx.coroutines.test.runTest
import kotlinx.coroutines.test.setMain
import org.junit.After
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test

/**
 * Unit tests for [VoiceRepository]'s dual-PC signaling wiring.
 *
 * Verifies:
 * - Publisher offer / ICE candidate callbacks produce correct WS sends with
 *   `pcType` labels.
 * - Incoming `VoicePublisherAnswer` / `VoiceSubscriberOffer` / `VoiceIceCandidate`
 *   events route to the matching `WebRtcManager` method.
 * - The repository's `_isConnected` state follows `WebRtcManager.voiceIceConnected`.
 *
 * Out of scope:
 * - Error propagation (`onError` → `_error`); covered by Phase 2 workstream B.2.
 * - Screen share / participant state.
 * - Cleanup race conditions on `iceConnectedJob.cancel()`.
 */
@OptIn(ExperimentalCoroutinesApi::class)
class VoiceRepositoryTest {

    private lateinit var webRtcManager: WebRtcManager
    private lateinit var webSocket: KaikuWebSocket
    private lateinit var audioRouteManager: AudioRouteManager
    private lateinit var voiceServiceEvents: VoiceServiceEvents
    private lateinit var context: Context
    private lateinit var repository: VoiceRepository

    private val testDispatcher = StandardTestDispatcher()

    // Mutable backing flows for the WebSocket event stream and the publisher-
    // / subscriber-side ICE-connected StateFlow. Tests drive these to simulate
    // server events and dual-PC state transitions.
    private val wsEvents = MutableSharedFlow<ServerEvent>(extraBufferCapacity = 8)
    private val voiceIceConnected = MutableStateFlow(false)

    // Callback slots that capture what VoiceRepository wires into WebRtcManager.
    private val onPublisherOfferSlot = slot<(String) -> Unit>()
    private val onPublisherIceSlot = slot<(String) -> Unit>()
    private val onSubscriberAnswerSlot = slot<(String) -> Unit>()
    private val onSubscriberIceSlot = slot<(String) -> Unit>()

    @Before
    fun setUp() {
        Dispatchers.setMain(testDispatcher)
        webRtcManager = mockk(relaxed = true)
        webSocket = mockk(relaxed = true)
        audioRouteManager = mockk(relaxed = true)
        voiceServiceEvents = mockk(relaxed = true)
        context = mockk(relaxed = true)

        every { webSocket.events } returns wsEvents
        every { webRtcManager.voiceIceConnected } returns voiceIceConnected
        every { voiceServiceEvents.events } returns MutableSharedFlow()

        // Capture the callbacks so tests can invoke them directly.
        every { webRtcManager.onPublisherOffer = capture(onPublisherOfferSlot) } answers {}
        every { webRtcManager.onPublisherIceCandidate = capture(onPublisherIceSlot) } answers {}
        every { webRtcManager.onSubscriberAnswer = capture(onSubscriberAnswerSlot) } answers {}
        every { webRtcManager.onSubscriberIceCandidate = capture(onSubscriberIceSlot) } answers {}

        repository = VoiceRepository(
            webRtcManager = webRtcManager,
            webSocket = webSocket,
            audioRouteManager = audioRouteManager,
            voiceServiceEvents = voiceServiceEvents,
            context = context,
        )
    }

    @After
    fun tearDown() {
        Dispatchers.resetMain()
    }

    @Test
    fun `joinChannel sends VoicePublisherOffer when onPublisherOffer fires`() = runTest {
        repository.joinChannel("ch-1")
        advanceUntilIdle()

        // Invoke the captured publisher-offer callback as if WebRtcManager produced an SDP.
        onPublisherOfferSlot.captured.invoke("v=0\r\no=- 1 IN IP4 0.0.0.0\r\n")

        verify { webSocket.send(match<ClientEvent.VoicePublisherOffer> { event ->
            event.channelId == "ch-1" && event.sdp.startsWith("v=0")
        }) }
    }

    @Test
    fun `joinChannel sends VoiceIceCandidate with pcType=publisher for publisher ICE`() = runTest {
        repository.joinChannel("ch-1")
        advanceUntilIdle()

        onPublisherIceSlot.captured.invoke("""{"candidate":"c0","sdpMLineIndex":0,"sdpMid":"0"}""")

        verify { webSocket.send(match<ClientEvent.VoiceIceCandidate> { event ->
            event.channelId == "ch-1" && event.pcType == "publisher" && event.candidate.contains("c0")
        }) }
    }

    @Test
    fun `joinChannel sends VoiceIceCandidate with pcType=subscriber for subscriber ICE`() = runTest {
        repository.joinChannel("ch-1")
        advanceUntilIdle()

        onSubscriberIceSlot.captured.invoke("""{"candidate":"c1","sdpMLineIndex":0,"sdpMid":"0"}""")

        verify { webSocket.send(match<ClientEvent.VoiceIceCandidate> { event ->
            event.channelId == "ch-1" && event.pcType == "subscriber" && event.candidate.contains("c1")
        }) }
    }

    @Test
    fun `VoicePublisherAnswer event routes to WebRtcManager handlePublisherAnswer`() = runTest {
        repository.joinChannel("ch-1")
        advanceUntilIdle()

        wsEvents.emit(ServerEvent.VoicePublisherAnswer(channelId = "ch-1", sdp = "v=0\r\no=answer\r\n"))
        advanceUntilIdle()

        coVerify { webRtcManager.handlePublisherAnswer(match { it.startsWith("v=0") }) }
    }

    @Test
    fun `VoiceSubscriberOffer event routes to WebRtcManager handleSubscriberOffer`() = runTest {
        repository.joinChannel("ch-1")
        advanceUntilIdle()

        wsEvents.emit(ServerEvent.VoiceSubscriberOffer(channelId = "ch-1", sdp = "v=0\r\no=offer\r\n"))
        advanceUntilIdle()

        coVerify { webRtcManager.handleSubscriberOffer(match { it.startsWith("v=0") }) }
    }

    @Test
    fun `_isConnected follows WebRtcManager voiceIceConnected StateFlow`() = runTest {
        repository.joinChannel("ch-1")
        advanceUntilIdle()

        repository.isConnected.test {
            // Initial: both PCs not yet connected.
            assertFalse(awaitItem())

            voiceIceConnected.value = true
            advanceUntilIdle()
            assertTrue(awaitItem())

            voiceIceConnected.value = false
            advanceUntilIdle()
            assertFalse(awaitItem())

            cancelAndIgnoreRemainingEvents()
        }
    }
}
```

**Notes on the test design:**
- Uses `MutableSharedFlow` for `wsEvents` (matches how `KaikuWebSocket.events` is consumed — hot flow that replays to new collectors if needed).
- Uses `slot<>` + `capture()` to grab the callbacks that `VoiceRepository` wires into `WebRtcManager` during `joinChannel`. Tests then invoke those callbacks directly to simulate `WebRtcManager` firing them.
- `coVerify` for the suspending `handlePublisherAnswer` / `handleSubscriberOffer` calls; `verify` for the non-suspending `webSocket.send` calls.
- `advanceUntilIdle()` drains the test dispatcher's pending coroutines (the `launch`-ed event-collection job and the ice-connected-collection job).

- [ ] **Step 3: Run the new tests**

```bash
./gradlew :app:testDebugUnitTest --tests 'io.wolftown.kaiku.data.repository.VoiceRepositoryTest' 2>&1 | tail -12
```

Expected: **6 tests completed, 0 failed.**

If a test fails, the most likely causes are:
1. `VoiceRepository`'s constructor signature differs from what the test assumes (e.g., a required parameter added since Phase 1). Read the constructor and align.
2. `KaikuWebSocket.events` is not actually a `Flow<ServerEvent>` type — might be a different flow or a `StateFlow`. Read `KaikuWebSocket` and adjust the `every { webSocket.events } returns wsEvents` line to match the declared type.
3. A `match<…>` pattern rejects the event — double-check the `ClientEvent.VoicePublisherOffer` / `VoiceIceCandidate` field names (`channelId`, `sdp`, `candidate`, `pcType`).

Fix any mismatches without changing the test's intent.

- [ ] **Step 4: Commit**

```bash
git add mobile/android/app/src/test/java/io/wolftown/kaiku/data/repository/VoiceRepositoryTest.kt
git commit -m "test(client): add VoiceRepositoryTest covering dual-PC signaling"
```

---

## Task 7: Add `AGENTS.md` for Android test-infra conventions

**Files:**
- Create: `mobile/android/app/src/test/AGENTS.md`

- [ ] **Step 1: Create the file**

```markdown
# Android Test Infrastructure Conventions

## Mocking final and abstract Android classes

`mockk-agent-jvm` is on the test classpath (see `app/build.gradle.kts`). You can do:

```kotlin
val surface: SurfaceView = mockk(relaxed = true)
val eglBase: EglBase = mockk(relaxed = true)
```

without writing a hand-rolled fake. The agent replaces byte-buddy subclass
proxying with class-load-time bytecode manipulation, working around Kotlin
`final` restrictions and Android-framework final classes.

## Never mock WebRTC native entry points directly

Do **NOT** use `mockkStatic(EglBase::class)` + `every { EglBase.create() } returns …`.
`EglBase.create()` dispatches into `android.opengl.EGL14` JNI, which MockK
cannot intercept. The test will fail at runtime with
`Method eglGetDisplay in android.opengl.EGL14 not mocked`.

Instead, inject the `EglBaseProvider` interface (defined in
`data/voice/EglBaseProvider.kt`):

```kotlin
val eglProvider: EglBaseProvider = mockk(relaxed = true)
val manager = WebRtcManager(context, voiceApi, eglProvider)
// manager.eglBase is lazy; only triggers provider.create() on first read.
```

If the test never reads `manager.eglBase`, the provider's `create()` is never
invoked and no EGL interaction occurs at all.

## Test pyramid

- **JVM unit (`test/`)**: signaling, state, serialization, data transforms. Mock platform deps.
  This is the default for Android unit tests in this repo.
- **Robolectric** (*not currently adopted*): for tests that need a real `Context`,
  lifecycle events, or `AudioManager`. Evaluate per-test before adding; do **not**
  blanket-adopt Robolectric — it divides the suite into fast and slow tiers.
- **Instrumented (`androidTest/`)**: for native WebRTC interactions, real
  `PeerConnectionFactory`, and full UI bindings. Requires emulator/device in CI.

## Currently `@Ignore`d tests

None. If you need to add `@Ignore`, leave a one-line comment explaining why and
link an issue or ticket.
```

- [ ] **Step 2: Commit**

```bash
git add mobile/android/app/src/test/AGENTS.md
git commit -m "docs(client): AGENTS.md for Android test-infra conventions"
```

---

## Task 8: CHANGELOG entry

**Files:**
- Modify: `CHANGELOG.md`

- [ ] **Step 1: Add entry under `## [Unreleased]` → `### Fixed`**

Append (do not replace — add alongside existing Fixed entries):

```
- Android: test infrastructure now injects EglBase via a provider so unit tests can be written without native EGL14 stubs; unblocks CI coverage for voice signaling
```

If the `[Unreleased]` section has multiple `### Fixed` subsections (common in this repo — e.g., one per merged PR), add to the topmost one. Do not introduce a new `## [Unreleased]` section.

- [ ] **Step 2: Commit**

```bash
git add CHANGELOG.md
git commit -m "docs(client): CHANGELOG entry for Android test infra fix"
```

---

## Final Verification (before opening PR)

- [ ] **All success criteria met**

Run the following in sequence and confirm each outcome:

```bash
./gradlew :app:compileDebugKotlin 2>&1 | grep -E "BUILD (SUCCESSFUL|FAILED)"
```
Expected: `BUILD SUCCESSFUL`. Abort if `BUILD FAILED`.

```bash
./gradlew :app:testDebugUnitTest --tests 'io.wolftown.kaiku.data.voice.WebRtcManagerTest' 2>&1 | grep -E "tests completed|BUILD" | tail -3
```
Expected: `15 tests completed, 0 failed, 0 skipped` (9 pure-data + 6 un-ignored). `BUILD SUCCESSFUL`.

```bash
./gradlew :app:testDebugUnitTest --tests 'io.wolftown.kaiku.ui.voice.VoiceViewModelTest' 2>&1 | grep -E "tests completed|BUILD" | tail -3
```
Expected: all `VoiceViewModelTest` cases pass. `BUILD SUCCESSFUL`.

```bash
./gradlew :app:testDebugUnitTest --tests 'io.wolftown.kaiku.data.repository.VoiceRepositoryTest' 2>&1 | grep -E "tests completed|BUILD" | tail -3
```
Expected: `6 tests completed, 0 failed, 0 skipped`. `BUILD SUCCESSFUL`.

```bash
./gradlew :app:testDebugUnitTest 2>&1 | grep -E "tests completed" | tail -3
```
Expected: `≤ 12 failures` total, and each failure is a class name in `{AuthStateTest, AuthFlowTest, MessageFlowTest, QrLoginFlowTest}`. Capture the summary for the PR body.

If the failure count or classes differ from the expected set, **STOP and investigate**: either Phase 1 merged changes that shifted the baseline, or the refactor introduced a regression. Don't file the PR with an unexpected delta.

- [ ] **Grep for residual anti-patterns**

```bash
grep -rn 'mockkStatic(EglBase::class)\|EglBase.create()' \
  mobile/android/app/src/test/
grep -rn '@Ignore' \
  mobile/android/app/src/test/java/io/wolftown/kaiku/data/voice/
```
Expected: zero matches for both. (`EglBase.create()` should exist only in `DefaultEglBaseProvider` under `main/`, never in `test/`.)

- [ ] **Verify commit log**

```bash
git log --oneline main..HEAD
```
Expected: 8 commits, in this order:
1. `chore(client): add mockk-agent-jvm to Android test deps`
2. `feat(client): EglBaseProvider interface for DI-based testability`
3. `feat(client): bind EglBaseProvider in VoiceModule`
4. `refactor(client): WebRtcManager uses injected EglBaseProvider`
5. `test(client): un-ignore voice tests; inject mock EglBaseProvider`
6. `test(client): add VoiceRepositoryTest covering dual-PC signaling`
7. `docs(client): AGENTS.md for Android test-infra conventions`
8. `docs(client): CHANGELOG entry for Android test infra fix`

- [ ] **Push and open PR**

```bash
git push -u origin fix/android-test-infra
gh pr create --base main --head fix/android-test-infra \
  --title "fix(client): Android test infrastructure — EglBaseProvider DI seam" \
  --body "$(cat <<'EOF'
## Summary

- Introduce `EglBaseProvider` DI seam so `WebRtcManager` unit tests can be written without triggering native EGL14 JNI.
- Add `mockk-agent-jvm` test dep to unblock final/abstract class mocking.
- Un-`@Ignore` 6 `WebRtcManagerTest` cases; fix 2 `VoiceViewModelTest` cases.
- Add `VoiceRepositoryTest` with 6 cases covering dual-PC signaling.
- Document test-infra conventions in `mobile/android/app/src/test/AGENTS.md`.

Phase 2 Workstream A. Spec: `docs/superpowers/specs/2026-04-16-android-test-infra-design.md`.

## Test plan

- [x] `./gradlew :app:compileDebugKotlin` — BUILD SUCCESSFUL
- [x] `./gradlew :app:testDebugUnitTest --tests WebRtcManagerTest` — 15/15 pass, no @Ignore
- [x] `./gradlew :app:testDebugUnitTest --tests VoiceViewModelTest` — all pass incl. the two previously failing
- [x] `./gradlew :app:testDebugUnitTest --tests VoiceRepositoryTest` — 6/6 new tests pass
- [x] Full `testDebugUnitTest` result: ≤ 12 failures, all in {AuthStateTest, AuthFlowTest, MessageFlowTest, QrLoginFlowTest} — out of this workstream's scope

## Known remaining failures (out of scope — separate initiative)

The following tests continue to fail on `main` and are **not** addressed by this PR:
- `AuthStateTest` (2 cases)
- `AuthFlowTest` (5 cases)
- `MessageFlowTest` (4 cases)
- `QrLoginFlowTest` (1 case)

Root cause unknown; flagged for a follow-up audit. See Phase 2 decomposition notes.

🤖 Generated with [Claude Code](https://claude.com/claude-code)
EOF
)"
```

The PR number will be returned; record it.

---

## Post-merge cleanup

After the PR is squash-merged:

```bash
cd /home/detair/GIT/detair/kaiku
git worktree remove .claude/worktrees/android-test-infra
git branch -d fix/android-test-infra
git push origin --delete fix/android-test-infra
git fetch --prune
```

---

## Notes for the implementer

- **Subagent-driven execution:** this plan is structured for the `superpowers:subagent-driven-development` skill. Dispatch one implementer subagent per Task (1–8), running the spec-compliance review + code-quality review between each. Each Task's commits are self-contained.
- **Do not batch Tasks.** Task 4's refactor breaks test-file compilation until Task 5 lands, but each Task commits a green production-code build. The test-code compile breakage in Step 5 of Task 4 is expected and enforces the DI seam before tests catch up in Task 5.
- **If Step 3 of Task 1 shows more than 3 new regressions from `mockk-agent-jvm`:** stop, capture the failure list, and escalate per the spec's Risk #2 threshold. Do not proceed.
- **If the Hilt codegen emits errors** in Task 3 or Task 4 (e.g., cycle warnings, missing `@Provides` for a constructor param), check that `DefaultEglBaseProvider` is annotated `@Singleton` and its constructor is `@Inject`. Hilt auto-resolves interfaces via `@Binds` when a single implementation exists.
- **If a test in Task 6's `VoiceRepositoryTest` is flaky:** the most likely culprit is `advanceUntilIdle()` placement. The StandardTestDispatcher queues the `iceConnectedJob.collect` and the event-collection job at `joinChannel()`; both must drain before assertions. If a test still fails intermittently, add an extra `advanceUntilIdle()` before the `wsEvents.emit` or before the `verify` call.
