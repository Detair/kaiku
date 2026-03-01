# Screen Sharing Phase 3: Client Implementation

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Enable users to start/stop screen sharing from the client with quality selection and permission checks.

**Architecture:** Extend VoiceAdapter interface with screen share methods, implement browser-side capture via getDisplayMedia(), add UI components for the share button and quality picker.

**Tech Stack:** Solid.js, TypeScript, WebRTC getDisplayMedia API, existing VoiceAdapter pattern

---

## Task 1: Screen Share Types and VoiceAdapter Interface Extension

**Files:**
- Modify: `client/src/lib/webrtc/types.ts`
- Modify: `client/src/lib/types.ts` (VoiceParticipant)

**Step 1: Add screen share types to types.ts**

Add after the `AudioDeviceList` interface (line ~75):

```typescript
/**
 * Screen share quality tier
 */
export type ScreenShareQuality = "low" | "medium" | "high" | "premium";

/**
 * Options for starting a screen share
 */
export interface ScreenShareOptions {
  quality?: ScreenShareQuality;
  withAudio?: boolean;
}

/**
 * Result of a screen share attempt
 */
export type ScreenShareResult =
  | { approved: true; stream: MediaStream }
  | { approved: false; reason: "user_cancelled" | "permission_denied" | "no_source" };

/**
 * Information about an active screen share
 */
export interface ScreenShareInfo {
  user_id: string;
  username: string;
  source_label: string;
  has_audio: boolean;
  quality: ScreenShareQuality;
  started_at: string;
}

/**
 * Pre-capture permission check result
 */
export interface ScreenShareCheckResult {
  allowed: boolean;
  granted_quality: ScreenShareQuality;
  error?: "no_permission" | "limit_reached" | "not_in_channel";
}
```

**Step 2: Add screen share events to VoiceAdapterEvents**

Extend the `VoiceAdapterEvents` interface:

```typescript
export interface VoiceAdapterEvents {
  // ... existing events ...

  // Screen share events
  onScreenShareStarted?: (info: ScreenShareInfo) => void;
  onScreenShareStopped?: (userId: string, reason: string) => void;
  onScreenShareTrack?: (userId: string, track: MediaStreamTrack) => void;
  onScreenShareTrackRemoved?: (userId: string) => void;
}
```

**Step 3: Add screen share methods to VoiceAdapter interface**

Add to the `VoiceAdapter` interface:

```typescript
export interface VoiceAdapter {
  // ... existing methods ...

  // Screen sharing
  startScreenShare(options?: ScreenShareOptions): Promise<VoiceResult<void>>;
  stopScreenShare(): Promise<VoiceResult<void>>;
  isScreenSharing(): boolean;
}
```

**Step 4: Update VoiceParticipant in lib/types.ts**

Ensure VoiceParticipant has `screen_sharing` field:

```typescript
export interface VoiceParticipant {
  user_id: string;
  username?: string;
  display_name?: string;
  muted: boolean;
  speaking: boolean;
  screen_sharing: boolean;  // Add this field
}
```

**Step 5: Run TypeScript check**

Run: `cd client && bun run check`
Expected: Type errors for missing implementations in adapters

**Step 6: Commit**

```bash
git add client/src/lib/webrtc/types.ts client/src/lib/types.ts
git commit -m "feat(client): add screen share types to VoiceAdapter interface"
```

---

## Task 2: Browser Adapter - Screen Share Implementation

**Files:**
- Modify: `client/src/lib/webrtc/browser.ts`

**Step 1: Add screen share state to BrowserVoiceAdapter**

Add after the existing private fields (around line 30):

```typescript
// Screen share state
private screenShareStream: MediaStream | null = null;
private screenShareTrack: RTCRtpSender | null = null;
```

**Step 2: Implement isScreenSharing method**

Add method to the class:

```typescript
isScreenSharing(): boolean {
  return this.screenShareStream !== null;
}
```

**Step 3: Implement startScreenShare method**

Add method to the class:

