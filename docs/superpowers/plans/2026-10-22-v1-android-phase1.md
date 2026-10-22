# Android Phase 1 ("Make the client work") Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the Android app usable every day for the family: history loads, the session survives past 15 minutes and doze, live updates come back after reconnects and catch up on what was missed, voice join no longer crashes, and errors, titles and images show correctly. All of this has to be i18n-ready.

**Architecture:** The plan has ten PR-sized tasks in `mobile/android`, plus one test-only server file that generates golden JSON fixtures from the real server types. The WebSocket class becomes a pure transport. A new singleton, `RealtimeConnectionManager`, takes over connect, refresh, backoff, network change and the pong watchdog. A `ResyncCoordinator` re-subscribes channels and catches up after every `Ready`. Token refresh runs through the normal Ktor pipeline behind a `TokenRefresher` interface, which Phase 2's wake-fetch worker will reuse.

**Tech Stack:** Kotlin 2.1, Jetpack Compose (BOM 2025.01.01), Hilt 2.54, Ktor client 3.1 (OkHttp engine), OkHttp 4.12, kotlinx-serialization 1.8, Coil 3.1, stream-webrtc-android 1.3. Tests use JUnit4, MockK, Turbine, kotlinx-coroutines-test, ktor-client-mock and MockWebServer. The server fixture test uses Rust with serde_json.

**Spec:** `docs/superpowers/specs/2026-10-20-v1-family-release-goal-design.md`
(scope items 2, 3, 5 and 6). Review: `docs/reviews/2026-10-22-v1-family-release-readiness.md`
(A-01..A-04, plus the A-05, A-06, A-08, A-09, A-12 and A-18 subsets). Contracts:
`docs/superpowers/plans/2026-10-22-v1-family-contracts.md` (C1–C5).

## Global Constraints

- Platform: `minSdk = 26`, `targetSdk = 35`, `compileSdk = 35`, JVM target 17. Do not raise any of these.
- No new Gradle dependencies in Phase 1. This includes WorkManager and lifecycle-process. Coil's OkHttp network artifact is already present.
- Wire format is `KaikuJson` (snake_case, `ignoreUnknownKeys`). Every new model field must have a default, so older and newer servers both parse.
- Contract names are used verbatim: C1 `GET /api/messages/channel/{id}?after=&limit=` (items ascending), C2 `Ready { user_id, server_time }`, C3 `dm_created { channel }`, C4 `password_change_required` and `POST /auth/me/password`, C5 unknown enums map to `UNKNOWN` and are hidden.
- i18n-ready (spec, "Answers & decisions"): English only for v1.0. Every user-facing literal in a file a task touches moves to `app/src/main/res/values/strings.xml` and is read with `stringResource()` or `context.getString()`. ViewModels expose `@StringRes Int` (or a small `UiText` wrapper), never English `String`. Keys are snake_case `area_purpose` and mirror the web keys (`chat.send_failed` ⇄ `chat_send_failed`). Touched copy says **"server"**, never "guild" (U-12). Do not add new hardcoded user-facing strings.
- UI colours come only from `MaterialTheme.colorScheme` (KaikuTheme tokens). No literal `Color(...)` for text or fills (CLAUDE.md contrast rules).
- Tokens never go into URLs, query strings or logs. Do not log whole WebSocket payloads either; log the event type only.
- No task needs a live server. Verify with `./gradlew testDebugUnitTest` / `./gradlew lintDebug` in `mobile/android` and with CI. The review host has no JDK or adb, so CI is the source of truth. Device behaviour is checked by the maintainer using the checklist at the end.
- Android unit tests in CI are still `continue-on-error` (R-14, owned by the release plan). A task counts as green only when its tests show as passing in the CI log, not just when the job is green.
- Git: one branch per task (`fix/…` or `feature/…`), squash-merge PR, Conventional Commit `type(scope): subject` of ≤72 chars with scope `client` (`api` for the server fixture file). Each user-visible change gets a `CHANGELOG.md` `[Unreleased]` entry written from the user's perspective, prefixed "Android:".
- Review gate for every task: Fable plus one external model (Gemini 4 Argon via `agy`, or Codex). The public repo means security-relevant changes are described neutrally.
- Push seam (decision): the `PushTransport` interface and wake-fetch worker are **left for Phase 2**. They need WorkManager plus hilt-work and an Application `Configuration.Provider`, and that is not cheap. Phase 1 instead ships the two seams Phase 2 needs, `TokenRefresher.ensureFresh()` (Task 2) and `ChatRepository.catchUp(channelId)` (Task 5).

## Review Focus

1. **Refresh token revoked elsewhere while the socket is reconnecting.** The app should log out once and go to Login, not loop on refresh or reconnect. Test: Task 4, `auth rejected refresh logs out and stops reconnecting`.
2. **Server closes with 1008 although the local expiry says the token is still valid** (clock skew, or a server-side revocation). The app should force a refresh and reconnect, not dead-end. Test: Task 4, `auth rejected close forces refresh even when token not locally expired`.
3. **More messages missed than the catch-up cap** (phone offline over a weekend). The channel should show the latest page with no silent hole. Test: Task 5, `catch up beyond page cap reloads latest page`.
4. **Microphone permission permanently denied ("Don't ask again").** The app should show the rationale and an "Open settings" action, not re-prompt in a loop or join silently. Test: Task 6, `permanently denied mic yields OpenSettings decision`.
5. **Image URL pointing at a foreign host** (an embed or avatar on another domain, or the user switching servers). The bearer token must never be sent to any host other than the configured server. Test: Task 8, `auth header not added for foreign host`.

---

## File Structure (new files)

| File | Responsibility |
|---|---|
| `server/tests/integration/android_contract_fixtures.rs` | Serialises real server types into Android golden fixtures (compare mode, or write mode when `KAIKU_UPDATE_GOLDEN=1`) |
| `app/src/test/resources/golden/*.json` | Golden wire fixtures consumed by Android tests |
| `app/src/test/java/io/wolftown/kaiku/testutil/Golden.kt` | `Golden.load(name: String): String` from the classpath |
| `domain/model/CursorPage.kt` | `CursorPage<T>` page model |
| `domain/MessageOrdering.kt` | Pure merge and ascending ordering of messages |
| `data/auth/TokenRefresher.kt` | `TokenRefresher` interface and `RefreshResult` |
| `domain/model/LenientEnumSerializer.kt` | Unknown-value-tolerant enum serializer (C5) |
| `data/ws/RealtimeSocket.kt` | Transport interface implemented by `KaikuWebSocket` |
| `data/ws/RealtimeConnectionManager.kt` | Owns connect, refresh, backoff, network change and the pong watchdog |
| `data/ws/ReconnectBackoff.kt` | Pure jittered exponential backoff |
| `di/RealtimeCoroutineScope.kt`, `di/RealtimeCoroutineScopeModule.kt` | Scope qualifier and provider (same pattern as `ChatCoroutineScope`) |
| `data/sync/ResyncCoordinator.kt` | Runs re-subscribe, catch-up and list reloads on every `Ready` |
| `ui/shared/ConnectionBanner.kt` | "Reconnecting…" / "Offline" strip |
| `ui/channel/EditMessageDialog.kt` | Edit dialog that replaces the dead "Edit" item |
| `ui/voice/VoicePermissions.kt` | Pure permission sets and the permission decision |
| `data/api/ServerUrls.kt` | Pure relative-URL resolution |
| `data/api/AuthHeaderInterceptor.kt` | Same-host bearer header for Coil |
| `domain/ChannelSections.kt` | Pure grouping of channels under named categories |
| `domain/model/Category.kt` | Guild category model |
| `domain/ChannelTitle.kt` | Pure title resolution for guild channels and DMs |
| `ui/auth/ChangePasswordScreen.kt`, `ui/auth/ChangePasswordViewModel.kt`, `ui/auth/PasswordChangeRequiredException.kt` | Forced password change (C4) |

