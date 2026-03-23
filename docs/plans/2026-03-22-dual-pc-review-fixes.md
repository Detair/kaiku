# Dual PeerConnection Review Fixes Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Fix all 15 findings from PR #485 code review — Tauri backend alignment, negotiation safety, error handling, type fixes, and test coverage.

**Architecture:** Five focused commits fixing issues grouped by concern: Tauri protocol alignment, negotiation concurrency, server error handling, client error handling, and test coverage.

**Tech Stack:** Rust (webrtc-rs, Tauri), TypeScript (Solid.js), serde, WebSocket signaling

---

## Task A: Tauri Backend Alignment (Findings #1, #6)

**Files:**
- Modify: `shared/vc-common/src/protocol/mod.rs` (ClientEvent ~line 54, ServerEvent ~line 168)
- Modify: `client/src-tauri/src/network/websocket.rs` (ClientEvent ~line 40, ServerEvent ~line 106, handle_server_message ~line 639)
- Modify: `client/src-tauri/src/commands/voice.rs` (handle_voice_offer ~line 118)
- Modify: `client/src-tauri/src/lib.rs` (invoke_handler ~line 108)

### Step 1: Update `vc-common` ClientEvent

In `shared/vc-common/src/protocol/mod.rs`, replace the old voice variants in `ClientEvent`:

Replace `VoiceOffer`/`VoiceAnswer`/`VoiceIce` (~lines 54-75) with:

```rust
VoicePublisherOffer {
    channel_id: Uuid,
    sdp: String,
},
VoiceSubscriberAnswer {
    channel_id: Uuid,
    sdp: String,
},
VoiceIceCandidate {
    channel_id: Uuid,
    candidate: String,
    #[serde(default = "default_pc_type")]
    pc_type: String,
},
```

Add `default_pc_type` function if not already present.

### Step 2: Update `vc-common` ServerEvent

Replace `VoiceOffer`/`VoiceAnswer`/`VoiceIce` in ServerEvent (~lines 168-195) with:

```rust
VoicePublisherAnswer {
    channel_id: Uuid,
    sdp: String,
},
VoiceSubscriberOffer {
    channel_id: Uuid,
    sdp: String,
},
VoiceIceCandidate {
    channel_id: Uuid,
    candidate: String,
    pc_type: String,
},
```

### Step 3: Update Tauri `websocket.rs` ClientEvent

In `client/src-tauri/src/network/websocket.rs`, replace `VoiceAnswer` (~line 40) with:

```rust
#[serde(rename = "voice_publisher_offer")]
VoicePublisherOffer {
    channel_id: String,
    sdp: String,
},
#[serde(rename = "voice_subscriber_answer")]
VoiceSubscriberAnswer {
    channel_id: String,
    sdp: String,
},
```

Update `VoiceIceCandidate` (~line 44) to add `pc_type`:

```rust
#[serde(rename = "voice_ice_candidate")]
VoiceIceCandidate {
    channel_id: String,
    candidate: String,
    #[serde(default = "default_pc_type")]
    pc_type: String,
},
```

### Step 4: Update Tauri `websocket.rs` ServerEvent

Replace `VoiceOffer` (~line 106) with:

```rust
#[serde(rename = "voice_publisher_answer")]
VoicePublisherAnswer {
    channel_id: String,
    sdp: String,
},
#[serde(rename = "voice_subscriber_offer")]
VoiceSubscriberOffer {
    channel_id: String,
    sdp: String,
},
```

Update `VoiceIceCandidate` (~line 110) to add `pc_type`:

```rust
#[serde(rename = "voice_ice_candidate")]
VoiceIceCandidate {
    channel_id: String,
    candidate: String,
    pc_type: String,
},
```

### Step 5: Update `handle_server_message` routing

In `handle_server_message` (~line 639), replace `VoiceOffer` routing:

```rust
ServerEvent::VoicePublisherAnswer { .. } => "ws:voice_publisher_answer",
ServerEvent::VoiceSubscriberOffer { .. } => "ws:voice_subscriber_offer",
```

Update `VoiceIceCandidate` routing to keep `"ws:voice_ice_candidate"` (unchanged name but now includes `pc_type` in payload).

