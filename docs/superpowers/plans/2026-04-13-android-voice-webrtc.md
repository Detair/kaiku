# Android Voice/WebRTC Fixes — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix WebRTC lifecycle bugs (SDP ordering, ICE buffering, resource leaks), replace static VoiceCallService callbacks with SharedFlow events, and fix audio routing issues.

**Architecture:** Nine targeted fixes centered on `WebRtcManager.kt` and `VoiceRepository.kt`. The largest structural change is replacing static companion object callbacks on `VoiceCallService` with a DI-injected `VoiceServiceEvents` SharedFlow. All other fixes are small (5-15 lines each).

**Tech Stack:** Kotlin, stream-webrtc-android, Hilt, Coroutines, Jetpack Compose

**Spec:** `docs/superpowers/specs/2026-04-13-mobile-implementation-fixes-design.md` — Sub-Project 2

**Branch:** `fix/android-voice-webrtc`

---

## File Map

| File | Responsibility |
|------|---------------|
| `mobile/android/app/src/main/java/io/wolftown/kaiku/data/voice/WebRtcManager.kt` | SDP ordering, ICE buffering, dispose(), AudioDeviceModule lifecycle |
| `mobile/android/app/src/main/java/io/wolftown/kaiku/data/voice/AudioRouteManager.kt` | Async Bluetooth SCO routing |
| `mobile/android/app/src/main/java/io/wolftown/kaiku/data/repository/VoiceRepository.kt` | leaveChannel mutex, scope cleanup, service event collection |
| `mobile/android/app/src/main/java/io/wolftown/kaiku/service/VoiceCallService.kt` | Replace static callbacks with VoiceServiceEvents |
| `mobile/android/app/src/main/java/io/wolftown/kaiku/data/voice/VoiceServiceEvents.kt` | NEW — SharedFlow event bus for service actions |
| `mobile/android/app/src/main/java/io/wolftown/kaiku/ui/voice/VoiceChannelScreen.kt` | Remove findVideoTrackForStream fallback |
| `mobile/android/app/src/test/java/io/wolftown/kaiku/data/voice/WebRtcManagerTest.kt` | Updated tests |
| `mobile/android/app/src/test/java/io/wolftown/kaiku/ui/voice/VoiceViewModelTest.kt` | Updated tests |

---

## Task 1: WebRtcManager — SDP ordering fix (#5)

**Files:**
- Modify: `mobile/android/app/src/main/java/io/wolftown/kaiku/data/voice/WebRtcManager.kt:223`

- [ ] **Step 1: Move onLocalDescription into onSetSuccess**

At `WebRtcManager.kt:223`, find the `setLocalDescription` call. The current code calls `onLocalDescription` immediately after enqueuing. Replace the no-op `SdpObserverAdapter` with an inline object:

```kotlin
pc.setLocalDescription(object : SdpObserverAdapter("setLocalDescription", onError) {
    override fun onSetSuccess() {
        super.onSetSuccess()
        onLocalDescription?.invoke(desc.description)
        logger.info("SDP answer created and set")
    }
}, desc)
```

Remove the `onLocalDescription?.invoke(desc.description)` line that currently follows the `setLocalDescription` call.

- [ ] **Step 2: Build and verify**

Run: `cd mobile/android && ./gradlew test`
Expected: PASS

- [ ] **Step 3: Commit**

```bash
git add mobile/android/app/src/main/java/io/wolftown/kaiku/data/voice/WebRtcManager.kt
git commit -m "fix(voice): send SDP answer only after setLocalDescription completes (#5)"
```

---

## Task 2: WebRtcManager — ICE candidate buffering (#10)

**Files:**
- Modify: `mobile/android/app/src/main/java/io/wolftown/kaiku/data/voice/WebRtcManager.kt:245,172`

- [ ] **Step 1: Add ICE buffering fields and logic**

At `WebRtcManager.kt`, add fields near the other peer connection fields:

```kotlin
private var remoteDescriptionSet = false
private val pendingCandidates = mutableListOf<String>()  // buffered JSON strings
```

Note: `addIceCandidate` takes `candidateJson: String` (not `IceCandidate`), so the buffer stores raw JSON strings.

