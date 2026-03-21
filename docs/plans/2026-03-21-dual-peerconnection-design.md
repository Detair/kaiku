# Dual PeerConnection Architecture for Screen Sharing

**Date:** 2026-03-21
**Status:** Approved
**Supersedes:** Screen share sections of `2026-03-21-screen-share-rewrite-plan.md`
**Priority:** High — eliminates the entire class of glare/negotiation problems blocking screen share

## Problem

Kaiku's SFU uses a single PeerConnection per user with server-driven offers. This architecture cannot support screen sharing because:

1. `addTrack` on the client creates orphaned transceivers that get rolled back by the server's next offer
2. `replaceTrack` on pre-allocated transceivers doesn't activate RTP (confirmed by Spike 1)
3. Client-initiated offers on a single PC require glare handling (perfect negotiation) — complex and untested in webrtc-rs

See `2026-03-21-screen-share-rewrite-plan.md` for full debugging history and spike results.

## Solution: Dual PeerConnection (LiveKit Model)

Split each participant into two separate PeerConnections:

```
┌──────────┐                    ┌──────────┐
│  Client  │                    │  Server  │
│          │                    │  (SFU)   │
│ Publisher ├───── mic ────────►│          │
│    PC     ├───── screen ─────►│ receives │
│           ├───── webcam ─────►│ tracks   │
│ (client   │                   │          │
│  offers)  │◄── answer ───────│          │
├──────────┤                    ├──────────┤
│Subscriber │◄── other users ──│ forwards │
│    PC     │◄── screen shares─│ tracks   │
│           │◄── webcams ──────│          │
│ (server   │                   │          │
│  offers)  │─── answer ──────►│          │
└──────────┘                    └──────────┘
```

**Rules:**
- **Publisher PC**: Client is always the offerer. `onnegotiationneeded` → create offer → send to server → server answers.
- **Subscriber PC**: Server is always the offerer. Server adds outgoing track → creates offer → sends to client → client answers.
- **No glare possible**: Each PC has exactly one designated offerer.

### Why This Approach

| Approach | Glare Risk | Complexity | Multi-Screen-Share | Industry Use |
|----------|-----------|-----------|-------------------|-------------|
| Single PC + server offers (current) | N/A — can't add tracks | Low | Broken | — |
| Single PC + perfect negotiation | Medium | High | Works but complex | mediasoup |
| Dual PeerConnection | **None** | Medium | **Native** | **LiveKit** |
| Separate PC per stream (Slack) | None | Low per stream | Connection explosion | Slack (older) |

Dual PC is the proven approach for SFUs that need dynamic track management at scale.

## Signaling Protocol

### New WebSocket Messages

**Replace:**

| Old Message | Direction |
|-------------|-----------|
| `VoiceOffer` | Server → Client |
| `VoiceAnswer` | Client → Server |

**With:**

| New Message | Direction | Purpose |
|-------------|-----------|---------|
| `VoicePublisherOffer` | Client → Server | Client offers after addTrack on publisher PC |
| `VoicePublisherAnswer` | Server → Client | Server answers publisher offer |
| `VoiceSubscriberOffer` | Server → Client | Server offers after adding outgoing track on subscriber PC |
| `VoiceSubscriberAnswer` | Client → Server | Client answers subscriber offer |

**Modified:**

| Message | Change |
|---------|--------|
| `VoiceIceCandidate` | Add `pc_type: "publisher" \| "subscriber"` field |

### Join Flow

```
1. Client sends VoiceJoin
2. Server creates both PCs (publisher + subscriber)
3. Client creates publisherPC, addTrack(mic), createOffer()
4. Client sends VoicePublisherOffer
5. Server: setRemoteDescription(offer) on publisher PC, createAnswer()
6. Server sends VoicePublisherAnswer
7. Client: setRemoteDescription(answer) — mic audio flows
8. Server adds outgoing tracks (existing users) to subscriber PC
9. Server sends VoiceSubscriberOffer
10. Client: setRemoteDescription(offer), createAnswer()
11. Client sends VoiceSubscriberAnswer — remote audio flows
```

### Screen Share Flow

