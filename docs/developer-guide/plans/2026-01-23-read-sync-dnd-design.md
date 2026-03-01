# Cross-Client Read Sync & Do Not Disturb Mode - Design

> **Status:** Approved
> **Date:** 2026-01-23

## Overview

Two features to improve the multi-device and notification experience:

1. **Cross-Client Read Sync** - When user reads a DM on one device, clear unread badges on all other devices in real-time
2. **Do Not Disturb Mode** - Suppress notification sounds when DND status is active or during scheduled quiet hours

---

## Feature 1: Cross-Client Read Sync

### Current State

| Component | Status |
|-----------|--------|
| `dm_read_state` table | ✅ Exists |
| `POST /api/dm/:id/read` endpoint | ✅ Exists |
| `handleDMReadEvent()` in frontend | ✅ Exists |
| WebSocket broadcast | ❌ TODO comment |
| User-specific Redis channel | ❌ Missing |

### Design Decision

**Problem:** Existing `presence:{user_id}` channel is for friends to see status updates. Read sync events should only go to the *same user's other devices*, not to friends or DM participants.

**Solution:** Add new `user:{user_id}` Redis channel pattern for user-targeted events.

### Architecture

```
Device A reads DM
    ↓
POST /api/dm/:id/read
    ↓
Server: Upsert dm_read_state
    ↓
Server: Publish DmRead to Redis user:{user_id}
    ↓
All WebSocket sessions for user_id receive event
    ↓
Device B: handleDMReadEvent() → clears unread badge
```

### Implementation

#### Backend Changes

**1. Add user events channel (`server/src/ws/mod.rs`)**

```rust
// In channels module
pub fn user_events(user_id: Uuid) -> String {
    format!("user:{user_id}")
}
```

**2. Add broadcast helper (`server/src/ws/mod.rs`)**

```rust
pub async fn broadcast_to_user(
    redis: &RedisClient,
    user_id: Uuid,
    event: &ServerEvent,
) -> Result<(), RedisError> {
    let payload = serde_json::to_string(event)?;
    redis.publish(channels::user_events(user_id), payload).await?;
    Ok(())
}
```

**3. Add ServerEvent variant (`server/src/ws/mod.rs`)**

```rust
ServerEvent::DmRead {
    channel_id: Uuid,
    last_read_message_id: Option<Uuid>,
}
```

**4. Subscribe to self on connect (`server/src/ws/mod.rs` in `handle_pubsub`)**

```rust
// Subscribe to own user channel for cross-device sync
let user_channel = channels::user_events(user_id);
subscriber.subscribe(&user_channel).await?;
```

**5. Handle user events in pubsub handler**

```rust
// Handle user events (user:{uuid})
else if let Some(uuid_str) = channel_name.strip_prefix("user:") {
    // Forward all user-targeted events
    if let Some(payload) = message.value.as_str() {
        if let Ok(event) = serde_json::from_str::<ServerEvent>(&payload) {
            let _ = tx.send(event).await;
        }
    }
}
```

**6. Broadcast on mark_as_read (`server/src/chat/dm.rs`)**

```rust
// After upsert, broadcast to user's other sessions
broadcast_to_user(
    &state.redis,
    auth.id,
    &ServerEvent::DmRead {
        channel_id,
        last_read_message_id: body.last_read_message_id,
    },
).await?;
```

#### Frontend Changes

**1. Handle dm_read event (`client/src/stores/websocket.ts`)**

```typescript
case "dm_read":
    handleDMReadEvent(event.channel_id);
    break;
```

### Data Model

No schema changes needed - `dm_read_state` table already exists:

```sql
CREATE TABLE dm_read_state (
    user_id UUID NOT NULL REFERENCES users(id),
    channel_id UUID NOT NULL REFERENCES channels(id),
    last_read_at TIMESTAMPTZ NOT NULL,
    last_read_message_id UUID REFERENCES messages(id),
    PRIMARY KEY (user_id, channel_id)
);
```

---

## Feature 2: Do Not Disturb Mode

### Current State

| Component | Status |
|-----------|--------|
| `UserStatus::Busy` (DND) | ✅ Exists in DB |
| Status visible to others | ✅ Works via presence |
| Sound suppression | ❌ Not implemented |
| Quiet hours | ❌ Not implemented |

### Requirements

- Suppress notification sounds only (badges/toasts still show)
- Scheduled quiet hours with configurable times
- DND status visible to other users (already works)

### Architecture

```
playNotification() called
    ↓
Check: Is DND active?
    ├─ User status == "busy"? → suppress
    ├─ Quiet hours active?    → suppress
    └─ Neither?               → continue to play
```

### Quiet Hours Logic

