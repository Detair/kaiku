# Voice Features Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Fix screen share track race condition + add a voice channel main view showing participants, webcams, and screen shares.

**Architecture:** Feature A fixes a Solid.js state batching issue in the screen share track registration. Feature B adds a new VoiceChannelView component that replaces MessageList when a voice channel is selected, showing a participant grid and screen share thumbnails.

**Tech Stack:** Solid.js, TanStack Virtual, WebRTC, UnoCSS

---

## Feature A: Screen Share Track Fix

### Task 1: Fix race condition in voice.ts

**Files:**
- Modify: `client/src/stores/voice.ts:439-454`

**Step 1: Fix the addAvailableTrack + startViewing race**

The `startViewing()` call runs before the store update from `addAvailableTrack()` flushes. Move `startViewing` into `addAvailableTrack` so it runs after the store update:

```typescript
// In voice.ts onScreenShareTrack handler (~line 439-454)
// Replace:
onScreenShareTrack: (userId, streamId, track) => {
  console.log("[Voice] Screen share track received:", userId, streamId);
  import("@/stores/screenShareViewer").then(
    ({ addAvailableTrack, startViewing }) => {
      const shareInfo = voiceState.screenShares.find(
        (s) => s.stream_id === streamId,
      );
      const username = shareInfo?.username || userId.slice(0, 8);
      const sourceLabel = shareInfo?.source_label || "Screen";
      addAvailableTrack(streamId, track, userId, username, sourceLabel);
      startViewing(streamId);
    },
  );
},

// With:
onScreenShareTrack: (userId, streamId, track) => {
  console.log("[Voice] Screen share track received:", userId, streamId);
  import("@/stores/screenShareViewer").then(
    ({ addAvailableTrack }) => {
      const shareInfo = voiceState.screenShares.find(
        (s) => s.stream_id === streamId,
      );
      const username = shareInfo?.username || userId.slice(0, 8);
      const sourceLabel = shareInfo?.source_label || "Screen";
      addAvailableTrack(streamId, track, userId, username, sourceLabel);
    },
  );
},
```

**Step 2: Auto-start viewing inside addAvailableTrack**

In `screenShareViewer.ts`, after registering the track, auto-start viewing if nothing is currently being viewed:

```typescript
// In addAvailableTrack, after setViewerState({ availableTracks: newTracks }):
// Auto-view if no stream is currently being viewed
if (!viewerState.viewingStreamId) {
  setViewerState({
    viewingStreamId: streamId,
    videoTrack: track,
  });
}
```

**Step 3: Build and verify**

Run: `cd client && bun run build`
Expected: `✓ built`

**Step 4: Commit**

```bash
git add client/src/stores/voice.ts client/src/stores/screenShareViewer.ts
git commit -m "fix(client): fix screen share track race condition

Move startViewing into addAvailableTrack so it runs after the store
update. Auto-start viewing when no stream is currently viewed."
```

---

### Task 2: Guard nil UUID on server

**Files:**
- Modify: `server/src/voice/sfu.rs:649`

**Step 1: Replace nil UUID fallback with warning + skip**

```rust
// Replace line 649:
//   .unwrap_or(TrackSource::ScreenVideo(Uuid::nil())),
// With:
.unwrap_or_else(|| {
    warn!("No pending video source for peer, skipping track");
    return;
}),
```

Note: this requires restructuring the match arm to handle the early return. If the refactor is too invasive, alternatively log a warning and keep the nil UUID but add a guard in the track creation code to skip nil stream IDs.

**Step 2: Build and verify**

Run: `SQLX_OFFLINE=true cargo clippy -p vc-server -- -D warnings`

**Step 3: Commit**

---

## Feature B: Voice Channel Main View

### Task 3: Create VoiceChannelView component

**Files:**
- Create: `client/src/components/voice/VoiceChannelView.tsx`

**Step 1: Create the component**

