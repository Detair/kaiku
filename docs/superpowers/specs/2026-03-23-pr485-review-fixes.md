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

**Caveat:** Verify that the client-side `websocket.ts` handler for `voice_error` events does not tear down the entire voice connection. If it does, a transient SDP error during renegotiation would disconnect the user. If the client does disconnect on `voice_error`, we should instead log + return `Ok(())` with a more descriptive warning (current behavior but with better logging).

#### 1.2 — `handle_ice_candidate` log but keep swallowing (#4)

**File:** `server/src/voice/ws_handler.rs:~597`

ICE candidate failures are expected during trickle ICE (candidates arriving before remote description is set). Unlike 1.1, propagating these errors would be incorrect — they should not be session-fatal. Keep returning `Ok(())` but ensure the existing `error!` log includes `channel_id` for debuggability.

#### 1.3 — `renegotiate` propagate signal send failure (#6)

**File:** `server/src/voice/sfu.rs:~885`

When `signal_tx.send(...)` fails, return `Err(VoiceError::Signaling("failed to send subscriber offer".into()))` instead of logging and returning `Ok(())`. Callers can then clean up the stale peer.

#### 1.4 — `max_screen_shares` log DB errors (#7)

**File:** `server/src/voice/ws_handler.rs:~758`

Add a `warn!` log before falling back. Use `inspect_err` (stable since Rust 1.76):
```rust
let max_shares = sqlx::query_scalar!(...)
    .fetch_optional(&state.db)
    .await
    .inspect_err(|e| {
        warn!(channel_id = %channel_id, error = %e, "Failed to query max_screen_shares, using default");
    })
    .ok()
    .flatten()
    .unwrap_or(6);
```

Keep the fallback behavior — this is a soft error. But operators get visibility.

#### 1.5 — `Peer::close()` honest return type (#9)

**File:** `server/src/voice/peer.rs:~204`

Change return type from `Result<(), VoiceError>` to `()`. The method already logs errors internally and always returns `Ok(())`. Remove the `Ok(())` return.

Callers that check `if let Err(e) = peer.close()` are dead code — update both call sites:
- `sfu.rs` `add_peer()` ~line 107: `if let Err(e) = old_peer.close().await` → `old_peer.close().await`
- `ws_handler.rs` ~line 375: same pattern → `peer.close().await`

#### 1.6 — `handle_webcam_start` use `map_permission_error` (#11)

**File:** `server/src/voice/ws_handler.rs:~957`

Replace `|_e| VoiceError::Unauthorized` with `map_permission_error(e, "webcam start")` for consistency with `handle_screen_share_start` and `handle_join`.

### Group 2: Browser Client Fixes

#### 2.1 — `onnegotiationneeded` await wsSend (#5)

**File:** `client/src/lib/webrtc/browser.ts:~1167`

Add `await` before `wsSend(...)` call inside the `onnegotiationneeded` handler. The existing `catch` block resets `this.isNegotiating = false`. Without `await`, a rejected `wsSend` promise is unhandled and the lock is never released. With `await`, the catch block fires and releases the lock.

Note: the catch block only resets the lock — it does not drain `pendingNegotiation`. Pending negotiations are drained when `handlePublisherAnswer` (triggered by the server's answer) sets `isNegotiating = false` and dispatches a new `negotiationneeded` event. On wsSend failure, no answer will arrive, so pending negotiations remain queued until the next successful negotiation cycle. This is acceptable — the alternative (retrying immediately) could cause a rapid-fire loop.

#### 2.2 — `fetchIceConfig` STUN fallback warning (#8)

**File:** `client/src/lib/webrtc/browser.ts:~1013`

When falling back to STUN-only, call the warning handler so the UI can inform the user. `BrowserVoiceAdapter` uses a `this.eventHandlers` pattern (not EventEmitter), so the call is:

```typescript
this.eventHandlers.onWarning?.({
  type: "turn_unavailable",
  message: "TURN server unavailable — voice may not work on restrictive networks",
});
```

**Required type change:** Add `onWarning` to `VoiceAdapterEvents` in `types.ts`:
```typescript
onWarning?: (warning: { type: string; message: string }) => void;
```

#### 2.3 — `setOutputDevice` await `setSinkId` (#13)

**File:** `client/src/lib/webrtc/browser.ts:~734`

The current code uses `forEach` which cannot `await`. Change to a `for...of` loop over the DOM-queried audio elements (the adapter uses `document.querySelectorAll` for audio elements, not a `remoteAudioElements` map). Wrap individual `setSinkId` calls in try/catch so one failed device doesn't stop the rest:

```typescript
for (const element of audioElements) {
  try {
    await (element as AudioElementWithSinkId).setSinkId(deviceId);
  } catch (err) {
    console.warn("[BrowserVoiceAdapter] Failed to set sink on element:", err);
  }
}
```

Check the actual DOM query pattern in `setOutputDevice` and adapt accordingly.

### Group 3: Protocol & Tauri Compatibility

#### 3.1 — `pc_type` String → enum (#10)

**Files:**
- `shared/vc-common/src/protocol/mod.rs` — Add `PcType` enum, update `VoiceIceCandidate` in both `ClientEvent` and `ServerEvent`
- `server/src/ws/mod.rs` — Update server `VoiceIceCandidate` event
- `server/src/voice/sfu.rs` — Match on `PcType` enum in `handle_ice_candidate` (receive path) AND update `register_ice_callback` to construct `PcType::Publisher`/`PcType::Subscriber` instead of `pc_label.to_string()` (send path)
- `server/src/voice/ws_handler.rs` — Update handler signatures
- `client/src-tauri/src/network/websocket.rs` — Update Tauri event types
- `client/src-tauri/src/commands/voice.rs` — Update ICE candidate construction from `pc_type: "publisher".to_string()` to `pc_type: PcType::Publisher`

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

**Note on websocket.ts double-send (#1):** The existing `result.ok === false` check in `websocket.ts` `handleVoiceSubscriberOffer` already skips sending the answer when the adapter returns an error. With the Tauri guard returning `Ok(())` (no answer SDP), the Tauri adapter's `handleSubscriberOffer` will return `{ ok: false, error: ... }` at the TypeScript level. No additional `websocket.ts` change is needed — the existing guard handles it.

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
