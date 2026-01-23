# Cross-Client Read Sync & Do Not Disturb - Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Sync DM read status across devices in real-time and suppress notification sounds during DND/quiet hours.

**Architecture:** New `user:{user_id}` Redis pub/sub channel for user-targeted events. DND checks user status and configurable quiet hours schedule before playing sounds.

**Tech Stack:** Rust (axum, fred Redis), TypeScript (Solid.js), localStorage for settings persistence

---

## Batch 1: Cross-Client Read Sync (Backend)

### Task 1: Add user_events Redis channel function

**Files:**
- Modify: `server/src/ws/mod.rs`

**Step 1: Add the channel function**

In the `channels` module (around line 447), add:

```rust
/// Redis channel for user-specific events (read sync, etc.)
#[must_use]
pub fn user_events(user_id: Uuid) -> String {
    format!("user:{user_id}")
}
```

**Step 2: Add broadcast_to_user function**

After `broadcast_admin_event` function (around line 500), add:

```rust
/// Broadcast an event to a specific user's sessions via Redis.
pub async fn broadcast_to_user(
    redis: &RedisClient,
    user_id: Uuid,
    event: &ServerEvent,
) -> Result<(), RedisError> {
    let payload = serde_json::to_string(event)
        .map_err(|e| RedisError::new(RedisErrorKind::Parse, format!("JSON error: {e}")))?;

    redis
        .publish::<(), _, _>(channels::user_events(user_id), payload)
        .await?;

    Ok(())
}
```

**Step 3: Verify compilation**

Run: `cd /home/detair/GIT/canis/.worktrees/read-sync-dnd/server && cargo check`
Expected: Compiles with no errors

**Step 4: Commit**

```bash
git add server/src/ws/mod.rs
git commit -m "feat(ws): add user_events Redis channel and broadcast_to_user helper"
```

---

### Task 2: Add DmRead ServerEvent variant

**Files:**
- Modify: `server/src/ws/mod.rs`

**Step 1: Add the event variant**

In the `ServerEvent` enum (around line 150), add after `Pong`:

```rust
    /// DM marked as read (for cross-device sync)
    DmRead {
        /// Channel that was marked as read
        channel_id: Uuid,
        /// Last message that was read (if any)
        last_read_message_id: Option<Uuid>,
    },
```

**Step 2: Verify compilation**

Run: `cd /home/detair/GIT/canis/.worktrees/read-sync-dnd/server && cargo check`
Expected: Compiles with no errors

**Step 3: Commit**

```bash
git add server/src/ws/mod.rs
git commit -m "feat(ws): add DmRead server event for read sync"
```

---

### Task 3: Subscribe to own user channel on WebSocket connect

**Files:**
- Modify: `server/src/ws/mod.rs`

**Step 1: Pass user_id to handle_pubsub**

Find the `handle_pubsub` spawn call (around line 598). The function signature already takes `friend_ids`. We need to also pass `user_id`.

Update the spawn call:

```rust
let pubsub_handle = tokio::spawn(async move {
    handle_pubsub(redis_client, tx_clone, subscribed_clone, admin_subscribed_clone, friend_ids, user_id).await;
});
```

**Step 2: Update handle_pubsub signature**

Find the `handle_pubsub` function (around line 826). Update signature to:

```rust
async fn handle_pubsub(
    redis: RedisClient,
    tx: mpsc::Sender<ServerEvent>,
    subscribed_channels: Arc<tokio::sync::RwLock<HashSet<Uuid>>>,
    admin_subscribed: Arc<tokio::sync::RwLock<bool>>,
    friend_ids: Vec<Uuid>,
    user_id: Uuid,
) {
```

**Step 3: Subscribe to own user channel**

After subscribing to friends' presence channels (around line 872), add:

```rust
    // Subscribe to own user channel for cross-device sync
    let user_channel = channels::user_events(user_id);
    if let Err(e) = subscriber.subscribe(&user_channel).await {
        warn!("Failed to subscribe to user events channel: {}", e);
    } else {
        debug!("Subscribed to user events channel: {}", user_channel);
    }
```

**Step 4: Handle user events in pubsub loop**

In the message handling loop (around line 906), after the presence handling block, add:

```rust
        // Handle user events (user:{uuid})
        else if channel_name.starts_with("user:") {
            // Forward all user-targeted events (read sync, etc.)
            if let Some(payload) = message.value.as_str() {
                if let Ok(event) = serde_json::from_str::<ServerEvent>(payload) {
                    if tx.send(event).await.is_err() {
                        break;
                    }
                }
            }
        }
```

**Step 5: Verify compilation**

Run: `cd /home/detair/GIT/canis/.worktrees/read-sync-dnd/server && cargo check`
Expected: Compiles with no errors

