# Android Test Infrastructure — Workstream A

**Date:** 2026-04-16
**Status:** Draft
**Goal:** Unblock Android JVM unit tests that instantiate `WebRtcManager` or mock `org.webrtc.EglBase`. Deliver a DI seam that removes native-EGL dependencies from unit-test construction, un-ignore the 6 currently-`@Ignore`d voice tests, fix the 2 `VoiceViewModelTest` failures, add a new `VoiceRepositoryTest` covering dual-PC signaling, and document the test-infra conventions so future authors don't repeat the trap.

## Context

Phase 1's dual-PC voice refactor (PR #533) un-masked a latent test-infrastructure problem: `WebRtcManager` eagerly creates an `EglBase` in a primary-constructor field initializer, and `EglBase.create()` dispatches through `android.opengl.EGL14` JNI that MockK cannot intercept via `mockkStatic`. Consequences:

- 6 tests in `WebRtcManagerTest.kt` (added or updated in Phase 1 PR E) had to be `@Ignore`d because their `mockkStatic(EglBase::class)` approach fails with `Method eglGetDisplay in android.opengl.EGL14 not mocked`.
- 2 tests in `VoiceViewModelTest.kt` (`screenShares state reflects repository`, `isConnected state reflects repository`) fail with `MockKException: Can't instantiate proxy for class org.webrtc.EglBase` because instance-level mocks on the abstract WebRTC class are unsupported by the default MockK byte-buddy backend.
- The test infrastructure gap is undocumented; the next test author who tries to mock `EglBase` (or any other final/abstract Android class) repeats the same trap.

This workstream is the first of Phase 2's seven workstreams. It is an explicit force-multiplier: every downstream workstream validates against Android CI, so making that CI signal trustworthy is the foundation for everything that follows.

**Scope decision:** This spec addresses **only** the 8 voice-test failures caused by the `EglBase` DI gap. The additional ~12 currently-failing Android unit tests (`AuthStateTest` × 2, `AuthFlowTest` × 5, `MessageFlowTest` × 4, `QrLoginFlowTest` × 1) are root-cause-unknown pre-existing failures out of scope for this workstream. They will remain red after A lands and must be addressed by a separate initiative.

## Approach

### Architecture — one-sentence change

Move `EglBase` construction from `WebRtcManager`'s field initializer to a lazy property that reads from an injected `EglBaseProvider`, enabling tests to swap the provider without invoking native EGL14.

### Components

**1. New interface and impl — `mobile/android/app/src/main/java/io/wolftown/kaiku/data/voice/EglBaseProvider.kt`:**

```kotlin
package io.wolftown.kaiku.data.voice

import org.webrtc.EglBase
import javax.inject.Inject
import javax.inject.Singleton

/**
 * Indirection over [EglBase.create] so tests can substitute the EGL stack.
 * Production code MUST inject this rather than calling [EglBase.create] directly.
 */
interface EglBaseProvider {
    fun create(): EglBase
}

@Singleton
class DefaultEglBaseProvider @Inject constructor() : EglBaseProvider {
    override fun create(): EglBase = EglBase.create()
}
```

**2. Hilt binding** in whichever `@Module @InstallIn(SingletonComponent::class)` binds existing voice deps. If the project's Hilt setup auto-resolves interfaces via a single `@Inject`-annotated concrete implementation, no explicit `@Binds` is required and this step is skipped. Otherwise:

```kotlin
@Binds @Singleton
abstract fun bindEglBaseProvider(impl: DefaultEglBaseProvider): EglBaseProvider
```

Exact module file to be identified during writing-plans.

**3. `WebRtcManager` constructor change** (`mobile/android/app/src/main/java/io/wolftown/kaiku/data/voice/WebRtcManager.kt`):

Add `eglBaseProvider: EglBaseProvider` as a constructor parameter. Replace the eager field initializer at line 110:

```kotlin
// Before:
val eglBase: EglBase = EglBase.create()

// After:
private val _eglBase: Lazy<EglBase> = lazy { eglBaseProvider.create() }

/**
 * The EGL stack used by SurfaceViewRenderer for video rendering.
 * Lazy so tests that never touch video rendering never trigger EGL init,
 * and so production startup defers the native cost until first use.
 */
val eglBase: EglBase get() = _eglBase.value
```

The external API is preserved: `webRtcManager.eglBase` returns `EglBase` as before. Consumers like `VoiceViewModel` that read the property still work unchanged.

