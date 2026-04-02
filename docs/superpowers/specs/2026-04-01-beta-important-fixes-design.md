# Beta Important Fixes — Design Spec

**Date:** 2026-04-01
**Status:** Approved
**Scope:** 4 remaining Important items from the 2026-03-19 beta checklist

## Fix 1: Permission queries per typing event

### Problem

Every `Typing` and `StopTyping` WebSocket event fires `require_channel_access()`, which runs 4 sequential DB queries (channel lookup, guild membership + owner, @everyone role + member roles, channel overrides). No server-side throttle exists — clients can spam typing events.

**Impact:** 4 DB queries per typing event per user. A room of 50 users typing = potentially 200 queries/sec just for typing indicators.

**Files:**
- `server/src/ws/mod.rs:1549-1594` — typing/stop-typing handlers
- `server/src/ws/mod.rs:74-83` — `ClientMessageState` struct

### Design

Replace the permission check with a subscription set lookup and add a server-side throttle.

#### Skip permission check for subscribed channels

The WS connection already maintains `subscribed_channels: RwLock<HashSet<Uuid>>`. Users pass a permission check when they subscribe to a channel. If a user is subscribed, they have permission to type — no DB query needed.

In the `Typing` handler (line 1549), replace:
```rust
let permission_result = crate::permissions::require_channel_access(&state.db, user_id, channel_id).await;
if permission_result.is_err() { return Ok(()); }
```

With:
```rust
if !subscribed_channels.read().await.contains(&channel_id) {
    return Ok(());
}
```

Same change for `StopTyping` (line 1574).

#### Add server-side typing throttle

Add `last_typing: HashMap<Uuid, Instant>` to `ClientMessageState`. Before broadcasting a typing event, check if the last typing event for this channel was less than 1 second ago. If so, silently ignore.

```rust
let now = Instant::now();
if let Some(last) = msg_state.last_typing.get(&channel_id) {
    if now.duration_since(*last) < Duration::from_secs(1) {
        return Ok(());
    }
}
msg_state.last_typing.insert(channel_id, now);
```

The throttle applies only to `Typing` events. `StopTyping` always passes through — throttling it would cause ghost typing indicators (user stops typing within 1s of starting, but the `StopTyping` is dropped and other clients show them as still typing). Both events still require the subscription check.

#### Trade-off

If a user's permissions are revoked mid-session (role removed), they can still type until they reconnect. This is acceptable — Discord behaves the same way, and the actual messages they send still go through full permission checks.

---

## Fix 2: Slow WS clients block Redis pubsub task

### Problem

Each WS connection has a per-connection pubsub task that calls `params.tx.send(OutboundMsg::Event(event)).await` into a bounded mpsc channel (capacity 100). If the WebSocket sender is slow (network congestion), the channel fills and `tx.send().await` blocks the pubsub task. The entire Redis pubsub connection for that user becomes stuck — no more events are received.

**Impact:** One slow client loses all real-time events. Messages delivered via Redis pubsub during the block are permanently lost.

**Files:**
- `server/src/ws/mod.rs:1792-1981` — `handle_pubsub()` function
- Send call sites at lines ~1892, 1920, 1932, 1953, 1964, 1975

### Design

Replace blocking sends with `try_send()` and disconnect on sustained backpressure.

#### Replace `tx.send().await` with `tx.try_send()`

At all 6 send sites in `handle_pubsub()`, replace:
```rust
if params.tx.send(OutboundMsg::Event(event)).await.is_err() {
    break;
}
```

With:
```rust
match params.tx.try_send(OutboundMsg::Event(event)) {
    Ok(()) => { backpressure_drops = 0; }
    Err(mpsc::error::TrySendError::Full(_)) => {
        backpressure_drops += 1;
        if backpressure_drops > 10 {
            warn!("Disconnecting slow WebSocket client (user {}): {} consecutive drops", user_id, backpressure_drops);
            break;
        }
    }
    Err(mpsc::error::TrySendError::Closed(_)) => {
        break;
    }
}
```

#### Backpressure counter

Declare `let mut backpressure_drops: u32 = 0;` at the top of the pubsub loop. Reset to 0 on every successful send. If 10 consecutive events are dropped (channel is persistently full), break out of the loop.

Breaking triggers the existing cleanup: `pubsub_handle.abort()` and `sender_handle.abort()` (lines 1470-1471), which closes the WebSocket. The client's reconnect logic handles recovery.

#### No changes to

- Channel capacity (stays at 100)
- Sender task (stays as-is, draining mpsc → WebSocket)
- Connection setup or upgrade logic

---

## Fix 3: Message edit doesn't decrypt in E2EE channels

### Problem

Both browser and Tauri `message_edit` WebSocket handlers set `event.content` directly on the store without calling `decryptMessageIfNeeded()`. Edited messages in E2EE channels display encrypted ciphertext.

**Impact:** Edited messages in encrypted DM channels show gibberish instead of decrypted content.

**Files:**
- `client/src/stores/websocket.ts:1011-1035` — browser `message_edit` handler
- `client/src/stores/websocket.ts:278-307` — Tauri `message_edit` handler (callback is synchronous, must be made `async`)
- `client/src/stores/messages.ts:58-133` — `decryptMessageIfNeeded()` function (currently not exported, must add `export`)

