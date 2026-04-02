# PR #497 Review Fixes — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix 8 issues found during code review of PR #497, split into a code-fix commit and a docs/process commit.

**Architecture:** All fixes are independent one-line to few-line changes across server Rust files, one client test file, and two documentation files. No new modules or dependencies.

**Tech Stack:** Rust (server), TypeScript/Vitest (client tests), Markdown (docs)

**Spec:** `docs/superpowers/specs/2026-04-02-pr497-review-fixes-design.md`

---

## File Map

### Commit 1 — Code fixes
| File | Responsibility |
|------|---------------|
| `server/src/chat/messages.rs` | Nonce validation + strip nonce from broadcast |
| `server/src/chat/uploads.rs` | Strip nonce from upload broadcast |
| `server/src/ws/mod.rs` | Backpressure reset logic + last_typing cleanup |
| `server/src/db/models.rs` | Update stale comment |
| `client/src/stores/__tests__/messages.test.ts` | E2EE decryption tests |

### Commit 2 — Docs/process
| File | Responsibility |
|------|---------------|
| `CHANGELOG.md` | Add 4 Fixed entries under [Unreleased] |
| `docs/developer-guide/plans/2026-03-19-beta-checklist.md` | Mark 4 items done, update summary table |

---

## Task 1: Server — Nonce validation, broadcast stripping, and comment fix

**Files:**
- Modify: `server/src/chat/messages.rs:306` (add validate attribute)
- Modify: `server/src/chat/messages.rs:1061` (strip nonce from broadcast)
- Modify: `server/src/chat/uploads.rs:766` (strip nonce from broadcast)
- Modify: `server/src/db/models.rs:130` (update comment)

- [ ] **Step 1: Add nonce length validation**

At `server/src/chat/messages.rs:306`, add the validate attribute:

```rust
// Before (line 306)
    pub nonce: Option<String>,

// After
    #[validate(length(max = 64))]
    pub nonce: Option<String>,
```

- [ ] **Step 2: Strip nonce from broadcast in messages.rs**

At `server/src/chat/messages.rs:1061`, replace the serialization line. This single line feeds both the `MessageNew` and `ThreadReplyNew` broadcast branches (the `if/else` at lines 1063-1096), so one change covers both.

```rust
// Before (line 1061)
    let message_json = serde_json::to_value(&response).unwrap_or_default();

// After
    let broadcast_response = MessageResponse { nonce: None, ..response.clone() };
    let message_json = serde_json::to_value(&broadcast_response).unwrap_or_default();
```

- [ ] **Step 3: Strip nonce from broadcast in uploads.rs**

At `server/src/chat/uploads.rs:766`, same pattern:

```rust
// Before (line 766)
    let message_json = serde_json::to_value(&response).unwrap_or_default();

// After
    let broadcast_response = MessageResponse { nonce: None, ..response.clone() };
    let message_json = serde_json::to_value(&broadcast_response).unwrap_or_default();
```

- [ ] **Step 4: Update DB model comment**

At `server/src/db/models.rs:130`:

```rust
// Before
    /// Encryption nonce (for E2EE).

// After
    /// Client-generated nonce for optimistic message matching (also used as E2EE encryption nonce).
```

- [ ] **Step 5: Build and verify**

Run: `SQLX_OFFLINE=true cargo clippy -p vc-server -- -D warnings`
Expected: PASS

---

## Task 2: Server — Backpressure reset and last_typing cleanup

**Files:**
- Modify: `server/src/ws/mod.rs:1860-1861` (remove reset from Ok arm)
- Modify: `server/src/ws/mod.rs:1881` (add reset at loop top)
- Modify: `server/src/ws/mod.rs:1565` (add retain after insert)

- [ ] **Step 1: Remove counter reset from try_forward! Ok arm**

At `server/src/ws/mod.rs:1860-1861`, replace the body of the `Ok(())` match arm:

```rust
// Before (lines 1860-1861)
                Ok(()) => {
                    $drops = 0;
                }

// After
                Ok(()) => {}
```

- [ ] **Step 2: Add counter reset at top of while loop**

At `server/src/ws/mod.rs:1881-1882`, add the reset after the `while let` line:

```rust
// Before (lines 1881-1882)
    while let Ok(message) = pubsub_stream.recv().await {
        let channel_name = message.channel.to_string();

// After
    while let Ok(message) = pubsub_stream.recv().await {
        // Reset backpressure counter at the start of each pubsub message.
        // Only consecutive back-to-back Full errors across sequential messages
        // trigger disconnect.
        backpressure_drops = 0;

        let channel_name = message.channel.to_string();
```

- [ ] **Step 3: Add last_typing retain after insert**

At `server/src/ws/mod.rs:1565`, add the retain call after the existing insert:

```rust
// Before (line 1565)
            msg_state.last_typing.insert(channel_id, now);

// After
            msg_state.last_typing.insert(channel_id, now);
            msg_state.last_typing.retain(|_, last| now.duration_since(*last) < Duration::from_secs(2));
```

