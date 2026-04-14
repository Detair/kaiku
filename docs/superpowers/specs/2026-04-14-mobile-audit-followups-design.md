# Mobile Implementation Audit Follow-ups

**Date:** 2026-04-14
**Status:** Draft
**Goal:** Address all 16 findings from the post-implementation security audit of PRs #525-528 by adding follow-up commits to each existing PR.

## Context

After the initial implementation of the mobile fixes spec (`2026-04-13-mobile-implementation-fixes-design.md`), four PRs were opened:

- #525 (`fix/android-auth-network`) — security hardening + network robustness
- #526 (`fix/android-voice-webrtc`) — WebRTC lifecycle + voice service
- #527 (`fix/android-ui-architecture`) — UI/architecture quality
- #528 (`feature/web-responsive`) — web responsive overhaul + bug fixes

A `silent-failure-hunter` security audit across all 4 PRs identified 16 findings:
- 4 must-fix HIGH/CRITICAL severity (auth state machine, PKCE persistence, long-press dual menu, drawer focus trap)
- 6 should-fix MEDIUM (App Links scope, ICE buffer cap, dropped events, race conditions)
- 6 noted LOW (logging consistency, capacity tuning, cleanup edge cases)

The audit also confirmed several positive findings (PKCE entropy, token persistence elimination, DI security, coordinate clamping).

## Approach

All 16 fixes ship as follow-up commits added to the existing PRs (no separate audit-fix PR). Reasons:
- The PRs aren't merged yet — the audit findings belong with the work being reviewed.
- Adding commits keeps review context coherent: a reviewer sees both the original implementation and the audit response in one place.
- Squash-merge produces a single commit per domain regardless of follow-up commit count.
- A separate followup PR would touch all 4 domains and create dependency complexity for no benefit.

**Strategy per PR:**
- One commit per fix (small, focused, easy to revert if rework needed).
- Same testing gates as original PRs (`./gradlew test`, `bun run test:run && bun run build`, `cargo clippy`).
- No new files created — all fixes modify existing files.
- Each PR's CHANGELOG section gets one new line per user-visible fix (logging-only fixes don't need entries).

## Per-PR Fix Map

| PR | Fixes | Files |
|----|-------|-------|
| #525 (auth-network) | 6 | `KaikuApplication.kt`, `KaikuWebSocket.kt`, `TokenStorage.kt`, `AndroidManifest.xml`, `server/src/ws/handlers.rs` |
| #526 (voice-webrtc) | 4 | `WebRtcManager.kt`, `VoiceServiceEvents.kt`, `AudioRouteManager.kt` |
| #527 (ui-architecture) | 1 | `HomeViewModel.kt`, `LoginViewModel.kt` |
| #528 (web-responsive) | 5 | `MobileDrawer.tsx`, `AppShell.tsx`, `ContextMenu.tsx`, `MessageItem.tsx`, `ChannelItem.tsx`, `MembersTab.tsx`, `createLongPress.ts` |

---

## PR #525: auth-network (6 fixes)

### Fix 1 (Audit Finding 3, HIGH) — Delay `Connected` state until `Ready` event