```typescript
async startScreenShare(options?: ScreenShareOptions): Promise<VoiceResult<void>> {
  if (!this.peerConnection) {
    return { ok: false, error: { type: "not_connected" } };
  }

  if (this.screenShareStream) {
    return { ok: false, error: { type: "unknown", message: "Already sharing screen" } };
  }

  try {
    // Request display media with quality constraints
    const constraints = this.getDisplayMediaConstraints(options?.quality ?? "medium");

    const stream = await navigator.mediaDevices.getDisplayMedia({
      video: constraints.video,
      audio: options?.withAudio ?? false,
    });

    // Get the video track
    const videoTrack = stream.getVideoTracks()[0];
    if (!videoTrack) {
      stream.getTracks().forEach(t => t.stop());
      return { ok: false, error: { type: "unknown", message: "No video track in stream" } };
    }

    // Listen for track ending (user clicked "Stop sharing" in browser UI)
    videoTrack.onended = () => {
      console.log("[BrowserVoiceAdapter] Screen share track ended by user");
      this.handleScreenShareEnded();
    };

    // Add video track to peer connection
    this.screenShareTrack = this.peerConnection.addTrack(videoTrack, stream);

    // If audio track present, add it too
    const audioTrack = stream.getAudioTracks()[0];
    if (audioTrack) {
      this.peerConnection.addTrack(audioTrack, stream);
    }

    this.screenShareStream = stream;

    console.log("[BrowserVoiceAdapter] Screen share started", {
      hasAudio: !!audioTrack,
      quality: options?.quality ?? "medium",
    });

    return { ok: true, value: undefined };
  } catch (err) {
    console.error("[BrowserVoiceAdapter] Failed to start screen share:", err);

    // Handle specific errors
    if (err instanceof DOMException) {
      if (err.name === "NotAllowedError") {
        return { ok: false, error: { type: "permission_denied", message: "Screen share permission denied" } };
      }
      if (err.name === "AbortError") {
        // User cancelled the picker
        return { ok: false, error: { type: "unknown", message: "Screen share cancelled by user" } };
      }
    }

    return { ok: false, error: { type: "unknown", message: String(err) } };
  }
}
```

**Step 4: Add display media constraints helper**

Add private helper method:

```typescript
private getDisplayMediaConstraints(quality: ScreenShareQuality): DisplayMediaStreamOptions {
  const qualitySettings = {
    low: { width: 854, height: 480, frameRate: 15 },
    medium: { width: 1280, height: 720, frameRate: 30 },
    high: { width: 1920, height: 1080, frameRate: 30 },
    premium: { width: 1920, height: 1080, frameRate: 60 },
  };

  const settings = qualitySettings[quality];

  return {
    video: {
      cursor: "always",
      width: { ideal: settings.width, max: settings.width },
      height: { ideal: settings.height, max: settings.height },
      frameRate: { ideal: settings.frameRate, max: settings.frameRate },
    },
  };
}
```

**Step 5: Implement stopScreenShare method**

Add method:

```typescript
async stopScreenShare(): Promise<VoiceResult<void>> {
  if (!this.screenShareStream) {
    return { ok: false, error: { type: "unknown", message: "Not sharing screen" } };
  }

  try {
    // Remove track from peer connection
    if (this.screenShareTrack && this.peerConnection) {
      this.peerConnection.removeTrack(this.screenShareTrack);
    }

    // Stop all tracks in the stream
    this.screenShareStream.getTracks().forEach(track => track.stop());

    this.screenShareStream = null;
    this.screenShareTrack = null;

    console.log("[BrowserVoiceAdapter] Screen share stopped");
    return { ok: true, value: undefined };
  } catch (err) {
    console.error("[BrowserVoiceAdapter] Failed to stop screen share:", err);
    return { ok: false, error: { type: "unknown", message: String(err) } };
  }
}
```

**Step 6: Add handleScreenShareEnded helper**

Add private method to handle browser "Stop sharing" button:

```typescript
private handleScreenShareEnded(): void {
  this.screenShareStream = null;
  this.screenShareTrack = null;
  // Notify via event handler
  this.eventHandlers.onScreenShareStopped?.(this.userId ?? "unknown", "user_stopped");
}
```

**Step 7: Update leave() to cleanup screen share**

In the `leave()` method, add cleanup before closing peer connection:

```typescript
// In leave() method, add before peerConnection.close():
if (this.screenShareStream) {
  this.screenShareStream.getTracks().forEach(track => track.stop());
  this.screenShareStream = null;
  this.screenShareTrack = null;
}
```

**Step 8: Run TypeScript check**

