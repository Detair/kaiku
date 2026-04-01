# Beta Important Fixes — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix the 4 remaining Important items from the beta checklist: typing permission queries, WS backpressure, E2EE message edit decryption, and optimistic send race condition.

**Architecture:** Two server-only fixes (typing throttle, WS backpressure) in `ws/mod.rs`, two client-only fixes (E2EE edit, nonce matching) across `messages.ts`, `websocket.ts`, and `types.ts`. All 4 are independent with no cross-dependencies.

**Tech Stack:** Rust/tokio (server), Solid.js/TypeScript (client), Vitest (client tests)

**Spec:** `docs/superpowers/specs/2026-04-01-beta-important-fixes-design.md`

---

## File Map

### Server files to modify
| File | Responsibility |
|------|---------------|
| `server/src/ws/mod.rs` | Typing handler (subscription check + throttle), pubsub backpressure (`try_send`) |

### Client files to modify
| File | Responsibility |
|------|---------------|
| `client/src/lib/types.ts` | Add `nonce` field to `Message` interface |
| `client/src/stores/messages.ts` | Export `decryptMessageIfNeeded`, nonce generation in `sendMessage`, nonce matching in `addMessage` |
| `client/src/stores/websocket.ts` | Decrypt edited messages in both browser and Tauri handlers |
| `client/src/stores/__tests__/messages.test.ts` | Tests for nonce-based matching |

---

## Task 1: Server — Typing subscription check + throttle

**Files:**
- Modify: `server/src/ws/mod.rs:78-83` (struct), `server/src/ws/mod.rs:1549-1594` (handlers)

- [ ] **Step 1: Add `last_typing` field to `ClientMessageState`**

First, add the missing import at `server/src/ws/mod.rs:24` (alongside the existing `use std::collections::HashSet;`):

```rust
use std::collections::HashMap;
```

(`Duration` and `Instant` are already imported via `use std::time::{Duration, Instant};` at line 26.)

Then at `server/src/ws/mod.rs:78-83`, add the new field to the struct:

```rust
#[derive(Default)]
pub struct ClientMessageState {
    /// Activity rate limiting and deduplication state.
    pub activity: ActivityState,
    /// Custom status rate limiting and deduplication state.
    pub custom_status: CustomStatusState,
    /// Per-channel typing throttle (channel_id → last typing broadcast time).
    pub last_typing: HashMap<Uuid, Instant>,
}
```

Note: `HashMap` implements `Default` (empty map), so `#[derive(Default)]` still works.

- [ ] **Step 2: Replace `Typing` handler with subscription check + throttle**

At `server/src/ws/mod.rs:1549-1572`, replace the entire `ClientEvent::Typing` arm:

```rust
ClientEvent::Typing { channel_id } => {
    // Only allow typing in channels the user is subscribed to
    // (subscription is already permission-gated)
    if !subscribed_channels.read().await.contains(&channel_id) {
        return Ok(());
    }

    // Server-side throttle: max 1 typing event per second per channel
    let now = Instant::now();
    if let Some(last) = msg_state.last_typing.get(&channel_id) {
        if now.duration_since(*last) < Duration::from_secs(1) {
            return Ok(());
        }
    }
    msg_state.last_typing.insert(channel_id, now);

    // Broadcast typing indicator
    broadcast_to_channel(
        &state.redis,
        channel_id,
        &ServerEvent::TypingStart {
            channel_id,
            user_id,
        },
    )
    .await?;
}
```

- [ ] **Step 3: Replace `StopTyping` handler with subscription check (no throttle)**

At `server/src/ws/mod.rs:1574-1594`, replace the entire `ClientEvent::StopTyping` arm:

```rust
ClientEvent::StopTyping { channel_id } => {
    // Only allow in channels the user is subscribed to
    if !subscribed_channels.read().await.contains(&channel_id) {
        return Ok(());
    }

    // No throttle on StopTyping — throttling it causes ghost typing indicators

    // Broadcast stop typing
    broadcast_to_channel(
        &state.redis,
        channel_id,
        &ServerEvent::TypingStop {
            channel_id,
            user_id,
        },
    )
    .await?;
}
```

- [ ] **Step 4: Build and verify**

Run: `SQLX_OFFLINE=true cargo clippy -p vc-server -- -D warnings`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add server/src/ws/mod.rs
git commit -m "perf(ws): replace permission queries with subscription check for typing events