Main sources are under `mobile/android/app/src/main/java/io/wolftown/kaiku/` and tests under `mobile/android/app/src/test/java/io/wolftown/kaiku/`. Paths below are written relative to those roots. `res/` means `mobile/android/app/src/main/res/`.

---

### Task 1: Message history loads (`CursorPage<T>`, ordering, golden fixtures) — A-01

Branch `fix/android-message-history`. No dependencies.

**Files:**
- Create: `server/tests/integration/android_contract_fixtures.rs`; Modify: `server/tests/integration/main.rs` (add `mod android_contract_fixtures;`)
- Create: `app/src/test/resources/golden/messages_page.json`, `ws_message_new.json`, `auth_response.json`
- Create: `app/src/test/java/io/wolftown/kaiku/testutil/Golden.kt`, `domain/model/CursorPage.kt`, `domain/MessageOrdering.kt`
- Modify: `data/api/MessageApi.kt:12,33-50`, `data/repository/ChatRepository.kt:68-90,155-170`, `ui/channel/TextChannelViewModel.kt` (`canLoadMore`), `ui/channel/TextChannelScreen.kt` (initial scroll, load-more visibility, strings)
- Test: `data/api/GoldenContractTest.kt` (new), `domain/MessageOrderingTest.kt` (new), `integration/MessageFlowTest.kt` (rewrite to use `MessageApiImpl` over `MockEngine`)

**Interfaces:**
- Produces:
  - `@Serializable data class CursorPage<T>(val items: List<T>, val hasMore: Boolean = false, val nextCursor: String? = null)`
  - `MessageApi.getMessages(channelId: String, before: String? = null, after: String? = null, limit: Int = 50): CursorPage<Message>`. Calling it with both `before` and `after` throws `IllegalArgumentException`.
  - `object MessageOrdering { fun merge(existing: List<Message>, incoming: List<Message>): List<Message> }` returns the union by `id` (the incoming copy wins), sorted ascending by `Instant.parse(createdAt)` and then by `id`. Unparseable `createdAt` sorts last.
  - `ChatRepository.loadMessages(channelId: String, before: String? = null): Boolean` returns `hasMore`. Without `before` it replaces the list; with `before` it merges.
  - `TextChannelViewModel.canLoadMore: StateFlow<Boolean>`
  - Test helper `Golden.load(name: String): String`
  - Rust helper `fn check_golden(name: &str, value: serde_json::Value)`: in compare mode it asserts `serde_json::Value` equality with `mobile/android/app/src/test/resources/golden/<name>`; with `KAIKU_UPDATE_GOLDEN=1` it writes the pretty-printed file instead.

- [ ] **Step 1: Write the server fixture generator.** Write the tests `android_golden_messages_page`, `android_golden_ws_message_new` and `android_golden_auth_response` in `android_contract_fixtures.rs`. They are plain `#[test]`s, not `#[sqlx::test]`, and need no DB. They build fixed-UUID, fixed-timestamp instances of `CursorPaginatedResponse<MessageResponse>`, `ServerEvent::MessageNew` and `AuthResponse`, then call `check_golden`. Data rules:
  - The messages page has 2 items, **newest first** (the real server order), `has_more: true` and `next_cursor` set to the older item's id.
  - One item has an attachment whose `url` is `/api/messages/attachments/<id>/download`, plus `thumbnail_url`.
  - One `created_at` has 6 fractional digits and the other has none.
  - The author's `status` is `"online"`.
- [ ] **Step 2: Generate the fixtures.** Run: `KAIKU_UPDATE_GOLDEN=1 SQLX_OFFLINE=true cargo test -p vc-server --test integration android_contract_fixtures`, then the same command without the env var. Expected: 3 JSON files exist and the second run passes.
- [ ] **Step 3: Write the failing Android tests.**

```kotlin
// GoldenContractTest
@Test fun `messages page fixture decodes as CursorPage`() {
    val page = KaikuJson.decodeFromString<CursorPage<Message>>(Golden.load("messages_page.json"))
    assertEquals(2, page.items.size); assertTrue(page.hasMore); assertEquals(page.items[1].id, page.nextCursor)
}
@Test fun `ws message_new fixture decodes and its message decodes`()   // WsJson → ServerEvent.MessageNew → Message
@Test fun `auth response fixture decodes`()

// MessageOrderingTest
@Test fun `merge orders newest-first server page ascending`()          // result ids == [older, newer]
@Test fun `orders by instant not string`()                            // "…:00.5Z" vs "…:00.123456Z" vs "…:01Z"
@Test fun `merge dedupes by id and incoming wins`()
@Test fun `unparseable createdAt sorts last`()

// MessageFlowTest (MockEngine serving Golden.load("messages_page.json"))
@Test fun `initial load shows server page oldest first`()
@Test fun `load older merges before cursor and reports hasMore`()     // request has before=<first id>; returns false when has_more=false
@Test fun `live message is appended after loaded history`()
```

- [ ] **Step 4: Run them and confirm they fail.** Run: `cd mobile/android && ./gradlew testDebugUnitTest --tests '*GoldenContractTest' --tests '*MessageOrderingTest' --tests '*MessageFlowTest'`. Expected: compile failure (`CursorPage` and `MessageOrdering` do not exist).
- [ ] **Step 5: Implement.**
  - Add `CursorPage`, `MessageOrdering` and the new `getMessages` signature. Send `after` only when it is non-null; an older server ignores it and the merge still yields a correct order.
  - In `ChatRepository`, route `loadMessages`, `sendMessage` and `handleMessageNew` through `MessageOrdering.merge`.
  - In `TextChannelScreen`, scroll to the last item on the first non-empty load. Fix the off-by-one: the "load-more" item sits at index 0, so the last message is at `messages.size`. Show "Load earlier messages" only when `canLoadMore` is true.
  - Move this file's literals to `strings.xml` (`chat_load_earlier`, `chat_empty`, `common_back`).
- [ ] **Step 6: Run the tests and lint and confirm they pass.** Run the Step 4 command plus `./gradlew lintDebug`. Expected: PASS, with no new lint errors.
- [ ] **Step 7: Add a CHANGELOG entry and commit.** Changelog: "Android: message history now loads when opening a channel and shows oldest-to-newest." Commits: `test(api): golden JSON fixtures for the Android client` and `fix(client): Android message history uses the paginated response`.

---

### Task 2: Token refresh through the normal pipeline — A-02