Run: `cd client && bun run check`
Expected: PASS (or remaining errors from Tauri adapter)

**Step 9: Commit**

```bash
git add client/src/lib/webrtc/browser.ts
git commit -m "feat(client): implement screen share in BrowserVoiceAdapter"
```

---

## Task 3: Tauri Adapter - Screen Share Stubs

**Files:**
- Modify: `client/src/lib/webrtc/tauri.ts`

**Note:** Full Tauri implementation requires Rust-side work. This task adds TypeScript stubs that delegate to Tauri commands.

**Step 1: Add screen share state**

Add after existing private fields:

```typescript
private screenSharing = false;
```

**Step 2: Implement isScreenSharing**

```typescript
isScreenSharing(): boolean {
  return this.screenSharing;
}
```

**Step 3: Implement startScreenShare stub**

```typescript
async startScreenShare(options?: ScreenShareOptions): Promise<VoiceResult<void>> {
  console.log("[TauriVoiceAdapter] Starting screen share", options);

  try {
    await invoke("start_screen_share", {
      quality: options?.quality ?? "medium",
      withAudio: options?.withAudio ?? false,
    });
    this.screenSharing = true;
    return { ok: true, value: undefined };
  } catch (err) {
    console.error("[TauriVoiceAdapter] Failed to start screen share:", err);
    return { ok: false, error: this.mapTauriError(err) };
  }
}
```

**Step 4: Implement stopScreenShare stub**

```typescript
async stopScreenShare(): Promise<VoiceResult<void>> {
  console.log("[TauriVoiceAdapter] Stopping screen share");

  try {
    await invoke("stop_screen_share");
    this.screenSharing = false;
    return { ok: true, value: undefined };
  } catch (err) {
    console.error("[TauriVoiceAdapter] Failed to stop screen share:", err);
    return { ok: false, error: this.mapTauriError(err) };
  }
}
```

**Step 5: Add import for ScreenShareOptions**

Update import at top of file:

```typescript
import type {
  VoiceAdapter,
  VoiceConnectionState,
  VoiceError,
  VoiceResult,
  VoiceAdapterEvents,
  AudioDeviceList,
  ScreenShareOptions,  // Add this
} from "./types";
```

**Step 6: Run TypeScript check**

Run: `cd client && bun run check`
Expected: PASS

**Step 7: Commit**

```bash
git add client/src/lib/webrtc/tauri.ts
git commit -m "feat(client): add screen share stubs to TauriVoiceAdapter"
```

---

## Task 4: Voice Store - Screen Share State Management

**Files:**
- Modify: `client/src/stores/voice.ts`

**Step 1: Extend VoiceStoreState interface**

Update the interface:

```typescript
interface VoiceStoreState {
  // ... existing fields ...

  // Screen sharing
  screenSharing: boolean;
  screenShareInfo: ScreenShareInfo | null;
  screenShares: ScreenShareInfo[];  // All active screen shares in channel
}
```

**Step 2: Update initial state**

```typescript
const [voiceState, setVoiceState] = createStore<VoiceStoreState>({
  // ... existing fields ...
  screenSharing: false,
  screenShareInfo: null,
  screenShares: [],
});
```

**Step 3: Add screen share imports**

```typescript
import type { ScreenShareInfo, ScreenShareCheckResult } from "@/lib/webrtc/types";
```

**Step 4: Add startScreenShare function**

```typescript
/**
 * Start screen sharing.
 */
export async function startScreenShare(
  quality?: ScreenShareQuality
): Promise<{ ok: boolean; error?: string }> {
  if (voiceState.state !== "connected" || !voiceState.channelId) {
    return { ok: false, error: "Not connected to voice channel" };
  }

  if (voiceState.screenSharing) {
    return { ok: false, error: "Already sharing screen" };
  }

  const adapter = await createVoiceAdapter();

  const result = await adapter.startScreenShare({ quality });

  if (!result.ok) {
    console.error("Failed to start screen share:", result.error);
    return { ok: false, error: getErrorMessage(result.error) };
  }

  setVoiceState({ screenSharing: true });
  return { ok: true };
}
```

**Step 5: Add stopScreenShare function**