**4. `dispose()` change** (line ~368) — guard release on lazy-initialization state so `dispose()` doesn't accidentally trigger EglBase creation purely to release it:

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
        } catch (e: Exception) {
            logger.log(Level.WARNING, "Unexpected error releasing EglBase", e)
        }
    }
}
```

The explicit `Lazy<T>` wrapper (rather than the `by lazy` delegate syntax) is used specifically so `isInitialized()` is available.

### Data flow

**Production:** Hilt instantiates `DefaultEglBaseProvider` → injected into `WebRtcManager` at application start → first read of `webRtcManager.eglBase` triggers `_eglBase` lazy → calls `EglBase.create()` → returns a real `EglBase14` instance. Single instance per `WebRtcManager` lifetime; released on `dispose()` if ever initialized.

**Test (JVM unit):** Test constructs `WebRtcManager(mockContext, mockVoiceApi, fakeProvider)` where `fakeProvider = mockk<EglBaseProvider>(relaxed = true)` — a Kotlin interface, trivially proxyable by MockK with no native calls. If the test never reads `webRtcManager.eglBase`, the lazy never fires and even the mock-return cost is avoided.

For tests that do read `.eglBase` (e.g., `VoiceViewModelTest` passing it to `SurfaceViewRenderer`): the `mockk-agent-jvm` test dependency (see below) enables `mockk<EglBase>(relaxed = true)` directly, so the provider returns a usable mock instance.

### Test dependency: `mockk-agent-jvm`

Add to `mobile/android/app/build.gradle.kts`:

```kotlin
testImplementation("io.mockk:mockk:1.13.14")
testImplementation("io.mockk:mockk-agent-jvm:1.13.14")  // NEW
```

The agent replaces byte-buddy's subclass-proxy approach with class-load-time bytecode manipulation, which works around Kotlin `final` and Android final-class restrictions. Same MockK version as the existing dependency. Java-agent startup adds <1s to the test task; confirmed compatible with JDK 17 (Kaiku CI's current Java version).

**Broader benefit:** any future test that needs to mock a final/abstract Android class (e.g., `SurfaceView`, `AudioManager`, `MediaSession`) works without hand-rolled fakes.

## Test deliverables

### Group 1 — six currently-`@Ignore`d `WebRtcManagerTest` cases

All six share the same failure pattern: they instantiate a real `WebRtcManager` and rely on `mockkStatic(EglBase::class)` to intercept the field initializer. With the DI change:

- The `mockkStatic(EglBase::class)` + `unmockkStatic(EglBase::class)` + `@After tearDownEglBaseMock` scaffolding is **removed entirely** from the test class.
- Each test instantiates `WebRtcManager(mockContext, mockVoiceApi, mockEglBaseProvider)` where `mockEglBaseProvider = mockk<EglBaseProvider>(relaxed = true)`.
- None of the six tests read `.eglBase`, so `_eglBase` never fires. Zero EGL interaction.
- All six `@Ignore` annotations are removed.

Tests (behavior unchanged; only instantiation and cleanup scaffolding updated):

1. `addIceCandidate buffers up to MAX_PENDING_CANDIDATES then drops` (subscriber)
2. `addIceCandidate buffers publisher candidates up to cap then drops`
3. `addIceCandidate routes by pcType to publisher vs subscriber buffer`
4. `addIceCandidate with unknown pcType is no-op`
5. `voiceIceConnected starts false before any PC connects`
6. `voiceIceConnected emits true only when both PCs reach Connected`

### Group 2 — two `VoiceViewModelTest` cases

Failure: `val mockEglBase = mockk<EglBase>(relaxed = true)` throws `MockKException: Can't instantiate proxy for class org.webrtc.EglBase`.

With `mockk-agent-jvm` on the test classpath, that single line works unchanged — no test-code modifications needed beyond the `build.gradle.kts` addition.

Tests pass without further change:
- `screenShares state reflects repository`
- `isConnected state reflects repository`

### Group 3 — new `VoiceRepositoryTest`

**Location:** `mobile/android/app/src/test/java/io/wolftown/kaiku/data/repository/VoiceRepositoryTest.kt`.

**Scope:** dual-PC signaling glue between `VoiceRepository` and `WebRtcManager`. Pure Kotlin — `VoiceRepository`'s deps (`WebRtcManager`, `KaikuWebSocket`, `AudioRouteManager`, `VoiceServiceEvents`, `Context`) are all mockable. Uses `StandardTestDispatcher` + `runTest`, matching `VoiceViewModelTest`'s pattern.

**Test cases (6):**