**Step 6: Commit**

```bash
git add server/src/ws/mod.rs
git commit -m "feat(ws): subscribe to user channel for cross-device sync"
```

---

### Task 4: Broadcast DmRead event on mark_as_read

**Files:**
- Modify: `server/src/chat/dm.rs`

**Step 1: Add import**

At the top of the file, add to the ws imports:

```rust
use crate::ws::{broadcast_to_user, ServerEvent};
```

**Step 2: Add broadcast after upsert**

In the `mark_as_read` function (around line 547), replace the TODO comment with:

```rust
    // Broadcast dm_read event to all user's WebSocket sessions
    if let Err(e) = broadcast_to_user(
        &state.redis,
        auth.id,
        &ServerEvent::DmRead {
            channel_id,
            last_read_message_id: body.last_read_message_id,
        },
    )
    .await
    {
        warn!("Failed to broadcast dm_read event: {}", e);
    }
```

**Step 3: Verify compilation**

Run: `cd /home/detair/GIT/canis/.worktrees/read-sync-dnd/server && cargo check`
Expected: Compiles with no errors

**Step 4: Commit**

```bash
git add server/src/chat/dm.rs
git commit -m "feat(chat): broadcast DmRead event on mark_as_read"
```

---

## Batch 2: Cross-Client Read Sync (Frontend)

### Task 5: Handle dm_read event in WebSocket store

**Files:**
- Modify: `client/src/stores/websocket.ts`

**Step 1: Find the event handler switch**

Search for `case "message_created"` or similar event handling in websocket.ts.

**Step 2: Add dm_read handler**

Add a new case for the dm_read event:

```typescript
      case "dm_read":
        // Cross-device read sync
        handleDMReadEvent(event.channel_id);
        break;
```

**Step 3: Ensure import exists**

At the top of the file, ensure `handleDMReadEvent` is imported from dms store:

```typescript
import { handleDMReadEvent } from "@/stores/dms";
```

**Step 4: Verify TypeScript compiles**

Run: `cd /home/detair/GIT/canis/.worktrees/read-sync-dnd/client && bun run build 2>&1 | head -20`
Expected: No TypeScript errors

**Step 5: Commit**

```bash
git add client/src/stores/websocket.ts
git commit -m "feat(client): handle dm_read WebSocket event for cross-device sync"
```

---

## Batch 3: Do Not Disturb - Sound Suppression

### Task 6: Add quiet hours state to sound store

**Files:**
- Modify: `client/src/stores/sound.ts`

**Step 1: Add QuietHours interface**

Near the top of the file, add:

```typescript
export interface QuietHoursSettings {
  enabled: boolean;
  startTime: string; // "HH:MM" 24h format
  endTime: string;   // "HH:MM" 24h format
}

const DEFAULT_QUIET_HOURS: QuietHoursSettings = {
  enabled: false,
  startTime: "22:00",
  endTime: "08:00",
};
```

**Step 2: Add quiet hours to store state**

Find the store state interface and add quietHours. Then update the initial state to load from localStorage:

```typescript
// Add to store state
quietHours: QuietHoursSettings;
```

```typescript
// In initial state, load from localStorage
quietHours: (() => {
  if (typeof window === "undefined") return DEFAULT_QUIET_HOURS;
  try {
    const stored = localStorage.getItem("vc:quietHours");
    return stored ? JSON.parse(stored) : DEFAULT_QUIET_HOURS;
  } catch {
    return DEFAULT_QUIET_HOURS;
  }
})(),
```

**Step 3: Add setQuietHours function**

```typescript
export function setQuietHours(settings: QuietHoursSettings): void {
  setSoundState("quietHours", settings);
  if (typeof window !== "undefined") {
    localStorage.setItem("vc:quietHours", JSON.stringify(settings));
  }
}

export function getQuietHours(): QuietHoursSettings {
  return soundState.quietHours;
}
```

**Step 4: Verify TypeScript compiles**

Run: `cd /home/detair/GIT/canis/.worktrees/read-sync-dnd/client && bun run build 2>&1 | head -20`

**Step 5: Commit**

```bash
git add client/src/stores/sound.ts
git commit -m "feat(client): add quiet hours settings to sound store"
```

---

### Task 7: Add DND check functions

**Files:**
- Modify: `client/src/stores/sound.ts`

**Step 1: Add time parsing helper**

```typescript
function parseTimeToMinutes(time: string): number {
  const [hours, minutes] = time.split(":").map(Number);
  return hours * 60 + minutes;
}
```

**Step 2: Add isQuietHoursActive function**

