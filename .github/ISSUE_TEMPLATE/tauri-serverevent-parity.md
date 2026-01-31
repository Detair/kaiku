---
name: Tauri ServerEvent Parity
about: Expand Rust ServerEvent enum for full WebSocket parity
title: 'feat(client): expand Tauri ServerEvent enum for full WebSocket parity'
labels: enhancement, tauri, websocket
assignees: ''
---

## Problem

The Rust `ServerEvent` enum in `client/src-tauri/src/network/websocket.rs` currently defines ~20 event variants, while browser mode in `client/src/stores/websocket.ts` handles 40+ event types. This creates a feature gap where Tauri users cannot receive certain real-time updates.

**Current State:**
- Browser mode: Full event coverage via JSON deserialization
- Tauri mode: Limited to events defined in Rust enum

## Missing Event Categories

### 🔐 Admin Events (4 events)
Events emitted when admin actions occur on the platform.

```rust
AdminUserBanned {
    user_id: String,
    username: String,
},
AdminUserUnbanned {
    user_id: String,
    username: String,
},
AdminGuildSuspended {
    guild_id: String,
    guild_name: String,
},
AdminGuildUnsuspended {
    guild_id: String,
    guild_name: String,
},
```

**Frontend handlers exist:** `stores/admin.ts`
- `handleUserBannedEvent()`
- `handleUserUnbannedEvent()`
- `handleGuildSuspendedEvent()`
- `handleGuildUnsuspendedEvent()`

---

### 📞 Call Events (5 events)
Additional call signaling events beyond basic voice.

```rust
IncomingCall {
    channel_id: String,
    initiator: String,
    initiator_name: String,
},
CallStarted {
    channel_id: String,
},
CallEnded {
    channel_id: String,
    reason: String,
    duration_secs: Option<u64>,
},
CallParticipantJoined {
    channel_id: String,
    user_id: String,
    username: String,
},
CallParticipantLeft {
    channel_id: String,
    user_id: String,
},
CallDeclined {
    channel_id: String,
    user_id: String,
},
```

**Frontend handlers exist:** `stores/call.ts`
- `receiveIncomingCall()`
- `callConnected()`
- `callEndedExternally()`
- `participantJoined()`
- `participantLeft()`

**Status:** Partially implemented — IncomingCall, CallEnded, CallParticipantJoined/Left were added via frontend listeners but Rust enum is missing these variants

---

### 🎭 Reaction Events (2 events)
Message reaction add/remove notifications.

```rust
ReactionAdd {
    channel_id: String,
    message_id: String,
    user_id: String,
    emoji: String,
},
ReactionRemove {
    channel_id: String,
    message_id: String,
    user_id: String,
    emoji: String,
},
```

**Frontend handlers exist:** `stores/websocket.ts`
- `handleReactionAdd()`
- `handleReactionRemove()`

---

### 👥 Friend Events (2 events)
Friend request notifications.

```rust
FriendRequestReceived {
    // Triggers loadPendingRequests()
},
FriendRequestAccepted {
    // Triggers loadFriends() and loadPendingRequests()
},
```

**Frontend handlers exist:** `stores/websocket.ts`
- Triggers `loadPendingRequests()`
- Triggers `loadFriends()`

---

### 📖 Read Sync Events (3 events)
Cross-device read status synchronization.

```rust
DmRead {
    channel_id: String,
},
DmNameUpdated {
    channel_id: String,
    name: String,
},
ChannelRead {
    channel_id: String,
},
```

**Frontend handlers exist:**
- `stores/dms.ts` → `handleDMReadEvent()`, `handleDMNameUpdated()`
- `stores/channels.ts` → `handleChannelReadEvent()`

---

### ⚙️ Preferences Event (1 event)
User preferences updated from another device.

```rust
PreferencesUpdated {
    // Payload structure TBD
    preferences: serde_json::Value,
},
```

**Frontend handler exists:** `stores/preferences.ts` → `handlePreferencesUpdated()`

---

### 🔄 State Patch Event (1 event)
Generic entity state patch for granular updates.

```rust
Patch {
    entity_type: String, // "user" | "guild" | "member"
    entity_id: String,
    diff: serde_json::Value,
},
```

**Frontend handler exists:** `stores/websocket.ts` → `handlePatchEvent()`
Dispatches to:
- `stores/presence.ts` → `patchUser()`
- `stores/guilds.ts` → `patchGuild()`
- `stores/members.ts` → `patchMember()`

---

### 🖥️ Screen Share Events (3 events)
Screen sharing lifecycle events.

```rust
ScreenShareStarted {
    channel_id: String,
    user_id: String,
    username: String,
    source_label: String,
    has_audio: bool,
    quality: String,
    started_at: String,
},
ScreenShareStopped {
    channel_id: String,
    user_id: String,
    reason: String,
},
ScreenShareQualityChanged {
    channel_id: String,
    user_id: String,
    new_quality: String,
},
```

**Frontend handlers exist:** `stores/websocket.ts`
- `handleScreenShareStarted()`
- `handleScreenShareStopped()`
- `handleScreenShareQualityChanged()`

---

### 📊 Voice Stats Event (1 event)
Real-time voice quality metrics.

```rust
VoiceUserStats {
    channel_id: String,
    user_id: String,
    latency: f64,
    packet_loss: f64,
    jitter: f64,
    quality: f64,
},
```

**Frontend handler exists:** `stores/voice.ts` → `handleVoiceUserStats()`

---

## Implementation Checklist

### Phase 1: Core Functionality (High Priority)
- [ ] Add Call events (IncomingCall, CallEnded, CallParticipantJoined/Left, CallDeclined, CallStarted)
- [ ] Add Read sync events (DmRead, ChannelRead, DmNameUpdated)
- [ ] Add Reaction events (ReactionAdd, ReactionRemove)
- [ ] Add Screen Share events (Started, Stopped, QualityChanged)

### Phase 2: Admin & Social (Medium Priority)
- [ ] Add Admin events (UserBanned/Unbanned, GuildSuspended/Unsuspended)
- [ ] Add Friend events (FriendRequestReceived, FriendRequestAccepted)
- [ ] Add VoiceUserStats event

### Phase 3: Advanced (Low Priority)
- [ ] Add Patch event (generic state sync)
- [ ] Add PreferencesUpdated event

## Technical Notes

### Rust Implementation
1. Add variants to `ServerEvent` enum in `client/src-tauri/src/network/websocket.rs`
2. Update WebSocket message handler to emit Tauri events for new variants
3. Follow existing pattern: `app.emit("ws:{snake_case_name}", payload)`

### Frontend Compatibility
All frontend handlers already exist and are tested in browser mode. No frontend changes needed — handlers will automatically work once Rust emits the events.

### Testing
For each new event type:
1. Verify Rust enum compiles
2. Trigger server event (e.g., create reaction, ban user)
3. Verify Tauri app emits `ws:{event_name}` event
4. Verify frontend handler processes event correctly

## References

- **Rust enum:** `client/src-tauri/src/network/websocket.rs:59`
- **Browser mode handlers:** `client/src/stores/websocket.ts:299-526`
- **Existing Tauri listeners:** `client/src/stores/websocket.ts:119-270`

## Success Criteria

✅ All browser mode events have corresponding Rust ServerEvent variants
✅ Tauri mode users receive same real-time updates as browser users
✅ No regressions in existing event handling