Typing and StopTyping events now check the subscribed_channels set
instead of running 4 DB queries per event. Adds a 1-second server-side
throttle for Typing events. StopTyping bypasses the throttle to prevent
ghost typing indicators."
```

---

## Task 2: Server — WS pubsub backpressure with `try_send()`

**Files:**
- Modify: `server/src/ws/mod.rs:1792-1981` (`handle_pubsub` function)

- [ ] **Step 1: Add backpressure counter at the top of the pubsub message loop**

In `handle_pubsub()` (line 1792), find the `while let Ok(message) = pubsub_stream.recv().await` loop (line 1854). Declare the counter **immediately before** the `while let` line (outside the loop, so it persists across iterations):

```rust
let mut backpressure_drops: u32 = 0;
while let Ok(message) = pubsub_stream.recv().await {
```

- [ ] **Step 2: Create a helper macro or closure for the try_send pattern**

To avoid repeating the same 10-line block at 6 call sites, add a macro inside `handle_pubsub()` just before the loop:

```rust
macro_rules! try_forward {
    ($tx:expr, $event:expr, $drops:ident, $user_id:expr) => {
        match $tx.try_send(OutboundMsg::Event($event)) {
            Ok(()) => { $drops = 0; }
            Err(tokio::sync::mpsc::error::TrySendError::Full(_)) => {
                $drops += 1;
                if $drops > 10 {
                    warn!(
                        "Disconnecting slow WebSocket client (user {}): {} consecutive drops",
                        $user_id, $drops
                    );
                    break;
                }
            }
            Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => {
                break;
            }
        }
    };
}
```

- [ ] **Step 3: Replace send site 1 — channel events (line 1891-1895)**

Replace:
```rust
if !should_filter
    && params.tx.send(OutboundMsg::Event(event)).await.is_err()
{
    break;
}
```

With:
```rust
if !should_filter {
    try_forward!(params.tx, event, backpressure_drops, params.user_id);
}
```

Note: `params` must have a `user_id` field. Check the `HandlePubsubParams` struct — if it doesn't have `user_id`, pass it as a separate field or use a placeholder identifier in the log message. The current code at line 1366 passes `user_id` into the params — verify this.

- [ ] **Step 4: Replace send site 2 — user events (line 1920-1922)**

Replace:
```rust
if params.tx.send(OutboundMsg::Event(event)).await.is_err() {
    break;
}
```

With:
```rust
try_forward!(params.tx, event, backpressure_drops, params.user_id);
```

- [ ] **Step 5: Replace send site 3 — admin events (line 1932-1934)**

Same replacement as Step 4.

- [ ] **Step 6: Replace send site 4 — presence events (line 1953-1955)**

Replace:
```rust
if !should_filter && params.tx.send(OutboundMsg::Event(event)).await.is_err() {
    break;
}
```

With:
```rust
if !should_filter {
    try_forward!(params.tx, event, backpressure_drops, params.user_id);
}
```

- [ ] **Step 7: Replace send site 5 — user cross-device sync (line 1964-1966)**

Same replacement as Step 4.

- [ ] **Step 8: Replace send site 6 — guild events (line 1975-1977)**

Same replacement as Step 4.

- [ ] **Step 9: Verify `HandlePubsubParams` has `user_id`**

Check the struct definition. If `user_id` is not a field, add it and pass it from the `handle_socket` call site at the `tokio::spawn` for `handle_pubsub`.

- [ ] **Step 10: Build and verify**

Run: `SQLX_OFFLINE=true cargo clippy -p vc-server -- -D warnings`
Expected: PASS

- [ ] **Step 11: Commit**

```bash
git add server/src/ws/mod.rs
git commit -m "fix(ws): replace blocking pubsub send with try_send and backpressure disconnect

Slow WebSocket clients no longer block the per-connection Redis pubsub
task. Messages are dropped when the channel is full, and clients are
disconnected after 10 consecutive drops. The client's existing reconnect
logic handles recovery."
```

---

## Task 3: Client — Decrypt edited messages in E2EE channels

**Files:**
- Modify: `client/src/stores/messages.ts:58` (add `export`)
- Modify: `client/src/stores/websocket.ts:1011-1035` (browser handler)
- Modify: `client/src/stores/websocket.ts:278-306` (Tauri handler)

- [ ] **Step 1: Export `decryptMessageIfNeeded`**

At `client/src/stores/messages.ts:58`, change:
```typescript
async function decryptMessageIfNeeded(message: Message): Promise<Message> {
```

To:
```typescript
export async function decryptMessageIfNeeded(message: Message): Promise<Message> {
```

- [ ] **Step 2: Add import in `websocket.ts`**

Find the existing import from `../messages` (or `@/stores/messages`) in `client/src/stores/websocket.ts` and add `decryptMessageIfNeeded` to it.

- [ ] **Step 3: Update browser `message_edit` handler**

At `client/src/stores/websocket.ts:1011-1035`, replace:

```typescript
case "message_edit": {
  const editMessages = messagesState.byChannel[event.channel_id];
  if (editMessages) {
    const editIndex = editMessages.findIndex(
      (m) => m.id === event.message_id,
    );
    if (editIndex !== -1) {
      setMessagesState(
        "byChannel",
        event.channel_id,
        editIndex,
        "content",
        event.content,
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

With:

```typescript
case "message_edit": {
  const editMessages = messagesState.byChannel[event.channel_id];
  if (editMessages) {
    const editIndex = editMessages.findIndex(
      (m) => m.id === event.message_id,
    );
    if (editIndex !== -1) {
      const existingMsg = editMessages[editIndex];
      let newContent = event.content;

      if (existingMsg.encrypted) {
        const temp = { ...existingMsg, content: event.content };
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

- [ ] **Step 4: Update Tauri `ws:message_edit` handler**

At `client/src/stores/websocket.ts:278-306`, replace:

```typescript
listen<{
  channel_id: string;
  message_id: string;
  content: string;
  edited_at: string;
}>("ws:message_edit", (event) => {
  const { channel_id, message_id, content, edited_at } = event.payload;
  const messages = messagesState.byChannel[channel_id];
  if (messages) {
    const index = messages.findIndex((m) => m.id === message_id);
    if (index !== -1) {
      setMessagesState(
        "byChannel",
        channel_id,
        index,
        "content",
        content,
      );
      setMessagesState(
        "byChannel",
        channel_id,
        index,
        "edited_at",
        edited_at,
      );
    }
  }
}),
```

With:

```typescript
listen<{
  channel_id: string;
  message_id: string;
  content: string;
  edited_at: string;
}>("ws:message_edit", async (event) => {
  const { channel_id, message_id, content, edited_at } = event.payload;
  const messages = messagesState.byChannel[channel_id];
  if (messages) {
    const index = messages.findIndex((m) => m.id === message_id);
    if (index !== -1) {
      const existingMsg = messages[index];
      let newContent = content;

      if (existingMsg.encrypted) {
        const temp = { ...existingMsg, content };
        const decrypted = await decryptMessageIfNeeded(temp);
        newContent = decrypted.content;
      }

      setMessagesState(
        "byChannel",
        channel_id,
        index,
        "content",
        newContent,
      );
      setMessagesState(
        "byChannel",
        channel_id,
        index,
        "edited_at",
        edited_at,
      );
    }
  }
}),
```

Note the two changes: `(event) =>` becomes `async (event) =>`, and the decryption logic is added.

- [ ] **Step 5: Verify build**

Run: `cd client && bunx tsc --noEmit`
Expected: PASS (no type errors)

- [ ] **Step 6: Commit**

```bash
git add client/src/stores/messages.ts client/src/stores/websocket.ts
git commit -m "fix(chat): decrypt edited messages in E2EE channels

The message_edit WebSocket handler now checks the existing message's
encrypted flag and calls decryptMessageIfNeeded() before updating the
store. Applies to both browser and Tauri handlers."
```

---

## Task 4: Client — Nonce-based optimistic message matching

**Files:**
- Modify: `client/src/lib/types.ts:276-294` (Message interface)
- Modify: `client/src/stores/messages.ts:329-357` (sendMessage), `client/src/stores/messages.ts:422-431` (addMessage)
- Test: `client/src/stores/__tests__/messages.test.ts`

- [ ] **Step 1: Write failing test for nonce-based matching**

Add to `client/src/stores/__tests__/messages.test.ts`. First, update the `createMessage` helper to include `nonce`:

```typescript
function createMessage(id: string, channelId = "ch-1"): Message {
  return {
    id,
    channel_id: channelId,
    // ... existing fields ...
    nonce: null,  // Add this line
  };
}
```

Then add the test:

```typescript
describe("addMessage nonce matching", () => {
  it("replaces the correct pending message when multiple are pending", async () => {
    // Set up two pending messages
    const pending1: Message = {
      ...createMessage("pending:nonce-AAA", "ch-1"),
      author: { id: "me", username: "me", display_name: "me", avatar_url: null, status: "online" },
      nonce: "nonce-AAA",
    };
    const pending2: Message = {
      ...createMessage("pending:nonce-BBB", "ch-1"),
      author: { id: "me", username: "me", display_name: "me", avatar_url: null, status: "online" },
      nonce: "nonce-BBB",
    };
    setMessagesState("byChannel", "ch-1", [pending1, pending2]);

    // Server confirms the SECOND message first
    const real2: Message = {
      ...createMessage("msg-002", "ch-1"),
      author: { id: "me", username: "me", display_name: "me", avatar_url: null, status: "online" },
      nonce: "nonce-BBB",
    };
    await addMessage(real2);

    const msgs = messagesState.byChannel["ch-1"]!;
    // pending1 should still be there, pending2 should be replaced by real2
    expect(msgs).toHaveLength(2);
    expect(msgs[0].id).toBe("pending:nonce-AAA");
    expect(msgs[1].id).toBe("msg-002");
  });

  it("appends message from other user without touching pending messages", async () => {
    const pending1: Message = {
      ...createMessage("pending:nonce-AAA", "ch-1"),
      author: { id: "me", username: "me", display_name: "me", avatar_url: null, status: "online" },
      nonce: "nonce-AAA",
    };
    setMessagesState("byChannel", "ch-1", [pending1]);

    const otherMsg: Message = {
      ...createMessage("msg-other", "ch-1"),
      author: { id: "other", username: "other", display_name: "Other", avatar_url: null, status: "online" },
      nonce: null,
    };
    await addMessage(otherMsg);

    const msgs = messagesState.byChannel["ch-1"]!;
    expect(msgs).toHaveLength(2);
    expect(msgs[0].id).toBe("pending:nonce-AAA");
    expect(msgs[1].id).toBe("msg-other");
  });
});
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd client && bun run test:run -- --reporter=verbose messages.test`
Expected: FAIL — `nonce` property doesn't exist on `Message` type

- [ ] **Step 3: Add `nonce` to Message type**

At `client/src/lib/types.ts:293` (before the closing `}` of the `Message` interface), add:

```typescript
  nonce?: string | null;
```

The interface becomes:
```typescript
export interface Message {
  id: string;
  channel_id: string;
  author: UserProfile;
  content: string;
  encrypted: boolean;
  attachments: Attachment[];
  reply_to: string | null;
  parent_id: string | null;
  thread_reply_count: number;
  thread_last_reply_at: string | null;
  edited_at: string | null;
  created_at: string;
  mention_type: "direct" | "everyone" | "here" | null;
  reactions?: Reaction[];
  thread_info?: ThreadInfo;
  pinned: boolean;
  message_type: string; // "user" | "system"
  nonce?: string | null;
}
```

- [ ] **Step 4: Generate nonce in `sendMessage`**

At `client/src/stores/messages.ts:329`, replace:
```typescript
const pendingId = `pending:${crypto.randomUUID()}`;
const optimisticMessage: Message = {
  id: pendingId,
```

With:
```typescript
const nonce = crypto.randomUUID();
const pendingId = `pending:${nonce}`;
const optimisticMessage: Message = {
  id: pendingId,
  nonce,
```

- [ ] **Step 5: Pass nonce to server**

At `client/src/stores/messages.ts:355-358`, replace:
```typescript
const { message, status } = await tauri.sendMessageWithStatus(
  channelId,
  trimmedContent
);
```

With:
```typescript
const { message, status } = await tauri.sendMessageWithStatus(
  channelId,
  trimmedContent,
  { nonce },
);
```

- [ ] **Step 6: Replace pending matching in `addMessage` with nonce-based lookup**

At `client/src/stores/messages.ts:422-431`, replace:
```typescript
  // If this is a confirmed echo of our own message, replace the oldest pending placeholder
  const me = currentUser();
  if (me && processedMessage.author.id === me.id) {
    const pendingIdx = existing.findIndex((m) => m.id.startsWith("pending:"));
    if (pendingIdx !== -1) {
      const base = [...existing];
      base.splice(pendingIdx, 1);
      setMessagesState("byChannel", channelId, [...base, processedMessage]);
      return;
    }
  }
```

With:
```typescript
  // If this is a confirmed echo of our own message, replace the matching pending placeholder by nonce
  const me = currentUser();
  if (me && processedMessage.author.id === me.id && processedMessage.nonce) {
    const pendingIdx = existing.findIndex(
      (m) => m.id.startsWith("pending:") && m.nonce === processedMessage.nonce,
    );
    if (pendingIdx !== -1) {
      const base = [...existing];
      base.splice(pendingIdx, 1);
      setMessagesState("byChannel", channelId, [...base, processedMessage]);
      return;
    }
  }
```

- [ ] **Step 7: Run tests to verify they pass**

Run: `cd client && bun run test:run -- --reporter=verbose messages.test`
Expected: PASS

- [ ] **Step 8: Run full client test suite**

Run: `cd client && bun run test:run`
Expected: PASS (no regressions)

- [ ] **Step 9: Commit**

```bash
git add client/src/lib/types.ts client/src/stores/messages.ts client/src/stores/__tests__/messages.test.ts
git commit -m "fix(chat): use nonce for optimistic message matching to prevent reordering

Each sent message now includes a nonce (UUID) that links the optimistic
pending message to the server-confirmed echo. addMessage() matches by
nonce instead of finding the first pending message, fixing a race
condition where rapid sends could reorder messages in the UI."
```