In `handleOffer` (or wherever `setRemoteDescription` is called), set `remoteDescriptionSet = false` at the start. In the `setRemoteDescription` observer's `onSetSuccess`, drain the buffer by calling the existing `addIceCandidate` method for each:

```kotlin
override fun onSetSuccess() {
    super.onSetSuccess()
    remoteDescriptionSet = true
    val candidates = pendingCandidates.toList()
    pendingCandidates.clear()
    candidates.forEach { addIceCandidate(it) }
}
```

Modify `addIceCandidate` (line 245) — add the buffering guard at the top of the existing method, before the JSON parsing:

```kotlin
fun addIceCandidate(candidateJson: String) {
    if (!remoteDescriptionSet) {
        pendingCandidates.add(candidateJson)
        return
    }

    val pc = peerConnection ?: run {
        // ... existing null guard
    }
    // ... existing JSON parse and addIceCandidate logic
}
```

In `closePeerConnection()` (line 172), add:

```kotlin
remoteDescriptionSet = false
pendingCandidates.clear()
```

- [ ] **Step 2: Build and verify**

Run: `cd mobile/android && ./gradlew test`
Expected: PASS

- [ ] **Step 3: Commit**

```bash
git add mobile/android/app/src/main/java/io/wolftown/kaiku/data/voice/WebRtcManager.kt
git commit -m "fix(voice): buffer ICE candidates until remote description is set (#10)"
```

---

## Task 3: WebRtcManager — Resource cleanup (#12, #13)

**Files:**
- Modify: `mobile/android/app/src/main/java/io/wolftown/kaiku/data/voice/WebRtcManager.kt:115,172`

- [ ] **Step 1: Store and release JavaAudioDeviceModule**

At `WebRtcManager.kt:115`, change the local `audioDeviceModule` variable to a field:

```kotlin
private var audioDeviceModule: AudioDeviceModule? = null
```

In the `initialize()` function, assign:

```kotlin
audioDeviceModule = JavaAudioDeviceModule.builder(context)
    .createAudioDeviceModule()
```

In `dispose()`, release it after the factory but before EglBase:

```kotlin
fun dispose() {
    closePeerConnection()
    factory?.dispose()
    factory = null
    audioDeviceModule?.release()
    audioDeviceModule = null
    // ... EglBase release
}
```

- [ ] **Step 2: Add PeerConnection.dispose() to closePeerConnection**

At `WebRtcManager.kt:172`, in `closePeerConnection()`, change:

```kotlin
peerConnection?.close()
peerConnection = null
```

To:

```kotlin
peerConnection?.close()
peerConnection?.dispose()
peerConnection = null
```

- [ ] **Step 3: Build and verify**

Run: `cd mobile/android && ./gradlew test`
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add mobile/android/app/src/main/java/io/wolftown/kaiku/data/voice/WebRtcManager.kt
git commit -m "fix(voice): release AudioDeviceModule and dispose PeerConnection (#12, #13)"
```

---

## Task 4: VoiceServiceEvents — Replace static callbacks (#6)

**Files:**
- Create: `mobile/android/app/src/main/java/io/wolftown/kaiku/data/voice/VoiceServiceEvents.kt`
- Modify: `mobile/android/app/src/main/java/io/wolftown/kaiku/service/VoiceCallService.kt:44`
- Modify: `mobile/android/app/src/main/java/io/wolftown/kaiku/data/repository/VoiceRepository.kt:132`

- [ ] **Step 1: Create VoiceServiceEvents**

Create new file `data/voice/VoiceServiceEvents.kt`:

```kotlin
package io.wolftown.kaiku.data.voice

import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.SharedFlow
import kotlinx.coroutines.flow.asSharedFlow
import javax.inject.Inject
import javax.inject.Singleton

sealed class VoiceServiceEvent {
    data object MuteToggle : VoiceServiceEvent()
    data object Disconnect : VoiceServiceEvent()
}

@Singleton
class VoiceServiceEvents @Inject constructor() {
    private val _events = MutableSharedFlow<VoiceServiceEvent>(extraBufferCapacity = 5)
    val events: SharedFlow<VoiceServiceEvent> = _events.asSharedFlow()

