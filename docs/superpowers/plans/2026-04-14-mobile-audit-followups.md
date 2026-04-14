# Mobile Audit Followups Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Apply 16 audit-followup fixes as new commits to the 4 open PRs (#525, #526, #527, #528).

**Architecture:** Each fix is a small, focused commit in the appropriate PR's worktree. All fixes are additive corrections to the original implementation. No new files are created — every fix modifies existing files.

**Tech Stack:** Kotlin (Android), Rust (server), TypeScript/Solid.js (web client)

**Spec:** `docs/superpowers/specs/2026-04-14-mobile-audit-followups-design.md`

**Parallelization safe:** The 4 PRs touch disjoint file sets (each in its own worktree, no shared files). Tasks within a PR are serial; tasks across PRs can run in parallel.

---

## Pre-Execution Verified Facts

The following has been verified against the current worktree state to make this plan self-contained. The implementer can rely on these without re-checking:

- **`KaikuApplication.kt`**: Already has `import java.util.logging.Logger` and `private val logger = Logger.getLogger("KaikuApplication")`. Task 4 just replaces the existing try/catch.
- **`TokenStorage.kt`**: Already has `import java.util.logging.Logger` (line 4) and `private val logger = Logger.getLogger("TokenStorage")` in the companion object (line 19). Tasks 1 and 2 use the existing logger — no field needs to be added.
- **`KaikuWebSocket.connect()`**: Calls `doConnect()` which sets `_connectionState.value = ConnectionState.Connecting` before reaching `onOpen`. The state machine precondition for Task 6's Ready-gated transition is satisfied.
- **`AudioRouteManager.kt`**: Already has `import kotlinx.coroutines.cancel` (line 19), `private val scope = CoroutineScope(...)` (line 74), and `private fun unregisterReceivers()` (line 329). Task 11's `close()` can call them directly.
- **`WebRtcManager` constructor**: `@Inject constructor(@ApplicationContext private val context: Context, private val voiceApi: VoiceApi)` — exactly two parameters. Task 8's test must mock both.
- **`MessageItem.tsx`** (line 581): Has `handleContextMenu` and `longPress` already defined; spread is `onContextMenu={handleContextMenu}` (line 604).
- **`ChannelItem.tsx`** (line 155): Same pattern as MessageItem — `handleContextMenu` + `longPress`, spread on the element.
- **`MembersTab.tsx`** (line 201): Uses `memberLongPress` and an inline `onContextMenu={(e) => ...}` handler (line 213) — different pattern from the others.
- **`ContextMenu.tsx`**: `items.length * 36` appears at exactly two locations (lines 74 and 108). Both calculate `menuHeight` (not `menuH`).

---

## File Map

| Worktree | Files Modified |
|----------|----------------|
| `.claude/worktrees/auth-network` | `KaikuApplication.kt`, `KaikuWebSocket.kt`, `KaikuWebSocketTest.kt`, `TokenStorage.kt`, `AndroidManifest.xml`, `server/src/ws/handlers.rs`, `CHANGELOG.md` |
| `.claude/worktrees/voice-webrtc` | `WebRtcManager.kt`, `WebRtcManagerTest.kt`, `VoiceServiceEvents.kt`, `AudioRouteManager.kt`, `CHANGELOG.md` |
| `.claude/worktrees/ui-arch` | `HomeViewModel.kt`, `LoginViewModel.kt` |
| `.claude/worktrees/web-responsive` | `MobileDrawer.tsx`, `AppShell.tsx`, `ContextMenu.tsx`, `MessageItem.tsx`, `ChannelItem.tsx`, `MembersTab.tsx`, `createLongPress.ts`, `CHANGELOG.md` |

---

## PR #525 (auth-network) — 6 fixes

Work from: `/home/detair/GIT/detair/kaiku/.claude/worktrees/auth-network`

### Task 1: Fix 2 — PKCE state uses commit() (HIGH)

**File:** `mobile/android/app/src/main/java/io/wolftown/kaiku/data/local/TokenStorage.kt`

- [ ] **Step 1: Locate `saveOidcPkceState` function**

The function uses `.apply()` which is asynchronous. Replace with `.commit()` which is synchronous and returns success boolean.

- [ ] **Step 2: Update saveOidcPkceState**

Find the existing `saveOidcPkceState` function. Replace the body:

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

The `logger` field already exists in the companion object (line 19) — no need to add it.

- [ ] **Step 3: Commit**

