# Android Unit Test Failure Triage — 2026-04-16

Phase 2.5 / Block 3 time-boxed triage of 13 pre-existing unit test failures that
Workstream A (#534 — the voice coroutine test hardening PR) carved out of scope.

- **Baseline commit:** `58c97e0` (`main` at spike start)
- **Run command:** `./gradlew :app:testDebugUnitTest --rerun-tasks` in `mobile/android/`
- **Result:** `174 tests, 1 skipped, 13 failures, 0 errors`
- **Skipped test (unchanged, not in scope):**
  `WebRtcManagerTest.voiceIceConnected emits true only when both PCs reach Connected`
  — already `@Ignore`d; Block 4 un-ignores it via `@VoiceCoroutineScope` injection.
- **Budget consumed:** ~55 min investigation, 0 min fixes (no S-rated clusters found).

## Executive Summary

All 13 failures share a single root cause: **production code creates its own
`CoroutineScope(Dispatchers.Default)` or `CoroutineScope(Dispatchers.IO)` that
`TestCoroutineScheduler` cannot drive**, so tests that assert on `.value`
immediately after a state change read stale state. This is the same class of
bug that `docs/developer-guide/testing` already documents in
`app/src/test/AGENTS.md` §"`CoroutineScope(Dispatchers.IO)` + `StandardTestDispatcher` caveat".

Two clusters:

| Cluster | Production class | # failures | Size | Recommendation |
|---|---|---|---|---|
| A | `AuthState` (uses `CoroutineScope(SupervisorJob() + Dispatchers.Default)` for `stateIn`) | 8 | **M** | Follow-up workstream: inject `@AuthCoroutineScope`, mirror Block 4 pattern |
| B | `ChatRepository` (uses `CoroutineScope(SupervisorJob() + Dispatchers.IO)` for event collector) | 5 | **M** | Follow-up workstream: inject `@ChatCoroutineScope`, same pattern |

No independent real bugs found. No production regressions uncovered.

## Baseline Failure Table

| # | Class | Test | Failure | Cluster |
|---|---|---|---|---|
| 1 | `data.local.AuthStateTest` | `initialize with valid tokens sets logged in state` | `AssertionError` (assertTrue on `isLoggedIn.value`) | A |
| 2 | `data.local.AuthStateTest` | `initialize with expired token but valid refresh token stays logged in` | `AssertionError` (assertTrue on `isLoggedIn.value`) | A |
| 3 | `integration.AuthFlowTest` | `initialize restores auth state from valid stored tokens` | `AssertionError` (assertTrue on `isLoggedIn.value`) | A |
| 4 | `integration.AuthFlowTest` | `logout clears tokens and sets auth state to logged out` | `AssertionError` (assertTrue on `isLoggedIn.value`) | A |
| 5 | `integration.AuthFlowTest` | `login stores tokens and sets auth state to logged in` | `AssertionError` (assertTrue on `isLoggedIn.value`) | A |
| 6 | `integration.AuthFlowTest` | `OIDC login stores tokens and sets auth state` | `AssertionError` (assertTrue on `isLoggedIn.value`) | A |
| 7 | `integration.AuthFlowTest` | `initialize stays logged in with expired token but valid refresh token` | `AssertionError` (assertTrue on `isLoggedIn.value`) | A |
| 8 | `integration.QrLoginFlowTest` | `QR redeem stores server URL, tokens, and sets auth state to logged in` | `AssertionError` (assertTrue on `isLoggedIn.value`) | A |
| 9 | `integration.MessageFlowTest` | `new WebSocket message appears in message list` | `AssertionError: expected:<3> but was:<2>` | B |
| 10 | `integration.MessageFlowTest` | `edit event updates message content in list` | `ComparisonFailure: expected:<Updated content> but was:<First message>` | B |
| 11 | `integration.MessageFlowTest` | `delete event removes message from list` | `AssertionError: expected:<1> but was:<2>` | B |
| 12 | `integration.MessageFlowTest` | `typing start adds user to typing set` | `AssertionError` (assertTrue on `typing.contains("user-2")`) | B |
| 13 | `integration.MessageFlowTest` | `typing stop removes user from typing set` | `AssertionError` (assertTrue on `!typing.contains("user-2")`) | B |

Per-failure metadata: `/tmp/android-failures.tsv`. Full stacks: `/tmp/android-failures-full.txt`.

---

## Cluster A — `AuthState` derived StateFlows use unreachable dispatcher

### Failures (8)

All assertion failures read `authState.isLoggedIn.value` or
`authState.currentUserId.value` immediately after a synchronous mutation
(`setLoggedIn`, `setLoggedOut`, `initialize`, or via `AuthRepository.login/logout/...`)
inside a `runTest { }` block.

Affected: rows 1–8 above.

### Root-cause hypothesis

`AuthState` backs `isLoggedIn` and `currentUserId` with
`stateIn(scope, SharingStarted.Eagerly, …)` where `scope =
CoroutineScope(SupervisorJob() + Dispatchers.Default)`. When the test calls
`setLoggedIn("user-123")`, `_session.value` flips synchronously but the derived
StateFlows only re-emit after the `map` transformation is processed on
`Dispatchers.Default` — which `TestCoroutineScheduler` does **not** drive.
`advanceUntilIdle()` drains the test dispatcher, not `Dispatchers.Default`, so
`.value` reads the pre-update cached value and `assertTrue` fails.

Tests that use Turbine (`authState.isLoggedIn.test { … }`) pass because Turbine's
1-second default timeout covers real dispatcher emission. Tests that read
`.value` directly fail deterministically.

### Evidence

- Production (`mobile/android/app/src/main/java/io/wolftown/kaiku/data/local/AuthState.kt:25`):
  ```kotlin
  private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Default)

  val isLoggedIn: StateFlow<Boolean> = _session.map { it is AuthSession.LoggedIn }
      .stateIn(scope, SharingStarted.Eagerly, false)
  ```
- Test (`AuthStateTest.kt:87-98`) reads `.value` right after synchronous `initialize`:
  ```kotlin
  authState.initialize(tokenStorage)
  assertTrue(authState.isLoggedIn.value)        // fails: still false
  ```
- Passing counterparts in the same file (e.g. `setLoggedIn updates isLoggedIn
  to true and sets currentUserId`) use `authState.isLoggedIn.test { … }` Turbine
  collector and succeed.
- Historical: #527 (commit `863d33b`) introduced the named
  `CoroutineScope(SupervisorJob() + Dispatchers.Default)` — the previous anonymous
  scope also used `Dispatchers.Default`, so these tests were already broken
  prior to the refactor (verified by `git log --follow`).
- Matches the documented caveat in
  `mobile/android/app/src/test/AGENTS.md` §"`CoroutineScope(Dispatchers.IO)` +
  `StandardTestDispatcher` caveat".

### Size: **M (<4h)**

Fixing requires mirroring the exact pattern Block 4 (#534 follow-up) is
implementing for `WebRtcManager`:

1. Introduce `@AuthCoroutineScope` qualifier.
2. Add a Hilt `@Provides` in `AppModule` that defaults to
   `CoroutineScope(SupervisorJob() + Dispatchers.Default)`.
3. Make `AuthState` accept the scope via constructor injection.
4. Add a test `@Provides` override with `TestScope(testDispatcher)` / a small
   helper (`MainDispatcherRule`) and switch the 8 failing tests to construct
   `AuthState(testScope)`.
5. Re-run: 8 tests green.

Budget estimate: 1–2h including test suite re-run and CHANGELOG entry. Out of
the 30-minute optional fix budget.

### Recommended action

**Defer to follow-up workstream** — reuse the same brainstorm + plan pattern
Block 4 used for `@VoiceCoroutineScope`. Candidate plan filename:
`docs/superpowers/plans/2026-04-17-auth-coroutine-scope-injection.md`.

No production bug: `isLoggedIn`/`currentUserId` work correctly for real
subscribers (which observe the flow, not read `.value`). This is purely a
testability gap.

---

## Cluster B — `ChatRepository` event collector uses unreachable dispatcher

### Failures (5)

All assertions check `chatRepository.getMessages(…).value` or
`chatRepository.getTypingUsers(…).value` after `eventsFlow.emit(…)` +
`advanceUntilIdle()`. The emitted event never reaches the handler before the
assertion runs.

Affected: rows 9–13 above.

### Root-cause hypothesis

`ChatRepository` owns a `CoroutineScope(SupervisorJob() + Dispatchers.IO)` and
`launch`es a `webSocket.events.collect { handleServerEvent(event) }` inside
`init { startCollectingEvents() }`. Test-side `eventsFlow.emit(...)` publishes
onto a real `SharedFlow`, but the collector runs on `Dispatchers.IO`
(real thread pool) — not on the `TestCoroutineScheduler`.
`advanceUntilIdle()` returns before the IO thread has had a chance to run the
handler, so `getMessages(…).value` and `getTypingUsers(…).value` still read
the previous state.

### Evidence

- Production (`mobile/android/app/src/main/java/io/wolftown/kaiku/data/repository/ChatRepository.kt:46,152-159`):
  ```kotlin
  private val scope = CoroutineScope(SupervisorJob() + Dispatchers.IO)
  // …
  private fun startCollectingEvents() {
      eventCollectionJob?.cancel()
      eventCollectionJob = scope.launch {
          webSocket.events.collect { event -> handleServerEvent(event) }
      }
  }
  ```
- Test (`MessageFlowTest.kt:112-117`):
  ```kotlin
  eventsFlow.emit(ServerEvent.MessageNew(channelId = "ch-1", message = messageJson))
  advanceUntilIdle()
  assertEquals(3, chatRepository.getMessages("ch-1").value.size)   // actually 2
  ```
- Passing counterparts in the same file (e.g. `subscribe sends WebSocket event
  and loads messages` at line 77, `sending message calls API and adds to list
  optimistically` at line 144) don't depend on the collector — they mutate
  state synchronously via `loadMessages`/`sendMessage` calls.
- `duplicate WebSocket message is not added twice` (line 123) also passes, but
  only trivially: the assertion is `assertEquals(2, …)` which is the initial
  state regardless of whether the (correctly-filtered) duplicate event was
  processed. Not a useful green.
- Same root-cause pattern as `WebRtcManagerTest.voiceIceConnected` (the already-
  `@Ignore`d test Block 4 targets).

### Size: **M (<4h)**

Fix is the same shape as Cluster A and Block 4:

1. Introduce `@ChatCoroutineScope` qualifier.
2. Hilt provider defaulting to `CoroutineScope(SupervisorJob() + Dispatchers.IO)`.
3. Constructor-inject into `ChatRepository`.
4. Update 5 tests to pass `TestScope(testDispatcher)` (sharing the same
   `MainDispatcherRule` helper Block 4 introduces).
5. Re-run: 5 tests green. Consider tightening the `duplicate WebSocket message`
   test at the same time so it actually exercises the dedupe path.

Budget estimate: 1–2h. Out of scope for this triage.

### Recommended action

**Defer to follow-up workstream**, bundled with Cluster A. A single
"inject named `CoroutineScope` into `AuthState` and `ChatRepository`" PR can
resolve all 13 failures, matching the test-dispatcher hardening pattern
Block 4 establishes for `WebRtcManager`.

No production bug — the IO-scope collector works correctly in app runtime.

---

## Recommended Next Steps

1. **Follow-up workstream (est. 2–4h total):** "Android coroutine scope
   injection for test-driveable components". Scope: `AuthState` (cluster A) +
   `ChatRepository` (cluster B). Pattern already validated by Block 4 on
   `WebRtcManager`. Resolves all 13 failing tests + 1 `@Ignore`d test in one
   sweep. Propose plan file:
   `docs/superpowers/plans/2026-04-17-android-scope-injection.md`.

2. **Documentation touch-up:** once the scope injection pattern ships for all
   three classes, promote the `app/src/test/AGENTS.md` §"CoroutineScope(Dispatchers.IO)
   caveat" workaround #2 from "best long-term fix" to "standard pattern" and
   remove the `@Ignore` entry.

3. **Keep the green baseline:** no test-only workarounds (e.g. replacing
   `.value` reads with Turbine probes in the existing tests) — those would
   obscure the real design gap and complicate the scope-injection migration.

## Out of Scope

- **No production fixes** — triage did not reveal any runtime bugs. `AuthState`
  and `ChatRepository` behave correctly for real `collect { }` subscribers; the
  failures are test-harness artifacts only.
- **No auth/messaging/QR-login regressions** — the assertion failures are about
  test timing, not logic. Login/logout/OIDC/QR/edit/delete/typing production
  code paths all execute successfully during the failing tests (verified by
  `verify { … }` mockk blocks that pass *before* the `.value` assertion fails).
- **No changes to the `@Ignore`d `WebRtcManagerTest.voiceIceConnected` test** —
  Block 4 owns that.
- **No test-only band-aid fixes** (rewriting assertions with Turbine) — would
  obscure the underlying design gap and produce churn when the proper
  scope-injection PR lands.
