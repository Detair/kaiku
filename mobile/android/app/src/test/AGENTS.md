# Android Test Infrastructure Conventions

## Mocking final and abstract Android classes

`mockk-agent-jvm` is on the test classpath (see `app/build.gradle.kts`). You can do:

```kotlin
val surface: SurfaceView = mockk(relaxed = true)
val audioManager: AudioManager = mockk(relaxed = true)
```

without writing a hand-rolled fake. The agent replaces byte-buddy subclass
proxying with class-load-time bytecode manipulation, working around Kotlin
`final` restrictions and Android-framework final classes.

## Never mock `EglBase` directly

Do **NOT** use `mockkStatic(EglBase::class) + every { EglBase.create() } returns …`
or `mockk<EglBase>(relaxed = true)`.

- `EglBase.create()` dispatches into `android.opengl.EGL14` JNI, which MockK
  cannot intercept (`Method eglGetDisplay in android.opengl.EGL14 not mocked`).
- `mockk<EglBase>()` fails with `NoClassDefFoundError: javax/microedition/khronos/egl/EGLContext` —
  an Android stub class absent on the JVM unit-test classpath that byte-buddy
  needs to build the proxy. The agent does not fix this; it's a classpath
  issue, not a finality issue.

Instead, inject the `EglBaseProvider` interface (defined in
`data/voice/EglBaseProvider.kt`):

```kotlin
val eglProvider: EglBaseProvider = mockk(relaxed = true)
val manager = WebRtcManager(context, voiceApi, eglProvider)
// manager.eglBase is lazy; only triggers provider.create() on first read.
```

If the test never reads `manager.eglBase`, the provider's `create()` is never
invoked and no EGL interaction occurs at all.

For the same reason, any property that reads `eglBase` (e.g.,
`VoiceViewModel.eglContext`) should be `by lazy` so JVM tests don't trigger
it during construction.

## Stubbing Android `Intent`-based companion objects

Production code that starts Android `Service`s or `Activity`s builds `Intent`
instances with `putExtra()`. On the JVM unit-test target, calls into
`android.content.Intent` throw:

```
RuntimeException: Method putExtra in android.content.Intent not mocked
```

Example: `VoiceRepository.joinChannel()` calls `VoiceCallService.start(...)`.
To unit-test that flow, stub the companion:

```kotlin
import io.mockk.mockkObject
import io.mockk.unmockkObject

@Before fun setUp() {
    mockkObject(VoiceCallService.Companion)
    every { VoiceCallService.start(any(), any(), any()) } answers {}
    every { VoiceCallService.stop(any()) } answers {}
    // ...
}

@After fun tearDown() {
    unmockkObject(VoiceCallService.Companion)
    // ...
}
```

Same pattern applies to any `companion object` method that constructs
`Intent`s or touches other `android.*` classes directly.

## Test pyramid

- **JVM unit (`test/`)**: signaling, state, serialization, data transforms. Mock platform deps.
  This is the default for Android unit tests in this repo.
- **Robolectric** (*not currently adopted*): for tests that need a real `Context`,
  lifecycle events, or `AudioManager`. Evaluate per-test before adding; do **not**
  blanket-adopt Robolectric — it divides the suite into fast and slow tiers.
- **Instrumented (`androidTest/`)**: for native WebRTC interactions, real
  `PeerConnectionFactory`, and full UI bindings. Requires emulator/device in CI.

## `CoroutineScope(Dispatchers.IO)` + `StandardTestDispatcher` caveat

Production code that wraps a flow in `stateIn(CoroutineScope(Dispatchers.IO), Eagerly, …)`
or launches collectors on its own `Dispatchers.IO` scope cannot be driven by
`TestCoroutineScheduler`. `runCurrent()` / `advanceUntilIdle()` only drain the
test dispatcher, so synchronous assertions on `.value` right after mutating the
source flow may read stale state.

Workarounds (in order of preference):

1. **Use `turbine.test { }` with `awaitItem()`** — turbine's default timeout
   (1s) covers real IO emission, so downstream state changes are observable
   even though the producer runs on `Dispatchers.IO`.
2. **Inject the scope** — make the production class accept a `CoroutineScope`
   constructor parameter so tests can pass `TestScope(testDispatcher)`. Best
   long-term fix; requires a Hilt `@Provides` for the default scope.
3. **`@Ignore` with explicit reason** — acceptable when the test can only be
   driven synchronously and option 2 is out of scope. Leave a one-line reason
   referencing this constraint.

## Currently `@Ignore`d tests

- `WebRtcManagerTest.voiceIceConnected emits true only when both PCs reach Connected`
  — blocked by option 2 above: `WebRtcManager.voiceIceConnected` uses
  `stateIn(CoroutineScope(Dispatchers.IO), …)`. Un-ignore when the scope is
  injectable.

If you add a new `@Ignore`, append it here with a one-line reason and link
an issue or ticket.