**Storage:** Client-side (localStorage for web, Tauri store for desktop)

```typescript
interface QuietHoursSettings {
    enabled: boolean;
    startTime: string;  // "22:00" (24h format)
    endTime: string;    // "08:00"
}
```

**Time comparison with midnight wrap-around:**

```typescript
function isQuietHoursActive(): boolean {
    if (!settings.enabled) return false;

    const now = new Date();
    const currentMinutes = now.getHours() * 60 + now.getMinutes();
    const startMinutes = parseTime(settings.startTime);
    const endMinutes = parseTime(settings.endTime);

    if (startMinutes <= endMinutes) {
        // Same day range (e.g., 09:00 - 17:00)
        return currentMinutes >= startMinutes && currentMinutes < endMinutes;
    } else {
        // Overnight range (e.g., 22:00 - 08:00)
        return currentMinutes >= startMinutes || currentMinutes < endMinutes;
    }
}
```

### Implementation

#### Frontend Changes

**1. Add DND state (`client/src/stores/sound.ts`)**

```typescript
interface SoundSettings {
    // ... existing fields
    quietHours: {
        enabled: boolean;
        startTime: string;
        endTime: string;
    };
}

const defaultQuietHours = {
    enabled: false,
    startTime: "22:00",
    endTime: "08:00",
};
```

**2. Add DND check functions (`client/src/stores/sound.ts`)**

```typescript
export function isQuietHoursActive(): boolean {
    // Time comparison logic as above
}

export function isDndActive(): boolean {
    const user = currentUser();
    if (user?.status === "busy") return true;
    return isQuietHoursActive();
}
```

**3. Add DND check to playNotification (`client/src/lib/sound/index.ts`)**

```typescript
export async function playNotification(event: SoundEvent): Promise<void> {
    // NEW: Check DND first
    if (isDndActive()) {
        return;
    }

    // ... rest of existing checks
}
```

**4. Add DND check to ring sounds (`client/src/lib/sound/ring.ts`)**

```typescript
export function startRinging(): void {
    if (isDndActive()) {
        console.log("[Ring] Suppressed by DND");
        return;
    }
    // ... existing logic
}
```

**5. Add Quiet Hours UI (`client/src/components/settings/NotificationSettings.tsx`)**

New section with:
- Enable/disable toggle
- Start time input (time picker or text input, 24h format)
- End time input
- Preview showing current status ("Quiet hours active" / "Next quiet period: 22:00")

### Settings Persistence

**Web (localStorage):**
```typescript
localStorage.setItem("vc:quietHours", JSON.stringify(settings));
```

**Tauri (tauri-plugin-store or app data):**
```typescript
await invoke("save_settings", { key: "quietHours", value: settings });
```

For MVP, use localStorage for both platforms (works in Tauri WebView too).

---

## Testing

### Cross-Client Read Sync

1. Open app on two devices/tabs with same account
2. Send message in DM from another user
3. Both devices show unread badge
4. Read DM on Device A
5. **Verify:** Device B unread badge clears within 1 second

### Do Not Disturb

1. Set status to "Do Not Disturb"
2. Receive DM from another user
3. **Verify:** No sound plays, but badge appears

### Quiet Hours

1. Enable quiet hours with current time in range
2. Receive DM
3. **Verify:** No sound plays
4. Disable quiet hours
5. Receive another DM
6. **Verify:** Sound plays

### Ring Suppression

1. Enable DND or quiet hours
2. Receive incoming call
3. **Verify:** No ring sound, but call banner appears

---

## Files Summary

### Backend

| File | Changes |
|------|---------|
| `server/src/ws/mod.rs` | Add `user_events()` channel, `broadcast_to_user()`, `ServerEvent::DmRead`, pubsub handler for `user:*` |
| `server/src/chat/dm.rs` | Call `broadcast_to_user()` after mark_as_read |

### Frontend

| File | Changes |
|------|---------|
| `client/src/stores/sound.ts` | Add quiet hours state, `isQuietHoursActive()`, `isDndActive()` |
| `client/src/stores/websocket.ts` | Handle `dm_read` event |
| `client/src/lib/sound/index.ts` | Add DND check in `playNotification()` |
| `client/src/lib/sound/ring.ts` | Add DND check in `startRinging()` |
| `client/src/components/settings/NotificationSettings.tsx` | Add Quiet Hours section |

---

## Future Considerations

- **Server-synced preferences:** Quiet hours settings could sync across devices when server-synced preferences feature is implemented
- **Per-day schedules:** Allow different quiet hours for weekdays vs weekends
- **OS integration:** Detect system DND/Focus mode and respect it
- **Read receipts:** The `user:{user_id}` channel could later be used for other user-targeted events
