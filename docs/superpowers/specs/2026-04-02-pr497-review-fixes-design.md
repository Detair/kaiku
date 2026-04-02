# PR #497 Review Fixes — Design Spec

**Date:** 2026-04-02
**Status:** Draft
**Scope:** 8 issues found during code review of PR #497 (fix/beta-important-items)

## Overview

PR #497 implements 4 Important beta fixes. Code review found 8 issues scoring >= 50/100 confidence. This spec addresses all 8, split into two commits: code fixes and docs/process updates.

## Commit 1: Code Fixes

### Fix 1a: Nonce length validation

**Problem:** `CreateMessageRequest.nonce` (`server/src/chat/messages.rs:306`) has no `#[validate]` attribute. The DB column is `VARCHAR(64)`. A nonce > 64 chars causes a PostgreSQL error surfaced as HTTP 500 instead of 400.

**Fix:** Add `#[validate(length(max = 64))]` to the `nonce` field. The struct already derives `Validate` and `body.validate()` runs in the handler.

**File:** `server/src/chat/messages.rs:306`

```rust
// Before
pub nonce: Option<String>,

// After
#[validate(length(max = 64))]
pub nonce: Option<String>,
```

### Fix 1b: Backpressure counter reset logic

**Problem:** `backpressure_drops` in `server/src/ws/mod.rs:1880` is shared across all 6 pubsub event branches. A successful send of any event type (e.g., presence) resets the counter to 0, forgiving prior dropped channel messages. In mixed-traffic scenarios, a slow client can drop many messages without hitting the 10-drop disconnect threshold.

**Fix:** Move the counter reset from the `Ok(())` arm of `try_forward!` to the top of the `while let` loop. Each pubsub message starts fresh — only consecutive back-to-back failures across sequential messages trigger disconnect.

**File:** `server/src/ws/mod.rs:1857-1881`

```rust
// try_forward! macro — remove the reset from Ok
macro_rules! try_forward {
    ($tx:expr, $event:expr, $drops:ident, $user_id:expr) => {
        match $tx.try_send(OutboundMsg::Event($event)) {
            Ok(()) => {
                // Don't reset here — reset at top of loop instead
            }
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

let mut backpressure_drops: u32 = 0;
while let Ok(message) = pubsub_stream.recv().await {
    // Reset backpressure counter at the start of each pubsub message.
    // Only consecutive back-to-back Full errors across sequential messages
    // trigger disconnect — a successful send on the previous message resets.
    backpressure_drops = 0;

    let channel_name = message.channel.to_string();
    // ... rest of handler
}
```

### Fix 1c: Prune stale `last_typing` entries

**Problem:** `ClientMessageState.last_typing` (`server/src/ws/mod.rs:84`) is a `HashMap<Uuid, Instant>` that grows unboundedly — entries are inserted on typing events but never removed.

**Fix:** After each insert, `retain` entries younger than 2 seconds. The throttle window is 1 second, so anything older is guaranteed stale. Piggybacks on the existing typing handler — no background task needed.

**File:** `server/src/ws/mod.rs:1565`

```rust
msg_state.last_typing.insert(channel_id, now);
msg_state.last_typing.retain(|_, last| now.duration_since(*last) < Duration::from_secs(2));
```

### Fix 1d: Update DB model comment

**Problem:** `server/src/db/models.rs:130` says `/// Encryption nonce (for E2EE).` but nonce is now used for all messages as an optimistic-matching token.

**Fix:** Update the comment to match the `MessageResponse` doc.

**File:** `server/src/db/models.rs:130`

```rust
// Before
/// Encryption nonce (for E2EE).

// After
/// Client-generated nonce for optimistic message matching (also used as E2EE encryption nonce).
```

### Fix 1e: Strip nonce from broadcast

**Problem:** The sender's nonce (a UUID) is included in `MessageNew` WebSocket broadcasts to all channel members. Other users don't need it — the client only uses nonces for its own pending messages.

**Fix:** After building the `MessageResponse`, serialize a copy with `nonce: None` for the broadcast. The sender still receives the nonce in their HTTP response. Apply to both `messages.rs` (regular + thread reply) and `uploads.rs` (file upload).