### Step 6: Update Tauri voice commands

In `client/src-tauri/src/commands/voice.rs`:

Rename `handle_voice_offer` to `handle_voice_publisher_answer` (~line 118). This command receives the server's answer to the client's publisher offer:

```rust
#[tauri::command]
async fn handle_voice_publisher_answer(
    channel_id: String,
    sdp: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    // Set remote description (answer) on publisher PC
    let voice_state = state.voice.lock().await;
    voice_state.webrtc.set_remote_description_answer(&sdp)
        .map_err(|e| e.to_string())
}
```

Add `handle_voice_subscriber_offer` command:

```rust
#[tauri::command]
async fn handle_voice_subscriber_offer(
    channel_id: String,
    sdp: String,
    state: State<'_, AppState>,
) -> Result<String, String> {
    // Set remote description (offer), create answer, return answer SDP
    let voice_state = state.voice.lock().await;
    let answer = voice_state.webrtc.handle_subscriber_offer(&sdp)
        .map_err(|e| e.to_string())?;
    Ok(answer)
}
```

Update `handle_voice_ice_candidate` to accept `pc_type`:

```rust
#[tauri::command]
async fn handle_voice_ice_candidate(
    channel_id: String,
    candidate: String,
    pc_type: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let voice_state = state.voice.lock().await;
    voice_state.webrtc.add_ice_candidate(&candidate, &pc_type)
        .map_err(|e| e.to_string())
}
```

### Step 7: Update Tauri command registration

In `client/src-tauri/src/lib.rs` (~line 108), replace:
- `commands::voice::handle_voice_offer` → `commands::voice::handle_voice_publisher_answer`
- Add `commands::voice::handle_voice_subscriber_offer`

### Step 8: Fix compilation and commit

```bash
SQLX_OFFLINE=true cargo clippy -- -D warnings
```

Note: The Tauri native WebRTC adapter (`client/src-tauri/src/voice/`) may need updates to support dual PCs. If the WebRTC native implementation doesn't exist yet or is a stub, add TODO comments and ensure it compiles. The critical fix is the signaling protocol alignment.

```bash
git add -A
git commit -m "fix(voice): align Tauri backend with dual-PC protocol

Update vc-common shared types, Tauri websocket.rs events, and voice
commands for publisher/subscriber offer/answer message types."
```

---

## Task B: Negotiation Lock (Finding #2)

**Files:**
- Modify: `client/src/lib/webrtc/browser.ts` (~lines 1125-1141)

### Step 1: Add negotiation state fields

Add to `BrowserVoiceAdapter` class properties:

```typescript
private isNegotiating = false;
private pendingNegotiation = false;
```

### Step 2: Update `onnegotiationneeded` with lock

In `setupPublisherPC()`, replace the current `onnegotiationneeded` handler (~lines 1125-1141):

```typescript
this.publisherPC.onnegotiationneeded = async () => {
  if (!this.publisherPC) return;

  if (this.isNegotiating) {
    this.pendingNegotiation = true;
    return;
  }

  this.isNegotiating = true;
  try {
    const offer = await this.publisherPC.createOffer();
    await this.publisherPC.setLocalDescription(offer);
    const { wsSend } = await import("@/lib/tauri");
    wsSend({
      type: "voice_publisher_offer",
      channel_id: channelId,
      sdp: offer.sdp!,
    });
  } catch (err) {
    console.error("[BrowserVoiceAdapter] Publisher negotiation failed:", err);
  } finally {
    this.isNegotiating = false;
  }
};
```

### Step 3: Release lock in `handlePublisherAnswer`

In `handlePublisherAnswer()` (~line 348), after successfully setting the remote description, drain the pending negotiation:

```typescript
async handlePublisherAnswer(channelId: string, sdp: string): Promise<VoiceResult<void>> {
  if (!this.publisherPC) {
    return { ok: false, error: { type: "not_connected", reason: "Publisher PC not initialized", retriable: false } };
  }
  try {
    await this.publisherPC.setRemoteDescription(
      new RTCSessionDescription({ type: "answer", sdp })
    );

    // Drain pending negotiation
    if (this.pendingNegotiation) {
      this.pendingNegotiation = false;
      this.publisherPC.dispatchEvent(new Event("negotiationneeded"));
    }

    return { ok: true, value: undefined };
  } catch (error) {
    return { ok: false, error: { type: "connection_failed", reason: String(error), retriable: true } };
  }
}
```