```typescript
/**
 * Stop screen sharing.
 */
export async function stopScreenShare(): Promise<void> {
  if (!voiceState.screenSharing) return;

  const adapter = await createVoiceAdapter();
  const result = await adapter.stopScreenShare();

  if (!result.ok) {
    console.error("Failed to stop screen share:", result.error);
  }

  setVoiceState({
    screenSharing: false,
    screenShareInfo: null,
  });
}
```

**Step 6: Add event listeners for screen share events**

In the Tauri event listener setup (initVoice):

```typescript
// Screen share events
unlisteners.push(
  await listen<ScreenShareInfo>("ws:screen_share_started", (event) => {
    const info = event.payload;
    if (info.channel_id === voiceState.channelId) {
      setVoiceState(
        produce((state) => {
          state.screenShares.push(info);
          // Update participant's screen_sharing flag
          if (state.participants[info.user_id]) {
            state.participants[info.user_id].screen_sharing = true;
          }
        })
      );
    }
  })
);

unlisteners.push(
  await listen<{ channel_id: string; user_id: string; reason: string }>(
    "ws:screen_share_stopped",
    (event) => {
      const { channel_id, user_id } = event.payload;
      if (channel_id === voiceState.channelId) {
        setVoiceState(
          produce((state) => {
            state.screenShares = state.screenShares.filter(s => s.user_id !== user_id);
            // Update participant's screen_sharing flag
            if (state.participants[user_id]) {
              state.participants[user_id].screen_sharing = false;
            }
            // If it was us, clear local state
            if (state.screenShareInfo?.user_id === user_id) {
              state.screenSharing = false;
              state.screenShareInfo = null;
            }
          })
        );
      }
    }
  )
);
```

**Step 7: Update leaveVoice to cleanup screen share**

In `leaveVoice()`:

```typescript
setVoiceState({
  state: "disconnected",
  channelId: null,
  participants: {},
  speaking: false,
  screenSharing: false,        // Add
  screenShareInfo: null,       // Add
  screenShares: [],            // Add
});
```

**Step 8: Export new functions**

Add to exports:

```typescript
export {
  voiceState,
  setVoiceState,
  startScreenShare,    // Add
  stopScreenShare,     // Add
};
```

**Step 9: Run TypeScript check**

Run: `cd client && bun run check`
Expected: PASS

**Step 10: Commit**

```bash
git add client/src/stores/voice.ts
git commit -m "feat(client): add screen share state management to voice store"
```

---

## Task 5: Screen Share Button Component

**Files:**
- Create: `client/src/components/voice/ScreenShareButton.tsx`
- Modify: `client/src/components/voice/VoiceControls.tsx`

**Step 1: Create ScreenShareButton component**

```typescript
import { Component, Show, createSignal } from "solid-js";
import { MonitorUp, MonitorOff } from "lucide-solid";
import { voiceState, startScreenShare, stopScreenShare } from "@/stores/voice";

interface ScreenShareButtonProps {
  onShowQualityPicker?: () => void;
}

/**
 * Screen share toggle button.
 */
const ScreenShareButton: Component<ScreenShareButtonProps> = (props) => {
  const [loading, setLoading] = createSignal(false);
  const [error, setError] = createSignal<string | null>(null);

  const handleClick = async () => {
    setError(null);

    if (voiceState.screenSharing) {
      // Stop sharing
      setLoading(true);
      try {
        await stopScreenShare();
      } finally {
        setLoading(false);
      }
    } else {
      // Show quality picker before starting
      props.onShowQualityPicker?.();
    }
  };

  return (
    <div class="relative">
      <button
        onClick={handleClick}
        disabled={voiceState.state !== "connected" || loading()}
        class={`p-2 rounded-full transition-colors ${
          voiceState.screenSharing
            ? "bg-success/20 text-success hover:bg-danger/20 hover:text-danger"
            : "bg-background-secondary text-text-secondary hover:bg-background-primary hover:text-text-primary"
        } ${loading() ? "opacity-50 cursor-wait" : ""}`}
        title={voiceState.screenSharing ? "Stop Sharing" : "Share Screen"}
      >
        {voiceState.screenSharing ? (
          <MonitorOff class="w-5 h-5" />
        ) : (
          <MonitorUp class="w-5 h-5" />
        )}
      </button>

      <Show when={error()}>
        <div class="absolute bottom-full left-1/2 -translate-x-1/2 mb-2 px-2 py-1 bg-danger text-white text-xs rounded whitespace-nowrap">
          {error()}
        </div>
      </Show>
    </div>
  );
};

export default ScreenShareButton;
```