**File:** `server/src/chat/messages.rs:1060-1061`

```rust
// Before
let message_json = serde_json::to_value(&response).unwrap_or_default();

// After
let broadcast_response = MessageResponse { nonce: None, ..response.clone() };
let message_json = serde_json::to_value(&broadcast_response).unwrap_or_default();
```

Same pattern in `server/src/chat/uploads.rs:766`.

Note: This single replacement at line 1061 covers both `MessageNew` and `ThreadReplyNew` broadcasts, since `message_json` is computed before the `if/else` branch. `MessageResponse` already derives `Clone`.

### Fix 1f: E2EE edit decryption test

**Problem:** The new `decryptMessageIfNeeded` export is called from `websocket.ts` in two E2EE edit handlers, but no tests cover this path.

**Fix:** Add a test in `client/src/stores/__tests__/messages.test.ts` that verifies `decryptMessageIfNeeded` correctly handles an encrypted-flagged message. Since the actual Megolm decryption requires crypto setup, test the two observable behaviors:
1. Non-encrypted message passes through unchanged
2. Encrypted message with no session returns the `[Unable to decrypt message]` placeholder

**File:** `client/src/stores/__tests__/messages.test.ts`

Add `decryptMessageIfNeeded` to the existing import from `"../messages"` (lines 28-44). The existing `e2eeStore` mock provides `status: vi.fn(() => ({ initialized: false }))`, which causes the early-exit branch for encrypted messages without a session — no additional mock setup needed.

```typescript
describe("decryptMessageIfNeeded", () => {
  it("returns non-encrypted message unchanged", async () => {
    const msg = createMessage("m1");
    msg.encrypted = false;
    msg.content = "hello";
    const result = await decryptMessageIfNeeded(msg);
    expect(result.content).toBe("hello");
  });

  it("returns placeholder for encrypted message without session", async () => {
    const msg = createMessage("m1");
    msg.encrypted = true;
    msg.content = "ciphertext-blob";
    const result = await decryptMessageIfNeeded(msg);
    expect(result.content).toContain("[");
  });
});
```

## Commit 2: Docs/Process

### Fix 2a: CHANGELOG.md entries

Add under `### Fixed` in `[Unreleased]`:

```markdown
- Edited messages in E2EE channels now display decrypted content instead of ciphertext
- Sending multiple messages rapidly no longer reorders them in the chat view
- Slow WebSocket clients are now disconnected instead of blocking the message bus
- Typing indicators no longer trigger database permission queries on every keystroke
```

### Fix 2b: Beta checklist updates

In `docs/developer-guide/plans/2026-03-19-beta-checklist.md`:

1. Mark the 4 Important items as `[x]` with `(#497)`:
   - `[x] Permission resolution fires 3-5 queries per typing event (#497)`
   - `[x] Slow WS clients block Redis pubsub task (#497)`
   - `[x] Message edit doesn't decrypt in E2EE channels (#497)`
   - `[x] Optimistic message send race condition (#497)`

2. Update progress summary table: Important `19 | 19 | 0`, Total `48 | 26 | 22`

## File Map

### Commit 1 files
| File | Change |
|------|--------|
| `server/src/chat/messages.rs:306` | Add `#[validate(length(max = 64))]` to nonce |
| `server/src/chat/messages.rs:1060-1061` | Strip nonce from broadcast JSON |
| `server/src/chat/uploads.rs:766` | Strip nonce from broadcast JSON |
| `server/src/ws/mod.rs:1857-1881` | Fix backpressure reset logic |
| `server/src/ws/mod.rs:1565` | Add `last_typing` retain after insert |
| `server/src/db/models.rs:130` | Update comment |
| `client/src/stores/__tests__/messages.test.ts` | Add decryptMessageIfNeeded tests |

### Commit 2 files
| File | Change |
|------|--------|
| `CHANGELOG.md` | Add 4 entries under `### Fixed` |
| `docs/developer-guide/plans/2026-03-19-beta-checklist.md` | Mark 4 items done, update table |
