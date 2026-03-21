# Screen Share WebRTC Rewrite Plan

**Date:** 2026-03-21
**Status:** Planned
**Priority:** High — voice audio works, screen share is the remaining broken feature

## What We Learned (2026-03-21 debugging session)

### What Works
- Voice audio between clients: fully functional (addTrack at join, single audio transceiver, RTP forwarded by SFU)
- Screen share capture: getDisplayMedia works, tracks are valid
- Server SFU: receives tracks, forwards RTP packets, subscriber creation works
- Screen share signaling: WS broadcasts arrive, stream IDs match

### What Doesn't Work (and why)

| Approach | Result | Root Cause |
|----------|--------|------------|
| `addTrack` before server offer | Works ONCE on fresh connection, fails on subsequent shares | `setRemoteDescription(offer)` rolls back unmatched local transceivers |
| `addTrack` between setRemoteDescription and createAnswer | Sends 1 RTP packet then stops | Browser doesn't fully activate RTP for tracks added mid-negotiation |
| `replaceTrack` on pre-allocated transceiver | Sends 1 RTP packet then stops | Direction change (recvonly→sendrecv) doesn't activate sending even with renegotiation |
| Multiple video transceivers (dummy tracks) | "Duplicate a=msid lines detected" on subscriber | Server's add_outgoing_track creates transceivers that conflict with dummy recv transceivers |

### Core Architecture Problem

The SFU uses **server-driven offers** (server is always the offerer). This means:
1. Client can't add tracks independently — addTrack creates orphaned transceivers
2. replaceTrack on recvonly transceivers doesn't activate sending
3. Adding/removing video transceivers dynamically causes SDP conflicts

Audio works because it's a single transceiver created during the initial join and never changed.

## Proposed Solution: Client-Initiated Offers for Media Changes

### Approach

Instead of the server creating offers when media changes, let the **client create offers** when it adds/removes tracks. The server becomes the answerer for these client-initiated offers.

### Flow

```
Current (broken):
1. Client: addTrack(video)
2. Client: tell server via WS
3. Server: add recv transceiver → create offer → send to client
4. Client: setRemoteDescription(offer) ← ROLLS BACK client's transceiver!
5. Client: createAnswer → send to server
6. Result: no video flows

Proposed (client-initiated):
1. Client: addTrack(video)
2. Client: createOffer() → send offer to server via WS
3. Server: setRemoteDescription(offer) ← sees the new video track
4. Server: createAnswer() → send answer to client via WS
5. Client: setRemoteDescription(answer)
6. Result: video flows immediately (no rollback)
```

### Implementation Steps

#### Step 1: Add client-offer WebSocket message type

**Server** (`server/src/ws/mod.rs`):
- Add `voice_offer` inbound message type (client sends offer to server)
- Add handler that calls `peer.peer_connection.set_remote_description(offer)` then `create_answer()` then sends answer back

**Client** (`client/src/stores/websocket.ts`):
- Add `wsSendVoiceOffer(channelId, sdp)` function

#### Step 2: Client creates offer after addTrack

**Client** (`client/src/lib/webrtc/browser.ts`):
- `startScreenShare`: call `addTrack(videoTrack)` then `createOffer()` then `setLocalDescription(offer)` then send offer to server via WS
- Add `handleAnswer(sdp)` method for processing server's answer
- Remove ALL pending screen share infrastructure

#### Step 3: Server handles client offer

**Server** (`server/src/voice/ws_handler.rs`):
- `handle_voice_offer`: set remote description, create answer, send answer
- Don't add recv transceivers for screen shares (the client's offer includes them)
- Keep pending source queue for track identification

#### Step 4: Remove dummy track workaround

**Server** (`server/src/voice/peer.rs`):
- Revert `add_recv_transceiver` to use `add_transceiver_from_kind` (simpler)
- Or remove it entirely if only the initial join needs recv transceivers

#### Step 5: Fix subscriber forwarding

**Server** (`server/src/voice/sfu.rs`):
- When adding outgoing tracks to subscribers, ensure unique msid
- Test with 2 users, both sharing screens simultaneously