- [ ] **Step 4: Build and verify**

Run: `SQLX_OFFLINE=true cargo clippy -p vc-server -- -D warnings`
Expected: PASS

---

## Task 3: Client — E2EE decryption tests

**Files:**
- Modify: `client/src/stores/__tests__/messages.test.ts:28-44` (add import)
- Modify: `client/src/stores/__tests__/messages.test.ts:304` (add test block)

- [ ] **Step 1: Add decryptMessageIfNeeded to imports**

At `client/src/stores/__tests__/messages.test.ts:28-44`, add `decryptMessageIfNeeded` to the import block:

```typescript
// Before (lines 28-44)
import {
  messagesState,
  setMessagesState,
  loadMessages,
  loadInitialMessages,
  sendMessage,
  addMessage,
  updateMessage,
  removeMessage,
  getChannelMessages,
  isLoadingMessages,
  hasMoreMessages,
  clearChannelMessages,
  clearCurve25519KeyCache,
  editingMessageId,
  setEditingMessageId,
} from "../messages";

// After
import {
  messagesState,
  setMessagesState,
  loadMessages,
  loadInitialMessages,
  sendMessage,
  addMessage,
  updateMessage,
  removeMessage,
  getChannelMessages,
  isLoadingMessages,
  hasMoreMessages,
  clearChannelMessages,
  clearCurve25519KeyCache,
  editingMessageId,
  setEditingMessageId,
  decryptMessageIfNeeded,
} from "../messages";
```

- [ ] **Step 2: Add decryptMessageIfNeeded test block**

At `client/src/stores/__tests__/messages.test.ts:304`, before the `describe("updateMessage"` block, add:

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

- [ ] **Step 3: Run tests to verify they pass**

Run: `cd client && bun run test:run -- --reporter=verbose src/stores/__tests__/messages.test.ts`
Expected: PASS — both new tests should pass (non-encrypted passthrough; encrypted without session returns placeholder)

---

## Task 4: Commit code fixes

- [ ] **Step 1: Commit all code changes**

```bash
git add server/src/chat/messages.rs server/src/chat/uploads.rs server/src/ws/mod.rs server/src/db/models.rs client/src/stores/__tests__/messages.test.ts
git commit -m "fix: address code review issues from PR #497

Add nonce length validation (#[validate(length(max = 64))]),
strip nonce from WS broadcasts to other users, fix backpressure
counter reset logic, prune stale last_typing entries, update DB
model comment, and add E2EE decryption tests."
```

---

## Task 5: Docs — CHANGELOG and beta checklist

**Files:**
- Modify: `CHANGELOG.md:28` (add entries after existing Fixed items)
- Modify: `docs/developer-guide/plans/2026-03-19-beta-checklist.md:48,57,87,98,168,170` (mark done, update table)

- [ ] **Step 1: Add CHANGELOG entries**

At `CHANGELOG.md`, after the last existing entry under `### Fixed` in `[Unreleased]` (after line 39 — `- DM unread aggregate query now uses correct \`dm_read_state\` table`), add:

```markdown
- Edited messages in E2EE channels now display decrypted content instead of ciphertext
- Sending multiple messages rapidly no longer reorders them in the chat view
- Slow WebSocket clients are now disconnected instead of blocking the message bus
- Typing indicators no longer trigger database permission queries on every keystroke
```

- [ ] **Step 2: Mark 4 beta checklist items as done**

In `docs/developer-guide/plans/2026-03-19-beta-checklist.md`:

Line 48: `- [ ] **Permission resolution fires 3-5 queries per typing event**`
→ `- [x] **Permission resolution fires 3-5 queries per typing event** (#497)`

Line 57: `- [ ] **Slow WS clients block Redis pubsub task**`
→ `- [x] **Slow WS clients block Redis pubsub task** (#497)`

Line 87: `- [ ] **Message edit doesn't decrypt in E2EE channels**`
→ `- [x] **Message edit doesn't decrypt in E2EE channels** (#497)`

Line 98: `- [ ] **Optimistic message send race condition**`
→ `- [x] **Optimistic message send race condition** (#497)`

- [ ] **Step 3: Update progress summary table**

At `docs/developer-guide/plans/2026-03-19-beta-checklist.md:168,170`:

```markdown
// Before
| Important | 19 | 15 | 4 |
...
| **Total** | **48** | **22** | **26** |

// After
| Important | 19 | 19 | 0 |
...
| **Total** | **48** | **26** | **22** |
```

- [ ] **Step 4: Commit docs changes**

```bash
git add CHANGELOG.md docs/developer-guide/plans/2026-03-19-beta-checklist.md
git commit -m "docs: update CHANGELOG and beta checklist for PR #497 fixes

Add 4 Fixed entries to CHANGELOG [Unreleased] and mark all 4
Important beta checklist items as done with (#497) references."
```

- [ ] **Step 5: Push and verify**

Run: `git push`
Expected: Branch updated on remote, PR #497 shows new commits.