**Step 2: Update VoiceControls to include ScreenShareButton**

Update imports:

```typescript
import { Component, createSignal, Show } from "solid-js";
import { Mic, MicOff, Headphones, VolumeX, Settings } from "lucide-solid";
import { voiceState, toggleMute, toggleDeafen } from "@/stores/voice";
import MicrophoneTest from "./MicrophoneTest";
import ScreenShareButton from "./ScreenShareButton";
import ScreenShareQualityPicker from "./ScreenShareQualityPicker";
```

Add quality picker state:

```typescript
const VoiceControls: Component = () => {
  const [showMicTest, setShowMicTest] = createSignal(false);
  const [showQualityPicker, setShowQualityPicker] = createSignal(false);
```

Add ScreenShareButton to the controls (after deafen button):

```typescript
{/* Screen share button */}
<ScreenShareButton onShowQualityPicker={() => setShowQualityPicker(true)} />
```

Add quality picker modal:

```typescript
{/* Screen Share Quality Picker */}
<Show when={showQualityPicker()}>
  <ScreenShareQualityPicker onClose={() => setShowQualityPicker(false)} />
</Show>
```

**Step 3: Run TypeScript check**

Run: `cd client && bun run check`
Expected: Error for missing ScreenShareQualityPicker (expected, will create next)

**Step 4: Commit partial progress**

```bash
git add client/src/components/voice/ScreenShareButton.tsx client/src/components/voice/VoiceControls.tsx
git commit -m "feat(client): add screen share button component"
```

---

## Task 6: Screen Share Quality Picker Component

**Files:**
- Create: `client/src/components/voice/ScreenShareQualityPicker.tsx`

**Step 1: Create the quality picker component**

```typescript
import { Component, createSignal, For } from "solid-js";
import { X, Monitor } from "lucide-solid";
import { startScreenShare } from "@/stores/voice";
import type { ScreenShareQuality } from "@/lib/webrtc/types";

interface ScreenShareQualityPickerProps {
  onClose: () => void;
}

const qualityOptions: { value: ScreenShareQuality; label: string; description: string; premium?: boolean }[] = [
  { value: "low", label: "480p 15fps", description: "Best for slow connections" },
  { value: "medium", label: "720p 30fps", description: "Recommended" },
  { value: "high", label: "1080p 30fps", description: "Good connections" },
  { value: "premium", label: "1080p 60fps", description: "Premium only", premium: true },
];

/**
 * Quality selection dialog shown before starting screen share.
 */
const ScreenShareQualityPicker: Component<ScreenShareQualityPickerProps> = (props) => {
  const [selectedQuality, setSelectedQuality] = createSignal<ScreenShareQuality>("medium");
  const [loading, setLoading] = createSignal(false);
  const [error, setError] = createSignal<string | null>(null);

  const handleStart = async () => {
    setError(null);
    setLoading(true);

    try {
      const result = await startScreenShare(selectedQuality());

      if (!result.ok) {
        setError(result.error ?? "Failed to start screen share");
        return;
      }

      props.onClose();
    } finally {
      setLoading(false);
    }
  };

  const handleBackdropClick = (e: MouseEvent) => {
    if (e.target === e.currentTarget) {
      props.onClose();
    }
  };

  return (
    <div
      class="fixed inset-0 bg-black/50 flex items-center justify-center z-50"
      onClick={handleBackdropClick}
    >
      <div class="bg-background-secondary rounded-lg shadow-xl w-full max-w-sm mx-4">
        {/* Header */}
        <div class="flex items-center justify-between p-4 border-b border-background-primary">
          <div class="flex items-center gap-2">
            <Monitor class="w-5 h-5 text-primary" />
            <h2 class="text-lg font-semibold text-text-primary">Share Screen</h2>
          </div>
          <button
            onClick={props.onClose}
            class="p-1 text-text-muted hover:text-text-primary transition-colors"
          >
            <X class="w-5 h-5" />
          </button>
        </div>

        {/* Quality options */}
        <div class="p-4 space-y-2">
          <p class="text-sm text-text-secondary mb-3">Select quality:</p>

          <For each={qualityOptions}>
            {(option) => (
              <label
                class={`flex items-center gap-3 p-3 rounded-lg cursor-pointer transition-colors ${
                  selectedQuality() === option.value
                    ? "bg-primary/20 border border-primary"
                    : "bg-background-primary hover:bg-background-tertiary border border-transparent"
                } ${option.premium ? "opacity-50 cursor-not-allowed" : ""}`}
              >
                <input
                  type="radio"
                  name="quality"
                  value={option.value}
                  checked={selectedQuality() === option.value}
                  onChange={() => !option.premium && setSelectedQuality(option.value)}
                  disabled={option.premium}
                  class="w-4 h-4 text-primary"
                />
                <div class="flex-1">
                  <div class="flex items-center gap-2">
                    <span class="text-sm font-medium text-text-primary">{option.label}</span>
                    {option.premium && (
                      <span class="text-xs px-1.5 py-0.5 bg-warning/20 text-warning rounded">
                        Premium
                      </span>
                    )}
                  </div>
                  <span class="text-xs text-text-muted">{option.description}</span>
                </div>
              </label>
            )}
          </For>
        </div>

        {/* Error message */}
        {error() && (
          <div class="px-4 pb-2">
            <p class="text-sm text-danger">{error()}</p>
          </div>
        )}

        {/* Actions */}
        <div class="flex justify-end gap-2 p-4 border-t border-background-primary">
          <button
            onClick={props.onClose}
            class="px-4 py-2 text-sm text-text-secondary hover:text-text-primary transition-colors"
          >
            Cancel
          </button>
          <button
            onClick={handleStart}
            disabled={loading()}
            class="px-4 py-2 text-sm bg-primary text-white rounded-lg hover:bg-primary/90 transition-colors disabled:opacity-50"
          >
            {loading() ? "Starting..." : "Start Sharing"}
          </button>
        </div>
      </div>
    </div>
  );
};

export default ScreenShareQualityPicker;
```