Branch `fix/android-token-refresh`. No dependencies.

**Files:**
- Create: `data/auth/TokenRefresher.kt`
- Modify: `data/api/KaikuHttpClient.kt:46,125-199`, `data/local/TokenStorage.kt` (add `getAccessTokenExpiresAtMillis(): Long`), `di/NetworkModule.kt` (bind `TokenRefresher` → `KaikuHttpClient`)
- Test: `data/api/KaikuHttpClientTest.kt`

**Interfaces:**
- Produces:
  - `enum class RefreshResult { SUCCESS, AUTH_REJECTED, NETWORK_ERROR, NO_SESSION }`, made public and moved out of `KaikuHttpClient`.
  - `interface TokenRefresher`:
    - `suspend fun ensureFresh(minValidityMs: Long = 60_000): RefreshResult` returns SUCCESS without a network call when the token is valid for at least `minValidityMs`.
    - `suspend fun forceRefresh(): RefreshResult`
  - Both methods share one `Mutex` with the 401 path and keep the existing double-check against a token another caller already rotated. AUTH_REJECTED and NO_SESSION call `authState.setLoggedOut()` and `tokenStorage.clear()`.
- Decision: keep the existing `HttpSend` interceptor, but issue the refresh as `activeClient().post("/auth/refresh") { attributes.put(SkipAuthInterceptor, true); setBody(RefreshRequest(token)) }`. That goes through `defaultRequest`, so it gets the base URL and content type. Delete the hand-built `HttpRequestBuilder` and `Sender.execute` path. Do not adopt Ktor `Auth`; that would mean re-testing every 401 edge case for no gain.

- [ ] **Step 1: Write the failing tests.**

```kotlin
@Test fun `refresh request is sent to configured server host`()  // MockEngine: url.protocol==HTTPS, url.host=="kaiku.example.com", path "/auth/refresh",
                                                                  //   body JSON has "refresh_token":"valid-refresh-token"
@Test fun `ensureFresh skips network when token valid long enough`()  // expiresAt = now+10min → 0 requests, SUCCESS
@Test fun `ensureFresh refreshes when token expires within lead time`() // expiresAt = now+30s → 1 refresh, saveTokens called
@Test fun `forceRefresh with rejected refresh token logs out`()   // 401 on /auth/refresh → AUTH_REJECTED, authState.setLoggedOut(), tokenStorage.clear()
@Test fun `forceRefresh network failure keeps session`()          // engine throws IOException → NETWORK_ERROR, setLoggedOut NOT called
@Test fun `concurrent 401s trigger a single refresh`()            // two parallel requests → exactly 1 /auth/refresh
```

- [ ] **Step 2: Run them and confirm they fail.** Run: `./gradlew testDebugUnitTest --tests '*KaikuHttpClientTest'`. Expected: the host test fails (`localhost` ≠ `kaiku.example.com`) and the others fail to compile.
- [ ] **Step 3: Implement** `TokenRefresher` on `KaikuHttpClient` and `TokenStorage.getAccessTokenExpiresAtMillis()` as specified above.
- [ ] **Step 4: Run the tests and confirm they pass,** including the existing `on 401 triggers token refresh and retries with new token`.
- [ ] **Step 5: Add a CHANGELOG entry and commit.** Changelog: "Android: you no longer get stuck about 15 minutes after signing in." Commit: `fix(client): Android token refresh uses the configured server URL`.

---

### Task 3: Unknown channel types and visible load errors — C5 / A-03

Branch `fix/android-unknown-channel-types`. No dependencies.

**Files:**
- Create: `domain/model/LenientEnumSerializer.kt`, `app/src/test/resources/golden/guild_channels.json`
- Modify: `domain/model/Channel.kt:6-11`, `domain/model/User.kt` (`UserStatus`), `ui/home/HomeViewModel.kt`, `ui/home/HomeScreen.kt:100-140`, `res/values/strings.xml`, `server/tests/integration/android_contract_fixtures.rs` (add `android_golden_guild_channels`: `Vec<ChannelWithUnread>` with text, voice, forum and announcement channels)
- Test: `domain/model/LenientEnumTest.kt` (new), `data/api/GoldenContractTest.kt`, `ui/home/HomeViewModelTest.kt`

**Interfaces:**
- Produces:
  - `abstract class LenientEnumSerializer<T : Enum<T>>(serialName: String, private val values: Array<T>, private val unknown: T) : KSerializer<T>`. It decodes by `@SerialName` and maps anything else to `unknown`. Encoding writes the serial name; `UNKNOWN` is never sent.
  - `ChannelType` gains `UNKNOWN` and uses `@Serializable(with = ChannelTypeSerializer::class)`.
  - `UserStatus` gains `UNKNOWN` (`UserStatusSerializer`).
  - `HomeViewModel.channels` drops `UNKNOWN` and `DM` channels.
  - `HomeViewModel.error: StateFlow<Int?>` holds a `@StringRes` value.

- [ ] **Step 1: Generate the fixture.** Run: `KAIKU_UPDATE_GOLDEN=1 SQLX_OFFLINE=true cargo test -p vc-server --test integration android_golden_guild_channels`.
- [ ] **Step 2: Write the failing tests.**

```kotlin
@Test fun `unknown channel_type decodes to UNKNOWN`()              // {"channel_type":"forum",…} → ChannelType.UNKNOWN
@Test fun `known channel types still decode`()                     // text/voice/dm
@Test fun `unknown user status decodes to UNKNOWN`()
@Test fun `guild channels fixture decodes with forum and announcement`() // GoldenContractTest: 4 items, 2 UNKNOWN
@Test fun `unknown channel types are hidden from channel list`()   // HomeViewModelTest: channels.value has only TEXT/VOICE
@Test fun `channel load error is exposed when guilds exist`()      // guilds non-empty, getChannels throws → error.value == R.string.home_channels_load_failed
```

- [ ] **Step 3: Run them and confirm they fail.** Run: `./gradlew testDebugUnitTest --tests '*LenientEnumTest' --tests '*GoldenContractTest' --tests '*HomeViewModelTest'`.
- [ ] **Step 4: Implement the serializers, the filter and the error handling.**
  - In `HomeScreen`, show a load error even when guilds exist: an inline error row with a Retry action above the channel list (keep the full-screen error only for the empty case).
  - Move every literal in `HomeScreen` and `HomeViewModel` to `strings.xml` with "server" wording ("Open servers", "No servers yet", "Select a server").
- [ ] **Step 5: Run the tests and lint and confirm they pass.**
- [ ] **Step 6: Add a CHANGELOG entry and commit.** Changelog: "Android: the channel list no longer goes blank when a server has forum or announcement channels; load errors are now shown." Commit: `fix(client): Android tolerates unknown channel types`.

---

### Task 4: Realtime connection owner (reconnect, refresh, watchdog) — scope item 2 / A-09

Branch `fix/android-realtime-reconnect`. Depends on Task 2 (`TokenRefresher`).