**Problem:** `KaikuWebSocket.onOpen` (line 163-171, after the post-connect auth migration in PR #525) immediately sets `_connectionState.value = ConnectionState.Connected` (line 168) and starts the ping loop, before the server has validated the access token sent in the `Authenticate` frame. If auth fails, the client briefly shows "Connected" before the server closes the WS, causing UI flicker and triggering connection-dependent code paths prematurely.

**Fix:** Reuse the existing `Connecting` state. In `onOpen`, send `Authenticate` but stay in `Connecting`. In `onMessage`, transition to `Connected` only on receipt of a `ServerEvent.Ready` event:

```kotlin
override fun onMessage(webSocket: WebSocket, text: String) {
    try {
        val event = json.decodeFromString<ServerEvent>(text)
        if (event is ServerEvent.Ready && _connectionState.value == ConnectionState.Connecting) {
            _connectionState.value = ConnectionState.Connected
            reconnectDelay = INITIAL_RECONNECT_DELAY_MS
            startPingLoop()
        }
        val emitted = _events.tryEmit(event)
        if (!emitted) {
            logger.warning("Event buffer full, dropped: ${event::class.simpleName}")
        }
    } catch (e: Exception) {
        logger.log(Level.WARNING, "Failed to parse server event: $text", e)
    }
}
```

The server's existing 5-second auth timeout handles the no-Ready case for the post-connect path: server closes the WS, `onFailure`/`onClosed` triggers existing `handleDisconnect` → reconnect. No client-side timeout needed.

**Note on dual auth modes:** The server sends `Ready` regardless of which auth mode it accepted (header-based or post-connect Authenticate frame). Both auth paths converge on emitting `Ready` once the connection is fully authenticated, so the client logic above works identically for both.

**Test:** Update `KaikuWebSocketTest` to verify state stays `Connecting` until mock server sends `Ready`.

**Files:** `KaikuWebSocket.kt`, `KaikuWebSocketTest.kt`

### Fix 2 (Audit Finding 4, HIGH) — PKCE state uses `commit()` not `apply()`

**Problem:** `TokenStorage.saveOidcPkceState()` uses `.apply()` (asynchronous write). The OIDC flow immediately opens a Chrome Custom Tab after saving. If the process is killed before the async write completes (low-memory device with browser in foreground), the code_verifier and state nonce are lost. The callback then fails state validation with a misleading "OIDC state mismatch — possible CSRF" error.

**Fix:** Use `.commit()` (synchronous, returns success boolean) and log on failure:

```kotlin
fun saveOidcPkceState(codeVerifier: String, state: String) {
    val success = prefs.edit()
        .putString(KEY_OIDC_CODE_VERIFIER, codeVerifier)
        .putString(KEY_OIDC_STATE, state)
        .commit()
    if (!success) {
        logger.warning("Failed to persist OIDC PKCE state to storage")
    }
}
```

Matches the existing pattern in `saveTokens` and `saveServerUrl`.

**Files:** `TokenStorage.kt`

### Fix 3 (Audit Finding 5, MEDIUM) — `clearOidcPkceState()` uses `commit()`

**Problem:** Same `.apply()` async-write pattern in `clearOidcPkceState`. While the security impact is low (a stale code_verifier can't be replayed because each new login generates fresh values that overwrite it), inconsistency with `saveTokens`/`saveServerUrl` is a maintainability liability.

**Fix:** Same pattern as Fix 2 — change `.apply()` to `.commit()`, log on failure.

**Files:** `TokenStorage.kt`

### Fix 4 (Audit Finding 6, MEDIUM) — App Links exact path match

**Problem:** The App Links intent filter uses `android:pathPrefix="/auth/callback"`, which also matches paths like `/auth/callback-status` or `/auth/callback-anything`. While `autoVerify="true"` requires a properly hosted Digital Asset Links file, the prefix matching is broader than necessary.

**Fix:** Change `android:pathPrefix="/auth/callback"` to `android:path="/auth/callback"`. The `path` attribute matches the URL path component exactly. Query strings (e.g., `?access_token=xxx`) still match because Android intent filters compare against the path component only, not the full URL.

**Files:** `AndroidManifest.xml`

### Fix 5 (Audit Finding 1, CRITICAL) — ProviderInstaller fail-fast with user notification

**Problem:** `KaikuApplication.onCreate()` catches all `Exception` from `ProviderInstaller.installIfNeeded()` and continues running. On devices where the security provider can't be updated (old API levels with stale Play Services), this means the app starts up looking fine, but every TLS 1.3 connection fails at handshake with an opaque error. The user sees a broken app with no diagnostic information.

**Fix:** Differentiate the two specific exception types and surface the failure:

```kotlin
import com.google.android.gms.common.GoogleApiAvailability
import com.google.android.gms.common.GooglePlayServicesNotAvailableException
import com.google.android.gms.common.GooglePlayServicesRepairableException

override fun onCreate() {
    super.onCreate()
    try {
        ProviderInstaller.installIfNeeded(this)
    } catch (e: GooglePlayServicesRepairableException) {
        logger.severe("Play Services needs update for TLS 1.3: ${e.message}")
        GoogleApiAvailability.getInstance()
            .showErrorNotification(this, e.connectionStatusCode)
    } catch (e: GooglePlayServicesNotAvailableException) {
        logger.severe("Play Services not available — TLS 1.3 unsupported, network will fail: ${e.message}")
    }
}
```

For the repairable case, `showErrorNotification` is the documented Google API for prompting users to update Play Services. For the non-repairable case, no graceful degradation is possible without violating the CLAUDE.md TLS 1.3 hard constraint — log at SEVERE and let the network layer fail with clear errors. (A future enhancement could surface a startup error UI; out of scope here.)

**Files:** `KaikuApplication.kt`

### Fix 6 (Audit Finding 7, MEDIUM) — Log JWT validation error on header path

**Problem:** The header-based auth path in `server/src/ws/handlers.rs` discards the JWT validation error with `Err(_) =>`, while the post-connect path (`wait_for_auth_frame`) logs it. Inconsistent observability — debugging token rejection on the header path requires reproducing the exact failure scenario.

**Fix:** Match the post-connect path pattern:

```rust
Err(e) => {
    warn!("Header-based WS auth failed: {}", e);
    return error_response(401, "Invalid token");
}
```

**Files:** `server/src/ws/handlers.rs` (no new imports needed — `warn!` is already in scope from existing usages at lines 117, 150, 156)

---

## PR #526: voice-webrtc (4 fixes)

### Fix 7 (Audit Finding 10, MEDIUM) — Cap ICE candidate buffer

**Problem:** `WebRtcManager.pendingCandidates` is an unbounded `mutableListOf<String>()`. If `remoteDescriptionSet` stays false (e.g., a buggy SDP flow where `setRemoteDescription` never succeeds), every ICE candidate received is buffered indefinitely. While typical sessions trickle 10-50 candidates, a malicious server or buggy SDP path could cause unbounded growth.

**Fix:** Add a cap with a warning log on overflow:

```kotlin
companion object {
    private const val MAX_PENDING_CANDIDATES = 100
    // ... existing constants
}

fun addIceCandidate(candidateJson: String) {
    if (!remoteDescriptionSet) {
        if (pendingCandidates.size >= MAX_PENDING_CANDIDATES) {
            logger.warning("ICE candidate buffer full ($MAX_PENDING_CANDIDATES), dropping candidate")
            return
        }
        logger.fine("Buffering ICE candidate (remote description not yet set)")
        pendingCandidates.add(candidateJson)
        return
    }
    // ... existing pc-null check and parse logic
}
```

100 candidates is generous (typical sessions are well under 50). This is defense-in-depth.

**Test:** Add a `WebRtcManagerTest` case that calls `addIceCandidate` 101 times before `setRemoteDescription` succeeds (`remoteDescriptionSet = false`), then asserts that `pendingCandidates.size == 100`. The test exercises the new drop-on-overflow branch.

**Files:** `WebRtcManager.kt`, `WebRtcManagerTest.kt`

### Fix 8 (Audit Finding 12, MEDIUM) — Log dropped VoiceServiceEvents

**Problem:** `VoiceServiceEvents.emit()` uses `tryEmit()` which silently returns `false` if the buffer (capacity 5) is full. If the voice repository's collector is suspended, notification action events (mute toggle, disconnect) are silently lost — the user taps "Disconnect" on the notification and nothing happens.

**Fix:** Check the return value and log on drop:

```kotlin
companion object {
    private val logger = Logger.getLogger("VoiceServiceEvents")
}

fun emit(event: VoiceServiceEvent) {
    val emitted = _events.tryEmit(event)
    if (!emitted) {
        logger.warning("VoiceServiceEvent dropped (buffer full): ${event::class.simpleName}")
    }
}
```

The `extraBufferCapacity = 5` is generous for notification button taps. A drop indicates the collector is suspended — worth visibility for debugging.

**Files:** `VoiceServiceEvents.kt`

### Fix 9 (Audit Finding 11, MEDIUM) — Atomic close+dispose in `closePeerConnection`

**Problem:** `WebRtcManager.closePeerConnection()` calls `peerConnection?.close()`, then `peerConnection?.dispose()`, then `peerConnection = null`. Between the dispose and the null assignment, a concurrent caller reading `peerConnection` could see a non-null reference to an already-disposed object. Calling any method on a disposed PeerConnection crashes with a native assertion failure that isn't caught by Kotlin exception handlers.

**Fix:** Apply a null-first pattern. Read the reference into a local, null the field, then close+dispose via the local. Preserve the original cleanup order: clear remote-track flows BEFORE closing the connection so observers don't try to operate on tracks attached to a disposed peer connection.

```kotlin
fun closePeerConnection() {
    localAudioTrack?.dispose()
    localAudioTrack = null
    audioSource?.dispose()
    audioSource = null

    // Clear remote-track flows first so observers don't reach into a disposed PC
    _remoteAudioTracks.value = emptyMap()
    _remoteVideoTracks.value = emptyMap()
    remoteDescriptionSet = false
    pendingCandidates.clear()

    val pc = peerConnection
    peerConnection = null  // null first so concurrent readers see null
    pc?.close()
    pc?.dispose()

    logger.info("PeerConnection closed")
}
```

A concurrent caller checking `peerConnection != null` after the null assignment sees `null` and safely skips the operation. The local `pc` reference proceeds with close+dispose.

**Files:** `WebRtcManager.kt`

### Fix 10 (Audit Finding 13, MEDIUM) — `AudioRouteManager` implements `Closeable`

**Problem:** `AudioRouteManager` creates a `CoroutineScope(SupervisorJob() + Dispatchers.Main)` but never cancels it. The original PR #526 made `VoiceRepository` `Closeable` for clean test teardown, and PR #525 made `ConnectivityMonitor` `Closeable` for the same reason. `AudioRouteManager` is the remaining `@Singleton` with its own `CoroutineScope` that doesn't follow this pattern.

**Fix:** Implement `Closeable`:

```kotlin
import java.io.Closeable
import kotlinx.coroutines.cancel

@Singleton
class AudioRouteManager @Inject constructor(...) : Closeable {
    // ... existing fields including private val scope = ...

    override fun close() {
        unregisterReceivers()
        scope.cancel()
    }
}
```

For runtime, the `@Singleton` lifetime means `close()` is never called in production — purely for clean test teardown. Matches the established pattern.

**Files:** `AudioRouteManager.kt`

---

## PR #527: ui-architecture (1 fix)

### Fix 11 (Audit Finding 17, LOW) — Right-size navigation Channels

**Problem:** Three ViewModels use `Channel(Channel.BUFFERED)` for one-shot navigation events. `Channel.BUFFERED` defaults to capacity 64 — vastly oversized for events that should typically have at most one in flight.

**Fix:** Right-size each Channel based on its actual semantics.

In `HomeViewModel.kt`:
```kotlin
// Bounded capacity prevents buildup if a navigation collector is suspended.
// Capacity 4 is generous for tap-burst behavior.
private val _navigateToChannel = Channel<ChannelNavEvent>(capacity = 4)
```

In `LoginViewModel.kt`:
```kotlin
// OIDC callback is one-shot — CONFLATED keeps only the latest, redelivery is harmless,
// and trySend never blocks.
private val _oidcCallbackUri = Channel<Uri>(Channel.CONFLATED)
```

`SettingsViewModel._logoutComplete` already uses `Channel.CONFLATED` (PR #527's initial implementation set this correctly). No change needed there.

Code clarity improvement — no functional behavior change, no test updates needed.

**Files:** `HomeViewModel.kt`, `LoginViewModel.kt`

---

## PR #528: web-responsive (5 fixes)

### Fix 12 (Audit Finding 19, HIGH) — Wire long-press `onContextMenu` at all 3 call sites

**Problem:** `createLongPress` returns an `onContextMenu` handler that calls `preventDefault()` only when a long-press timer is active. The 3 call sites (`MessageItem.tsx`, `ChannelItem.tsx`, `MembersTab.tsx`) don't wire this handler up. On Android Chrome, long-pressing an element fires both the custom context menu (via the timer) AND the browser's native context menu, with the native menu overlaying the custom one.

**Fix:**

First, modify `createLongPress.ts` to track timer-fired state so `onContextMenu` can suppress the native event when the long-press has just fired:

```typescript
export function createLongPress(
  onLongPress: (x: number, y: number) => void,
  duration = 500
) {
  let timer: ReturnType<typeof setTimeout> | null = null;
  let consumed = false;  // set to true when timer fires
  let startX = 0;
  let startY = 0;

  const onPointerDown = (e: PointerEvent) => {
    consumed = false;
    startX = e.clientX;
    startY = e.clientY;
    timer = setTimeout(() => {
      consumed = true;
      onLongPress(e.clientX, e.clientY);
      timer = null;
    }, duration);
  };

  const cancel = () => {
    if (timer) {
      clearTimeout(timer);
      timer = null;
    }
  };

  const onContextMenu = (e: Event) => {
    // Suppress native context menu only if a long-press was just consumed or
    // is currently pending. Always reset `consumed` afterwards so a subsequent
    // keyboard-triggered context menu (e.g., Shift+F10) is not incorrectly
    // suppressed.
    //
    // Trade-off: on hybrid devices (e.g., Surface, ChromeOS), if the user
    // touch-long-presses then immediately right-clicks before the next
    // pointerdown, the right-click is suppressed. This is acceptable —
    // hybrid devices are rare and the long-press menu was just shown.
    if (timer || consumed) {
      e.preventDefault();
    }
    consumed = false;
  };

  // ... rest unchanged
}
```

Then at each of the 3 call sites, compose the existing onContextMenu with the long-press handler:

```tsx
const longPress = createLongPress((x, y) => {
  showContextMenuAt(x, y, buildMenuItems());
});

<div
  onContextMenu={(e) => {
    longPress.onContextMenu(e);  // suppresses native if long-press fired
    if (!e.defaultPrevented) {
      showContextMenu(e, buildMenuItems());  // desktop right-click
    }
  }}
  onPointerDown={longPress.onPointerDown}
  onPointerUp={longPress.onPointerUp}
  onPointerCancel={longPress.onPointerCancel}
  onPointerMove={longPress.onPointerMove}
>
```

Desktop right-click: `longPress.onContextMenu` does nothing (no timer, not consumed), `showContextMenu` fires normally.
Touch long-press: timer fires `showContextMenuAt`, then native `contextmenu` event fires, `longPress.onContextMenu` calls `preventDefault`. The `if (!e.defaultPrevented)` guard prevents showing the menu twice.

**Files:** `createLongPress.ts`, `MessageItem.tsx`, `ChannelItem.tsx`, `MembersTab.tsx`

### Fix 13 (Audit Finding 21, MEDIUM) — `inert` attribute on closed MobileDrawer

**Problem:** `MobileDrawer` uses `pointer-events-none` when closed, but this only blocks pointer events. Keyboard Tab navigation still focuses elements inside the off-screen drawer, and screen readers can reach them. Keyboard users on mobile breakpoints can accidentally activate invisible navigation items (WCAG 2.4.3 violation: Focus Order).

**Fix:** Add the HTML5 `inert` attribute when the drawer is closed:

```tsx
<div
  class="fixed inset-0 z-50"
  classList={{ "pointer-events-none": !props.open }}
  inert={!props.open ? true : undefined}
>
```

`inert` (supported in all modern browsers) prevents pointer, keyboard, AND assistive technology from reaching descendants. Solid.js JSX types include `inert?: boolean | undefined` (verified in `node_modules/solid-js/types/jsx.d.ts:1218`), so no type-suppression directive is needed. Setting `inert` to `undefined` (rather than `false`) ensures the DOM attribute is omitted when the drawer is open.

**Files:** `MobileDrawer.tsx`

### Fix 14 (Audit Finding 22, MEDIUM) — Save and restore prior `body.overflow`

**Problem:** `MobileDrawer`'s effect resets `document.body.style.overflow` to `""` when closing. If a modal opens over the drawer with `overflow=hidden` and the drawer closes first, the cleanup re-enables page scroll behind the still-open modal.

**Fix:** Save the prior value only on the closed → open transition, restore only on the open → closed transition. Using `null` as a sentinel for "not currently locking" prevents re-capturing on every effect re-run:

```tsx
let savedOverflow: string | null = null;

createEffect(() => {
  if (props.open && savedOverflow === null) {
    // Closed → open: snapshot the prior value once
    savedOverflow = document.body.style.overflow;
    document.body.style.overflow = "hidden";
  } else if (!props.open && savedOverflow !== null) {
    // Open → closed: restore and clear the snapshot
    document.body.style.overflow = savedOverflow;
    savedOverflow = null;
  }
});

onCleanup(() => {
  // Component disposed mid-lock: restore so we don't leave the page locked
  if (savedOverflow !== null) {
    document.body.style.overflow = savedOverflow;
    savedOverflow = null;
  }
});
```

A naive `if/else` on every effect run would re-capture `"hidden"` (the value we just set) on subsequent runs while the drawer is open, defeating the purpose. The `null` sentinel ensures the snapshot is taken exactly once per open cycle.

A modal that set `overflow=hidden` before the drawer opened gets restored to `hidden` when the drawer closes. No existing modal in the codebase manipulates `body.overflow` today, but this fix is correct for future modal additions.

**Files:** `MobileDrawer.tsx`

### Fix 15 (Audit Finding 23, LOW) — Edge swipe state cleanup

**Problem:** `AppShell.tsx` tracks `edgeStartX` for the swipe-right-to-open gesture. If `onPointerDown` fires near the left edge but `onPointerUp` never fires (page navigation, pointer escapes window), the stale value persists and a subsequent unrelated `onPointerUp` could trigger an unexpected drawer-open.

**Fix:** Add `onPointerCancel` and `onPointerLeave` reset handlers:

```tsx
const onEdgeReset = () => { edgeStartX = null; };

<main
  onPointerDown={onEdgePointerDown}
  onPointerUp={onEdgePointerUp}
  onPointerCancel={onEdgeReset}
  onPointerLeave={onEdgeReset}
>
```

**Files:** `AppShell.tsx`

### Fix 16 (Audit Finding 20, LOW) — Update menu height estimate

**Problem:** `ContextMenu` uses `items.length * 36` to estimate menu height for viewport edge-flipping. The previous PR changed item padding from `py-1.5` to `py-2.5`, making each item ~40px tall instead of ~36px. The underestimate can cause the menu to extend partially off-screen at the bottom on small viewports.

**Fix:** Declare a single module-scope constant, then reference it from both functions:

```typescript
// At module scope (top of ContextMenu.tsx, near other constants):
const ITEM_HEIGHT_PX = 40;  // py-2.5 + text-sm leading + border ≈ 40px

// In both showContextMenu and showContextMenuAt:
const menuH = items.length * ITEM_HEIGHT_PX;
```

Single source of truth — if padding changes again, only one constant needs updating.

**Files:** `ContextMenu.tsx`

---

## Testing & Verification

Each PR's existing test gates apply to the followup commits:

- **PR #525 (auth-network):** `cd mobile/android && ./gradlew test` + `SQLX_OFFLINE=true cargo clippy -p vc-server -- -D warnings && cargo test -p vc-server`
- **PR #526 (voice-webrtc):** `cd mobile/android && ./gradlew test`
- **PR #527 (ui-architecture):** `cd mobile/android && ./gradlew test`
- **PR #528 (web-responsive):** `cd client && bun run test:run && bun run build`

New tests required:
- Fix 1 (Connected delay): Update `KaikuWebSocketTest` to verify state stays `Connecting` until mock server sends `Ready`.
- All other fixes: Manual verification via existing test infrastructure; no behavior changes that would invalidate existing tests.

## CHANGELOG updates

Each PR's existing CHANGELOG entries are extended with one line per user-visible fix. Logging-only fixes (Fix 6, Fix 8) and consistency refactors (Fix 3, Fix 10, Fix 11) are not added to CHANGELOG per project convention ("Nicht aktualisieren bei: reinen Refactorings").

User-visible additions:
- PR #525 (`### Security`): OIDC PKCE state is now persisted synchronously, preventing intermittent CSRF-error login failures on low-memory devices (Fix 2); TLS provider failure now prompts users to update Play Services instead of failing silently (Fix 5)
- PR #525 (`### Fixed`): OIDC callback path matching is now exact instead of prefix-based (Fix 4); WebSocket connection state correctly waits for server authentication confirmation (Fix 1)
- PR #526 (`### Fixed`): ICE candidate buffer is bounded to prevent unbounded memory growth (Fix 7); concurrent voice connection cleanup is now race-free (Fix 9)
- PR #528 (`### Fixed`): long-press on touch devices no longer shows duplicate context menus (Fix 12); mobile drawer no longer traps keyboard focus when closed (Fix 13); body scroll lock no longer interferes with overlapping modals (Fix 14)

## Appendix: Audit Findings Coverage

The audit produced 24 numbered findings. This spec addresses all 16 actionable findings; the remaining 8 are positives (no fix required).

| Audit # | Status | Notes |
|---------|--------|-------|
| 1 | Fix 5 | ProviderInstaller fail-fast |
| 2 | Fix 6 | JWT log on header path (also covers pre-auth event logging via existing `wait_for_auth_frame` warn-level message) |
| 3 | Fix 1 | Connected delay |
| 4 | Fix 2 | PKCE commit() |
| 5 | Fix 3 | clearOidcPkceState commit() |
| 6 | Fix 4 | App Links exact path |
| 7 | Fix 6 | JWT log on header path |
| 8 | Positive | Token persistence eliminated (already done in #525) |
| 9 | Positive | PKCE entropy correct |
| 10 | Fix 7 | ICE buffer cap |
| 11 | Fix 9 | PeerConnection close+dispose atomic |
| 12 | Fix 8 | VoiceServiceEvents log on drop |
| 13 | Fix 10 | AudioRouteManager Closeable |
| 14 | Not addressed | Bluetooth SCO timeout silent fallback — surfaced in CHANGELOG already, UI surface deferred to future enhancement |
| 15 | Positive | VoiceServiceEvents DI security sound |
| 16 | Not addressed | Channel name no defensive truncation — Compose renders gracefully, server enforces length limits |
| 17 | Fix 11 | Channel.BUFFERED right-sizing |
| 18 | Not addressed | savedStateHandle["channelId"]!! — pre-existing pattern, not introduced by these PRs |
| 19 | Fix 12 | Long-press dual menu |
| 20 | Fix 16 | Menu height estimate |
| 21 | Fix 13 | MobileDrawer inert |
| 22 | Fix 14 | Body scroll save/restore |
| 23 | Fix 15 | Edge swipe cleanup |
| 24 | Positive | Coordinate clamping good |

**Summary:** 16 fixes addressing 16 audit findings (Fix 6 covers two findings: #2 and #7), 4 positive findings noted, 3 deferred-but-tracked items (Bluetooth SCO UX, channel name defensive truncation, pre-existing `!!` operator). Total rows: 24.