### Design

Check the existing message's `encrypted` flag and decrypt the edited content before updating the store.

#### Browser handler (lines 1011-1035)

Replace the current direct content assignment with:

```typescript
case "message_edit": {
  const editMessages = messagesState.byChannel[event.channel_id];
  if (editMessages) {
    const editIndex = editMessages.findIndex(
      (m) => m.id === event.message_id,
    );
    if (editIndex !== -1) {
      const existing = editMessages[editIndex];
      let newContent = event.content;

      if (existing.encrypted) {
        const temp = { ...existing, content: event.content };
        const decrypted = await decryptMessageIfNeeded(temp);
        newContent = decrypted.content;
      }

      setMessagesState(
        "byChannel",
        event.channel_id,
        editIndex,
        "content",
        newContent,
      );
      setMessagesState(
        "byChannel",
        event.channel_id,
        editIndex,
        "edited_at",
        event.edited_at,
      );
    }
  }
  break;
}
```

The `case "message_edit"` block is inside an `async` handler (the `handleServerEvent` function), so `await` is valid.

#### Export `decryptMessageIfNeeded`

Add `export` to the function declaration at `messages.ts:58`:
```typescript
export async function decryptMessageIfNeeded(message: Message): Promise<Message> {
```

#### Tauri handler (lines 278-307)

Same pattern. The callback at line 284 is currently synchronous — change to `async (event) => { ... }` to support `await decryptMessageIfNeeded()`.

#### No server changes

The server already sends the raw (encrypted) content in the `message_edit` event. Decryption is the client's responsibility, same as for `message_new`.

---

## Fix 4: Optimistic message send race condition

### Problem

`addMessage()` uses `findIndex(m => m.id.startsWith("pending:"))` to find a pending message to replace with the real server-confirmed message. This finds the *oldest* pending message, not the one that corresponds to the arriving confirmation. Rapid sends cause message reordering:

1. User sends "Hello" → `[pending:UUID-A]`
2. User sends "World" → `[pending:UUID-A, pending:UUID-B]`
3. Server confirms "World" first → `addMessage()` finds `pending:UUID-A` (wrong!) → replaces it
4. Messages appear in wrong order

**Impact:** Rapid message sends cause visible message reordering in the UI.

**Files:**
- `client/src/stores/messages.ts:329-377` — `sendMessage()` (optimistic creation + HTTP replace)
- `client/src/stores/messages.ts:405-434` — `addMessage()` (WebSocket echo handling)
- `client/src/lib/types.ts:276-294` — `Message` type
- `client/src/lib/tauri.ts:1225-1280` — `sendMessageWithStatus()`

### Design

Use the existing `nonce` field for exact pending-to-real matching.

#### Add `nonce` to TypeScript Message type

In `client/src/lib/types.ts`, add to the `Message` interface:
```typescript
nonce?: string | null;
```

The server already stores `nonce: Option<String>` in the DB and returns it in the `Message` struct (both HTTP response and WebSocket broadcast).

#### Generate nonce on every message send

In `sendMessage()` (messages.ts ~line 329), generate a nonce and use it for both the optimistic message and the server request:

```typescript
const nonce = crypto.randomUUID();
const pendingId = `pending:${nonce}`;

const optimisticMessage: Message = {
  id: pendingId,
  nonce,
  // ... rest unchanged
};
```

Pass the nonce to the server via the existing `options.nonce` parameter:
```typescript
const { message, status } = await tauri.sendMessageWithStatus(
  channelId,
  trimmedContent,
  { nonce },
);
```

#### Match by nonce in `addMessage()`

Replace line 425:
```typescript
const pendingIdx = existing.findIndex((m) => m.id.startsWith("pending:"));
```

With:
```typescript
const pendingIdx = processedMessage.nonce
  ? existing.findIndex(
      (m) => m.id.startsWith("pending:") && m.nonce === processedMessage.nonce,
    )
  : -1;
```

If no nonce match (message from another user, or no nonce), fall through to the append path. This means own-authored messages arriving without a nonce (e.g., from an older client) skip pending replacement in `addMessage()` and append normally. The HTTP response handler in `sendMessage()` (line 370) still cleans up the pending entry by exact `pendingId`, so a transient duplicate may appear for one round-trip but is resolved immediately.

#### Server changes: none

The server already:
- Accepts `nonce: Option<String>` in the create message request
- Stores it in the `messages` table
- Returns it in the `Message` response and WebSocket broadcast

---

## Changes Summary

| Fix | Server | Client | Complexity |
|-----|--------|--------|------------|
| 1. Typing permission bypass | `ws/mod.rs` — replace permission check with subscription check, add throttle map | None | Low |
| 2. WS backpressure | `ws/mod.rs` — `try_send()` + backpressure counter at 6 call sites | None | Low |
| 3. E2EE message edit | None | `websocket.ts` — decrypt edited content in both browser and Tauri handlers | Low |
| 4. Optimistic send race | None | `messages.ts` — generate nonce, match by nonce; `types.ts` — add nonce field | Low |
