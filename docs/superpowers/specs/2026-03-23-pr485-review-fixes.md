# PR #485 Review Fixes

**Date:** 2026-03-23
**Status:** Approved
**Branch:** `feature/dual-peerconnection`
**Depends on:** VP8-only codec constraint (implemented), simplifier refactors (committed)

## Problem

PR #485 code review found 13 issues across the dual PeerConnection implementation. Two are critical Tauri dual-PC architectural gaps; the rest are error handling, protocol typing, and browser client fixes.

## Scope

Fix 11 issues directly. Add a compatibility guard for the 2 Tauri dual-PC issues and defer full Tauri dual-PC to a follow-up PR.

## Changes

### Group 1: Server Error Propagation

#### 1.1 — `handle_subscriber_answer` propagate errors (#3)

**File:** `server/src/voice/ws_handler.rs:~570`

Currently logs the error and returns `Ok(())`. Change to return `Err(e)` so the caller can send an error event to the client. The caller (`handle_voice_event`) already has error handling that sends `VoiceError` back via WebSocket.

#### 1.2 — `handle_ice_candidate` propagate errors (#4)

**File:** `server/src/voice/ws_handler.rs:~597`

Same pattern as 1.1. Return `Err(e)` instead of swallowing. The caller handles errors.

#### 1.3 — `renegotiate` propagate signal send failure (#6)

**File:** `server/src/voice/sfu.rs:~885`

When `signal_tx.send(...)` fails, return `Err(VoiceError::Signaling("failed to send subscriber offer".into()))` instead of logging and returning `Ok(())`. Callers can then clean up the stale peer.

#### 1.4 — `max_screen_shares` log DB errors (#7)

**File:** `server/src/voice/ws_handler.rs:~758`

Add `warn!` log before the `.ok()` fallback:
```rust
let max_shares = sqlx::query_scalar!(...)
    .fetch_optional(&state.db)
    .await
    .map_err(|e| {
        warn!(channel_id = %channel_id, error = %e, "Failed to query max_screen_shares, using default");
        e
    })
    .ok()
    .flatten()
    .unwrap_or(6);
```

Keep the fallback behavior — this is a soft error. But operators get visibility.

#### 1.5 — `Peer::close()` honest return type (#9)

**File:** `server/src/voice/peer.rs:~204`

Change return type from `Result<(), VoiceError>` to `()`. The method already logs errors internally and always returns `Ok(())`. Callers that check `if let Err(e) = peer.close()` are dead code — remove those checks.

#### 1.6 — `handle_webcam_start` use `map_permission_error` (#11)

**File:** `server/src/voice/ws_handler.rs:~957`

Replace `|_e| VoiceError::Unauthorized` with `map_permission_error(e, "webcam start")` for consistency with `handle_screen_share_start` and `handle_join`.

### Group 2: Browser Client Fixes

#### 2.1 — `onnegotiationneeded` await wsSend (#5)

**File:** `client/src/lib/webrtc/browser.ts:~1167`

Add `await` before `wsSend(...)` call inside the `onnegotiationneeded` handler. The existing `catch` block already resets `this.isNegotiating = false` and calls `this.processPendingNegotiation()`, so this just makes it actually fire on send failures.

#### 2.2 — `fetchIceConfig` STUN fallback warning (#8)

**File:** `client/src/lib/webrtc/browser.ts:~1013`

When falling back to STUN-only, emit a warning event so the UI can inform the user:
```typescript
this.emit("warning", {
  type: "turn_unavailable",
  message: "TURN server unavailable — voice may not work on restrictive networks",
});
```

Check that `VoiceAdapterEvents` in `types.ts` supports a `warning` event. If not, add it.

#### 2.3 — `setOutputDevice` await `setSinkId` (#13)

**File:** `client/src/lib/webrtc/browser.ts:~734`

Await each `setSinkId` call. Wrap individual calls in try/catch so one failed device doesn't stop the rest:
```typescript
for (const element of this.remoteAudioElements.values()) {
  try {
    await (element as AudioElementWithSinkId).setSinkId(deviceId);
  } catch (err) {
    console.warn("[BrowserVoiceAdapter] Failed to set sink on element:", err);
  }
}
```

### Group 3: Protocol & Tauri Compatibility

#### 3.1 — `pc_type` String → enum (#10)

**Files:**
- `shared/vc-common/src/protocol/mod.rs` — Add `PcType` enum, update `VoiceIceCandidate`
- `server/src/ws/mod.rs` — Update server `VoiceIceCandidate` event
- `server/src/voice/sfu.rs` — Match on `PcType` enum instead of string comparison
- `server/src/voice/ws_handler.rs` — Update handler signatures
- `client/src-tauri/src/network/websocket.rs` — Update Tauri event types

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum PcType {
    Publisher,
    Subscriber,
}
```

Browser TypeScript doesn't change — it already sends `"publisher"` / `"subscriber"` as strings, which match the `#[serde(rename_all = "snake_case")]` format.

The server's `handle_ice_candidate` changes from:
```rust
if pc_type == "subscriber" { ... } else { ... }
```
to:
```rust
match pc_type {
    PcType::Publisher => { ... }
    PcType::Subscriber => { ... }
}
```

Invalid `pc_type` values are now rejected at deserialization rather than silently routed to publisher.

#### 3.2 — Tauri ICE candidate logging (#12)

**File:** `client/src-tauri/src/webrtc/mod.rs:~399`

Add `warn!` when `to_json()` fails. Add `warn!` when `try_read()` fails. When `serde_json::to_string` fails, skip sending instead of sending empty string.

#### 3.3 — Tauri compatibility guard (#1, #2)

**File:** `client/src-tauri/src/commands/voice.rs`

For `handle_voice_subscriber_offer`: Log a warning ("Tauri client does not support dual-PC subscriber offers yet") and return `Ok(())`. Do NOT apply the offer to the publisher PC.

For subscriber ICE candidates with `pc_type: "subscriber"`: Same — log and discard.

**File:** `client/src/stores/websocket.ts:~1734`

In the subscriber offer handler, check if the adapter returned an answer. If the result is empty/undefined (Tauri path), skip sending the `voice_subscriber_answer` message. This prevents the double-send even after Tauri dual-PC is implemented.

## What Does Not Change

- Browser dual-PC architecture — working and tested
- VP8-only codec constraint — already fixed
- RTP forwarding logic — untouched
- Signaling protocol messages — unchanged (just pc_type typing)
- Tauri native screen share / webcam pipeline — unchanged

## Testing

1. `SQLX_OFFLINE=true cargo clippy -- -D warnings` — full workspace
2. `cargo test -p vc-server` (voice tests that don't need DB)
3. `bun run test:run` — frontend tests
4. Manual: browser-to-browser screen share still works on VPS
5. Manual: Tauri client can join voice (audio only) without crashing

## Future Work

- Full Tauri dual-PC implementation (separate PR)
- Server-side subscriber renegotiation lock (#4 from code review — acknowledged but deferred as it requires architectural thought about the concurrency model)