**Step 2: Run TypeScript check**

Run: `cd client && bun run check`
Expected: PASS

**Step 3: Commit**

```bash
git add client/src/components/voice/ScreenShareQualityPicker.tsx
git commit -m "feat(client): add screen share quality picker component"
```

---

## Task 7: WebSocket Event Handler Integration

**Files:**
- Modify: `client/src/stores/websocket.ts` (or wherever WS events are handled for browser)

**Step 1: Find the WebSocket event handler**

Search for where `voice_room_state` events are handled in the browser store.

**Step 2: Add screen share event handlers**

Add handlers for:
- `screen_share_started` - Update participants and screenShares list
- `screen_share_stopped` - Remove from screenShares, update participant flag
- `screen_share_quality_changed` - Update quality in screenShares list

Pattern to follow (based on existing voice events):

```typescript
case "screen_share_started": {
  const info = data as ScreenShareInfo & { channel_id: string };
  if (info.channel_id === voiceState.channelId) {
    setVoiceState(
      produce((state) => {
        state.screenShares.push(info);
        if (state.participants[info.user_id]) {
          state.participants[info.user_id].screen_sharing = true;
        }
      })
    );
  }
  break;
}

case "screen_share_stopped": {
  const { channel_id, user_id, reason } = data;
  if (channel_id === voiceState.channelId) {
    setVoiceState(
      produce((state) => {
        state.screenShares = state.screenShares.filter(s => s.user_id !== user_id);
        if (state.participants[user_id]) {
          state.participants[user_id].screen_sharing = false;
        }
      })
    );
  }
  break;
}
```

**Step 3: Update voice_room_state handler to include screen_shares**

The server already sends `screen_shares` in `VoiceRoomState`. Ensure the handler processes it:

```typescript
case "voice_room_state": {
  const { channel_id, participants, screen_shares } = data;
  if (channel_id === voiceState.channelId) {
    setVoiceState({
      participants: Object.fromEntries(participants.map(p => [p.user_id, p])),
      screenShares: screen_shares ?? [],
    });
  }
  break;
}
```

**Step 4: Run TypeScript check**

Run: `cd client && bun run check`
Expected: PASS

**Step 5: Commit**

```bash
git add client/src/stores/websocket.ts
git commit -m "feat(client): add screen share WebSocket event handlers"
```

---

## Task 8: Integration Test - Manual Browser Test

**Files:** None (manual testing)

**Step 1: Start the development environment**