| # | Test | Verifies |
|---|------|----------|
| 1 | `joinChannel sends VoicePublisherOffer when createPublisherOffer fires its callback` | `onPublisherOffer` → `VoicePublisherOffer` WS send |
| 2 | `joinChannel sends VoiceIceCandidate with pcType=publisher for onPublisherIceCandidate` | Publisher ICE path carries `pcType="publisher"` |
| 3 | `joinChannel sends VoiceIceCandidate with pcType=subscriber for onSubscriberIceCandidate` | Subscriber ICE path carries `pcType="subscriber"` |
| 4 | `handleServerEvent dispatches VoicePublisherAnswer to webRtcManager.handlePublisherAnswer` | Receive path for publisher answer |
| 5 | `handleServerEvent dispatches VoiceSubscriberOffer to webRtcManager.handleSubscriberOffer` | Receive path for subscriber offer |
| 6 | `_isConnected follows webRtcManager.voiceIceConnected StateFlow` | Connection state gated on both PCs |

Each test uses `mockk<WebRtcManager>()` / `mockk<KaikuWebSocket>()` etc., stubs `webRtcManager.voiceIceConnected` as a controllable `MutableStateFlow<Boolean>`, and asserts WebSocket sends via `verify { webSocket.send(match<ClientEvent.XXX> { … }) }` and state transitions via `turbine.test`.

**Dependency verification during writing-plans:** confirm `app.cash.turbine:turbine` is already a `testImplementation` dep (`VoiceViewModelTest` uses it, so it should be). If absent, add as a fourth `testImplementation` line in `build.gradle.kts` alongside `mockk-agent-jvm`.

**Out of scope (deferred deliberately):**
- Error propagation (`onError` → `_error.value`). Belongs in workstream B.2 (error UX).
- Screen share / participant state. Covered by integration tests at a higher layer, not repository unit tests.
- Concurrent `iceConnectedJob` cancellation races. Not a thread-safety fix; the spec explicitly carries thread-safety deferrals to Phase 2's next workstream.

Approximate file size: ~220 lines. Larger than `WebRtcManagerTest`'s pure-data tests because `VoiceRepository`'s setup requires mocking more dependencies.

## CI posture

Per Add-on 1a:

**State before A lands** (post-PR E merge):
- `./gradlew assembleDebug --no-daemon`: green.
- `./gradlew testDebugUnitTest --no-daemon`: red on 6 `WebRtcManagerTest` `@Ignore`d tests (reported as skipped, not fail) + 2 `VoiceViewModelTest` failures + ~12 pre-existing `Auth/Msg/QR` failures.

**State after A lands:**
- `assembleDebug`: green (unchanged).
- `testDebugUnitTest`: red on exactly the ~12 pre-existing `Auth/Msg/QR` failures. All 8 voice failures resolved. 6 new `VoiceRepositoryTest` cases passing.

No `continue-on-error: true` added. No test-filter flag in CI. The Android job stays honest-red on the 12 pre-existing failures, providing a visible queue for a future initiative. Whether Android CI is a required check for branch protection is a repo-config concern outside this spec.

## Test-infra documentation

Per Add-on 2a, add `mobile/android/app/src/test/AGENTS.md` documenting:

- How to mock final/abstract Android classes (use `mockk-agent-jvm`, already on classpath).
- Why `EglBaseProvider` exists and when to use it.
- Anti-pattern: never use `mockkStatic(EglBase::class)` again.
- Test pyramid conventions: JVM unit for signaling/state, Robolectric (not yet adopted), androidTest for native WebRTC.
- Current `@Ignore` registry (target: empty).

~40 lines of markdown.

## File map

**New (3):**
- `mobile/android/app/src/main/java/io/wolftown/kaiku/data/voice/EglBaseProvider.kt`
- `mobile/android/app/src/test/java/io/wolftown/kaiku/data/repository/VoiceRepositoryTest.kt`
- `mobile/android/app/src/test/AGENTS.md`

**Modified (5):**
- `mobile/android/app/build.gradle.kts` — add `mockk-agent-jvm`
- `mobile/android/app/src/main/java/io/wolftown/kaiku/data/voice/WebRtcManager.kt` — constructor param + `_eglBase: Lazy<EglBase>` + guarded `dispose()`
- `mobile/android/app/src/main/java/…/<HiltModule>.kt` — `@Binds` `EglBaseProvider` (file TBD; possibly unnecessary)
- `mobile/android/app/src/test/java/io/wolftown/kaiku/data/voice/WebRtcManagerTest.kt` — drop 6 `@Ignore`, drop `mockkStatic` scaffolding, inject mock provider
- `CHANGELOG.md` — `### Fixed` entry