### What This Preserves
- Server-initiated offers for JOIN (initial connection setup)
- Audio mic track flow (unchanged)
- Screen share signaling (WS broadcasts, stream IDs)
- RTP forwarding pipeline (track_router, spawn_rtp_forwarder)

### What Changes
- Screen share uses CLIENT-initiated offers instead of server-initiated
- Bidirectional offer/answer (server can initiate OR client can initiate)
- Remove dummy track msid workaround
- Remove all pending screen share deferred logic

### Risk Assessment
- **Low risk**: audio continues working (unchanged flow)
- **Medium risk**: client-initiated offers need careful handling of offer collision (both sides create offer simultaneously)
- **Mitigation**: use "perfect negotiation" pattern — one side is always "polite" (rolls back its offer if collision detected)

### Effort Estimate
- WebSocket message types: 1 hour
- Client offer creation: 2 hours
- Server offer handling: 2 hours
- Testing + debugging: 2 hours
- Total: ~7 hours (1 focused session)

## Architectural Review Findings (Elrond)

### Critical: Test `sendrecv` pre-allocated transceivers FIRST

The `replaceTrack` 1-packet problem was likely caused by the transceiver being `recvonly`, NOT by `replaceTrack` itself. If the server creates the video transceiver as `sendrecv` (not `recvonly`) at join time, `replaceTrack` should activate RTP immediately — no direction change, no renegotiation needed.

**Recommended sequence:**
1. **Spike 1 (1h):** Server creates video transceiver as `sendrecv` with dummy track. Client uses `replaceTrack(videoTrack)` when sharing. If RTP flows → DONE. No client-initiated offers needed.
2. **Spike 2 (1h):** If spike 1 fails, test webrtc-rs `set_remote_description(offer)` on an established connection to validate client-initiated offers work.
3. **Full implementation:** Only if both spikes fail, proceed with client-initiated offers + glare handling.

### High Risk: Offer collision (glare) unspecified

Client-initiated offers require "perfect negotiation" to handle simultaneous offers (e.g., client starts screen share while server adds a new subscriber). Needs:
- Signaling state check before creating offers on both sides
- Rollback mechanism for the "polite" peer
- Clear polite/impolite role assignment (client = impolite, server = polite)
- Negotiation queue in `SfuServer::renegotiate` that checks `signaling_state()`

### Medium Risk: webrtc-rs state machine

webrtc-rs 0.11 may not handle the offerer→stable→have-remote-offer transition correctly. Must validate with a spike test before building the full implementation.

### Medium: `onnegotiationneeded` suppression

Currently suppressed in browser.ts. For client-initiated offers, this event should trigger offer creation — needs a negotiation queue instead of blanket suppression.

### Effort: 14-20 hours (realistic) not 7 hours

Additional time for: browser compatibility testing, glare edge cases, multi-user scenarios, webrtc-rs debugging.

## Spike Results

### Spike 1: sendrecv + replaceTrack — FAILED (2026-03-21)

Changed server video transceiver from `Recvonly` to `Sendrecv`. Client uses `replaceTrack(videoTrack)` on the pre-allocated transceiver. No renegotiation.

**Result:** Zero video RTP packets reached the server. `replaceTrack` does not activate video sending on a transceiver negotiated with a dummy track, regardless of direction. The dummy track's codec negotiation is likely incompatible with the real video track's RTP stream.

**Conclusion:** `replaceTrack` cannot be used with dummy-track transceivers. The transceiver must be negotiated with the actual video track (or null track with matching codec) for RTP to flow.

### Spike 2: Client-initiated offers — NOT YET TESTED

This is the remaining path. Must validate:
1. webrtc-rs supports `set_remote_description(client_offer)` on a connection where the server was the original offerer
2. Glare handling when both sides create offers simultaneously

## Next Steps

1. **Spike 2:** Test webrtc-rs client-initiated offer support in isolation
2. If it works, implement bidirectional offer/answer with glare handling
3. Estimated effort: 14-20 hours (dedicated session)