**Files:**
- Create: `data/ws/RealtimeSocket.kt`, `data/ws/RealtimeConnectionManager.kt`, `data/ws/ReconnectBackoff.kt`, `di/RealtimeCoroutineScope.kt`, `di/RealtimeCoroutineScopeModule.kt`
- Modify:
  - `data/ws/KaikuWebSocket.kt`: remove `shouldReconnect`, `scheduleReconnect`, the ping loop and the connectivity job. Make `connect` idempotent while Connecting or Connected. Log parse failures with the event type only.
  - `data/ws/ConnectivityMonitor.kt`: add `networkId` and implement `ConnectivitySignal`.
  - `di/WebSocketModule.kt`: bind the interfaces. Keep OkHttp `pingInterval(0)`.
  - `KaikuApplication.kt`: inject the manager and call `start()`.
  - `ui/MainActivity.kt`: `onStart` calls `onAppForeground()`.
  - `ui/home/HomeViewModel.kt`: remove `connectWebSocket()`.
- Test: `data/ws/RealtimeConnectionManagerTest.kt` (new, fakes plus virtual time), `data/ws/ReconnectBackoffTest.kt` (new), `data/ws/KaikuWebSocketTest.kt` (adapt to `connect(url, token)`)

**Interfaces:**
- Consumes: `TokenRefresher.ensureFresh()`, `TokenRefresher.forceRefresh()`, `RefreshResult` (Task 2)
- Produces:
  - `enum class ConnectionState { Connected, Connecting, Disconnected, AuthRejected }` (rename of `TokenExpired`)
  - `interface RealtimeSocket { val connectionState: StateFlow<ConnectionState>; val events: SharedFlow<ServerEvent>; fun connect(serverUrl: String, accessToken: String); fun disconnect(); fun send(event: ClientEvent): Boolean }`, implemented by `KaikuWebSocket`. `ChatRepository` and `VoiceRepository` keep injecting `KaikuWebSocket`.
  - `interface ConnectivitySignal { val isConnected: StateFlow<Boolean>; val networkId: StateFlow<Long?> }` (`Network.getNetworkHandle()`)
  - `fun interface WallClock { fun nowMillis(): Long }` (production: `System::currentTimeMillis`)
  - `class ReconnectBackoff(initialMs: Long = 1_000, maxMs: Long = 30_000, jitter: Double = 0.2, random: Random = Random.Default) { fun next(): Long; fun reset() }`
  - `enum class RealtimeStatus { Idle, Connecting, Connected, Reconnecting, Offline }`
  - `@Singleton class RealtimeConnectionManager(socket: RealtimeSocket, refresher: TokenRefresher, tokenStorage: TokenStorage, authState: AuthState, connectivity: ConnectivitySignal, @RealtimeCoroutineScope scope: CoroutineScope, clock: WallClock)` with `fun start()`, `fun onAppForeground()` and `val status: StateFlow<RealtimeStatus>`
  - Constants in its companion: `PING_INTERVAL_MS = 30_000`, `PONG_TIMEOUT_MS = 75_000`, `REFRESH_LEAD_MS = 60_000`, `MAX_CONSECUTIVE_AUTH_REJECTS = 3`
- Behaviour, as a state machine the tests pin:
  1. `AuthState` LoggedIn and a server URL present → `ensureFresh(REFRESH_LEAD_MS)` → `socket.connect(url, token)`. LoggedOut → `socket.disconnect()` and Idle.
  2. `Disconnected` → status Reconnecting, wait `backoff.next()`, then go back to step 1. Each attempt starts with `ensureFresh`, so an expired token is refreshed first; the old `TokenExpired` dead end is gone.
  3. `AuthRejected` (close 1008) → `forceRefresh()`.
     - SUCCESS → reconnect immediately.
     - AUTH_REJECTED or NO_SESSION → stop. The refresher has already logged the user out.
     - NETWORK_ERROR → back off.
     - After `MAX_CONSECUTIVE_AUTH_REJECTS` rejects despite successful refreshes → `authState.setLoggedOut()`.
  4. `ServerEvent.Ready` → Connected, `backoff.reset()`, reset the reject counter, record `lastPongAt = now`.
  5. While Connected, send `ClientEvent.Ping` every `PING_INTERVAL_MS`. Each `Pong` updates `lastPongAt`. If `now - lastPongAt > PONG_TIMEOUT_MS`, call `socket.disconnect()` and go to step 2.
  6. `connectivity.isConnected` false → Offline, cancel pending retries. True → reset the backoff and connect now. `networkId` changes while Connected → force a reconnect.
  7. `onAppForeground()`: if not Connected or Connecting, reset the backoff and connect now.
  8. While logged in, schedule `ensureFresh` at `expiresAt - REFRESH_LEAD_MS` so REST and image calls stay authenticated.

- [ ] **Step 1: Write the failing tests** (`StandardTestDispatcher`, `WallClock { testScheduler.currentTime }`, `FakeSocket`, `FakeRefresher`, `FakeConnectivity`).

```kotlin
@Test fun `login with expired token refreshes before connecting`()          // refresher.ensureFresh called before socket.connect, connect uses new token
@Test fun `disconnect schedules reconnect with backoff`()                    // advanceTimeBy(999) no connect; advanceTimeBy(2) connect
@Test fun `auth rejected close forces refresh even when token not locally expired`() // AuthRejected → forceRefresh → connect
@Test fun `auth rejected refresh logs out and stops reconnecting`()          // forceRefresh=AUTH_REJECTED → no further connect after 5 min virtual
@Test fun `repeated auth rejects after successful refresh log out`()         // 3x → authState.setLoggedOut()
@Test fun `missing pong triggers reconnect`()                                // Ready, no Pong, advance 75_001 → socket.disconnect + connect
@Test fun `pong keeps connection alive`()                                    // Pong every 30s for 5 min → no disconnect
@Test fun `network loss pauses and regain reconnects immediately`()
@Test fun `network change while connected forces reconnect`()
@Test fun `app foreground reconnects without waiting for backoff`()
@Test fun `logout disconnects socket`()
@Test fun `status reflects reconnecting then connected`()                    // Turbine on status
// ReconnectBackoffTest
@Test fun `backoff doubles to cap and resets`()                              // fixed Random → 1000,2000,4000,…,30000; reset → 1000
@Test fun `jitter stays within 20 percent`()
// KaikuWebSocketTest (adapted)
@Test fun `connect is idempotent while connecting`()                         // second connect() opens no second socket
@Test fun `close 1008 while authenticating reports AuthRejected`()
```

- [ ] **Step 2: Run them and confirm they fail.** Run: `./gradlew testDebugUnitTest --tests '*RealtimeConnectionManagerTest' --tests '*ReconnectBackoffTest' --tests '*KaikuWebSocketTest'`.
- [ ] **Step 3: Implement** the interfaces, the manager and the DI as specified. Keep `KaikuWebSocket` free of timers.
- [ ] **Step 4: Run all unit tests and confirm they pass.** Run: `./gradlew testDebugUnitTest`. `HomeViewModelTest` drops its expectation that the VM connects the socket.
- [ ] **Step 5: Add a CHANGELOG entry and commit.** Changelog: "Android: live updates now resume on their own after the phone sleeps, changes network or the session renews." Commit: `fix(ws): Android reconnects with refresh, backoff and watchdog`.

---

### Task 5: Re-subscribe and catch-up after reconnect, live DM creation — C1/C2/C3

