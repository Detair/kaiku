# PR #485 Review Fixes — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix 11 code review issues in PR #485 and add Tauri compatibility guard for 2 deferred architectural issues.

**Architecture:** Three independent groups of fixes: (1) server error propagation, (2) browser client fixes, (3) protocol typing + Tauri compatibility. Each group is a separate task that can be committed independently.

**Tech Stack:** Rust (server + Tauri), TypeScript/Solid.js (browser client)

**Spec:** `docs/superpowers/specs/2026-03-23-pr485-review-fixes.md`

**Worktree:** `/home/detair/GIT/detair/kaiku/.claude/worktrees/dual-pc` (branch `feature/dual-peerconnection`)

---

### Task 1: Server error propagation (#3, #4, #6, #7, #9, #11)

**Files:**
- Modify: `server/src/voice/ws_handler.rs:569-598, 756-764, 956`
- Modify: `server/src/voice/sfu.rs:778-792`
- Modify: `server/src/voice/peer.rs:204-212`

- [ ] **Step 1: Fix `handle_subscriber_answer` — propagate error (#3)**

In `server/src/voice/ws_handler.rs`, replace lines 569–572:

```rust
    if let Err(e) = SfuServer::handle_subscriber_answer(&peer, sdp.to_string()).await {
        error!(user_id = %user_id, error = %e, "failed to handle subscriber answer");
    }

    Ok(())
```

with:

```rust
    SfuServer::handle_subscriber_answer(&peer, sdp.to_string())
        .await
        .map_err(|e| {
            error!(user_id = %user_id, channel_id = %channel_id, error = %e, "failed to handle subscriber answer");
            e
        })?;

    Ok(())
```

- [ ] **Step 2: Fix `handle_ice_candidate` — add channel_id to log (#4)**

In `server/src/voice/ws_handler.rs`, line 597, add `channel_id` to the log:

```rust
    if let Err(e) = SfuServer::handle_ice_candidate(&peer, candidate, pc_type).await {
        error!(user_id = %user_id, channel_id = %channel_id, pc_type = %pc_type, error = %e, "failed to add ICE candidate");
    }
```

Keep returning `Ok(())` — ICE failures during trickle are expected and not session-fatal.

- [ ] **Step 3: Fix `renegotiate` — propagate signal send failure (#6)**

In `server/src/voice/sfu.rs`, replace lines 778–792:

```rust
        if let Err(e) = peer
            .signal_tx
            .send(OutboundMsg::Event(ServerEvent::VoiceSubscriberOffer {
                channel_id: peer.channel_id,
                sdp: offer.sdp,
            }))
            .await
        {
            tracing::error!(
                user_id = %peer.user_id,
                channel_id = %peer.channel_id,
                error = %e,
                "failed to send subscriber offer — client will not receive tracks"
            );
        }
        Ok(())
```

with:

```rust
        peer.signal_tx
            .send(OutboundMsg::Event(ServerEvent::VoiceSubscriberOffer {
                channel_id: peer.channel_id,
                sdp: offer.sdp,
            }))
            .await
            .map_err(|e| {
                tracing::error!(
                    user_id = %peer.user_id,
                    channel_id = %peer.channel_id,
                    error = %e,
                    "failed to send subscriber offer — client will not receive tracks"
                );
                VoiceError::Signaling("failed to send subscriber offer".into())
            })?;
        Ok(())
```

- [ ] **Step 4: Fix `max_screen_shares` — log DB error before fallback (#7)**

In `server/src/voice/ws_handler.rs`, replace lines 757–764:

```rust
    let max_screen_shares: i32 =
        sqlx::query_scalar("SELECT max_screen_shares FROM channels WHERE id = $1")
            .bind(params.channel_id)
            .fetch_optional(pool)
            .await
            .ok()
            .flatten()
            .unwrap_or(6);
```

with:

```rust
    let max_screen_shares: i32 =
        sqlx::query_scalar("SELECT max_screen_shares FROM channels WHERE id = $1")
            .bind(params.channel_id)
            .fetch_optional(pool)
            .await
            .inspect_err(|e| {
                warn!(channel_id = %params.channel_id, error = %e, "Failed to query max_screen_shares, using default");
            })
            .ok()
            .flatten()
            .unwrap_or(6);
```

- [ ] **Step 5: Fix `Peer::close` — honest return type (#9)**

In `server/src/voice/peer.rs`, replace lines 204–212:

```rust
    pub async fn close(&self) -> Result<(), super::error::VoiceError> {
        if let Err(e) = self.publisher_pc.close().await {
            tracing::warn!(user_id = %self.user_id, error = %e, "failed to close publisher PC");
        }
        if let Err(e) = self.subscriber_pc.close().await {
            tracing::warn!(user_id = %self.user_id, error = %e, "failed to close subscriber PC");
        }
        Ok(())
    }
```

with:

```rust
    pub async fn close(&self) {
        if let Err(e) = self.publisher_pc.close().await {
            tracing::warn!(user_id = %self.user_id, error = %e, "failed to close publisher PC");
        }
        if let Err(e) = self.subscriber_pc.close().await {
            tracing::warn!(user_id = %self.user_id, error = %e, "failed to close subscriber PC");
        }
    }
```

Then update both callers:

In `server/src/voice/sfu.rs` ~line 107, replace:
```rust
            if let Err(e) = old_peer.close().await {
                tracing::warn!(user_id = %peer.user_id, error = %e, "Failed to close stale peer connections");
            }
```
with:
```rust
            old_peer.close().await;
```

In `server/src/voice/ws_handler.rs` ~line 375, replace:
```rust
        if let Err(e) = peer.close().await {
            warn!(error = %e, "Error closing peer connection");
        }
```
with:
```rust
        peer.close().await;
```

- [ ] **Step 6: Fix `handle_webcam_start` — use `map_permission_error` (#11)**

In `server/src/voice/ws_handler.rs` line 956, replace:
```rust
        .map_err(|_e: crate::permissions::PermissionError| VoiceError::Unauthorized)?;
```
with:
```rust
        .map_err(|e| map_permission_error(e, "webcam start"))?;
```

- [ ] **Step 7: Verify server compiles**

Run: `cd /home/detair/GIT/detair/kaiku/.claude/worktrees/dual-pc && SQLX_OFFLINE=true cargo clippy -p vc-server -- -D warnings`
Expected: compiles clean

- [ ] **Step 8: Commit**

```bash
git add server/src/voice/ws_handler.rs server/src/voice/sfu.rs server/src/voice/peer.rs
git commit -m "fix(voice): improve error propagation and logging in voice handlers

Co-Authored-By: Claude Opus 4.6 (1M context) <noreply@anthropic.com>"
```

---

### Task 2: Browser client fixes (#5, #8, #13)

**Files:**
- Modify: `client/src/lib/webrtc/browser.ts:739-743, 1013-1016, 1167`
- Modify: `client/src/lib/webrtc/types.ts:58-75`

- [ ] **Step 1: Fix `onnegotiationneeded` — await wsSend (#5)**

In `client/src/lib/webrtc/browser.ts` line 1167, add `await`:

Change:
```typescript
        wsSend({
```
to:
```typescript
        await wsSend({
```

- [ ] **Step 2: Add `onWarning` to `VoiceAdapterEvents` (#8)**

In `client/src/lib/webrtc/types.ts`, add after the `onError` line (~line 60):

```typescript
  onWarning?: (warning: { type: string; message: string }) => void;
```

- [ ] **Step 3: Emit TURN fallback warning (#8)**

In `client/src/lib/webrtc/browser.ts`, in `fetchIceConfig`, add the warning call before BOTH `return fallback` paths:

1. The HTTP error path (~line 1038, where `!response.ok`) — before `return fallback;`
2. The exception catch block (~line 1064) — before `return fallback;`

Add at both locations:
```typescript
      this.eventHandlers.onWarning?.({
        type: "turn_unavailable",
        message: "TURN server unavailable — voice may not work on restrictive networks",
      });
```

Do NOT add it to the no-token path (~line 1023) — that's a config/auth issue, not TURN unavailability.

- [ ] **Step 4: Fix `setOutputDevice` — await `setSinkId` (#13)**

In `client/src/lib/webrtc/browser.ts`, replace lines 735–743:

```typescript
      for (const stream of this.remoteStreams.values()) {
        const audioElements = document.querySelectorAll(
          `audio[data-stream-id="${stream.id}"]`,
        );
        audioElements.forEach((audio) => {
          if ("setSinkId" in audio) {
            (audio as AudioElementWithSinkId).setSinkId(deviceId);
          }
        });
      }
```

with:

```typescript
      for (const stream of this.remoteStreams.values()) {
        const audioElements = document.querySelectorAll(
          `audio[data-stream-id="${stream.id}"]`,
        );
        for (const audio of audioElements) {
          if ("setSinkId" in audio) {
            try {
              await (audio as AudioElementWithSinkId).setSinkId(deviceId);
            } catch (err) {
              console.warn("[BrowserVoiceAdapter] Failed to set sink on element:", err);
            }
          }
        }
      }
```

- [ ] **Step 5: Run frontend tests**

Run: `cd /home/detair/GIT/detair/kaiku/.claude/worktrees/dual-pc/client && ~/.bun/bin/bun run test:run`
Expected: all tests pass

- [ ] **Step 6: Commit**

```bash
git add client/src/lib/webrtc/browser.ts client/src/lib/webrtc/types.ts
git commit -m "fix(client): await wsSend in negotiation, TURN fallback warning, await setSinkId

Co-Authored-By: Claude Opus 4.6 (1M context) <noreply@anthropic.com>"
```

---

### Task 3: PcType enum (#10)

**Files:**
- Modify: `shared/vc-common/src/protocol/mod.rs:70-77, 186-193`
- Modify: `server/src/ws/mod.rs:106-108, 164-172, 455-462`
- Modify: `server/src/voice/sfu.rs:697-702, 796-809`
- Modify: `server/src/voice/ws_handler.rs:65`
- Modify: `client/src-tauri/src/network/websocket.rs:17-20, 53-58, 125-129`
- Modify: `client/src-tauri/src/commands/voice.rs:65-68`

- [ ] **Step 1: Add `PcType` enum to shared protocol**

In `shared/vc-common/src/protocol/mod.rs`, add before `ClientEvent`:

```rust
/// Which PeerConnection an ICE candidate belongs to.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum PcType {
    Publisher,
    Subscriber,
}

impl std::fmt::Display for PcType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PcType::Publisher => write!(f, "publisher"),
            PcType::Subscriber => write!(f, "subscriber"),
        }
    }
}
```

`Display` is required because `tracing` macros use `%pc_type` (Display format) in `ws_handler.rs`.

Then update `ClientEvent::VoiceIceCandidate` (~line 70):
```rust
        pc_type: PcType,
```

And `ServerEvent::VoiceIceCandidate` (~line 186):
```rust
        pc_type: PcType,
```

- [ ] **Step 2: Update `server/src/ws/mod.rs`**

Replace `default_pc_type` function (lines 106–109):
```rust
fn default_pc_type() -> String {
    "publisher".to_string()
}
```
with:
```rust
fn default_pc_type() -> PcType {
    PcType::Publisher
}
```

Add import at top: `use vc_common::protocol::PcType;`

Update `ClientEvent::VoiceIceCandidate` (~line 170):
```rust
        pc_type: PcType,
```

Update `ServerEvent::VoiceIceCandidate` (~line 461):
```rust
        pc_type: PcType,
```

- [ ] **Step 3: Update `server/src/voice/sfu.rs`**

In `register_ice_callback` (~line 702), change:
```rust
                        pc_type: pc_label.to_string(),
```
to:
```rust
                        pc_type: if pc_label == "subscriber" { PcType::Subscriber } else { PcType::Publisher },
```

Add import: `use vc_common::protocol::PcType;` (the `vc-common` crate is a declared dependency in `server/Cargo.toml`).

In `handle_ice_candidate` (~line 800), change signature:
```rust
        pc_type: &str,
```
to:
```rust
        pc_type: &PcType,
```

And the match (~line 806):
```rust
        let pc = if pc_type == "subscriber" {
            &peer.subscriber_pc
        } else {
            &peer.publisher_pc
        };
```
to:
```rust
        let pc = match pc_type {
            PcType::Subscriber => &peer.subscriber_pc,
            PcType::Publisher => &peer.publisher_pc,
        };
```

- [ ] **Step 4: Update `server/src/voice/ws_handler.rs`**

In `handle_ice_candidate` (~line 577), change parameter type:
```rust
    pc_type: &str,
```
to:
```rust
    pc_type: &PcType,
```

And the call site (~line 65):
```rust
        } => handle_ice_candidate(sfu, user_id, channel_id, &candidate, &pc_type).await,
```
(This should still work since `pc_type` is now `PcType` not `String`.)

- [ ] **Step 5: Update Tauri `websocket.rs`**

In `client/src-tauri/src/network/websocket.rs`, replace `default_pc_type` (lines 17–20):
```rust
fn default_pc_type() -> String {
    "publisher".to_string()
}
```
with:
```rust
fn default_pc_type() -> PcType {
    PcType::Publisher
}
```

Add import: `use vc_common::protocol::PcType;`

Update `ClientEvent::VoiceIceCandidate` (~line 57):
```rust
        pc_type: PcType,
```

Update `ServerEvent::VoiceIceCandidate` (~line 128):
```rust
        pc_type: PcType,
```

- [ ] **Step 6: Update Tauri `commands/voice.rs`**

In `client/src-tauri/src/commands/voice.rs` ~line 68, change:
```rust
                            pc_type: "publisher".to_string(),
```
to:
```rust
                            pc_type: PcType::Publisher,
```

Also update `handle_voice_ice_candidate` parameter (~line 254), change:
```rust
    pc_type: String,
```
to:
```rust
    pc_type: PcType,
```

And update the debug log (~line 258) from `pc_type` to use `{pc_type}` (Display).

Add import: `use vc_common::protocol::PcType;`

- [ ] **Step 7: Update existing tests in `server/src/ws/mod.rs`**

The tests at ~lines 2175–2205 reference `pc_type` as strings. Update:

`test_ice_candidate_pc_type_default` (~line 2180):
```rust
            ClientEvent::VoiceIceCandidate { pc_type, .. } => assert_eq!(pc_type, PcType::Publisher),
```

`test_ice_candidate_pc_type_explicit` (~line 2191):
```rust
            ClientEvent::VoiceIceCandidate { pc_type, .. } => assert_eq!(pc_type, PcType::Subscriber),
```

`test_server_ice_candidate_includes_pc_type` (~line 2198):
```rust
        let event = ServerEvent::VoiceIceCandidate {
            channel_id: Uuid::nil(),
            candidate: "candidate:...".to_string(),
            pc_type: PcType::Subscriber,
        };
```

- [ ] **Step 8: Verify full workspace compiles**

Run: `cd /home/detair/GIT/detair/kaiku/.claude/worktrees/dual-pc && SQLX_OFFLINE=true cargo clippy -- -D warnings`
Expected: compiles clean (libspa issue is pre-existing)

- [ ] **Step 9: Commit**

```bash
git add shared/vc-common/src/protocol/mod.rs server/src/ws/mod.rs server/src/voice/sfu.rs server/src/voice/ws_handler.rs client/src-tauri/src/network/websocket.rs client/src-tauri/src/commands/voice.rs
git commit -m "refactor(voice): replace pc_type String with PcType enum

Co-Authored-By: Claude Opus 4.6 (1M context) <noreply@anthropic.com>"
```

---

### Task 4: Tauri compatibility guard + ICE logging (#1, #2, #12)

**Files:**
- Modify: `client/src-tauri/src/commands/voice.rs:201-244, 272-274`
- Modify: `client/src-tauri/src/webrtc/mod.rs:399-412`

- [ ] **Step 1: Guard subscriber offer — no-op instead of corrupting publisher PC (#1, #2)**

In `client/src-tauri/src/commands/voice.rs`, replace lines 219–244 (the TODO block and implementation):

```rust
    // TODO(dual-pc): The native WebRTC adapter currently uses a single PeerConnection.
    // For now, we handle subscriber offers on the same PC. A proper dual-PC
    // implementation would use a separate subscriber PeerConnection.
    let answer = voice_state
        .webrtc
        .handle_offer(&sdp)
        .await
        .map_err(|e| e.to_string())?;

    // Send subscriber answer to server
    let ws = state.websocket.read().await;
    if let Some(ws_manager) = ws.as_ref() {
        ws_manager
            .send(ClientEvent::VoiceSubscriberAnswer {
                channel_id: channel_id.clone(),
                sdp: answer,
            })
            .await
            .map_err(|e| format!("Failed to send VoiceSubscriberAnswer: {e}"))?;
    } else {
        return Err("WebSocket not connected".into());
    }

    info!("Subscriber answer sent for channel: {}", channel_id);
    Ok(())
```

with:

```rust
    // TODO(dual-pc): Tauri client uses a single PeerConnection. Applying a
    // subscriber offer to the publisher PC would corrupt its SDP state.
    // Ignore subscriber offers until Tauri gets its own dual-PC implementation.
    warn!(
        channel_id = %channel_id,
        "Tauri client does not support dual-PC subscriber offers yet — ignoring"
    );
    Ok(())
```

- [ ] **Step 2: Guard subscriber ICE candidates (#2)**

In `client/src-tauri/src/commands/voice.rs`, in `handle_voice_ice_candidate` (~line 272), replace:

```rust
    // TODO(dual-pc): Route to the correct PeerConnection based on pc_type.
    // Currently both publisher and subscriber use the same single PC.
    voice_state
        .webrtc
```

with:

```rust
    // TODO(dual-pc): Route to the correct PeerConnection based on pc_type.
    // Currently Tauri uses a single PC — only handle publisher candidates.
    if pc_type == PcType::Subscriber {
        warn!(channel_id = %channel_id, "Tauri client ignoring subscriber ICE candidate");
        return Ok(());
    }
    voice_state
        .webrtc
```

This works because Task 3 Step 6 changes `handle_voice_ice_candidate`'s `pc_type` parameter from `String` to `PcType`. Task 3 must be completed before Task 4.

- [ ] **Step 3: Fix Tauri ICE candidate logging (#12)**

In `client/src-tauri/src/webrtc/mod.rs`, replace lines 399–412:

```rust
        pc.on_ice_candidate(Box::new(move |candidate: Option<RTCIceCandidate>| {
            let on_ice_candidate = on_ice_candidate.clone();
            Box::pin(async move {
                if let Some(candidate) = candidate {
                    if let Ok(json) = candidate.to_json() {
                        if let Ok(callback) = on_ice_candidate.try_read() {
                            if let Some(ref cb) = *callback {
                                cb(serde_json::to_string(&json).unwrap_or_default());
                            }
                        }
                    }
                }
            })
        }));
```

with:

```rust
        pc.on_ice_candidate(Box::new(move |candidate: Option<RTCIceCandidate>| {
            let on_ice_candidate = on_ice_candidate.clone();
            Box::pin(async move {
                let Some(candidate) = candidate else { return };
                let json = match candidate.to_json() {
                    Ok(j) => j,
                    Err(e) => {
                        tracing::warn!(error = %e, "Failed to convert ICE candidate to JSON");
                        return;
                    }
                };
                let candidate_str = match serde_json::to_string(&json) {
                    Ok(s) => s,
                    Err(e) => {
                        tracing::warn!(error = %e, "Failed to serialize ICE candidate JSON");
                        return;
                    }
                };
                match on_ice_candidate.try_read() {
                    Ok(callback) => {
                        if let Some(ref cb) = *callback {
                            cb(candidate_str);
                        }
                    }
                    Err(_) => {
                        tracing::warn!("ICE candidate callback lock contended — candidate dropped");
                    }
                }
            })
        }));
```

- [ ] **Step 4: Verify compilation**

Run: `cd /home/detair/GIT/detair/kaiku/.claude/worktrees/dual-pc && SQLX_OFFLINE=true cargo clippy -p vc-server -- -D warnings`
Expected: compiles clean

- [ ] **Step 5: Commit**

```bash
git add client/src-tauri/src/commands/voice.rs client/src-tauri/src/webrtc/mod.rs
git commit -m "fix(voice): Tauri compatibility guard for dual-PC + ICE candidate logging

Co-Authored-By: Claude Opus 4.6 (1M context) <noreply@anthropic.com>"
```

---

### Task 5: Full verification

- [ ] **Step 1: Server clippy**

Run: `cd /home/detair/GIT/detair/kaiku/.claude/worktrees/dual-pc && SQLX_OFFLINE=true cargo clippy -p vc-server -- -D warnings`
Expected: clean

- [ ] **Step 2: Frontend tests**

Run: `cd /home/detair/GIT/detair/kaiku/.claude/worktrees/dual-pc/client && ~/.bun/bin/bun run test:run`
Expected: all pass

- [ ] **Step 3: Server voice tests**

Run: `cd /home/detair/GIT/detair/kaiku/.claude/worktrees/dual-pc && SQLX_OFFLINE=true cargo test -p vc-server --lib -- voice 2>&1 | grep -E "test result|FAILED"`
Expected: voice tests pass (DB-dependent tests may fail — that's pre-existing)