```
1. Client: getDisplayMedia() → addTrack(videoTrack) on publisher PC
2. Browser fires onnegotiationneeded
3. Client: createOffer() → sends VoicePublisherOffer
4. Server: setRemoteDescription(offer) — sees new video track
5. Server: createAnswer() → sends VoicePublisherAnswer
6. Client: setRemoteDescription(answer) — video RTP flows immediately
7. Server on_track fires → TrackRouter creates subscriber tracks
8. Server adds outgoing tracks to each subscriber's subscriber PC
9. Server sends VoiceSubscriberOffer to each subscriber
10. Subscribers answer → screen share video flows to viewers
```

## Server-Side Changes

### Peer Struct

```rust
// Current
pub struct Peer {
    peer_connection: Arc<RTCPeerConnection>,
    incoming_tracks: RwLock<HashMap<TrackSource, Arc<TrackRemote>>>,
    outgoing_tracks: RwLock<HashMap<(Uuid, TrackSource), Arc<TrackLocalStaticRTP>>>,
    pending_track_sources: RwLock<Vec<TrackSource>>,
    // ...
}

// New
pub struct Peer {
    publisher_pc: Arc<RTCPeerConnection>,   // receives tracks FROM client
    subscriber_pc: Arc<RTCPeerConnection>,  // sends tracks TO client
    incoming_tracks: RwLock<HashMap<TrackSource, Arc<TrackRemote>>>,   // on publisher_pc
    outgoing_tracks: RwLock<HashMap<(Uuid, TrackSource), Arc<TrackLocalStaticRTP>>>,  // on subscriber_pc
    pending_track_sources: RwLock<Vec<TrackSource>>,  // still needed for on_track identification
    // ...
}
```

### Method Changes

| Method | Current | New |
|--------|---------|-----|
| `add_recv_transceiver()` | Adds to single PC | **Removed** — publisher PC transceivers come from client's offer |
| `add_outgoing_track()` | Adds to single PC | Adds to `subscriber_pc` only |
| `remove_outgoing_track()` | Removes from single PC | Removes from `subscriber_pc` only |
| `create_peer()` | Creates 1 PC + recv transceivers | Creates 2 PCs, no pre-allocated transceivers |
| `setup_track_handler()` | on_track on single PC | on_track on `publisher_pc` only |
| `setup_ice_handler()` | on_ice on single PC | on_ice on **both** PCs (with pc_type routing) |
| `create_offer()` | Server creates offer | **Removed** for publisher. Kept as `create_subscriber_offer()` |
| `renegotiate()` | Creates offer on single PC | Only creates offer on `subscriber_pc` |
| `handle_answer()` | Processes answer for single PC | Split: `handle_publisher_offer()` + `handle_subscriber_answer()` |

### Unchanged Components

- `TrackRouter` — routes RTP by (user_id, TrackSource), no changes
- `spawn_rtp_forwarder()` — reads from publisher's remote track, unchanged
- `spawn_subscriber_remb_reader()` — reads RTCP from subscriber's sender, unchanged
- `ScreenShareInfo`, `WebcamInfo`, `ScreenShareLimiter` — metadata, unchanged
- Simulcast layer handling — unchanged

## Client-Side Changes

### BrowserVoiceAdapter

```typescript
// Current
peerConnection: RTCPeerConnection | null

// New
publisherPC: RTCPeerConnection | null   // sends our tracks
subscriberPC: RTCPeerConnection | null  // receives others' tracks
```

### join()

1. Send VoiceJoin
2. Create publisherPC, addTrack(mic)
3. `onnegotiationneeded` fires → createOffer() → send VoicePublisherOffer
4. Receive VoicePublisherAnswer → setRemoteDescription
5. Create subscriberPC (empty, waits for server)
6. Receive VoiceSubscriberOffer → setRemoteDescription → createAnswer → send VoiceSubscriberAnswer

### startScreenShare()

1. `getDisplayMedia()`
2. `addTrack(videoTrack)` on publisherPC
3. `addTrack(audioTrack)` on publisherPC (if withAudio)
4. `onnegotiationneeded` fires automatically
5. createOffer() → send VoicePublisherOffer
6. Receive VoicePublisherAnswer → done, video flows