Branch `feature/android-catch-up`. Depends on Tasks 1 and 4.
- **Server C2 and C3 must be merged first:** this task's Rust fixtures construct the new variants.
- C1 is soft: the merge is order-safe even on an older server, but `has_more` paging needs C1.

**Files:**
- Create: `data/sync/ResyncCoordinator.kt`, `app/src/test/resources/golden/ws_ready.json`, `ws_dm_created.json`, `dm_list.json`
- Modify:
  - `data/ws/ServerEvent.kt`: `Ready(userId, serverTime: String? = null)`; add `@SerialName("dm_created") data class DmCreated(val channel: JsonObject)`.
  - `data/repository/ChatRepository.kt`: add `subscribedChannels`; `unsubscribeFromChannel` removes from the set.
  - `data/repository/DmRepository.kt`: inject `KaikuWebSocket`, `Json` and `@ChatCoroutineScope`; handle `DmCreated`.
  - `data/repository/GuildRepository.kt`: add `reloadSelectedGuildChannels()`.
  - `KaikuApplication.kt`: start the coordinator.
  - `server/tests/integration/android_contract_fixtures.rs`: add `android_golden_ws_ready`, `android_golden_ws_dm_created`, `android_golden_dm_list`.
- Test: `data/repository/ChatRepositoryCatchUpTest.kt` (new, `MessageApiImpl` over MockEngine with golden pages), `data/repository/DmRepositoryTest.kt` (new), `data/api/GoldenContractTest.kt`

**Interfaces:**
- Consumes: `MessageApi.getMessages(..., after = …)` and `MessageOrdering.merge` (Task 1); `ConnectionState`/`RealtimeConnectionManager` (Task 4)
- Produces:
  - `ChatRepository.resubscribeAll()` sends `Subscribe` for every id in `subscribedChannels`.
  - `suspend fun ChatRepository.catchUp(channelId: String)` is the Phase 2 wake-fetch seam. It pages `after = last.id`, `limit = 100`, up to `CATCH_UP_MAX_PAGES = 5`. If `has_more` is still true after that, it replaces the list with `loadMessages(channelId)`. Missed messages in non-active channels call `guildRepository.onMessageReceived` once per message.
  - `ChatRepository.lastServerTime: StateFlow<String?>` (C2 watermark; stored only in Phase 1)
  - `@Singleton class ResyncCoordinator(webSocket: KaikuWebSocket, chat: ChatRepository, guilds: GuildRepository, dms: DmRepository, @ChatCoroutineScope scope)` with `fun start()`. On every `Ready` it runs `resubscribeAll()`, then `catchUp` for each channel with a non-empty list, `reloadSelectedGuildChannels()` and `loadDms()`. Each step is wrapped in `runCatching` and logged; one failure does not abort the others.
  - `DmRepository` on `DmCreated`: decode `channel` as `DmConversation` and upsert it at the top by `id`.

- [ ] **Step 1: Generate the fixtures** (requires server C2 and C3 on `main`). Run: `KAIKU_UPDATE_GOLDEN=1 SQLX_OFFLINE=true cargo test -p vc-server --test integration android_golden_ws`, then the same command with `android_golden_dm_list`.
- [ ] **Step 2: Write the failing tests.**

```kotlin
@Test fun `ready resubscribes all open channels`()                  // subscribe(a), subscribe(b), unsubscribe(b), Ready → Subscribe(a) only
@Test fun `catch up requests after last known id and merges ascending`() // request has after=<last id>, no before
@Test fun `catch up pages until has_more false`()
@Test fun `catch up beyond page cap reloads latest page`()          // 6 pages has_more=true → final request has neither before nor after
@Test fun `catch up skips channels never loaded`()
@Test fun `one failing step does not abort resync`()                // ResyncCoordinator: catchUp throws → loadDms still called
@Test fun `dm_created upserts conversation at top`()                // golden ws_dm_created.json
@Test fun `ready fixture with server_time decodes and is stored`()
@Test fun `ready without server_time still decodes`()               // {"type":"ready","user_id":"…"}
@Test fun `dm list fixture decodes`()
```

- [ ] **Step 3: Run them and confirm they fail.** Run: `./gradlew testDebugUnitTest --tests '*ChatRepositoryCatchUpTest' --tests '*DmRepositoryTest' --tests '*GoldenContractTest'`.
- [ ] **Step 4: Implement** as specified. `subscribeToChannel` still sends immediately. The Ready-triggered resend covers the case where the first send happened before the socket was connected.
- [ ] **Step 5: Run all unit tests and confirm they pass.**
- [ ] **Step 6: Add a CHANGELOG entry and commit.** Changelog: "Android: messages sent while you were offline appear after reconnecting; new DMs show up without reopening the app." Commit: `feat(chat): Android catch-up after reconnect and live DM creation`.

---

### Task 6: Voice permissions, foreground-service guard, audio route — A-04 / A-08 subset

Branch `fix/android-voice-permissions`. No dependencies.

**Files:**
- Create: `ui/voice/VoicePermissions.kt`
- Modify:
  - `AndroidManifest.xml`: add `POST_NOTIFICATIONS` and `MODIFY_AUDIO_SETTINGS`.
  - `ui/voice/VoiceViewModel.kt:69-72`: remove `init { onJoin() }`.
  - `ui/voice/VoiceChannelScreen.kt`: permission launcher, Join button state, rationale, error snackbar, strings.
  - `service/VoiceCallService.kt:92-109`: guard `startForeground` and move its strings to resources.
  - `data/voice/VoiceServiceEvents.kt`: add `StartFailed`.
  - `data/repository/VoiceRepository.kt:109-170`: precheck and handle `StartFailed`.
  - `data/voice/AudioRouteManager.kt:107-137,204-245`
- Test: `ui/voice/VoicePermissionsTest.kt` (new), `ui/voice/VoiceViewModelTest.kt`, `data/repository/VoiceRepositoryTest.kt`, `data/voice/AudioRouteSelectionTest.kt` (new)

**Interfaces:**
- Produces:
  - `object VoicePermissions`:
    - `fun toRequest(sdkInt: Int): List<String>` returns RECORD_AUDIO, plus BLUETOOTH_CONNECT if ≥31, plus POST_NOTIFICATIONS if ≥33.
    - `fun decide(micGranted: Boolean, shouldShowRationale: Boolean, askedBefore: Boolean): VoicePermissionDecision`
  - `sealed interface VoicePermissionDecision { Join; Request; ShowRationale; OpenSettings }`. Only RECORD_AUDIO gates joining; Bluetooth and notification permissions are optional.
  - `interface MicPermissionChecker { fun isGranted(): Boolean }`, with a Hilt production implementation using `ContextCompat.checkSelfPermission`.
  - `class MissingMicPermissionException : IllegalStateException()`. `VoiceRepository.joinChannel` throws it before touching WebRTC when the permission is missing.
  - `VoiceServiceEvent.StartFailed`. `VoiceCallService` checks RECORD_AUDIO before `startForeground(…, MICROPHONE)` and catches `SecurityException` and (on API 31+) `ForegroundServiceStartNotAllowedException`. On failure it emits `StartFailed` and calls `stopSelf()`. `VoiceRepository` reacts with `leaveChannel()` and `_error = R.string.voice_start_failed`.
  - `VoiceViewModel.onJoin()` is a no-op when `voiceRepository.currentChannelId.value == channelId`.
  - `VoiceViewModel.error: StateFlow<Int?>` holds a `@StringRes` value.
  - `fun selectDefaultRoute(available: Set<AudioRoute>): AudioRoute` (internal, pure): Bluetooth > WiredHeadset > Speaker. Earpiece is never the default; the family uses group voice, as on Discord.
  - `AudioRouteManager` changes:
    - Bluetooth is "available" only if `audioManager.getDevices(GET_DEVICES_OUTPUTS)` contains `TYPE_BLUETOOTH_SCO`. "Adapter enabled" no longer counts.
    - `requestAudioFocus()` applies `selectDefaultRoute` immediately, so the UI and the actual output match.
    - On API 31+, routes are switched with `setCommunicationDevice` / `clearCommunicationDevice`; below 31 the existing speakerphone/SCO calls remain.