```bash
git add mobile/android/app/src/main/java/io/wolftown/kaiku/data/local/TokenStorage.kt
git commit -m "fix(auth): persist OIDC PKCE state synchronously to survive process death (audit Fix 2)"
```

---

### Task 2: Fix 3 — clearOidcPkceState uses commit() (MEDIUM)

**File:** Same `TokenStorage.kt` (one task per logical fix per CLAUDE.md commit convention).

- [ ] **Step 1: Update clearOidcPkceState**

Find the existing `clearOidcPkceState` function. Replace `.apply()` with `.commit()`:

```kotlin
fun clearOidcPkceState() {
    val success = prefs.edit()
        .remove(KEY_OIDC_CODE_VERIFIER)
        .remove(KEY_OIDC_STATE)
        .commit()
    if (!success) {
        logger.warning("Failed to clear OIDC PKCE state from storage")
    }
}
```

- [ ] **Step 2: Commit**

```bash
git add mobile/android/app/src/main/java/io/wolftown/kaiku/data/local/TokenStorage.kt
git commit -m "fix(auth): clear OIDC PKCE state synchronously for consistency (audit Fix 3)"
```

---

### Task 3: Fix 4 — App Links exact path match (MEDIUM)

**File:** `mobile/android/app/src/main/AndroidManifest.xml`

- [ ] **Step 1: Locate the App Links intent filter**

