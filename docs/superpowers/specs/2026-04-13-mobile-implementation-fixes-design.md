# Mobile Implementation Fixes & Web Responsive Overhaul

**Date:** 2026-04-13
**Status:** Draft
**Goal:** Address 30 issues from the mobile implementation review across Android auth/network, voice/WebRTC, UI/architecture, and web client responsive patterns. Includes a full responsive overhaul of the web client with drawer-based mobile navigation.

## Context

A comprehensive review of the mobile implementation (Android native app + web client responsive patterns) identified 30 actionable issues spanning security vulnerabilities, WebRTC lifecycle bugs, UI/architecture defects, and web responsive gaps. The issues range from critical security concerns (bearer token exposure, spoofable OIDC deep links) to important quality items (missing `remember`, touch target sizing). Two issues from the original review (#19 TokenStorage singleton, #22 ScreenShareView state) were verified as non-issues during spec review and dropped.

The Android app (`mobile/android/`) was introduced in PR #363 as Milestone 1 — Kotlin + Jetpack Compose with Hilt DI, Ktor HTTP, OkHttp WebSocket, and stream-webrtc-android. The web client (`client/`) is Solid.js + UnoCSS, desktop-first with minimal responsive design.

## Approach

Four independent sub-projects, each with its own branch and PR. No cross-branch dependencies — all four can be implemented and merged in parallel.

| Branch | Scope | Issues |
|--------|-------|--------|
| `fix/android-auth-network` | Security hardening + network robustness | 9 issues |
| `fix/android-voice-webrtc` | WebRTC lifecycle + voice service | 9 issues |
| `fix/android-ui-architecture` | Navigation, state management, accessibility | 7 issues |
| `feature/web-responsive` | Bug fixes + responsive overhaul with drawer nav | 5 issues + feature |

**Testing gate per branch:**
- Android branches: `./gradlew test` (unit tests) must pass + new tests for each fix using existing test patterns (mockk, Turbine, kotlinx.coroutines.test)
- Server changes in Sub-Project 1: `SQLX_OFFLINE=true cargo clippy -p vc-server -- -D warnings` + `cargo test -p vc-server`
- Web branch: `bun run test:run` + `bun run build` + manual viewport testing at 800px, 768px, 600px, and 375px widths

---

## Sub-Project 1: `fix/android-auth-network`

### Issue #1 — Bearer token in WebSocket upgrade header

**Problem:** `KaikuWebSocket.kt:145` sends the access token as `Sec-WebSocket-Protocol: access_token.$token`. This header is visible in proxy logs, server access logs, and network capture tools.

**Fix:** Remove the token from the handshake header. After `onOpen`, send an `Authenticate` client event as the first WebSocket frame. The server holds or rejects other events until authentication completes.

**Server-side change required:** Add an `Authenticate` variant to `ClientEvent` in `server/src/ws/mod.rs`. Add a pre-auth state to the WebSocket handler that buffers events until the token is validated. Add an auth timeout (5s) that closes the connection if no `Authenticate` frame arrives. This is ~100-150 lines including the state machine, timeout, and tests.

**Migration strategy:** The `Sec-WebSocket-Protocol` auth pattern is also used by the Tauri desktop client (`client/src-tauri/src/network/websocket.rs:503`) and the web client (`client/src/lib/tauri/websocket.ts:44`). To avoid a breaking change:
1. The server accepts both old (`Sec-WebSocket-Protocol` header) and new (post-connect `Authenticate` frame) auth for one release cycle.
2. This PR updates the Android client to the new auth pattern.
3. A follow-up PR updates the Tauri/web client and removes legacy header auth from the server.

**Files:** `KaikuWebSocket.kt`, `server/src/ws/mod.rs`, `server/src/ws/handlers.rs`

### Issue #2 — No TLS 1.3 enforcement

**Problem:** Neither the WebSocket `OkHttpClient` (`WebSocketModule.kt:28`) nor the Ktor HTTP engine (`KaikuHttpClient.kt:68`) specifies a `ConnectionSpec`. OkHttp defaults to TLS 1.2 on older API levels, violating the CLAUDE.md requirement of TLS 1.3 for all connections.

**Fix:** Create a shared `ConnectionSpec` in `NetworkModule`:

```kotlin
val tls13Spec = ConnectionSpec.Builder(ConnectionSpec.MODERN_TLS)
    .tlsVersions(TlsVersion.TLS_1_3)
    .build()
```

Apply to both the WebSocket `OkHttpClient` and the Ktor `OkHttp` engine. Add `ProviderInstaller.installIfNeeded(context)` in `Application.onCreate()` to ensure the security provider is current on API 26-28 devices (TLS 1.3 is native on API 29+).

**Files:** `NetworkModule.kt`, `WebSocketModule.kt`, `KaikuApplication.kt`

### Issue #3 — OIDC deep link spoofable + no PKCE

**Problem:** `kaiku://auth/callback` is a custom URI scheme that any app can intercept or spoof. The primary token path in `OidcHandler.kt:79` accepts tokens directly from the redirect URI without verifying that the redirect originated from a login the app actually initiated (no `state` nonce). The authorization code path does validate `state`, but no PKCE (`code_verifier`/`code_challenge`) is used in either path, leaving the code exchange vulnerable to interception.

**Fix (two parts):**

*Part A — PKCE + state nonce:*
- In `launchOidcLogin()`: generate 32-byte `code_verifier` (compute SHA-256 `code_challenge`) and 32-byte `state` nonce. Persist both to `EncryptedSharedPreferences`.
- Add `code_challenge`, `code_challenge_method=S256`, and `state` to the authorization URL.
- In `handleCallback()`: validate `state` matches stored nonce before processing. Include `code_verifier` in the token exchange. Clear stored values after success.

*Part B — Android App Links:*
- Add intent filter for `https://kaiku.pmind.de/auth/callback` with `autoVerify="true"` in `AndroidManifest.xml`.
- Keep existing `kaiku://` scheme as fallback.
- Server-side: deploy `/.well-known/assetlinks.json` via Caddy with the app's signing certificate fingerprint (separate deployment task).

**Files:** `OidcHandler.kt`, `AndroidManifest.xml`, server-side `assetlinks.json`

### Issue #4 — Tokens persisted before `getMe` succeeds

**Problem:** `AuthRepository.kt:30` calls `tokenStorage.saveTokens()` with `userId = ""` before calling `getMe()`. If the process is killed between these calls, an orphaned valid token is persisted without a user ID.

**Fix:** Hold tokens in local variables during the login/register/OIDC sequence. Only call `tokenStorage.saveTokens(accessToken, refreshToken, userId)` after `getMe()` succeeds and returns the real `userId`.

**Files:** `AuthRepository.kt`

### Issue #15 — ConnectivityMonitor false positives on captive portals

**Problem:** `ConnectivityMonitor.kt:44` checks `NET_CAPABILITY_INTERNET` without `NET_CAPABILITY_VALIDATED`. Captive-portal networks report connected, triggering WebSocket reconnect storms against an unreachable server.

**Fix:** Require `NET_CAPABILITY_VALIDATED` in addition to `NET_CAPABILITY_INTERNET` in both `checkCurrentConnectivity()` and `onCapabilitiesChanged`.

**Files:** `ConnectivityMonitor.kt`

### Issue #16 — Stale token on WebSocket reconnect

**Problem:** `KaikuWebSocket.kt:129` reads the access token at reconnect time without checking expiry. After backoff delay, the token may have expired, causing a 401 reject loop.

**Fix:** In `doConnect()`, check `tokenStorage.isAccessTokenExpired()` before building the request. If expired, emit a new `ConnectionState.TokenExpired` event that repositories can observe to trigger a token refresh before retrying.

**Files:** `KaikuWebSocket.kt`

### Issue #17 — WebSocket connectivity collector leaks

**Problem:** `KaikuWebSocket.kt:57` launches a connectivity collector via `scope.launch` without tracking the returned `Job`. Calling `setConnectivityMonitor` again launches a duplicate collector.

**Fix:** Store the `Job` as a field. Cancel it in `disconnect()` and before launching a new one in `setConnectivityMonitor`.

**Files:** `KaikuWebSocket.kt`

### Issue #18 — ConnectivityMonitor callback never unregistered

**Problem:** `ConnectivityMonitor.kt:53` registers a `NetworkCallback` in `init` with no `unregisterNetworkCallback` call. Prevents clean test teardown.

**Fix:** Implement `Closeable`. Add `fun close()` that calls `connectivityManager.unregisterNetworkCallback(networkCallback)`.

**Files:** `ConnectivityMonitor.kt`

### Issue #27 — Legacy BLUETOOTH permission not scoped

**Problem:** `AndroidManifest.xml:7` declares `android.permission.BLUETOOTH` without `maxSdkVersion="30"`. The legacy permission is unnecessary on API 31+ where `BLUETOOTH_CONNECT` (already declared) applies.

**Fix:** Add `android:maxSdkVersion="30"` to the `BLUETOOTH` permission.

**Files:** `AndroidManifest.xml`

---

## Sub-Project 2: `fix/android-voice-webrtc`

### Issue #5 — SDP answer sent before `setLocalDescription` completes

**Problem:** `WebRtcManager.kt:222` invokes `onLocalDescription` immediately after enqueuing `setLocalDescription`, which is asynchronous. The SDP answer is sent over WebSocket before the local description is committed.

**Fix:** The current code passes a no-op `SdpObserverAdapter` to `setLocalDescription`. Replace it with an inline `object` that overrides `onSetSuccess` and invokes `onLocalDescription` from there:

```kotlin
pc.setLocalDescription(object : SdpObserverAdapter("setLocalDescription", onError) {
    override fun onSetSuccess() {
        super.onSetSuccess()
        onLocalDescription?.invoke(desc.description)
    }
}, desc)
```

Remove the `onLocalDescription` call that currently follows the `setLocalDescription` enqueue.

**Files:** `WebRtcManager.kt`

### Issue #6 — Static callbacks on VoiceCallService

**Problem:** `VoiceCallService.kt:44` uses `companion object` `var` fields for mute/disconnect callbacks. `VoiceRepository.kt:133` writes lambdas capturing `this` into these fields. Race condition between notification action intents and `cleanUp()`.

**Fix:** Replace static callbacks with a `SharedFlow`-based event pattern.

Introduce `VoiceServiceEvents` as a `@Singleton`:

```kotlin
@Singleton
class VoiceServiceEvents @Inject constructor() {
    private val _events = MutableSharedFlow<VoiceServiceEvent>(extraBufferCapacity = 5)
    val events: SharedFlow<VoiceServiceEvent> = _events.asSharedFlow()
    fun emit(event: VoiceServiceEvent) { _events.tryEmit(event) }
}

sealed class VoiceServiceEvent {
    data object MuteToggle : VoiceServiceEvent()
    data object Disconnect : VoiceServiceEvent()
}
```

`VoiceCallService` is annotated `@AndroidEntryPoint` (prerequisite: `KaikuApplication` has `@HiltAndroidApp`, which is confirmed). The service must call `super.onCreate()` before accessing injected fields. It injects `VoiceServiceEvents` and emits events in `onStartCommand`. `VoiceRepository` injects `VoiceServiceEvents` and collects events, launching a collector in `joinChannel()` and cancelling it in `cleanUp()`.

**Files:** `VoiceCallService.kt`, `VoiceRepository.kt`, new `VoiceServiceEvents.kt`

### Issue #10 — ICE candidates added before remote description set

**Problem:** `VoiceRepository.kt:285` calls `addIceCandidate` as soon as events arrive. If the server sends candidates before `setRemoteDescription` completes, they are silently dropped.

**Fix:** Add `remoteDescriptionSet: Boolean` flag and `pendingCandidates: MutableList<IceCandidate>` to `WebRtcManager`. Buffer candidates when the flag is false. Drain in `setRemoteDescription`'s `onSetSuccess`. Reset both in `closePeerConnection()`.

**Files:** `WebRtcManager.kt`

### Issue #12 — JavaAudioDeviceModule never released

**Problem:** `WebRtcManager.kt:115` creates a `JavaAudioDeviceModule` as a local variable. The module is handed to the factory but never stored, so `release()` is never called.

**Fix:** Store as `private var audioDeviceModule: AudioDeviceModule? = null`. Release in `dispose()` after `factory?.dispose()`.

**Files:** `WebRtcManager.kt`

### Issue #13 — PeerConnection.dispose() missing

**Problem:** `WebRtcManager.kt:172` calls `peerConnection?.close()` but not `dispose()`. Native C++ resources are not freed, leaking across reconnects.

**Fix:** Add `peerConnection?.dispose()` between `close()` and nulling the reference.

**Files:** `WebRtcManager.kt`

### Issue #14 — leaveChannel called twice without guard

**Problem:** `VoiceViewModel.kt:80` calls `leaveChannel()` from `onLeave()`, and `onCleared()` calls it again via `NonCancellable`. Both can run concurrently with no synchronization.

**Fix:** Add a `Mutex` to `VoiceRepository.leaveChannel()`. Early-return inside `withLock` if `_currentChannelId.value` is null.

```kotlin
private val leaveMutex = Mutex()

suspend fun leaveChannel() {
    leaveMutex.withLock {
        val channelId = _currentChannelId.value ?: return@withLock
        // ... existing cleanup
    }
}
```

**Note:** `VoiceViewModel.onCleared()` currently uses `viewModelScope.launch(NonCancellable)`, but `viewModelScope` is already cancelled at that point. The coroutine runs but is untracked. The Mutex fix makes the double-call safe regardless of which scope runs it. No change to `onCleared` needed — the Mutex handles the concurrency.

**Files:** `VoiceRepository.kt`

### Issue #21 — Bluetooth SCO set synchronously

**Problem:** `AudioRouteManager.kt:208` sets `isBluetoothScoOn = true` immediately after `startBluetoothSco()`, which is asynchronous. The UI shows "Bluetooth" while audio is still on the earpiece.

**Fix:** Register a `BroadcastReceiver` for `ACTION_SCO_AUDIO_STATE_UPDATED`. Only update `_currentRoute` to `AudioRoute.Bluetooth` when `SCO_AUDIO_STATE_CONNECTED` is received. Fall back to speaker on `SCO_AUDIO_STATE_ERROR` or 3-second timeout. Unregister receiver in `release()`. Also update `switchRoute()` to defer `_currentRoute.value = route` for the Bluetooth case — the route should not be updated synchronously when switching to Bluetooth, only after the BroadcastReceiver confirms SCO is connected.

**Files:** `AudioRouteManager.kt`

### Issue #23 — findVideoTrackForStream fallback mismatches

**Problem:** `VoiceChannelScreen.kt:345` has a `if (remoteVideoTracks.size == 1) return firstOrNull()` fallback that returns the wrong track in multi-peer screen share scenarios.

**Fix:** Remove the size-1 fallback. Return only tracks matching the requested `streamId`. The caller already handles null.

**Files:** `VoiceChannelScreen.kt`

### Issue #32 — VoiceRepository scope cancellation

**Problem:** `VoiceRepository.kt:50` creates `CoroutineScope(SupervisorJob() + Dispatchers.IO)` with no cancellation mechanism. Test teardown leaks.

**Fix:** Implement `Closeable` with `scope.cancel()`. Ensure `cleanUp()` cancels `eventCollectionJob`.

**Files:** `VoiceRepository.kt`

---

## Sub-Project 3: `fix/android-ui-architecture`

### Issue #7 — channelName displays raw UUID

**Problem:** `KaikuNavGraph.kt:116` passes `channelId` as `channelName` to `TextChannelScreen` and `VoiceChannelScreen`. The TopAppBar displays a UUID.

**Fix:** Resolve the channel name in the ViewModel. Inject `GuildRepository` into `TextChannelViewModel`. Derive a `channelName: StateFlow<String>` from `guildRepository.channels`:

```kotlin
val channelName: StateFlow<String> = guildRepository.channels
    .map { channels -> channels.find { it.id == channelId }?.name ?: channelId }
    .stateIn(viewModelScope, SharingStarted.WhileSubscribed(5000), channelId)
```

Remove `channelName` parameter from `TextChannelScreen` — the screen reads from its ViewModel. Same approach for `VoiceChannelScreen` via `VoiceViewModel`. Fallback to `channelId` while channels load.

**Files:** `KaikuNavGraph.kt`, `TextChannelViewModel.kt`, `TextChannelScreen.kt`, `VoiceViewModel.kt`, `VoiceChannelScreen.kt`

### Issue #8 — Navigation SharedFlow drops events

**Problem:** `HomeViewModel.kt:50` uses `MutableSharedFlow(extraBufferCapacity = 1)`. `tryEmit` on a full buffer silently drops the event.

**Fix:** Replace with `Channel<ChannelNavEvent>(Channel.BUFFERED)` consumed via `receiveAsFlow()`. Producer uses `trySend`. Apply same pattern to `_oidcCallbackUri` in `LoginViewModel.kt`.

**Files:** `HomeViewModel.kt`, `LoginViewModel.kt`

### Issue #9 — Auto-scroll fires on pagination prepend

**Problem:** `TextChannelScreen.kt:41` scrolls to the bottom on every `messages.size` change, including when older messages are prepended via pagination.

**Fix:** Key `LaunchedEffect` on `messages.lastOrNull()?.id` instead of `messages.size`. Only auto-scroll when the last message ID changes (new message at the bottom) and the user is within 3 items of the bottom:

```kotlin
var lastMessageId by remember { mutableStateOf<String?>(null) }

LaunchedEffect(messages.lastOrNull()?.id) {
    val newLastId = messages.lastOrNull()?.id
    if (newLastId != null && newLastId != lastMessageId) {
        lastMessageId = newLastId
        val lastVisible = listState.layoutInfo.visibleItemsInfo.lastOrNull()?.index ?: 0
        if (lastVisible >= messages.size - 4) {
            listState.animateScrollToItem(messages.size - 1)
        }
    }
}
```

**Files:** `TextChannelScreen.kt`

### Issue #20 — AuthState anonymous CoroutineScopes

**Problem:** `AuthState.kt:25` creates two inline `CoroutineScope(Dispatchers.Default)` instances for `stateIn` calls. Neither is stored or cancellable.

**Fix:** Declare a single `private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Default)`. Reuse for both `stateIn` calls. Implement `Closeable` with `scope.cancel()` for test teardown.

**Files:** `AuthState.kt`

### Issue #24 — GuildIcon missing button semantics

**Problem:** `GuildSidebar.kt:68` uses `.clickable(onClick)` without `Role.Button` semantics. TalkBack does not announce the element as interactive.

**Fix:** Add `.semantics { role = Role.Button }` and `onClickLabel = "Select ${guild.name}"` to the clickable modifier.

**Files:** `GuildSidebar.kt`

### Issue #25 — formatTimestamp not remembered

**Problem:** `MessageItem.kt:91` calls `formatTimestamp(message.createdAt)` inline. Recomputed on every recomposition for every visible message.

**Fix:** Wrap in `remember(message.createdAt) { formatTimestamp(message.createdAt) }`.

**Files:** `MessageItem.kt`

### Issue #26 — SettingsViewModel.logout passes UI callback

**Problem:** `SettingsViewModel.kt:65` accepts `onLogoutComplete: () -> Unit` from the composable. The lambda captures stale navigation state during configuration changes.

**Fix:** Replace with a `Channel<Unit>(Channel.CONFLATED)` exposed as `logoutComplete: Flow<Unit>`. The composable collects via `LaunchedEffect`.

**Files:** `SettingsViewModel.kt`, `SettingsScreen.kt`

---

## Sub-Project 4: `feature/web-responsive`

### New utilities

**`client/src/lib/useBreakpoint.ts`** — Reactive breakpoint hook using `window.matchMedia`. Returns a Solid.js signal that updates when the viewport crosses the threshold. Exports `useIsMobile()` convenience (`max-width: 767px`).

**`client/src/lib/createLongPress.ts`** — Long-press directive for touch support. Uses `PointerEvent` for unified mouse/touch/pen input. Returns event handlers that call sites spread onto elements. ~30 lines.

### Bug fixes

**#11 — GuildSettingsModal overflow at 800px.** Change `w-[90vw] md:w-[900px] max-w-5xl` to `w-[90vw] max-w-[900px]`. Remove the `md:` breakpoint override. Add `overflow-x-hidden` safety net. File: `GuildSettingsModal.tsx`

**#28 — ContextMenu touch support.** Add `showContextMenuAt(x, y, items)` overload. Call sites spread `createLongPress` handlers alongside `onContextMenu`. Increase item padding from `py-1.5` to `py-2.5` for touch targets. Increase GuildSettingsModal close button from `p-1.5` to `p-2.5`. File: `ContextMenu.tsx`

**#29 — `text-text-secondary/50` opacity violations.** This pattern appears in 13 locations across the codebase, all violating the CLAUDE.md rule against `opacity-*` below 50% on readable text. Replace all occurrences of `text-text-secondary/50` with `text-text-muted` (the semantic token for minimum-readable text). For `placeholder:text-text-secondary/50` occurrences, replace with `placeholder:text-text-muted`. Affected files: `Sidebar.tsx` (2), `HomeSidebar.tsx` (1), `VoicePanel.tsx` (1), `SearchPanel.tsx` (1), `ReportsPanel.tsx` (1), `CommandCenterPanel.tsx` (3), `AuditLogPanel.tsx` (2), `ReportModal.tsx` (2).

**#30 — Register form scroll.** Change `min-h-screen` to `min-h-screen overflow-y-auto`. Apply to `Login.tsx`, `Register.tsx`, `ForgotPassword.tsx`, `ResetPassword.tsx`. Files: 4 view files.

**#31 — Emoji delete hover-only.** Add a `touch:` custom variant to `uno.config.ts` mapping to `@media (hover: none)`. Apply `touch:opacity-60` to the delete button so it is visible on touch devices. File: `EmojisTab.tsx`, `uno.config.ts`

### Responsive overhaul

**Breakpoint:** `md` (768px). Below: mobile layout with drawer. Above: current desktop layout.

**New component: `MobileDrawer.tsx`**
- Slide-out drawer from the left edge, `w-[300px]`, `z-50`
- Contains `ServerRail` (56px compact mode) + `Sidebar` (flex-1) side by side
- Backdrop: `bg-black/50`, tap to close
- CSS transform transition: `translateX(-100%) -> translateX(0)`
- Swipe-right to open: `PointerEvent` listener on left 20px edge, >50px rightward movement
- Swipe-left to close: >50px leftward movement on the drawer panel
- Auto-close on channel selection via `onNavigate` callback

**New component: `MobileHeader.tsx`**
- Top bar, `h-[44px]`, visible only on mobile
- Hamburger icon (left) opens drawer
- Guild name + `#channel` name (center) from existing stores
- `bg-surface-layer1`, `border-b border-border-default`

**Modified: `AppShell.tsx`**
- Use `useIsMobile()` to toggle between desktop layout (existing) and mobile layout (header + drawer)
- Desktop: existing `ServerRail` + `Sidebar` + main stage
- Mobile: `MobileDrawer` (contains `ServerRail compact` + `Sidebar`) + `MobileHeader` + main stage

**Modified: `ServerRail.tsx`**
- Add `compact` boolean prop (default: `false`)
- When `compact={true}`: `w-[56px]`, smaller guild icons, tighter padding (used in MobileDrawer)
- When `compact={false}` or omitted: existing `w-[72px]` behavior (existing call sites unchanged)

**Sidebar.tsx** — No structural changes. Inside the drawer it receives `flex-1` from the drawer layout, filling `244px` (close to its designed `240px`).

**HomeRightPanel** — Already `hidden xl:flex`. Always hidden on mobile. No changes needed.