```tsx
import { Component, For, Show, createMemo } from "solid-js";
import { Users, MonitorPlay } from "lucide-solid";
import { voiceState, getParticipants, isInChannel, joinVoice, leaveVoice } from "@/stores/voice";
import { viewerState, startViewing } from "@/stores/screenShareViewer";
import VoiceControls from "./VoiceControls";
import { QualityIndicator } from "./QualityIndicator";
import { getParticipantMetrics } from "@/stores/voice";
import { showToast } from "@/components/ui/Toast";

interface VoiceChannelViewProps {
  channelId: string;
  channelName: string;
}

const VoiceChannelView: Component<VoiceChannelViewProps> = (props) => {
  const isConnected = () => voiceState.state === "connected" && voiceState.channelId === props.channelId;
  const isConnecting = () => voiceState.state === "connecting";
  const participants = createMemo(() => getParticipants());
  const screenShares = () => voiceState.screenShares || [];

  const handleJoin = async () => {
    try {
      await joinVoice(props.channelId);
    } catch (err) {
      showToast({ type: "error", title: "Could not join voice channel", duration: 8000 });
    }
  };

  return (
    <div class="flex-1 flex flex-col min-h-0">
      {/* Not connected — preview */}
      <Show when={!isConnected()}>
        <div class="flex-1 flex flex-col items-center justify-center gap-6 px-8">
          <div class="w-20 h-20 rounded-full bg-accent-primary/20 flex items-center justify-center">
            <Users class="w-10 h-10 text-text-secondary" />
          </div>
          <div class="text-center">
            <h2 class="text-xl font-semibold text-text-primary mb-2">{props.channelName}</h2>
            <p class="text-text-secondary text-sm">
              <Show when={participants().length > 0} fallback="No one is in this channel yet.">
                {participants().length} {participants().length === 1 ? "person" : "people"} connected
              </Show>
            </p>
          </div>
          <button
            onClick={handleJoin}
            disabled={isConnecting()}
            class="px-8 py-3 bg-accent-primary text-on-accent rounded-xl font-semibold text-lg hover:opacity-90 transition-opacity disabled:opacity-50"
          >
            {isConnecting() ? "Connecting..." : "Join Voice"}
          </button>
        </div>
      </Show>

      {/* Connected — participant grid + controls */}
      <Show when={isConnected()}>
        <div class="flex-1 overflow-y-auto p-6">
          {/* Participant grid */}
          <div class="flex flex-wrap gap-4 justify-center mb-6">
            <For each={participants()}>
              {(participant) => {
                const metrics = () => getParticipantMetrics(participant.user_id);
                const isSpeaking = () => participant.speaking;
                return (
                  <div
                    class="w-32 flex flex-col items-center gap-2 p-4 rounded-xl transition-all"
                    classList={{
                      "bg-accent-primary/10 ring-2 ring-accent-primary/40": isSpeaking(),
                      "bg-surface-layer2": !isSpeaking(),
                    }}
                  >
                    {/* Avatar or webcam placeholder */}
                    <div class="w-16 h-16 rounded-full bg-surface-highlight flex items-center justify-center text-2xl font-bold text-text-primary">
                      {participant.display_name?.charAt(0)?.toUpperCase() || participant.username.charAt(0).toUpperCase()}
                    </div>
                    <span class="text-sm text-text-primary font-medium truncate w-full text-center">
                      {participant.display_name || participant.username}
                    </span>
                    <div class="flex items-center gap-1.5">
                      <QualityIndicator metrics={metrics() ?? null} mode="circle" />
                      <Show when={participant.muted}>
                        <span class="text-accent-danger text-xs" title="Muted">🔇</span>
                      </Show>
                      <Show when={participant.screen_sharing}>
                        <span class="text-accent-primary text-xs" title="Screen sharing">📺</span>
                      </Show>
                    </div>
                  </div>
                );
              }}
            </For>
          </div>

          {/* Screen share thumbnails */}
          <Show when={screenShares().length > 0}>
            <div class="mt-4">
              <h3 class="text-sm font-semibold text-text-secondary uppercase tracking-wide mb-3 flex items-center gap-2">
                <MonitorPlay class="w-4 h-4" />
                Screen Shares
              </h3>
              <div class="flex flex-wrap gap-3">
                <For each={screenShares()}>
                  {(share) => (
                    <button
                      onClick={() => startViewing(share.stream_id)}
                      class="flex items-center gap-2 px-4 py-2 rounded-lg bg-surface-layer2 hover:bg-surface-highlight text-text-primary text-sm transition-colors"
                    >
                      <MonitorPlay class="w-4 h-4 text-accent-primary" />
                      {share.username}'s screen
                      <Show when={share.source_label && share.source_label !== "Screen"}>
                        <span class="text-text-secondary">({share.source_label})</span>
                      </Show>
                    </button>
                  )}
                </For>
              </div>
            </div>
          </Show>
        </div>

        {/* Controls bar */}
        <div class="px-6 py-3 border-t border-white/10 bg-surface-layer1 flex items-center justify-center">
          <VoiceControls />
        </div>
      </Show>
    </div>
  );
};

export default VoiceChannelView;
```

**Step 2: Build and verify**

Run: `bun run build`

**Step 3: Commit**

```bash
git add client/src/components/voice/VoiceChannelView.tsx
git commit -m "feat(client): add VoiceChannelView component

Participant grid with speaking indicators, screen share buttons,
join voice preview, and controls bar."
```

---

### Task 4: Integrate into Main.tsx

**Files:**
- Modify: `client/src/views/Main.tsx:264-275`

**Step 1: Add conditional rendering for voice channels**

Replace the current MessageList/TypingIndicator/MessageInput block with a voice/text split:

```tsx
// Add import at top of Main.tsx:
import VoiceChannelView from "@/components/voice/VoiceChannelView";

// Replace lines 264-275 (the Messages/TypingIndicator/MessageInput block):
<Show
  when={channel()?.channel_type !== "voice"}
  fallback={
    <VoiceChannelView
      channelId={channel()!.id}
      channelName={channel()!.name}
    />
  }
>
  {/* Messages */}
  <MessageList channelId={channel()!.id} guildId={guildsState.activeGuildId ?? undefined} />
  {/* Typing Indicator */}
  <TypingIndicator channelId={channel()!.id} />
  {/* Message Input */}
  <MessageInput
    channelId={channel()!.id}
    channelName={channel()!.name}
    guildId={guildsState.activeGuildId ?? undefined}
  />
</Show>
```

**Step 2: Build and verify**

Run: `bun run build`

**Step 3: Commit**

```bash
git add client/src/views/Main.tsx
git commit -m "feat(client): render VoiceChannelView for voice channels

Voice channels now show participant grid instead of message list.
Text channels continue showing MessageList as before."
```

---

### Task 5: Deploy and verify

**Step 1: Build server (if Task 2 was done)**

```bash
./infra/scripts/build-and-push.sh
```

**Step 2: Deploy**

```bash
./infra/scripts/deploy.sh
```

**Step 3: Verify**
- Select a voice channel → see participant preview + "Join Voice" button
- Join → see participant grid with speaking indicators
- Start screen share → see thumbnail button
- Click screen share button → viewer opens without "no track" error