### stopScreenShare()

1. `removeTrack(videoSender)` on publisherPC
2. `removeTrack(audioSender)` on publisherPC (if audio)
3. Stop tracks
4. `onnegotiationneeded` fires → renegotiate
5. Send VoiceScreenShareStop (metadata cleanup)

### onnegotiationneeded on publisherPC

```typescript
// Currently suppressed. New: actually use it.
publisherPC.onnegotiationneeded = async () => {
  const offer = await publisherPC.createOffer();
  await publisherPC.setLocalDescription(offer);
  wsSendPublisherOffer(channelId, offer.sdp);
};
```

### Removed

- `handleOffer()` — replaced by `handleSubscriberOffer()`
- `pendingScreenShares` map
- Video transceiver search logic
- `replaceTrack` workaround
- All deferred screen share infrastructure

## Migration Plan

Audio keeps working at every step.

| Step | What | Risk | Validation |
|------|------|------|------------|
| 1 | Add new WS message types (publisher/subscriber offer/answer, pc_type on ICE) | None | Compiles, existing flow unchanged |
| 2 | Server: create two PCs in `create_peer()`, wire up on_track on publisher only, on_ice on both | Low | Unit test: both PCs created |
| 3 | Client: create two PCs in `join()`, publisherPC with mic, onnegotiationneeded wired | Medium | Audio works via publisher offer flow |
| 4 | Server: `handle_publisher_offer()` + subscriber offer flow for existing tracks | Medium | Two users can hear each other |
| 5 | Remove old single-PC code path, `handleOffer`/`VoiceOffer`/`VoiceAnswer` | Low | Clean up after step 4 works |
| 6 | Client: `startScreenShare()` uses addTrack on publisherPC | Low | Screen share video flows |
| 7 | Multi-screen-share: multiple addTrack calls, server fans out to subscribers | Low | Two users sharing simultaneously |

## Risk Assessment

**Medium: webrtc-rs as answerer on publisher PC.** The server has always been the offerer. Now it must call `set_remote_description(client_offer)` then `create_answer()` on the publisher PC. Steps 3-4 validate this immediately with just audio before touching screen share code.

**Low: ICE for two connections.** Two STUN/TURN negotiations per user. Doubles ICE traffic briefly during join, but steady-state media bandwidth is identical. Gaming community sizes make this negligible.

**Low: Backward compatibility.** Screen share is currently broken, nothing to break. Audio migration in steps 3-4 is the only risk to existing functionality.

## What This Eliminates

- Perfect negotiation / glare handling — eliminated by design
- replaceTrack hacks — eliminated
- Dummy tracks / pre-allocated transceivers — eliminated
- Pending screen share deferred logic — eliminated
- `onnegotiationneeded` suppression — eliminated (now used properly)

## Research Sources

- [Discord: 2.5M concurrent voice users with WebRTC](https://discord.com/blog/how-discord-handles-two-and-half-million-concurrent-voice-users-using-webrtc) — custom signaling, no SDP
- [Discord Voice Connections Protocol](https://docs.discord.food/topics/voice-connections) — SSRC-based track management, simulcast
- [Slack Does WebRTC Video](https://webrtchacks.com/slack-video/) — Janus SFU, simulcast, TURN-only
- [Is Slack's WebRTC Really Slacking?](https://webrtchacks.com/slack-webrtc-slacking/) — multiple PeerConnections per participant
- [LiveKit SFU Internals](https://docs.livekit.io/reference/internals/livekit-sfu/) — Go/Pion, dual PC model
- [LiveKit Client Connection Flow](https://deepwiki.com/livekit/livekit/4.1-client-connection-flow) — publisher/subscriber PC split
- [LiveKit Screen Sharing](https://docs.livekit.io/transport/media/screenshare/) — screen as standard publisher track
- [MDN: Perfect Negotiation Pattern](https://developer.mozilla.org/en-US/docs/Web/API/WebRTC_API/Perfect_negotiation) — reference for single-PC alternative
- [mediasoup v3](https://mediasoup.org/documentation/v3/) — ORTC-like Producer/Consumer model