- [ ] **Step 1: Write the failing tests.**

```kotlin
@Test fun `api 30 requests only record audio`()
@Test fun `api 31 adds bluetooth connect`()
@Test fun `api 33 adds post notifications`()
@Test fun `granted mic yields Join`()
@Test fun `first denial yields ShowRationale`()                 // micGranted=false, shouldShowRationale=true
@Test fun `permanently denied mic yields OpenSettings decision`() // micGranted=false, shouldShowRationale=false, askedBefore=true
@Test fun `init does not join channel`()                        // VoiceViewModelTest: coVerify(exactly = 0) { joinChannel(any()) }
@Test fun `onJoin skips when already in this channel`()
@Test fun `join without mic permission fails fast`()            // VoiceRepositoryTest: throws MissingMicPermissionException, webRtcManager.initialize not called
@Test fun `service start failure leaves channel and sets error`()
@Test fun `default route prefers bluetooth then wired then speaker`()
@Test fun `earpiece is never the default`()
```

- [ ] **Step 2: Run them and confirm they fail.** Run: `./gradlew testDebugUnitTest --tests '*VoicePermissionsTest' --tests '*VoiceViewModelTest' --tests '*VoiceRepositoryTest' --tests '*AudioRouteSelectionTest'`.
- [ ] **Step 3: Implement.**
  - `VoiceChannelScreen` uses `rememberLauncherForActivityResult(RequestMultiplePermissions())`. Store `askedBefore` in `rememberSaveable`. Map each `VoicePermissionDecision` to UI:
    - Join → `onJoin()`
    - Request → launch
    - ShowRationale → a dialog with one sentence and a "Continue" button
    - OpenSettings → an `ACTION_APPLICATION_DETAILS_SETTINGS` intent
  - The screen auto-runs the decision once on first composition. Re-entering via the overlay does not re-dial because of the `onJoin` guard.
  - Collect `viewModel.error` into a `SnackbarHost`.
  - Move all literals in the touched files (screen, service notification, VM) to `strings.xml`.
- [ ] **Step 4: Run the tests and lint and confirm they pass.** The manifest change must not raise new lint errors (`./gradlew lintDebug`).
- [ ] **Step 5: Add a CHANGELOG entry and commit.** Changelog: "Android: joining voice asks for microphone access instead of crashing; audio defaults to speaker or your headset." Commit: `fix(voice): Android requests runtime permissions before joining`.

---

### Task 7: Chat errors, working Edit and the reconnect banner — UI-F1 / A-06

Branch `fix/android-chat-errors`. Depends on Task 4 (`RealtimeConnectionManager.status`).

**Files:**
- Create: `ui/channel/EditMessageDialog.kt`, `ui/shared/ConnectionBanner.kt`
- Modify: `ui/channel/TextChannelViewModel.kt` (errors as `@StringRes`, `editingMessage` state, `connectionStatus`), `ui/channel/TextChannelScreen.kt:32-37,125-128` (SnackbarHost, edit wiring, banner), `ui/channel/MessageInput.kt` (strings), `ui/home/HomeScreen.kt` (banner), `ui/home/HomeViewModel.kt` (expose status), `res/values/strings.xml`
- Test: `ui/channel/TextChannelViewModelTest.kt`

**Interfaces:**
- Consumes: `RealtimeConnectionManager.status: StateFlow<RealtimeStatus>` (Task 4)
- Produces:
  - `TextChannelViewModel.error: StateFlow<Int?>` (`chat_send_failed`, `chat_edit_failed`, `chat_delete_failed`, `chat_load_failed`, `chat_reaction_failed`)
  - `TextChannelViewModel.editingMessage: StateFlow<Message?>`, with `fun onStartEdit(messageId: String)`, `fun onConfirmEdit(content: String)` and `fun onCancelEdit()`
  - `TextChannelViewModel.connectionStatus: StateFlow<RealtimeStatus>`
  - `@Composable fun ConnectionBanner(status: RealtimeStatus, modifier: Modifier = Modifier)` renders nothing for Connected and Idle, "Reconnecting…" for Connecting and Reconnecting, and "You're offline" for Offline.
  - `@Composable fun EditMessageDialog(initial: String, onConfirm: (String) -> Unit, onDismiss: () -> Unit)`. Blank or unchanged content disables Save.
- Snackbars: send failure shows `chat_send_failed` with a "Retry" action that calls `onSendMessage()` (the input text has already been restored). The others show a message only. `clearError()` runs after the snackbar is shown.

- [ ] **Step 1: Write the failing tests.**

```kotlin
@Test fun `send failure exposes error and restores input`()   // error.value == R.string.chat_send_failed, messageInput.value == "hi"
@Test fun `edit failure exposes error`()                      // R.string.chat_edit_failed
@Test fun `delete failure exposes error`()
@Test fun `confirm edit calls repository and closes dialog`()  // coVerify { editMessage(id, "new") }, editingMessage.value == null
@Test fun `blank edit is ignored`()
@Test fun `connection status mirrors realtime manager`()
```

- [ ] **Step 2: Run them and confirm they fail.** Run: `./gradlew testDebugUnitTest --tests '*TextChannelViewModelTest'`.
- [ ] **Step 3: Implement** as specified. Edit is offered only for own messages (`isOwnMessage`).
- [ ] **Step 4: Run the tests and lint and confirm they pass.**
- [ ] **Step 5: Add a CHANGELOG entry and commit.** Changelog: "Android: failed sends, edits and deletes now show a message (with Retry); editing your own messages works; a banner shows when the app is reconnecting." Commit: `fix(client): Android surfaces chat errors and enables editing`.

---

### Task 8: Images load (relative URLs, same-host auth) and the encrypted-message placeholder — A-05, spec item 3

Branch `fix/android-image-urls`. No dependencies.

**Files:**
- Create: `data/api/ServerUrls.kt`, `data/api/AuthHeaderInterceptor.kt`
- Modify:
  - `KaikuApplication.kt`: implement `SingletonImageLoader.Factory`.
  - `ui/MainActivity.kt`: provide `LocalServerBaseUrl` in `KaikuApp`.
  - `ui/channel/MessageItem.kt:65`, `ui/channel/MessageAttachments.kt:37-38`, `ui/home/GuildSidebar.kt:84`, `ui/dm/DmListScreen.kt:131`: resolve URLs before `AsyncImage`; move strings.
  - `ui/channel/MessageItem.kt`: encrypted placeholder.
  - `res/values/strings.xml`