Find the intent filter for `https://kaiku.pmind.de/auth/callback` (added in PR #525). It currently uses `android:pathPrefix="/auth/callback"`.

- [ ] **Step 2: Change pathPrefix to path**

Replace:
```xml
<data android:scheme="https"
      android:host="kaiku.pmind.de"
      android:pathPrefix="/auth/callback" />
```

With:
```xml
<data android:scheme="https"
      android:host="kaiku.pmind.de"
      android:path="/auth/callback" />
```

`android:path` matches the URL path component exactly. Query strings (`?access_token=xxx`) and fragments (`#id_token=xxx`) still match because Android intent filters compare against the path component only.

- [ ] **Step 3: Commit**

```bash
git add mobile/android/app/src/main/AndroidManifest.xml
git commit -m "fix(auth): use exact path match for App Links to prevent prefix collisions (audit Fix 4)"
```

---

### Task 4: Fix 5 — ProviderInstaller fail-fast with user notification (CRITICAL)

**File:** `mobile/android/app/src/main/java/io/wolftown/kaiku/KaikuApplication.kt`

- [ ] **Step 1: Add new imports**

Add these three imports alongside the existing `com.google.android.gms.security.ProviderInstaller` import:

```kotlin
import com.google.android.gms.common.GoogleApiAvailability
import com.google.android.gms.common.GooglePlayServicesNotAvailableException
import com.google.android.gms.common.GooglePlayServicesRepairableException
```

(`Logger` and `ProviderInstaller` are already imported.)

- [ ] **Step 2: Replace the catch-all Exception with specific exception handlers**

The existing `onCreate` (lines 12-19) uses `catch (e: Exception)`. Replace just the try/catch block:

```kotlin
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

The `logger` field already exists at line 10.

- [ ] **Step 3: Commit**

```bash
git add mobile/android/app/src/main/java/io/wolftown/kaiku/KaikuApplication.kt
git commit -m "fix(client): differentiate Play Services failures and notify user on repairable error (audit Fix 5)"
```

---

### Task 5: Fix 6 — Log JWT validation error on header path (MEDIUM)

**File:** `server/src/ws/handlers.rs`

- [ ] **Step 1: Find header-based auth path**

In the `handler` function (around line 99), find the JWT validation:
```rust
let claims = match jwt::validate_access_token(&token, &state.config.jwt_public_key) {
    Ok(claims) => claims,
    Err(_) => {
        return error_response(401, "Invalid token");
    }
};
```

- [ ] **Step 2: Add warn! log on validation failure**

Replace the `Err(_)` arm:

```rust
let claims = match jwt::validate_access_token(&token, &state.config.jwt_public_key) {
    Ok(claims) => claims,
    Err(e) => {
        warn!("Header-based WS auth failed: {}", e);
        return error_response(401, "Invalid token");
    }
};
```

No new imports needed — `warn!` is already in scope from existing usages at lines 117, 150, 156.

- [ ] **Step 3: Verify server clippy passes**

Run: `SQLX_OFFLINE=true cargo clippy -p vc-server -- -D warnings`
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add server/src/ws/handlers.rs
git commit -m "fix(ws): log JWT validation errors on header auth path for debugging parity (audit Fix 6)"
```

---

### Task 6: Fix 1 — Delay Connected state until Ready event (HIGH)

**Files:**
- Modify: `mobile/android/app/src/main/java/io/wolftown/kaiku/data/ws/KaikuWebSocket.kt`
- Modify: `mobile/android/app/src/test/java/io/wolftown/kaiku/data/ws/KaikuWebSocketTest.kt`

**Precondition (verified):** `connect()` calls `doConnect()` (line 96-102), which sets `_connectionState.value = ConnectionState.Connecting` before the WebSocket upgrade. So when `onOpen` fires, the state is already `Connecting`. The Ready-gated transition in Step 2 below relies on this.

- [ ] **Step 1: Update onOpen to stay in Connecting state**

In `KaikuWebSocket.kt`, find `onOpen` (around line 163). The current code sets state to Connected immediately:

```kotlin
override fun onOpen(webSocket: WebSocket, response: Response) {
    send(ClientEvent.Authenticate(token))
    logger.info("WebSocket connected")
    _connectionState.value = ConnectionState.Connected
    reconnectDelay = INITIAL_RECONNECT_DELAY_MS
    startPingLoop()
}
```

Replace with:

```kotlin
override fun onOpen(webSocket: WebSocket, response: Response) {
    send(ClientEvent.Authenticate(token))
    logger.info("WebSocket open, awaiting Ready for authentication confirmation")
    // Stay in Connecting state until server sends Ready event (auth confirmed).
    // Server's 5s auth timeout closes the WS if auth fails — handled by onClosed/onFailure.
}
```

- [ ] **Step 2: Update onMessage to transition on Ready**

In the same file, find `onMessage` (around line 159). Wrap the existing event handling with a state transition for `Ready`:

```kotlin
override fun onMessage(webSocket: WebSocket, text: String) {
    try {
        val event = json.decodeFromString<ServerEvent>(text)
        // Transition to Connected on Ready event (works for both header-based
        // and post-connect Authenticate auth modes — server sends Ready in both)
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

- [ ] **Step 3: Verify existing WebSocket test setup, then add new test**

First, read `KaikuWebSocketTest.kt` to find the test `connect sends Authenticate frame instead of header` (added in PR #525). Confirm the mock server's `WebSocketListener.onMessage` sends a `Ready` event in response to the `Authenticate` message — if so, existing tests will still pass with the new state-machine logic.

If existing tests do NOT send `Ready` (the mock server only handles `Authenticate` without responding), update the mock server in those tests to also send `Ready` so the existing connection-state assertions still pass:

```kotlin
// In existing mock setup, after receiving Authenticate:
webSocket.send("""{"type":"ready","heartbeat_interval_ms":30000,"protocol_version":1}""")
// (use the real ServerEvent.Ready JSON shape — check ServerEvent.kt for fields)
```

Then add one new test that verifies state stays `Connecting` until `Ready` arrives:

```kotlin
@Test
fun `connect stays in Connecting state until Ready event arrives`() = runTest {
    // ... existing test setup with mockWebServer ...

    // Server accepts upgrade but does NOT send Ready immediately
    mockWebServer.enqueue(MockResponse().withWebSocketUpgrade(object : WebSocketListener() {
        override fun onMessage(webSocket: WebSocket, text: String) {
            // Receive Authenticate but don't respond with Ready yet
        }
    }))

    webSocket.connect("http://localhost:${mockWebServer.port}")

    webSocket.connectionState.test {
        assertEquals(ConnectionState.Disconnected, awaitItem())
        assertEquals(ConnectionState.Connecting, awaitItem())
        // No transition to Connected — server hasn't sent Ready
        expectNoEvents()
    }
}
```

(Adjust mock server setup to match existing test patterns in the file.)

- [ ] **Step 4: Run tests to verify**

Run: `cd mobile/android && ./gradlew test`
Expected: PASS — existing tests still pass (mock server sends Ready), new test verifies the gating.

- [ ] **Step 5: Commit**

```bash
git add mobile/android/app/src/main/java/io/wolftown/kaiku/data/ws/KaikuWebSocket.kt \
  mobile/android/app/src/test/java/io/wolftown/kaiku/data/ws/KaikuWebSocketTest.kt
git commit -m "fix(client): delay Connected state until server Ready event confirms auth (audit Fix 1)"
```

---

### Task 7: PR #525 CHANGELOG updates

**File:** `CHANGELOG.md` (in worktree root)

- [ ] **Step 1: Add Security and Fixed entries**

In the `## [Unreleased]` section, find the existing `### Security` block (added by PR #525). Add at the end:

```markdown
- Android: OIDC PKCE state is persisted synchronously, preventing intermittent CSRF-error login failures on low-memory devices
- Android: TLS 1.3 provider failure now prompts users to update Play Services instead of failing silently on every network call
```

In the existing `### Fixed` block, add:

```markdown
- Android: OIDC callback path matching is now exact instead of prefix-based, preventing potential intent collisions
- Android: WebSocket connection state correctly waits for server authentication confirmation before reporting Connected
```

(Logging-only fixes #3, #6 are not added per CLAUDE.md "Nicht aktualisieren bei: reinen Refactorings".)

- [ ] **Step 2: Commit**

```bash
git add CHANGELOG.md
git commit -m "docs(client): add CHANGELOG entries for PR #525 audit followup fixes"
```

---

## PR #526 (voice-webrtc) — 4 fixes

Work from: `/home/detair/GIT/detair/kaiku/.claude/worktrees/voice-webrtc`

### Task 8: Fix 7 — Cap ICE candidate buffer (MEDIUM)

**Files:**
- Modify: `mobile/android/app/src/main/java/io/wolftown/kaiku/data/voice/WebRtcManager.kt`
- Modify: `mobile/android/app/src/test/java/io/wolftown/kaiku/data/voice/WebRtcManagerTest.kt`

- [ ] **Step 1: Add MAX_PENDING_CANDIDATES constant**

In `WebRtcManager.kt`, find the `companion object` block. Add:

```kotlin
companion object {
    private val logger = Logger.getLogger("WebRtcManager")
    private const val LOCAL_AUDIO_TRACK_ID = "kaiku-local-audio"
    private const val MAX_PENDING_CANDIDATES = 100
}
```

- [ ] **Step 2: Add cap and logging in addIceCandidate**

Find the `addIceCandidate` function. The current buffer-when-not-set branch (added in PR #526) looks like:

```kotlin
fun addIceCandidate(candidateJson: String) {
    if (!remoteDescriptionSet) {
        pendingCandidates.add(candidateJson)
        return
    }
    // ... existing pc null check and parse logic
}
```

Modify to:

```kotlin
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
    // ... existing pc null check and parse logic
}
```

- [ ] **Step 3: Expose pendingCandidatesSize for testing**

In `WebRtcManager.kt`, add an `internal` accessor near the `pendingCandidates` field (avoids reflection):

```kotlin
/** Test-only accessor for the buffered ICE candidate count. */
internal fun pendingCandidatesSize(): Int = pendingCandidates.size
```

- [ ] **Step 4: Add unit test for buffer cap**

`WebRtcManager`'s constructor is `@Inject constructor(@ApplicationContext context: Context, voiceApi: VoiceApi)` (verified) — two parameters. In `WebRtcManagerTest.kt`, follow the existing test patterns for mocking. Add:

```kotlin
@Test
fun `addIceCandidate buffers up to MAX_PENDING_CANDIDATES then drops`() {
    val webRtcManager = WebRtcManager(mockContext, mockVoiceApi)
    // remoteDescriptionSet defaults to false — buffering branch is taken

    repeat(101) { i ->
        webRtcManager.addIceCandidate("""{"candidate":"c$i","sdpMLineIndex":0,"sdpMid":"0"}""")
    }

    assertEquals(100, webRtcManager.pendingCandidatesSize())
}
```

Use whatever mock setup the existing tests in `WebRtcManagerTest.kt` use for `mockContext` and `mockVoiceApi` (likely `mockk`).

- [ ] **Step 5: Run tests to verify**

Run: `cd mobile/android && ./gradlew test`
Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add mobile/android/app/src/main/java/io/wolftown/kaiku/data/voice/WebRtcManager.kt \
  mobile/android/app/src/test/java/io/wolftown/kaiku/data/voice/WebRtcManagerTest.kt
git commit -m "fix(voice): cap ICE candidate buffer to prevent unbounded memory growth (audit Fix 7)"
```

---

### Task 9: Fix 8 — Log dropped VoiceServiceEvents (MEDIUM)

**File:** `mobile/android/app/src/main/java/io/wolftown/kaiku/data/voice/VoiceServiceEvents.kt`

- [ ] **Step 1: Add logger and check tryEmit return value**

Modify the file:

```kotlin
package io.wolftown.kaiku.data.voice

import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.SharedFlow
import kotlinx.coroutines.flow.asSharedFlow
import java.util.logging.Logger
import javax.inject.Inject
import javax.inject.Singleton

sealed class VoiceServiceEvent {
    data object MuteToggle : VoiceServiceEvent()
    data object Disconnect : VoiceServiceEvent()
}

@Singleton
class VoiceServiceEvents @Inject constructor() {
    companion object {
        private val logger = Logger.getLogger("VoiceServiceEvents")
    }

    private val _events = MutableSharedFlow<VoiceServiceEvent>(extraBufferCapacity = 5)
    val events: SharedFlow<VoiceServiceEvent> = _events.asSharedFlow()

    fun emit(event: VoiceServiceEvent) {
        val emitted = _events.tryEmit(event)
        if (!emitted) {
            logger.warning("VoiceServiceEvent dropped (buffer full): ${event::class.simpleName}")
        }
    }
}
```

- [ ] **Step 2: Run tests to verify**

Run: `cd mobile/android && ./gradlew test`
Expected: PASS

- [ ] **Step 3: Commit**

```bash
git add mobile/android/app/src/main/java/io/wolftown/kaiku/data/voice/VoiceServiceEvents.kt
git commit -m "fix(voice): log dropped VoiceServiceEvents for debugging visibility (audit Fix 8)"
```

---

### Task 10: Fix 9 — Atomic close+dispose in closePeerConnection (MEDIUM)

**File:** `mobile/android/app/src/main/java/io/wolftown/kaiku/data/voice/WebRtcManager.kt`

- [ ] **Step 1: Reorder cleanup operations**

Find `closePeerConnection`. Current code (after PR #526 changes):

```kotlin
fun closePeerConnection() {
    localAudioTrack?.dispose()
    localAudioTrack = null
    audioSource?.dispose()
    audioSource = null
    peerConnection?.close()
    peerConnection?.dispose()
    peerConnection = null

    _remoteAudioTracks.value = emptyMap()
    _remoteVideoTracks.value = emptyMap()
    remoteDescriptionSet = false
    pendingCandidates.clear()

    logger.info("PeerConnection closed")
}
```

Replace with:

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

- [ ] **Step 2: Run tests to verify**

Run: `cd mobile/android && ./gradlew test`
Expected: PASS

- [ ] **Step 3: Commit**

```bash
git add mobile/android/app/src/main/java/io/wolftown/kaiku/data/voice/WebRtcManager.kt
git commit -m "fix(voice): atomic close+dispose to prevent use-after-dispose race (audit Fix 9)"
```

---

### Task 11: Fix 10 — AudioRouteManager implements Closeable (MEDIUM)

**File:** `mobile/android/app/src/main/java/io/wolftown/kaiku/data/voice/AudioRouteManager.kt`

**Verified facts:** `kotlinx.coroutines.cancel` is already imported (line 19), `private val scope` is already defined (line 74), and `private fun unregisterReceivers()` exists at line 329.

- [ ] **Step 1: Add Closeable import and modify class declaration**

Add the import alongside existing `java.util.logging.Logger` import:
```kotlin
import java.io.Closeable
```

Change the class declaration from:
```kotlin
@Singleton
class AudioRouteManager @Inject constructor(
    @ApplicationContext private val context: Context
) {
```

To:
```kotlin
@Singleton
class AudioRouteManager @Inject constructor(
    @ApplicationContext private val context: Context
) : Closeable {
```

- [ ] **Step 2: Add close() method**

Add this method to the class (place it near `abandonAudioFocus()` at line 143):

```kotlin
override fun close() {
    unregisterReceivers()
    scope.cancel()
}
```

`unregisterReceivers()` is the existing private method at line 329 that handles cleanup of `headsetReceiver`, `bluetoothReceiver`, and `scoReceiver`.

- [ ] **Step 2: Run tests to verify**

Run: `cd mobile/android && ./gradlew test`
Expected: PASS

- [ ] **Step 3: Commit**

```bash
git add mobile/android/app/src/main/java/io/wolftown/kaiku/data/voice/AudioRouteManager.kt
git commit -m "fix(voice): implement Closeable on AudioRouteManager for clean test teardown (audit Fix 10)"
```

---

### Task 12: PR #526 CHANGELOG updates

**File:** `CHANGELOG.md`

- [ ] **Step 1: Add Fixed entries**

In the `## [Unreleased]` `### Fixed` block, add:

```markdown
- Android: ICE candidate buffer is bounded to prevent unbounded memory growth on buggy SDP flows
- Android: concurrent voice connection cleanup is now race-free, preventing rare crashes on rapid reconnect
```

(Fixes #8 and #10 are observability/test-only and not added per CLAUDE.md convention.)

- [ ] **Step 2: Commit**

```bash
git add CHANGELOG.md
git commit -m "docs(voice): add CHANGELOG entries for PR #526 audit followup fixes"
```

---

## PR #527 (ui-architecture) — 1 fix

Work from: `/home/detair/GIT/detair/kaiku/.claude/worktrees/ui-arch`

### Task 13: Fix 11 — Right-size navigation Channels (LOW)

**Files:**
- Modify: `mobile/android/app/src/main/java/io/wolftown/kaiku/ui/home/HomeViewModel.kt`
- Modify: `mobile/android/app/src/main/java/io/wolftown/kaiku/ui/auth/LoginViewModel.kt`

- [ ] **Step 0: Verify current state**

Confirm `HomeViewModel._navigateToChannel` and `LoginViewModel._oidcCallbackUri` currently use `Channel<X>(Channel.BUFFERED)` (set in PR #527's initial implementation). If either uses a different Channel construction, stop and flag — the spec assumes both use `BUFFERED`.

- [ ] **Step 1: Update HomeViewModel**

Find `_navigateToChannel = Channel<ChannelNavEvent>(Channel.BUFFERED)`. Replace with:

```kotlin
// Bounded capacity prevents buildup if a navigation collector is suspended.
// Capacity 4 is generous for tap-burst behavior — if 4 navigation events
// queue, something is wrong with the consumer.
private val _navigateToChannel = Channel<ChannelNavEvent>(capacity = 4)
```

- [ ] **Step 2: Update LoginViewModel**

Find `_oidcCallbackUri = Channel<Uri>(Channel.BUFFERED)`. Replace with:

```kotlin
// OIDC callback is one-shot — CONFLATED keeps only the latest, redelivery
// is harmless, and trySend never blocks.
private val _oidcCallbackUri = Channel<Uri>(Channel.CONFLATED)
```

- [ ] **Step 3: Run tests to verify**

Run: `cd mobile/android && ./gradlew test`
Expected: PASS — code clarity change, no functional behavior change.

- [ ] **Step 4: Commit**

```bash
git add mobile/android/app/src/main/java/io/wolftown/kaiku/ui/home/HomeViewModel.kt \
  mobile/android/app/src/main/java/io/wolftown/kaiku/ui/auth/LoginViewModel.kt
git commit -m "refactor(client): right-size navigation Channel capacities (audit Fix 11)"
```

(Per CLAUDE.md, this is a refactor — no CHANGELOG entry needed.)

---

## PR #528 (web-responsive) — 5 fixes

Work from: `/home/detair/GIT/detair/kaiku/.claude/worktrees/web-responsive`

### Task 14: Fix 16 — Update menu height estimate (LOW)

**File:** `client/src/components/ui/ContextMenu.tsx`

- [ ] **Step 1: Add module-scope ITEM_HEIGHT_PX constant**

At the top of the file, near other constants/imports, add:

```typescript
// Each context menu item: py-2.5 (10px top + 10px bottom) + text-sm leading + border ≈ 40px
const ITEM_HEIGHT_PX = 40;
```

- [ ] **Step 2: Use the constant in showContextMenu and showContextMenuAt**

`items.length * 36` appears at exactly two locations (verified): line 74 in `showContextMenu` and line 108 in `showContextMenuAt`. Both calculate a local `menuHeight` variable. Replace `items.length * 36` with `items.length * ITEM_HEIGHT_PX` in both.

- [ ] **Step 3: Verify build**

Run: `cd client && bun run build`
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add client/src/components/ui/ContextMenu.tsx
git commit -m "fix(client): update context menu height estimate for new item padding (audit Fix 16)"
```

---

### Task 15: Fix 13 — inert attribute on closed MobileDrawer (MEDIUM)

**File:** `client/src/components/layout/MobileDrawer.tsx`

- [ ] **Step 1: Add inert attribute to drawer container**

Find the outermost `<div class="fixed inset-0 z-50">`. Add the `inert` attribute:

```tsx
<div
  class="fixed inset-0 z-50"
  classList={{ "pointer-events-none": !props.open }}
  inert={!props.open ? true : undefined}
>
```

`inert` is in Solid.js JSX types (`node_modules/solid-js/types/jsx.d.ts:1218`), no `@ts-expect-error` needed. Setting to `undefined` (not `false`) ensures the attribute is omitted when the drawer is open.

- [ ] **Step 2: Verify build and tests**

Run: `cd client && bun run build && bun run test:run`
Expected: PASS, 577/577 tests pass

- [ ] **Step 3: Commit**

```bash
git add client/src/components/layout/MobileDrawer.tsx
git commit -m "fix(client): add inert attribute to closed MobileDrawer for keyboard accessibility (audit Fix 13)"
```

---

### Task 16: Fix 14 — Save and restore prior body.overflow (MEDIUM)

**File:** `client/src/components/layout/MobileDrawer.tsx`

- [ ] **Step 1: Replace the body.overflow effect**

Find the existing `createEffect` that sets `document.body.style.overflow`. Replace with the null-sentinel pattern:

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

The `null` sentinel is critical — without it, every re-run of `createEffect` while the drawer is open would re-capture `"hidden"` (the value we just set), defeating the purpose.

- [ ] **Step 2: Verify build and tests**

Run: `cd client && bun run build && bun run test:run`
Expected: PASS

- [ ] **Step 3: Commit**

```bash
git add client/src/components/layout/MobileDrawer.tsx
git commit -m "fix(client): save and restore prior body.overflow to avoid modal scroll-lock conflicts (audit Fix 14)"
```

---

### Task 17: Fix 15 — Edge swipe state cleanup (LOW)

**File:** `client/src/components/layout/AppShell.tsx`

- [ ] **Step 1: Add reset handlers**

Find the `<main>` element with `onPointerDown={onEdgePointerDown}` and `onPointerUp={onEdgePointerUp}`. Add a reset helper and `onPointerCancel` only:

```tsx
const onEdgeReset = () => { edgeStartX = null; };

<main
  onPointerDown={onEdgePointerDown}
  onPointerUp={onEdgePointerUp}
  onPointerCancel={onEdgeReset}
  // ... existing class and other props
>
```

**Why only `onPointerCancel`, not `onPointerLeave`:** A valid edge-swipe gesture starts at `clientX < 20` (left edge of `<main>`) and moves rightward. Since the gesture is rightward-into-`<main>`, the pointer never leaves `<main>` during the gesture — so `onPointerLeave` would only fire after the gesture completes (post-`onPointerUp` reset already cleared the state) or for unrelated pointer escapes. `onPointerCancel` is the correct signal for "browser interrupted this gesture" (e.g., system gesture, page navigation), which is exactly when we need to reset stale state.

- [ ] **Step 2: Verify build and tests**

Run: `cd client && bun run build && bun run test:run`
Expected: PASS

- [ ] **Step 3: Commit**

```bash
git add client/src/components/layout/AppShell.tsx
git commit -m "fix(client): reset edge swipe state on pointer cancel (audit Fix 15)"
```

---

### Task 18: Fix 12 — Wire long-press onContextMenu (HIGH)

This is the largest web fix — 4 files. Modify `createLongPress.ts` first (utility update), then 3 call sites.

**Files:**
- Modify: `client/src/lib/createLongPress.ts`
- Modify: `client/src/components/messages/MessageItem.tsx`
- Modify: `client/src/components/channels/ChannelItem.tsx`
- Modify: `client/src/components/guilds/MembersTab.tsx`

- [ ] **Step 1: Update createLongPress to track consumed state**

Modify `client/src/lib/createLongPress.ts` to track when the long-press timer fired:

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
    // Trade-off: on hybrid devices, if the user touch-long-presses then
    // immediately right-clicks before the next pointerdown, the right-click
    // is suppressed. This is acceptable.
    if (timer || consumed) {
      e.preventDefault();
    }
    consumed = false;
  };

  const onPointerMove = (e: PointerEvent) => {
    if (timer && (Math.abs(e.clientX - startX) > 10 || Math.abs(e.clientY - startY) > 10)) {
      cancel();
    }
  };

  return {
    onPointerDown,
    onPointerUp: cancel,
    onPointerCancel: cancel,
    onPointerMove,
    onContextMenu,
  };
}
```

- [ ] **Step 2: Update MessageItem.tsx handleContextMenu**

`MessageItem.tsx` has `handleContextMenu` at line 581 and `longPress` at line 585. The element spreads `onContextMenu={handleContextMenu}` (line 604). Update `handleContextMenu` to call `longPress.onContextMenu` first:

Replace lines 581-583:
```tsx
const handleContextMenu = (e: MouseEvent) => {
  longPress.onContextMenu(e);  // suppresses native if long-press fired
  if (!e.defaultPrevented) {
    showContextMenu(e, buildContextMenuItems());
  }
};
```

Note: This requires `longPress` to be defined before `handleContextMenu`. Currently `longPress` is at line 585 (after `handleContextMenu`). Either swap their order, or reorganize so both are defined before use. The cleanest fix: move `longPress` definition above `handleContextMenu`.

The element spreads at lines 604-608 (`onContextMenu={handleContextMenu}`, `onPointerDown={longPress.onPointerDown}`, etc.) stay unchanged — they already wire all needed handlers.

- [ ] **Step 3: Update ChannelItem.tsx handleContextMenu**

`ChannelItem.tsx` has the identical pattern: `handleContextMenu` at line 155, `longPress` at line 159, element spreads at line 166. Apply the same change as Step 2 — update `handleContextMenu` to call `longPress.onContextMenu` first, ensure `longPress` is defined before `handleContextMenu`.

- [ ] **Step 4: Update MembersTab.tsx inline handler**

`MembersTab.tsx` uses a different pattern (line 201-223): `memberLongPress` is defined inside a per-member render block, with an inline `onContextMenu={(e) => ...}` at line 213. Update the inline handler to call `memberLongPress.onContextMenu` first:

```tsx
onContextMenu={(e) => {
  memberLongPress.onContextMenu(e);
  if (!e.defaultPrevented) {
    // ... existing handler body (whatever it was)
  }
}}
```

Read lines 213-220 of `MembersTab.tsx` to see the existing handler body and preserve it inside the `if (!e.defaultPrevented)` branch.

- [ ] **Step 5: Verify build and tests**

Run: `cd client && bun run build && bun run test:run`
Expected: PASS, 577/577 tests pass

- [ ] **Step 6: Commit**

```bash
git add client/src/lib/createLongPress.ts \
  client/src/components/messages/MessageItem.tsx \
  client/src/components/channels/ChannelItem.tsx \
  client/src/components/guilds/MembersTab.tsx
git commit -m "fix(client): wire long-press onContextMenu to suppress duplicate native menus (audit Fix 12)"
```

---

### Task 19: PR #528 CHANGELOG updates

**File:** `CHANGELOG.md`

- [ ] **Step 1: Add Fixed entries**

In the `## [Unreleased]` `### Fixed` block, add:

```markdown
- Long-press on touch devices no longer shows duplicate context menus (custom menu plus browser native)
- Mobile drawer no longer traps keyboard focus or screen reader navigation when closed
- Body scroll lock no longer interferes with overlapping modals
```

- [ ] **Step 2: Commit**

```bash
git add CHANGELOG.md
git commit -m "docs(client): add CHANGELOG entries for PR #528 audit followup fixes"
```

---

## Final Verification (per PR)

After all tasks are complete in a worktree, verify before pushing:

- [ ] **PR #525 (auth-network):**
  - `cd mobile/android && ./gradlew test` — all unit tests pass
  - `SQLX_OFFLINE=true cargo clippy -p vc-server -- -D warnings` — clean
  - `cargo test -p vc-server` — passes
  - `git log --oneline main..HEAD` — verify 7 new commits (6 fixes + CHANGELOG)
  - `git push origin fix/android-auth-network` — push followups

- [ ] **PR #526 (voice-webrtc):**
  - `cd mobile/android && ./gradlew test` — all unit tests pass
  - `git log --oneline main..HEAD` — verify 5 new commits (4 fixes + CHANGELOG)
  - `git push origin fix/android-voice-webrtc` — push followups

- [ ] **PR #527 (ui-architecture):**
  - `cd mobile/android && ./gradlew test` — all unit tests pass
  - `git log --oneline main..HEAD` — verify 1 new commit (1 fix, no CHANGELOG)
  - `git push origin fix/android-ui-architecture` — push followups

- [ ] **PR #528 (web-responsive):**
  - `cd client && bun run test:run && bun run build` — 577/577 tests pass, build succeeds
  - `git log --oneline main..HEAD` — verify 6 new commits (5 fixes + CHANGELOG)
  - `git push origin feature/web-responsive` — push followups

After push, the existing PR pages on GitHub will show the new commits automatically.