```typescript
export function isQuietHoursActive(): boolean {
  const { enabled, startTime, endTime } = soundState.quietHours;
  if (!enabled) return false;

  const now = new Date();
  const currentMinutes = now.getHours() * 60 + now.getMinutes();
  const startMinutes = parseTimeToMinutes(startTime);
  const endMinutes = parseTimeToMinutes(endTime);

  if (startMinutes <= endMinutes) {
    // Same day range (e.g., 09:00 - 17:00)
    return currentMinutes >= startMinutes && currentMinutes < endMinutes;
  } else {
    // Overnight range (e.g., 22:00 - 08:00)
    return currentMinutes >= startMinutes || currentMinutes < endMinutes;
  }
}
```

**Step 3: Add isDndActive function**

```typescript
import { currentUser } from "@/stores/auth";

export function isDndActive(): boolean {
  // Check user status first
  const user = currentUser();
  if (user?.status === "busy") return true;

  // Check quiet hours
  return isQuietHoursActive();
}
```

**Step 4: Verify TypeScript compiles**

Run: `cd /home/detair/GIT/canis/.worktrees/read-sync-dnd/client && bun run build 2>&1 | head -20`

**Step 5: Commit**

```bash
git add client/src/stores/sound.ts
git commit -m "feat(client): add isDndActive and isQuietHoursActive functions"
```

---

### Task 8: Add DND check to playNotification

**Files:**
- Modify: `client/src/lib/sound/index.ts`

**Step 1: Import isDndActive**

At the top, add:

```typescript
import { isDndActive } from "@/stores/sound";
```

**Step 2: Add DND check at start of playNotification**

In the `playNotification` function, add as the FIRST check (before `getSoundEnabled`):

```typescript
export async function playNotification(event: SoundEvent): Promise<void> {
  // DND check - suppress all sounds when active
  if (isDndActive()) {
    return;
  }

  // Quick exit: sounds disabled globally
  if (!getSoundEnabled()) {
    return;
  }
  // ... rest of function
```

**Step 3: Verify TypeScript compiles**

Run: `cd /home/detair/GIT/canis/.worktrees/read-sync-dnd/client && bun run build 2>&1 | head -20`

**Step 4: Commit**

```bash
git add client/src/lib/sound/index.ts
git commit -m "feat(client): add DND check to playNotification"
```

---

### Task 9: Add DND check to ring sounds

**Files:**
- Modify: `client/src/lib/sound/ring.ts`

**Step 1: Import isDndActive**

At the top, add:

```typescript
import { isDndActive } from "@/stores/sound";
```

**Step 2: Add DND check to startRinging**

At the start of the `startRinging` function:

```typescript
export function startRinging(): void {
  // Suppress ring sounds during DND
  if (isDndActive()) {
    console.log("[Ring] Suppressed by DND mode");
    return;
  }

  // ... rest of function
```

**Step 3: Verify TypeScript compiles**

Run: `cd /home/detair/GIT/canis/.worktrees/read-sync-dnd/client && bun run build 2>&1 | head -20`

**Step 4: Commit**

```bash
git add client/src/lib/sound/ring.ts
git commit -m "feat(client): add DND check to ring sounds"
```

---

## Batch 4: Quiet Hours UI

### Task 10: Add Quiet Hours section to NotificationSettings

**Files:**
- Modify: `client/src/components/settings/NotificationSettings.tsx`

**Step 1: Import quiet hours functions**

```typescript
import { getQuietHours, setQuietHours, isQuietHoursActive } from "@/stores/sound";
```

**Step 2: Add quiet hours state**

Inside the component, add:

```typescript
const [quietHours, setQuietHoursLocal] = createSignal(getQuietHours());

const updateQuietHours = (updates: Partial<QuietHoursSettings>) => {
  const newSettings = { ...quietHours(), ...updates };
  setQuietHoursLocal(newSettings);
  setQuietHours(newSettings);
};
```

**Step 3: Add Quiet Hours UI section**

Add after the existing notification settings sections:

```tsx
{/* Quiet Hours */}
<div class="space-y-4">
  <h3 class="text-sm font-medium text-text-primary">Quiet Hours</h3>
  <p class="text-xs text-text-muted">
    Automatically suppress notification sounds during specified hours.
  </p>

  {/* Enable toggle */}
  <label class="flex items-center justify-between">
    <span class="text-sm text-text-secondary">Enable Quiet Hours</span>
    <input
      type="checkbox"
      checked={quietHours().enabled}
      onChange={(e) => updateQuietHours({ enabled: e.currentTarget.checked })}
      class="w-4 h-4 accent-accent-primary"
    />
  </label>

  {/* Time pickers */}
  <Show when={quietHours().enabled}>
    <div class="flex items-center gap-4">
      <div class="flex-1">
        <label class="block text-xs text-text-muted mb-1">Start Time</label>
        <input
          type="time"
          value={quietHours().startTime}
          onChange={(e) => updateQuietHours({ startTime: e.currentTarget.value })}
          class="w-full px-3 py-2 bg-surface-overlay border border-white/10 rounded text-text-primary"
        />
      </div>
      <div class="flex-1">
        <label class="block text-xs text-text-muted mb-1">End Time</label>
        <input
          type="time"
          value={quietHours().endTime}
          onChange={(e) => updateQuietHours({ endTime: e.currentTarget.value })}
          class="w-full px-3 py-2 bg-surface-overlay border border-white/10 rounded text-text-primary"
        />
      </div>
    </div>

    {/* Status indicator */}
    <div class="text-xs text-text-muted">
      <Show
        when={isQuietHoursActive()}
        fallback={<span>Quiet hours are currently inactive</span>}
      >
        <span class="text-accent-primary">Quiet hours are currently active</span>
      </Show>
    </div>
  </Show>
</div>
```

**Step 4: Add imports for Show and createSignal if needed**

```typescript
import { Show, createSignal } from "solid-js";
import type { QuietHoursSettings } from "@/stores/sound";
```

**Step 5: Verify TypeScript compiles and build succeeds**

Run: `cd /home/detair/GIT/canis/.worktrees/read-sync-dnd/client && bun run build 2>&1 | tail -10`

**Step 6: Commit**

```bash
git add client/src/components/settings/NotificationSettings.tsx
git commit -m "feat(client): add Quiet Hours UI to notification settings"
```

---

## Batch 5: Testing & Documentation

### Task 11: Run full test suite

**Step 1: Run server tests**

Run: `cd /home/detair/GIT/canis/.worktrees/read-sync-dnd/server && cargo test`
Expected: All tests pass

**Step 2: Run client build**

Run: `cd /home/detair/GIT/canis/.worktrees/read-sync-dnd/client && bun run build`
Expected: Build succeeds

**Step 3: Run clippy**

Run: `cd /home/detair/GIT/canis/.worktrees/read-sync-dnd/server && cargo clippy -- -D warnings`
Expected: No warnings

---

### Task 12: Update CHANGELOG

**Files:**
- Modify: `CHANGELOG.md`

**Step 1: Add entries under [Unreleased]**

```markdown
### Added
- Cross-client read sync for DM messages
  - Read status syncs in real-time across all logged-in devices
  - Uses new `user:{user_id}` Redis pub/sub channel for user-targeted events
- Do Not Disturb mode with sound suppression
  - Setting status to "Do Not Disturb" now suppresses notification sounds
  - Configurable Quiet Hours with start/end time schedule
  - Overnight ranges supported (e.g., 22:00 - 08:00)
  - Ring sounds also suppressed during DND
```

**Step 2: Commit**

```bash
git add CHANGELOG.md
git commit -m "docs: add read sync and DND to changelog"
```

---

### Task 13: Update roadmap

**Files:**
- Modify: `docs/project/roadmap.md`

**Step 1: Mark Cross-Client Read Sync as complete**

Find the `Cross-Client Read Sync` entry and mark it:

```markdown
- [x] **[Chat] Cross-Client Read Sync** ✅
  - Sync read position across all user's devices/tabs.
  - Clear unread badges and highlights when read on any client.
  - Uses `user:{user_id}` Redis pub/sub channel.
```

**Step 2: Mark Do Not Disturb Mode as complete**

```markdown
- [x] **[UX] Do Not Disturb Mode** ✅
  - App-level DND toggle via status picker (suppresses sounds).
  - Configurable Quiet Hours schedule in notification settings.
```

**Step 3: Update the date and phase progress**

**Step 4: Commit**

```bash
git add docs/project/roadmap.md
git commit -m "docs: mark read sync and DND as complete in roadmap"
```

---

## Verification Checklist

### Cross-Client Read Sync
- [ ] Open app on two devices/tabs with same account
- [ ] Receive DM, both show unread badge
- [ ] Read DM on Device A
- [ ] Device B badge clears within 1 second

### Do Not Disturb
- [ ] Set status to "Do Not Disturb"
- [ ] Receive DM - no sound, but badge appears
- [ ] Set status back to "Online"
- [ ] Receive DM - sound plays

### Quiet Hours
- [ ] Enable quiet hours with current time in range
- [ ] Receive DM - no sound
- [ ] Disable quiet hours
- [ ] Receive DM - sound plays

### Ring Suppression
- [ ] Enable DND or quiet hours
- [ ] Receive incoming call - no ring sound, call banner appears