```bash
# Terminal 1: Server
cd server && cargo run

# Terminal 2: Client
cd client && bun run dev
```

**Step 2: Test flow**

1. Open browser to localhost:5173
2. Login and join a voice channel
3. Click the screen share button
4. Verify quality picker appears
5. Select a quality and click "Start Sharing"
6. Verify browser's screen picker appears
7. Select a screen/window
8. Verify screen share starts (button changes state)
9. Click button again to stop sharing
10. Verify screen share stops

**Step 3: Check console for errors**

Open browser DevTools console and verify no errors during the flow.

**Step 4: Document any issues found**

If issues are found, create follow-up tasks.

---

## Task 9: Participant Screen Share Indicator

**Files:**
- Modify: `client/src/components/voice/VoicePanel.tsx`

**Step 1: Add screen share indicator to participant display**

Update the participant rendering:

```typescript
<For each={participants()}>
  {(participant) => (
    <div
      class={`flex items-center gap-1 px-2 py-1 rounded text-xs ${
        participant.speaking
          ? "bg-success/20 text-success"
          : "bg-background-secondary text-text-secondary"
      } ${participant.muted ? "opacity-50" : ""}`}
      title={participant.muted ? "Muted" : undefined}
    >
      <div class="w-4 h-4 rounded-full bg-primary/50" />
      <span class="truncate max-w-20">{participant.user_id.slice(0, 8)}</span>
      {participant.screen_sharing && (
        <MonitorUp class="w-3 h-3 text-success" />
      )}
    </div>
  )}
</For>
```

**Step 2: Add import**

```typescript
import { PhoneOff, Signal, MonitorUp } from "lucide-solid";
```

**Step 3: Run TypeScript check**

Run: `cd client && bun run check`
Expected: PASS

**Step 4: Commit**

```bash
git add client/src/components/voice/VoicePanel.tsx
git commit -m "feat(client): add screen share indicator to participant list"
```

---

## Task 10: Final Verification and Cleanup

**Step 1: Run full TypeScript check**

```bash
cd client && bun run check
```
Expected: PASS

**Step 2: Run linter**

```bash
cd client && bun run lint
```
Expected: PASS (or fix any issues)

**Step 3: Run client tests**

```bash
cd client && bun test
```
Expected: PASS

**Step 4: Final commit if any fixes needed**

```bash
git add -A
git commit -m "chore(client): cleanup Phase 3 implementation"
```

**Step 5: Update CHANGELOG.md**

Add under `[Unreleased]` → `### Added`:

```markdown
- Screen sharing capability with quality selection (browser)
- Screen share button in voice controls
- Quality picker for screen share (480p/720p/1080p options)
- Screen share indicator on participant avatars
```

```bash
git add CHANGELOG.md
git commit -m "docs: update changelog for Phase 3 screen sharing"
```

---

## Summary

Phase 3 implements the client-side screen sharing functionality:

1. **Types** - Extended VoiceAdapter interface with screen share methods and types
2. **Browser Adapter** - Full getDisplayMedia() integration with quality constraints
3. **Tauri Adapter** - Stubs for future Rust-side implementation
4. **Voice Store** - Screen share state management and event handling
5. **UI Components** - ScreenShareButton and ScreenShareQualityPicker
6. **WebSocket Integration** - Event handlers for screen share notifications
7. **Participant Indicator** - Visual indicator for users sharing their screen

The Tauri implementation is stubbed - full native screen capture will require Phase 3b with Rust-side work.

## Implementation Status

**Completed:** 2026-01-23

All Phase 3 tasks have been implemented:
- `client/src/lib/webrtc/types.ts` - ScreenShareOptions, ScreenShareResult, ScreenShareInfo types
- `client/src/lib/webrtc/browser.ts` - Full getDisplayMedia() implementation with quality constraints
- `client/src/lib/webrtc/tauri.ts` - Stubs that invoke Tauri commands
- `client/src/stores/voice.ts` - startScreenShare/stopScreenShare exports, screenShares state
- `client/src/components/voice/ScreenShareButton.tsx` - Toggle button component
- `client/src/components/voice/ScreenShareQualityPicker.tsx` - Quality selection dialog
- `client/src/stores/websocket.ts` - screen_share_started/stopped event handlers
- `client/src/components/voice/VoicePanel.tsx` - Screen share indicator on participants