### Step 4: Reset state in `leave()`/`cleanup()`

In `cleanup()`, reset the flags:

```typescript
this.isNegotiating = false;
this.pendingNegotiation = false;
```

### Step 5: Type check and commit

```bash
cd client && ~/.bun/bin/bun run typecheck && ~/.bun/bin/bun run test:run
git add client/src/lib/webrtc/browser.ts
git commit -m "fix(voice): add negotiation lock to prevent concurrent offers"
```

---

## Task C: Server Error Handling (Findings #3, #4, #7, #8, #10, #12, #13)

**Files:**
- Modify: `server/src/voice/ws_handler.rs` (~lines 404-572)
- Modify: `server/src/voice/sfu.rs` (~lines 800-926)
- Modify: `server/src/voice/peer.rs` (~lines 122-197)

### Step 1: Fix `renegotiate()` — log signaling send failure (Finding #4)

In `sfu.rs` `renegotiate()` (~line 918), replace `let _ =` with error logging:

```rust
pub async fn renegotiate(peer: &Arc<Peer>) -> Result<(), VoiceError> {
    let offer = peer.subscriber_pc.create_offer(None).await?;
    peer.subscriber_pc
        .set_local_description(offer.clone())
        .await?;

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
}
```

### Step 2: Fix `handle_publisher_offer` handler — log and propagate send failure (Finding #3)

In `ws_handler.rs` `handle_publisher_offer()` (~line 424), replace `let _ =`:

```rust
if let Err(e) = peer
    .signal_tx
    .send(OutboundMsg::Event(ServerEvent::VoicePublisherAnswer {
        channel_id,
        sdp: answer_sdp,
    }))
    .await
{
    tracing::error!(
        user_id = %user_id,
        channel_id = %channel_id,
        error = %e,
        "failed to send publisher answer — client connection will hang"
    );
    return Err(VoiceError::Signaling(
        "Failed to send publisher answer".to_string(),
    ));
}
```

Note: Check if `VoiceError::Signaling` variant exists. If not, use `VoiceError::Internal` or add it.

### Step 3: Propagate error in `handle_publisher_offer` failure path (Finding #10)

In `ws_handler.rs` (~line 438), change the error handler to propagate:

```rust
Err(e) => {
    tracing::error!(user_id = %user_id, error = %e, "failed to handle publisher offer");
    return Err(e);
}
```

### Step 4: Add `has_subscribed` flag to Peer (Finding #8)

In `peer.rs`, add to the struct:

```rust
use std::sync::atomic::{AtomicBool, Ordering};

pub has_subscribed: AtomicBool,
```

Initialize in `new()`:

```rust
has_subscribed: AtomicBool::new(false),
```

In `ws_handler.rs` `handle_publisher_offer()` (~line 433), replace:

```rust
// Old:
let needs_subscription = peer.outgoing_tracks_count().await == 0;
if needs_subscription {

// New:
if !peer.has_subscribed.swap(true, Ordering::Relaxed) {
```

### Step 5: Fix `subscribe_to_existing_tracks` lock scope (Finding #7)

In `ws_handler.rs` (~lines 456-511), collect data under lock then drop before async work:

```rust
async fn subscribe_to_existing_tracks(
    _sfu: &Arc<SfuServer>,
    room: &Arc<super::sfu::Room>,
    peer: &Arc<super::peer::Peer>,
    user_id: Uuid,
) {
    // Collect peer data under read lock, then release
    let peer_tracks: Vec<(Uuid, Arc<super::peer::Peer>, Vec<(TrackSource, Arc<TrackRemote>)>)> = {
        let peers = room.peers.read().await;
        peers
            .iter()
            .filter(|(id, _)| **id != user_id)
            .map(|(id, p)| {
                // We need the incoming tracks — but we can't hold two locks
                // So we clone the peer Arc and collect later
                (*id, p.clone(), Vec::new())
            })
            .collect()
    };
    // Read lock released here

    for (other_id, other_peer, _) in &peer_tracks {
        let incoming = other_peer.incoming_tracks.read().await;
        for (source, track) in incoming.iter() {
            match room.track_router.create_subscriber_track(
                *other_id,
                source.clone(),
                peer,
                track,
            ).await {
                Ok(local_track) => {
                    match peer.add_outgoing_track(
                        *other_id,
                        source.clone(),
                        local_track.clone(),
                    ).await {
                        Ok(sender) => {
                            // ... spawn REMB reader, PLI for screen shares ...
                        }
                        Err(e) => {
                            tracing::error!(
                                user_id = %user_id,
                                source_user = %other_id,
                                source = ?source,
                                error = %e,
                                "failed to add outgoing track"
                            );
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!(
                        user_id = %user_id,
                        source_user = %other_id,
                        source = ?source,
                        error = %e,
                        "failed to create subscriber track — user will not hear this source"
                    );
                }
            }
        }
    }

    if peer.outgoing_tracks_count().await > 0 {
        if let Err(e) = SfuServer::renegotiate(peer).await {
            tracing::error!(user_id = %user_id, error = %e, "failed to renegotiate subscriber after join");
        }
    }
}
```

Note: The exact signature of `create_subscriber_track` may differ — match the actual API. The key change is: (1) drop `room.peers` lock before async work, (2) log `create_subscriber_track` failures instead of silently dropping them.

### Step 6: Fix ICE candidate JSON serialization logging (Finding #12 from silent-failure)

In `sfu.rs` `setup_ice_handler()` (~lines 813-815, 856-858), replace `if let Ok(candidate_str)` with match:

```rust
// Publisher handler (~line 813):
match serde_json::to_string(&json) {
    Ok(candidate_str) => {
        // ... existing send logic
    }
    Err(e) => {
        tracing::error!(error = %e, "failed to serialize publisher ICE candidate");
    }
}

// Same for subscriber handler (~line 856)
```

### Step 7: Fix `Peer::close()` error logging (Finding #12)

In `peer.rs` `close()` (~line 193):

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

### Step 8: Fix `remove_outgoing_track` error logging (Finding from silent-failure #7)

In `peer.rs` `remove_outgoing_track()` (~line 134):

```rust
// Replace: let _ = self.subscriber_pc.remove_track(&sender).await;
// With:
if let Err(e) = self.subscriber_pc.remove_track(&sender).await {
    tracing::warn!(
        user_id = %self.user_id,
        error = %e,
        "failed to remove track from subscriber PC"
    );
}
```

### Step 9: Compile, test, commit

```bash
SQLX_OFFLINE=true cargo clippy -p vc-server -- -D warnings
cargo test -p vc-server
git add server/src/voice/
git commit -m "fix(voice): improve error handling for dual-PC signaling

- Log signaling send failures instead of discarding with let _ =
- Add has_subscribed flag for robust first-offer detection
- Reduce lock scope in subscribe_to_existing_tracks
- Log ICE serialization, track creation, and PC close failures"
```

---

## Task D: Client Error Handling (Findings #5, #9, #11, #15)

**Files:**
- Modify: `client/src/lib/webrtc/browser.ts`
- Modify: `client/src/lib/types.ts` (~line 450)

### Step 1: Fix ClientEvent types (Finding #5)

In `client/src/lib/types.ts` (~line 450), replace:

```typescript
| { type: "voice_answer"; channel_id: string; sdp: string }
| { type: "voice_ice_candidate"; channel_id: string; candidate: string }
```

With:

```typescript
| { type: "voice_publisher_offer"; channel_id: string; sdp: string }
| { type: "voice_subscriber_answer"; channel_id: string; sdp: string }
| { type: "voice_ice_candidate"; channel_id: string; candidate: string; pc_type?: string }
```

### Step 2: Add `.catch()` to ICE candidate sends (Finding #9)

In `browser.ts`, publisher ICE handler (~line 1148):