- Test: `data/api/ServerUrlsTest.kt` (new), `data/api/AuthHeaderInterceptorTest.kt` (new, fake `Interceptor.Chain`), `domain/MessageDisplayTest.kt` (new)

**Interfaces:**
- Produces:
  - `object ServerUrls { fun resolve(serverBaseUrl: String?, url: String?): String? }`:
    - null or blank → null
    - `https://…` and `http://…` → returned unchanged
    - a path starting with `/` → `serverBaseUrl.trimEnd('/') + url`
    - anything else, or a missing base → null
  - `val LocalServerBaseUrl = staticCompositionLocalOf<String?> { null }`
  - `class AuthHeaderInterceptor(private val tokenStorage: TokenStorage) : okhttp3.Interceptor` adds `Authorization: Bearer …` only when the request scheme is https and the host equals the host of `tokenStorage.getServerUrl()`.
  - Image loader: `KaikuApplication.newImageLoader(context)` uses `OkHttpNetworkFetcherFactory(callFactory = { okHttp.newBuilder().addInterceptor(AuthHeaderInterceptor(tokenStorage)).build() })` and reuses the injected TLS 1.3 `OkHttpClient`.
  - `fun messageDisplayContent(message: Message): MessageBody` (pure, in `domain/`) with `sealed interface MessageBody { data class Text(val markdown: String); data object EncryptedPlaceholder }`. `encrypted == true` → `EncryptedPlaceholder`, rendered as `R.string.chat_message_encrypted_placeholder` ("This message is end-to-end encrypted and can't be shown on this device yet.").

- [ ] **Step 1: Write the failing tests.**

```kotlin
@Test fun `relative path is prefixed with server base`()        // ("https://k.example/", "/api/files/a.png") → "https://k.example/api/files/a.png"
@Test fun `absolute https url is unchanged`()
@Test fun `null blank or missing base yields null`()
@Test fun `auth header added for server host`()                  // fake Chain, https://kaiku.example.com/api/files/a → header "Bearer t"
@Test fun `auth header not added for foreign host`()             // request to other.example → no Authorization
@Test fun `auth header not added over plain http`()
@Test fun `encrypted message renders placeholder`()
@Test fun `plain message renders text`()
```

- [ ] **Step 2: Run them and confirm they fail.** Run: `./gradlew testDebugUnitTest --tests '*ServerUrlsTest' --tests '*AuthHeaderInterceptorTest' --tests '*MessageDisplayTest'`.
- [ ] **Step 3: Implement** as specified. To test the interceptor without TLS, run it on an `okhttp3.Interceptor.Chain` built from a fake chain with explicit `HttpUrl`s, so no real https server is needed.
- [ ] **Step 4: Run the tests and lint and confirm they pass.**
- [ ] **Step 5: Add a CHANGELOG entry and commit.** Changelog: "Android: shared photos, avatars and server icons now load; encrypted messages show a clear note instead of raw data." Commit: `fix(client): Android resolves image URLs and authenticates them`.

---

### Task 9: Readable titles and category headers — A-12 / A-18 subsets

Branch `fix/android-titles-categories`. Should merge after Task 7 because both edit `TextChannelViewModel` and `TextChannelScreen`; there is no logic dependency.

**Files:**
- Create: `domain/model/Category.kt`, `domain/ChannelSections.kt`, `domain/ChannelTitle.kt`, `app/src/test/resources/golden/guild_categories.json`
- Modify:
  - `data/api/GuildApi.kt`: `getCategories`.
  - `data/repository/GuildRepository.kt`: `categories` flow; `loadChannels` also loads categories, and a failure there means an empty list.
  - `ui/home/HomeViewModel.kt`: `sections`.
  - `ui/home/ChannelList.kt:53-83`: render sections; `#` and voice prefixes come from resources.
  - `ui/channel/TextChannelViewModel.kt:35-37`: `title`; inject `DmRepository` and `AuthState`.
  - `ui/channel/TextChannelScreen.kt:61`, `ui/channel/MessageInput.kt` (placeholder)
  - `data/api/DmApi.kt:33-41`: send the request body.
  - `data/repository/DmRepository.kt`, `ui/dm/DmListViewModel.kt`
  - `server/tests/integration/android_contract_fixtures.rs`: `android_golden_guild_categories` (`Vec<Category>`)
- Test: `domain/ChannelSectionsTest.kt` (new), `domain/ChannelTitleTest.kt` (new), `data/api/DmApiTest.kt` (new, MockEngine), `data/api/GoldenContractTest.kt`

**Interfaces:**
- Produces:
  - `@Serializable data class Category(val id: String, val name: String, val position: Int = 0, val parentId: String? = null)`. `category_type` is ignored.
  - `GuildApi.getCategories(guildId: String): List<Category>` (`GET /api/guilds/{id}/categories`)
  - `data class ChannelSection(val title: String?, val channels: List<Channel>)`
  - `fun groupChannels(channels: List<Channel>, categories: List<Category>): List<ChannelSection>`:
    - Uncategorised channels come first, in a section with `title = null`.
    - The other sections follow in category `position` order.
    - A category id missing from `categories` gets `title = null` (no header, never a UUID).
    - Channels keep their `position` order within a section.
  - `data class ChannelTitle(val text: String, val isDm: Boolean)`
  - `fun resolveChannelTitle(channelId: String, channels: List<Channel>, dms: List<DmConversation>, currentUserId: String?): ChannelTitle?` uses the guild channel name or `DmRepository.conversationTitle`. Null means unknown, and the UI shows `R.string.chat_title_loading` ("Conversation"), never the id.
  - `TextChannelViewModel.title: StateFlow<ChannelTitle?>`. The screen prefixes `#` only when `!isDm`.
  - `DmApi.markAsRead(channelId: String, lastReadMessageId: String)` sends `{"last_read_message_id": …}`.
  - `DmRepository.markAsRead(channelId: String)` reads `lastMessage?.id` and skips the network call when it is null.

- [ ] **Step 1: Generate the fixture.** Run: `KAIKU_UPDATE_GOLDEN=1 SQLX_OFFLINE=true cargo test -p vc-server --test integration android_golden_guild_categories`.
- [ ] **Step 2: Write the failing tests.**

```kotlin
@Test fun `sections follow category position with uncategorized first`()
@Test fun `unknown category id yields headerless section`()       // title == null, never the id
@Test fun `guild channel title uses channel name`()
@Test fun `dm title uses other participant display name`()        // isDm == true
@Test fun `unknown channel yields null title`()
@Test fun `mark dm read sends last_read_message_id body`()        // DmApiTest: body JSON contains "last_read_message_id":"m1"
@Test fun `mark dm read without last message skips request`()
@Test fun `categories fixture decodes`()
```

