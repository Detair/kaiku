# Screen Share Track Negotiation Fix — Design

**Date:** 2026-03-21
**Status:** Planned

## Problem

When clicking "View" on a screen share, the error `Cannot start viewing — no track for stream` appears. The track hasn't been registered in `availableTracks` before `startViewing()` is called.

## Root Cause

**Solid.js state batching race condition** in `stores/voice.ts:451-452`:

```typescript
addAvailableTrack(streamId, track, userId, username, sourceLabel);
startViewing(streamId);  // Called immediately, before store update flushes
```

`addAvailableTrack()` calls `setViewerState()` which is batched by Solid.js. `startViewing()` runs synchronously and tries to read from the store — but the update hasn't flushed yet, so `availableTracks.get(streamId)` returns undefined.

### Secondary issues

1. **Uuid::nil() fallback** (server `sfu.rs:649`): If the pending source queue misses, the track gets labeled with a nil UUID, creating a stream ID mismatch on the client
2. **No retry on startViewing**: If the track isn't ready, the attempt is silently dropped
3. **Event timing**: `ScreenShareStarted` broadcast happens before WebRTC tracks are actually ready

## Fix

### Step 1: Fix the race condition (client)

In `stores/voice.ts`, wrap the `addAvailableTrack` + `startViewing` sequence in `batch()` from `solid-js`:

```typescript
import { batch } from "solid-js";

batch(() => {
  addAvailableTrack(streamId, track, userId, username, sourceLabel);
  startViewing(streamId);
});
```

Or simpler: move the `startViewing` call into `addAvailableTrack` itself, triggered after the store update.

### Step 2: Add retry to startViewing (client)

In `screenShareViewer.ts`, if the track isn't found, schedule a retry:

```typescript
export function startViewing(streamId: string): void {
  const info = viewerState.availableTracks.get(streamId);
  if (!info) {
    // Retry once after microtask (store update may be pending)
    queueMicrotask(() => {
      const retryInfo = viewerState.availableTracks.get(streamId);
      if (retryInfo) startViewingInternal(retryInfo, streamId);
    });
    return;
  }
  startViewingInternal(info, streamId);
}
```

### Step 3: Guard nil UUID on server (server)

In `sfu.rs:649`, log a warning and skip the track instead of using `Uuid::nil()` as a fallback stream ID. Nil UUIDs cause phantom tracks that can never be matched.

### Files to modify

**Client:**
- `client/src/stores/voice.ts` — batch the addAvailableTrack + startViewing calls
- `client/src/stores/screenShareViewer.ts` — add retry logic to startViewing

**Server:**
- `server/src/voice/sfu.rs` — guard against nil UUID fallback