    fun emit(event: VoiceServiceEvent) {
        _events.tryEmit(event)
    }
}
```

- [ ] **Step 2: Update VoiceCallService to use VoiceServiceEvents**

At `VoiceCallService.kt` (the Hilt plugin `com.google.dagger.hilt.android` is already in `build.gradle.kts:8`):
1. Add `@AndroidEntryPoint` annotation to the service class.
2. Inject `VoiceServiceEvents`: `@Inject lateinit var voiceServiceEvents: VoiceServiceEvents`
3. Add an `onCreate` override — Hilt injects fields during `super.onCreate()`, so this must be called before any access:

```kotlin
@AndroidEntryPoint
class VoiceCallService : Service() {
    @Inject lateinit var voiceServiceEvents: VoiceServiceEvents

    override fun onCreate() {
        super.onCreate()  // REQUIRED: Hilt injects fields here
    }
    // ...
}
```

4. Remove the `companion object` block with `onMuteToggle` and `onDisconnect` vars.
5. In `onStartCommand`, replace `onMuteToggle?.invoke()` with `voiceServiceEvents.emit(VoiceServiceEvent.MuteToggle)` and `onDisconnect?.invoke()` with `voiceServiceEvents.emit(VoiceServiceEvent.Disconnect)`.

- [ ] **Step 3: Update VoiceRepository to collect VoiceServiceEvents**

At `VoiceRepository.kt:132`:
1. Inject `VoiceServiceEvents` in the constructor.
2. Remove the static callback assignments (`VoiceCallService.onMuteToggle = { ... }` and `VoiceCallService.onDisconnect = { ... }`).
3. In `joinChannel()`, after starting the service, launch a collector:

```kotlin
serviceEventJob = scope.launch {
    voiceServiceEvents.events.collect { event ->
        when (event) {
            VoiceServiceEvent.MuteToggle -> toggleMute()
            VoiceServiceEvent.Disconnect -> leaveChannel()
        }
    }
}
```

4. Add `private var serviceEventJob: Job? = null` field.
5. In `cleanUp()`, add `serviceEventJob?.cancel()`.

- [ ] **Step 4: Build and verify**

Run: `cd mobile/android && ./gradlew test`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add mobile/android/app/src/main/java/io/wolftown/kaiku/data/voice/VoiceServiceEvents.kt \
  mobile/android/app/src/main/java/io/wolftown/kaiku/service/VoiceCallService.kt \
  mobile/android/app/src/main/java/io/wolftown/kaiku/data/repository/VoiceRepository.kt
git commit -m "fix(voice): replace static VoiceCallService callbacks with SharedFlow events (#6)"
```

---

## Task 5: VoiceRepository — leaveChannel mutex (#14) and scope cleanup (#32)

**Files:**
- Modify: `mobile/android/app/src/main/java/io/wolftown/kaiku/data/repository/VoiceRepository.kt:50,158`

- [ ] **Step 1: Add Mutex to leaveChannel**

At `VoiceRepository.kt`, add:

```kotlin
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import java.io.Closeable

private val leaveMutex = Mutex()
```

Wrap `leaveChannel()` (line 158):

```kotlin
suspend fun leaveChannel() {
    leaveMutex.withLock {
        val channelId = _currentChannelId.value ?: return@withLock
        // ... all existing cleanup logic
    }
}
```

- [ ] **Step 2: Implement Closeable**

Add `Closeable` to the class declaration and implement `close()`:

```kotlin
@Singleton
class VoiceRepository @Inject constructor(...) : Closeable {
    // ...
    override fun close() {
        scope.cancel()
    }
}
```

- [ ] **Step 3: Build and verify**