Total: ~300-400 LoC across 8 files.

## Risks

| # | Risk | Probability | Mitigation |
|---|------|-------------|-----------|
| 1 | Hilt module location unclear; `@Binds` may or may not be needed | Medium | Writing-plans skill identifies the module location. If Hilt auto-resolves via `@Inject` constructor, skip `@Binds`. |
| 2 | `mockk-agent-jvm` surfaces latent failures in previously-passing tests | Low | Run full `testDebugUnitTest` after adding; triage any new reds as in-scope fixes within this PR. **Escalation threshold:** if more than 3 new reds appear, pause and re-scope with the user before landing — at that volume the fix likely deserves its own initiative rather than being folded into A. |
| 3 | `VoiceRepositoryTest` requires test-only `internal` accessors on `VoiceRepository` | Medium | Phase 1 pattern already established (`publisherPendingCandidatesSize()` etc.). Apply same `ForTest` suffix convention as needed. |
| 4 | `by lazy`'s `isInitialized()` unavailable — would break `dispose()` guard | N/A — addressed in design | Use explicit `private val _eglBase: Lazy<EglBase>` + `get()` accessor. |
| 5 | `stream-webrtc-android` future version changes `EglBase` API | Low | `EglBase` is consumed as an interface, never implemented by app code. Version bumps affect provider implementation, not consumers. |

## Success criteria

1. All 6 `@Ignore` annotations removed from `WebRtcManagerTest.kt`. Zero `mockkStatic(EglBase::class)` references anywhere in `mobile/android/app/src/test/`.
2. `./gradlew :app:testDebugUnitTest --tests 'io.wolftown.kaiku.data.voice.WebRtcManagerTest'` passes. All 15 tests (9 pre-existing pure-data + 6 un-ignored WebRtcManager-instantiation).
3. `./gradlew :app:testDebugUnitTest --tests 'io.wolftown.kaiku.ui.voice.VoiceViewModelTest'` passes. All tests including `screenShares state reflects repository` and `isConnected state reflects repository`.
4. `./gradlew :app:testDebugUnitTest --tests 'io.wolftown.kaiku.data.repository.VoiceRepositoryTest'` passes. 6 new tests.
5. Overall `./gradlew :app:testDebugUnitTest` result: ≤ 12 failures, and every failure is in `{AuthStateTest, AuthFlowTest, MessageFlowTest, QrLoginFlowTest}`. No new regressions, no accidental improvements claimed for those 12 (out of scope). Phrased as "≤ and constrained to set" rather than "exactly 12" so the criterion is robust if Phase 1 PR merges shift the baseline before this workstream starts.
6. `./gradlew :app:assembleDebug` passes.
7. `mobile/android/app/src/test/AGENTS.md` exists with the conventions documented.

## Commits

Single PR, 6–7 commits:

1. `chore(client): add mockk-agent-jvm to Android test deps`
2. `feat(client): EglBaseProvider interface for DI-based testability`
3. `refactor(client): WebRtcManager uses injected EglBaseProvider with lazy init`
4. `test(client): un-ignore voice tests now that EglBase is injectable`
5. `test(client): add VoiceRepositoryTest covering dual-PC signaling`
6. `docs(client): add AGENTS.md for Android test-infra conventions`
7. `docs(client): CHANGELOG entry for Android test infrastructure fix`

PR body enumerates the 12 known-remaining `Auth/Msg/QR` failures, confirms they are out of A's scope, and flags them for a separate initiative.

## Out of scope

- The 12 `AuthStateTest` / `AuthFlowTest` / `MessageFlowTest` / `QrLoginFlowTest` failures. Root cause unknown. Fixing them requires its own audit + workstream.
- Instrumented (`androidTest/`) coverage for `WebRtcManager.createPublisherOffer` / `createSubscriberPeerConnection` / the native `PeerConnectionFactory` path. Belongs in a future workstream that introduces emulator-based CI.
- Robolectric adoption. Evaluated during brainstorming and rejected: it would divide the test suite into JVM-fast and Robolectric-slow tracks without clear benefit for the specific failures this workstream targets.
- Tauri-side test gaps (`cargo test` blocked by libspa on Linux dev hosts). Addressed in workstream G.

## CHANGELOG entry

Under `### Fixed`:

- Android: test infrastructure now injects EglBase via a provider so unit tests can be written without native EGL14 stubs; unblocks CI coverage for voice signaling