- [ ] **Step 3: Run them and confirm they fail.** Run: `./gradlew testDebugUnitTest --tests '*ChannelSectionsTest' --tests '*ChannelTitleTest' --tests '*DmApiTest' --tests '*GoldenContractTest'`.
- [ ] **Step 4: Implement** as specified and move the touched literals to `strings.xml`.
- [ ] **Step 5: Run the tests and lint and confirm they pass.**
- [ ] **Step 6: Add a CHANGELOG entry and commit.** Changelog: "Android: DMs show the person's name instead of an ID, channel groups show their category names, and opening a DM clears its unread badge on the server." Commit: `fix(client): Android shows DM titles and category names`.

---

### Task 10: Forced password change on first login — C4

Branch `feature/android-forced-password-change`. Depends on **server C4** being merged: the fixture and the real flow need it. The Android code is safe before then because `passwordChangeRequired` defaults to false.

**Files:**
- Create: `ui/auth/PasswordChangeRequiredException.kt`, `ui/auth/ChangePasswordViewModel.kt`, `ui/auth/ChangePasswordScreen.kt`, `app/src/test/resources/golden/auth_response_password_change.json`
- Modify:
  - `domain/model/AuthResponse.kt`: add `val passwordChangeRequired: Boolean = false`.
  - `data/api/AuthApi.kt`: `changePassword`.
  - `data/repository/AuthRepository.kt:23-48`: pending state and completion.
  - `ui/auth/LoginViewModel.kt:99-128`, `ui/auth/LoginScreen.kt` (navigate; move strings)
  - `ui/KaikuNavGraph.kt`: route `change_password`, added to `authRoutes`.
  - `res/values/strings.xml`
  - `server/tests/integration/android_contract_fixtures.rs`: `android_golden_auth_response_password_change`
- Test: `data/repository/AuthRepositoryPasswordChangeTest.kt` (new, `AuthApiImpl` over MockEngine), `ui/auth/ChangePasswordViewModelTest.kt` (new), `ui/auth/LoginViewModelTest.kt`, `data/api/GoldenContractTest.kt`

**Interfaces:**
- Produces:
  - `AuthApi.changePassword(accessToken: String, currentPassword: String, newPassword: String)`. It sends `POST /auth/me/password` with an explicit `Authorization: Bearer` header and body `{current_password, new_password, revoke_others: false}`. A non-2xx response throws `ApiException`.
  - `AuthRepository.login(...)`: when `authResponse.passwordChangeRequired`, it skips `getMe` and persists **nothing**. It keeps `PendingPasswordChange(accessToken, refreshToken, expiresIn, currentPassword)` in memory and returns `Result.failure(PasswordChangeRequiredException())`.
  - `suspend fun AuthRepository.completePasswordChange(newPassword: String): Result<User>` runs `changePassword`, then `authenticatedGetMe`, then `saveTokens`, then `setLoggedIn`, and clears the pending state. Without pending state it returns `IllegalStateException`.
  - `fun AuthRepository.cancelPasswordChange()` clears the pending state (Back on the screen also returns to Login).
  - `LoginUiState.passwordChangeRequired: Boolean`. `LoginScreen` navigates to `change_password` when it is true.
  - `ChangePasswordViewModel` exposes `uiState: StateFlow<ChangePasswordUiState(newPassword, confirm, isLoading, error: Int?, done: Boolean)>`. Validation matches the server: length 8–128 (`auth_password_too_short` / `auth_password_too_long`) and the two fields must match (`auth_password_mismatch`). On success it navigates to `home` with `popUpTo(0)`.
- Decision: the temporary password the user just typed becomes `current_password`, so a non-technical user only enters the new password twice. It is held in memory only and cleared on success, cancel or process death; after process death the user simply logs in again.

- [ ] **Step 1: Generate the fixture** (requires server C4 on `main`). Run: `KAIKU_UPDATE_GOLDEN=1 SQLX_OFFLINE=true cargo test -p vc-server --test integration android_golden_auth_response_password_change`.
- [ ] **Step 2: Write the failing tests.**

```kotlin
@Test fun `login requiring password change persists no tokens`()     // tokenStorage.saveTokens never called; authState stays LoggedOut; no GET /auth/me
@Test fun `complete password change posts with bearer then logs in`() // request path /auth/me/password, header "Bearer <tmp access>", body has current_password=<typed>, then /auth/me, saveTokens, setLoggedIn
@Test fun `complete without pending state fails`()
@Test fun `cancel clears pending state`()
@Test fun `short password rejected`()                                // error == R.string.auth_password_too_short, no API call
@Test fun `mismatched confirmation rejected`()
@Test fun `login view model flags password change`()                 // LoginViewModelTest: uiState.passwordChangeRequired == true
@Test fun `password change fixture decodes with flag true`()
```

- [ ] **Step 3: Run them and confirm they fail.** Run: `./gradlew testDebugUnitTest --tests '*AuthRepositoryPasswordChangeTest' --tests '*ChangePasswordViewModelTest' --tests '*LoginViewModelTest' --tests '*GoldenContractTest'`.
- [ ] **Step 4: Implement** as specified. The screen copy is short and plain, for example "Choose your own password" with one sentence of explanation, and every string comes from resources.
- [ ] **Step 5: Run the tests and lint and confirm they pass.**
- [ ] **Step 6: Add a CHANGELOG entry and commit.** Changelog: "Android: accounts created by the server admin are asked to choose their own password on first sign-in." Commit: `feat(auth): Android forced password change on first login`.

---

## Maintainer device checklist (manual, after the tasks merge; local dev stack via `docker-compose.dev.yml` or a later athene test instance)

- [ ] Open a channel with more than 50 messages: the newest message is at the bottom, and "Load earlier" pages backwards (Task 1).
- [ ] Stay signed in for 20 minutes or more, and lock the phone overnight (doze): live messages still arrive after unlocking, with no app restart (Tasks 2 and 4).
- [ ] Switch Wi-Fi → LTE during chat: the banner shows "Reconnecting…" and then clears, and messages sent from desktop meanwhile appear (Tasks 4, 5 and 7).
- [ ] The desktop starts a new DM: it appears in the Android DM list without a refresh (Task 5, needs server C3).
- [ ] Android 14 device, first voice join: the permission prompt appears and no crash follows. Deny twice and the screen offers "Open settings" (Task 6).
- [ ] Audio follows the route the UI shows (speaker by default, a Bluetooth headset when connected), and PC players hear no echo (Task 6).
- [ ] A photo sent from desktop shows in Android chat, and avatars and server icons load (Task 8).
- [ ] A server with forum and announcement channels still lists its text and voice channels (Task 3).
- [ ] An admin-created account prompts for a new password on first sign-in and then reaches Home (Task 10, needs server C4).
- [ ] Logcat during all of the above shows no access tokens and no full message payloads.

## Out of scope (Phase 2 or later, per the review)

- Push and its parts: the `PushTransport` interface and `WakeFetchWorker` (WorkManager plus hilt-work), notification channels, and the post-login `POST_NOTIFICATIONS` prompt.
- Voice and session: voice surviving Back (A-07), logout teardown (A-14).
- Chat features: guild `/read` (A-13), upload (A-20), the remaining A-19 daily-chat defects, timestamps.
- First-run UX: A-10 and A-11.
- Disconnecting the socket in the background.