Run: `cd mobile/android && ./gradlew test`
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add mobile/android/app/src/main/java/io/wolftown/kaiku/data/repository/VoiceRepository.kt
git commit -m "fix(voice): add leaveChannel mutex and Closeable to VoiceRepository (#14, #32)"
```

---

## Task 6: AudioRouteManager — Async Bluetooth SCO (#21)

**Files:**
- Modify: `mobile/android/app/src/main/java/io/wolftown/kaiku/data/voice/AudioRouteManager.kt:130,208`

- [ ] **Step 1: Add CoroutineScope and BroadcastReceiver for SCO state**

`AudioRouteManager` currently has no `CoroutineScope` (it's a `@Singleton` with synchronous state). Add one for the SCO timeout:

```kotlin
private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main)
```

Cancel it in `release()`: `scope.cancel()`

Then add a `BroadcastReceiver` that listens for `ACTION_SCO_AUDIO_STATE_UPDATED`:

```kotlin
private val scoReceiver = object : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        val state = intent.getIntExtra(AudioManager.EXTRA_SCO_AUDIO_STATE, -1)
        when (state) {
            AudioManager.SCO_AUDIO_STATE_CONNECTED -> {
                audioManager.isBluetoothScoOn = true
                _currentRoute.value = AudioRoute.Bluetooth
                scoTimeoutJob?.cancel()
            }
            AudioManager.SCO_AUDIO_STATE_ERROR,
            AudioManager.SCO_AUDIO_STATE_DISCONNECTED -> {
                audioManager.isBluetoothScoOn = false
                _currentRoute.value = AudioRoute.Speaker
                scoTimeoutJob?.cancel()
            }
        }
    }
}

private var scoTimeoutJob: Job? = null
```

Register the receiver in the class init or constructor. Unregister in `release()`.

- [ ] **Step 2: Update startBluetoothSco to be async**

At line 208, change `startBluetoothSco()`:

```kotlin
private fun startBluetoothSco() {
    try {
        @Suppress("DEPRECATION")
        audioManager.startBluetoothSco()
        // Do NOT set isBluetoothScoOn here — wait for BroadcastReceiver
        // Set a 3-second timeout fallback
        scoTimeoutJob?.cancel()
        scoTimeoutJob = scope.launch {
            delay(3000)
            if (_currentRoute.value != AudioRoute.Bluetooth) {
                logger.warning("Bluetooth SCO timeout — falling back to speaker")
                _currentRoute.value = AudioRoute.Speaker
            }
        }
    } catch (e: Exception) {
        logger.log(Level.WARNING, "Failed to start Bluetooth SCO", e)
        _currentRoute.value = AudioRoute.Speaker
    }
}
```

- [ ] **Step 3: Update switchRoute to defer Bluetooth route**

At `switchRoute()` (line 130), for the Bluetooth case, do not set `_currentRoute.value = route` synchronously. Let the BroadcastReceiver handle it:

```kotlin
AudioRoute.Bluetooth -> {
    startBluetoothSco()
    // Route will be updated by scoReceiver when SCO connects
}
```

- [ ] **Step 4: Build and verify**

Run: `cd mobile/android && ./gradlew test`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add mobile/android/app/src/main/java/io/wolftown/kaiku/data/voice/AudioRouteManager.kt
git commit -m "fix(voice): async Bluetooth SCO routing with BroadcastReceiver (#21)"
```

---

## Task 7: VoiceChannelScreen — Remove fallback (#23)

**Files:**
- Modify: `mobile/android/app/src/main/java/io/wolftown/kaiku/ui/voice/VoiceChannelScreen.kt:335`

- [ ] **Step 1: Remove size-1 fallback in findVideoTrackForStream**

At `VoiceChannelScreen.kt:335`, in `findVideoTrackForStream`, remove the block:

```kotlin
if (remoteVideoTracks.size == 1) {
    return remoteVideoTracks.values.firstOrNull()
}
```

Keep only the direct `streamId` lookup. Return null if no match found.

- [ ] **Step 2: Build and verify**

Run: `cd mobile/android && ./gradlew test`
Expected: PASS

- [ ] **Step 3: Commit**

```bash
git add mobile/android/app/src/main/java/io/wolftown/kaiku/ui/voice/VoiceChannelScreen.kt
git commit -m "fix(voice): remove findVideoTrackForStream fallback that mismatches multi-share tracks (#23)"
```

---

## Task 8: Final verification

- [ ] **Step 1: Run full test suite**

```bash
cd mobile/android && ./gradlew test
```
Expected: ALL PASS

- [ ] **Step 2: Self-review the branch diff**

```bash
git diff main...HEAD --stat
git log --oneline main..HEAD
```

Verify: 9 issues addressed, no regressions, no leftover static callback references.