```typescript
import("@/lib/tauri")
  .then(({ wsSend }) =>
    wsSend({
      type: "voice_ice_candidate",
      channel_id: channelId,
      candidate: candidateJson,
      pc_type: "publisher",
    })
  )
  .catch((err) => {
    console.error("[BrowserVoiceAdapter] Failed to send publisher ICE candidate:", err);
  });
```

Same fix for subscriber ICE handler (~line 1271), with `"subscriber"` in the log message.

### Step 3: Surface subscriber PC failure to user (Finding #11)

In `browser.ts`, subscriber `onconnectionstatechange` (~line 1283):

```typescript
this.subscriberPC.onconnectionstatechange = () => {
  const state = this.subscriberPC?.connectionState;
  console.log(`[BrowserVoiceAdapter] Subscriber connection state: ${state}`);

  if (state === "failed") {
    this.eventHandlers.onError?.({
      type: "connection_failed",
      reason: "Subscriber connection failed — you may not hear other participants",
      retriable: true,
    });
  }
};
```

### Step 4: Fix `answer.sdp!` null assertion (Finding #15)

In `handleSubscriberOffer()` (~line 406):

```typescript
const answerSdp = answer.sdp;
if (!answerSdp) {
  return {
    ok: false,
    error: {
      type: "connection_failed" as const,
      reason: "Browser returned empty SDP answer",
      retriable: true,
    },
  };
}
return { ok: true, value: answerSdp };
```

### Step 5: Type check, test, commit

```bash
cd client && ~/.bun/bin/bun run typecheck && ~/.bun/bin/bun run test:run
git add client/src/lib/webrtc/browser.ts client/src/lib/types.ts
git commit -m "fix(client): improve error handling for dual-PC signaling

- Update ClientEvent types for new wire format
- Add .catch() to ICE candidate sends
- Surface subscriber PC failure to user
- Replace answer.sdp! with null check"
```

---

## Task E: Test Coverage (Finding #14)

**Files:**
- Modify: `server/src/ws/mod.rs` (test module)

### Step 1: Add subscriber answer serialization test

```rust
#[test]
fn test_subscriber_answer_client_event_serialization() {
    let event = ClientEvent::VoiceSubscriberAnswer {
        channel_id: Uuid::nil(),
        sdp: "v=0\r\n".to_string(),
    };
    let json = serde_json::to_string(&event).unwrap();
    assert!(json.contains("voice_subscriber_answer"));
    let parsed: ClientEvent = serde_json::from_str(&json).unwrap();
    match parsed {
        ClientEvent::VoiceSubscriberAnswer { sdp, .. } => assert_eq!(sdp, "v=0\r\n"),
        _ => panic!("wrong variant"),
    }
}
```

### Step 2: Add ICE candidate with explicit pc_type roundtrip

```rust
#[test]
fn test_ice_candidate_publisher_pc_type_roundtrip() {
    let event = ClientEvent::VoiceIceCandidate {
        channel_id: Uuid::nil(),
        candidate: "candidate:1 1 UDP 2130706431 192.168.1.1 12345 typ host".to_string(),
        pc_type: "subscriber".to_string(),
    };
    let json = serde_json::to_string(&event).unwrap();
    let parsed: ClientEvent = serde_json::from_str(&json).unwrap();
    match parsed {
        ClientEvent::VoiceIceCandidate { pc_type, .. } => assert_eq!(pc_type, "subscriber"),
        _ => panic!("wrong variant"),
    }
}
```

### Step 3: Run tests and commit

```bash
cargo test -p vc-server -- ws::tests --nocapture
git add server/src/ws/mod.rs
git commit -m "test(voice): add serialization tests for subscriber answer and ICE pc_type"
```

---

## Summary

| Task | Findings | Files | Estimated |
|------|----------|-------|-----------|
| A: Tauri alignment | #1, #6 | vc-common, tauri websocket.rs, voice.rs, lib.rs | 1.5h |
| B: Negotiation lock | #2 | browser.ts | 30min |
| C: Server errors | #3,#4,#7,#8,#10,#12,#13 | ws_handler.rs, sfu.rs, peer.rs | 1h |
| D: Client errors | #5,#9,#11,#15 | browser.ts, types.ts | 30min |
| E: Tests | #14 | ws/mod.rs | 15min |
| **Total** | **15 findings** | | **~4h** |
