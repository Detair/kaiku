<!-- Parent: ../../AGENTS.md -->

# Voice Module

**PERFORMANCE CRITICAL** — WebRTC Selective Forwarding Unit (SFU) for low-latency voice channels (<50ms target).

## Purpose

- SFU server for managing voice rooms and peer connections
- WebRTC signaling (SDP offer/answer, ICE candidates) via WebSocket
- RTP track routing (forward audio from sender to all receivers)
- DM voice calls (1:1 or group calls in DM channels)
- ICE server configuration (STUN/TURN)
- Future: E2EE via MLS ("Paranoid Mode")

## Key Files

- `mod.rs` — Router setup for ICE servers endpoint
- `sfu.rs` — SFU server implementation (Room, ParticipantInfo, track management)
- `peer.rs` — WebRTC PeerConnection wrapper (offer/answer, ICE candidate handling)
- `track.rs` — RTP track routing logic (forward audio between peers)
- `ws_handler.rs` — WebSocket event handlers for voice signaling (VoiceJoin, VoiceAnswer, VoiceIceCandidate)
- `leave.rs` — `leave_voice_internal` (single leave path: explicit leave, socket close, stale-peer reaper) and `leave_voice_on_socket_close`
- `call.rs` — DM call state management (call initiation, ringing, acceptance)
- `call_handlers.rs` — HTTP endpoints for DM calls (start, accept, end)
- `call_service.rs` — Call lifecycle logic (ring timeout, participant tracking)
- `signaling.rs` — SDP munging and negotiation helpers
- `handlers.rs` — ICE server configuration endpoint
- `error.rs` — VoiceError type
- `rate_limit.rs` — Voice-specific rate limiting (future)

## For AI Agents

**PERFORMANCE CRITICAL MODULE**: Voice quality directly impacts user experience. Every millisecond counts. Latency budget: 10ms target, 20ms acceptable, 50ms maximum. Profile all hot paths, avoid allocations in RTP forwarding loop.

### Architecture Overview

**SFU (Selective Forwarding Unit)**:
- Server does NOT mix audio (unlike MCU)
- Each participant sends ONE stream to server
- Server forwards each stream to all OTHER participants
- Clients handle mixing (lower server CPU, better quality)

**Advantages Over P2P**:
- Works behind NAT/firewall (server as relay)
- Scales to large rooms (N participants = N server connections, not N*(N-1) P2P)
- Server can apply policies (mute enforcement, recording, transcription)

**Disadvantages vs MCU**:
- Higher client bandwidth (receives N-1 streams)
- More client CPU (decode and mix N-1 streams)
- Trade-off: For gaming communities, client resources available, server CPU savings critical

### WebRTC Stack

**Crate**: `webrtc-rs` (pure Rust WebRTC implementation)

**Components**:
- `RTCPeerConnection` — WebRTC connection per participant
- `RTCRtpSender`/`RTCRtpReceiver` — Send/receive RTP tracks
- `RTCIceTransport` — ICE connectivity establishment
- `RTCDtlsTransport` — DTLS-SRTP encryption (server-trusted for MVP)

**Security**: DTLS-SRTP provides hop-to-hop encryption (client ↔ server). Server can decrypt audio. For true E2EE, need MLS (future "Paranoid Mode").

### Voice Signaling Flow

**Guild Channel Voice**:
1. Client sends `VoiceJoin { channel_id }` via WebSocket
2. Server creates `Peer` in SFU room for `channel_id`
3. Server generates SDP offer (via `peer.create_offer()`)
4. Server sends `VoiceOffer { sdp }` to client
5. Client sets remote description, generates SDP answer
6. Client sends `VoiceAnswer { sdp }` to server
7. Server sets remote description on peer
8. Both sides exchange `VoiceIceCandidate` events as ICE gathering progresses
9. Connection established (ICE connected, DTLS handshake complete)
10. Client begins sending RTP audio, server forwards to other participants

**WebSocket Events** (in `ws::ClientEvent` and `ws::ServerEvent`):
```rust
// Client → Server
VoiceJoin { channel_id }
VoiceAnswer { channel_id, sdp }
VoiceIceCandidate { channel_id, candidate }
VoiceLeave { channel_id }
VoiceMute { channel_id }
VoiceUnmute { channel_id }

// Server → Client
VoiceOffer { channel_id, sdp }
VoiceIceCandidate { channel_id, candidate }
VoiceUserJoined { channel_id, user_id, username, display_name }
VoiceUserLeft { channel_id, user_id }
VoiceUserMuted { channel_id, user_id }
VoiceUserUnmuted { channel_id, user_id }
VoiceRoomState { channel_id, participants: Vec<VoiceParticipant> }
VoiceError { code, message }
```

### SFU Implementation

**Room** (in `sfu.rs`):
```rust
pub struct Room {
    channel_id: Uuid,
    participants: HashMap<Uuid, Participant>,  // user_id → Participant
}

pub struct Participant {
    user_id: Uuid,
    username: String,
    display_name: String,
    peer_connection: Arc<RTCPeerConnection>,
    muted: bool,
    joined_at: DateTime<Utc>,
}
```

**SfuServer** (singleton, shared via `Arc<SfuServer>` in `AppState`):
```rust
pub struct SfuServer {
    rooms: RwLock<HashMap<Uuid, Room>>,  // channel_id → Room
    webrtc_config: RTCConfiguration,      // ICE servers, DTLS config
}
```

**Key Methods**:
```rust
pub async fn join_room(&self, channel_id: Uuid, user_id: Uuid, username: String) -> Result<String, VoiceError> {
    // 1. Create or get Room for channel_id
    // 2. Create RTCPeerConnection for user
    // 3. Set up track event handlers (ontrack → forward to other participants)
    // 4. Generate SDP offer
    // 5. Add Participant to room
    // 6. Return SDP offer string
}

pub async fn handle_answer(&self, channel_id: Uuid, user_id: Uuid, sdp: String) -> Result<(), VoiceError> {
    // 1. Find participant in room
    // 2. Set remote description (SDP answer) on peer connection
}

pub async fn add_ice_candidate(&self, channel_id: Uuid, user_id: Uuid, candidate: String) -> Result<(), VoiceError> {
    // 1. Parse ICE candidate
    // 2. Add to peer connection's ICE agent
}

pub async fn leave_room(&self, channel_id: Uuid, user_id: Uuid) -> Result<(), VoiceError> {
    // 1. Remove participant from room
    // 2. Close peer connection
    // 3. Notify other participants (VoiceUserLeft event)
}
```

### Track Forwarding

**RTP Track Routing** (in `track.rs`):
```rust
// When participant's peer connection receives track (ontrack event)
peer_connection.on_track(Box::new(move |track, receiver, transceiver| {
    Box::pin(async move {
        // For each OTHER participant in room:
        for other_peer in room.participants.values() {
            if other_peer.user_id != sender_user_id {
                // Add track to their peer connection
                other_peer.peer_connection.add_track(track.clone()).await?;
            }
        }
    })
}));
```

**Performance Optimization**:
- Use `Arc<RTCRtpReceiver>` to avoid cloning heavy objects
- Forward RTP packets directly (no decoding/re-encoding)
- Avoid locking room for entire track duration (lock only for participant list lookup)

### ICE Configuration

**Endpoint**: `GET /api/voice/ice-servers`

**Response**:
```json
{
    "ice_servers": [
        { "urls": ["stun:stun.l.google.com:19302"] },
        {
            "urls": ["turn:turn.example.com:3478"],
            "username": "user",
            "credential": "pass"
        }
    ]
}
```

**Configuration** (in config):
```rust
pub struct IceServerConfig {
    pub urls: Vec<String>,         // STUN/TURN URLs
    pub username: Option<String>,  // TURN auth
    pub credential: Option<String>,
}
```

**STUN vs TURN**:
- **STUN**: Discover public IP for NAT traversal (free, public servers available)
- **TURN**: Relay if direct/STUN connection fails (requires server, costs bandwidth)
- **Fallback**: Try STUN first, use TURN if needed (webrtc-rs handles automatically)

**Deployment**: Self-host TURN with `coturn` (open-source TURN server) for production.

### DM Voice Calls

**Call Model**:
```rust
pub struct Call {
    dm_channel_id: Uuid,
    initiator_id: Uuid,
    participants: HashSet<Uuid>,
    state: CallState,  // Ringing, Active, Ended
    started_at: DateTime<Utc>,
    ended_at: Option<DateTime<Utc>>,
}

pub enum CallState {
    Ringing,  // Waiting for others to join
    Active,   // At least 2 participants connected
    Ended,    // Call finished
}
```

**Call Flow**:
1. `POST /api/dm/:id/call/start` — Initiator starts call (refused with 403 `blocked` if a DB or cached block exists either way, same rule as DM send)
2. Server sends `IncomingCall { channel_id, initiator, initiator_name }` with `broadcast_to_users` to every DM participant except the caller (user channels, so a callee without the DM open still rings). All call events (`CallParticipantJoined/Left`, `CallDeclined`, `CallEnded`) go to the members' user channels, not the DM channel.
3. Recipients see ringing notification in client
4. Recipient accepts: `POST .../call/join`, then `VoiceJoin { channel_id }` (same as guild voice)
5. Initiator and recipient both join SFU room (DM channel acts as voice room)

**Call end** (`call_service.rs`): a call ends — its stream is deleted and `CallEnded` is sent once — on the last decline (`all_declined`), ring timeout (`no_answer`, `Config::call_ring_timeout_secs`, 90 s), initiator cancel or last REST leave, when the last SFU peer leaves the DM room (hook in `leave::leave_voice_internal`: `last_left`, or `cancelled` while ringing), and when the caller's last socket closes while ringing (`cancelled`; open sockets counted with `PUBSUB NUMSUB user:{id}`, re-checked for up to 4 s in a detached task). `end_call(state, channel_id, reason)` ends whatever call is current.

**Concurrency**: the stream's first entry holds a random `call_id` (generation). Every change is a Lua compare-and-set on `call_id` + last entry id; on a mismatch the caller re-reads and decides again. Start is "create if absent". So ending is idempotent, exactly one ender broadcasts, a stale ring timer (`ring_timeout_fired(channel, call_id)`) or late hook cannot touch a newer call, and a new call right after an end succeeds. The ringing stream also has a TTL (timeout + 30 s) as a backstop if the ring timer is lost.

**Endpoints**: `GET /api/dm/{id}/call`, `POST /api/dm/{id}/call/{start,join,decline,leave}`.

### Latency Optimization

**Target**: <50ms end-to-end latency (audio capture → network → decode → speaker)

**Breakdown**:
- Audio capture: ~10ms (OS/driver)
- Encoding (Opus): ~2ms
- Network (one-way): 10-20ms (depends on geography)
- Server forwarding: **<5ms target** (critical path)
- Decoding: ~2ms
- Playback buffer: ~10ms

**Server-Side Targets**:
- **SDP negotiation**: <100ms (not in hot path)
- **ICE candidate handling**: <10ms
- **RTP packet forwarding**: <1ms (hot path, measure with tracing)

**Profiling Tools**:
```bash
# Use tokio-console for async task monitoring
cargo install tokio-console
RUSTFLAGS="--cfg tokio_unstable" cargo run --features tokio-console

# Use perf for CPU profiling
perf record -F 99 -g ./target/release/server
perf report
```

**Metrics to Track**:
- p50/p95/p99 latency for `join_room`, `handle_answer`, `add_ice_candidate`
- RTP packet loss rate (should be <1%)
- Reconnection rate (ICE failures)
- Voice channel participant count distribution

### Codec Configuration

**Audio Codec**: Opus (mandatory for WebRTC)
- Bitrate: 32-128 kbps (configurable per room, future)
- Sample rate: 48 kHz
- Frame size: 20ms (balance latency vs efficiency)
- DTX: Discontinuous Transmission (silence suppression, saves bandwidth)

**SDP Offer Parameters** (in `signaling.rs`):
```
m=audio 9 UDP/TLS/RTP/SAVPF 111
a=rtpmap:111 opus/48000/2
a=fmtp:111 minptime=10;useinbandfec=1
```

**Future Tuning**:
- Adaptive bitrate based on network conditions
- Forward Error Correction (FEC) for packet loss resilience
- JitterBuffer tuning (trade latency for smoothness)

### Muting

**Server-Side Mute**:
- `VoiceMute { channel_id }` — Server stops forwarding user's tracks to others
- User still hears others (one-way mute)
- Broadcasts `VoiceUserMuted` to all participants

**Client-Side Mute** (future):
- Client stops capturing audio (no RTP sent)
- Lower bandwidth, same effect
- Server cannot enforce (user can modify client)

**Server-Enforced Mute** (future, for moderation):
- Permission check: `MUTE_MEMBERS` permission
- Server closes user's audio track, rejects new tracks
- Used for disruptive users

### E2EE (Future: MLS "Paranoid Mode")

**Current State**: DTLS-SRTP encrypts client ↔ server. Server can decrypt audio.

**Planned** (MLS — Messaging Layer Security):
- Client-side encryption before RTP packetization
- Server forwards opaque encrypted RTP packets
- Only participants with group key can decrypt
- Key rotation on participant join/leave

**Trade-offs**:
- Higher client CPU (encryption overhead)
- Cannot do server-side features (recording, transcription, noise suppression)
- Latency increase: ~2-5ms for encryption/decryption

**Implementation Path**:
1. Integrate `openmls` crate (Rust MLS library)
2. Add key distribution via WebSocket (before voice join)
3. Modify RTP sender to encrypt payload (Opus → Encrypt → RTP)
4. Server forwards without decryption
5. Client decrypts RTP payload before decoding Opus

### Testing

**Required Tests**:
- [ ] Single user joins voice channel (SDP offer generated)
- [ ] Two users join, both receive each other's tracks
- [ ] User leaves, others notified (VoiceUserLeft)
- [ ] Socket close / failed connection removes the peer (no ghost participants)
- [ ] ICE candidates exchanged successfully
- [ ] Mute/unmute broadcasts events
- [ ] DM call starts, participants notified
- [ ] Call auto-ends if no one joins within timeout

**Load Testing**:
- Simulate 100 participants in one room (stress SFU track forwarding)
- Measure latency under load (p99 should stay <50ms)
- Monitor memory usage (should be linear with participant count)

### Peer Removal (no ghost participants)

A peer leaves its room through exactly one function, `leave::leave_voice_internal`, which broadcasts `ScreenShareStopped`/`WebcamStopped`/`VoiceUserLeft`, finalizes the session, calls `forget_peer` and removes an empty room. It never checks channel access: leaving is always allowed. Three paths call it:

1. **Explicit `VoiceLeave`** — `expected_session = None`.
2. **WebSocket close** — `handle_socket` cleanup calls `leave_voice_on_socket_close` before presence goes offline. It removes only the peer whose `signal_tx` is that socket's queue (`SfuServer::find_peer_owned_by`, `same_channel`), so closing a second socket of the same user does not end their call.
3. **Failed connection** — on `RTCPeerConnectionState::Failed` the PC state handler waits `PEER_FAILED_GRACE` (15 s). If the peer still exists and that PC is still `Failed`, it sends a `StalePeer` to the reaper (`spawn_stale_peer_reaper`, started in `main.rs`). `Disconnected` is only logged.

Paths 2 and 3 pass `expected_session = Some(session_id)`: removal is skipped if the user has re-joined since (a newer session), and the session check and removal are atomic (`Room::remove_peer_if_session`). The PC state callback holds only `Weak` references to the peer and the PC; the reaper holds a `Weak<SfuServer>`.

**Signal send bounds**: sends to *other* peers use `renegotiate` / `fan_out` (250 ms, stalled peers are skipped). The peer of the socket handling the current event (e.g. the joiner's first subscriber offer) uses `renegotiate_own`, which gets the full slow-client bound of `send_outbound`.

### Common Pitfalls

**DO NOT**:
- Block async executor in RTP forwarding path (use `spawn` for heavy work)
- Hold locks across `.await` points (deadlock risk)
- Clone `RTCPeerConnection` (use `Arc`)
- Forget to close peer connections on leave (memory leak)
- Use blocking I/O in voice module (breaks latency guarantees)

**DO**:
- Use `Arc` and `RwLock` for shared state (SfuServer, Rooms)
- Profile RTP forwarding path (critical for latency)
- Handle ICE failures gracefully (retry, fallback to TURN)
- Validate channel permissions before voice join
- Log voice events for debugging (join, leave, mute state changes)

### Future Enhancements

**Scalability**:
- Distributed SFU (multiple voice servers, route by channel_id)
- Redis pub/sub for cross-server voice signaling
- Load balancing (assign users to least-loaded SFU instance)

**Features**:
- Screen sharing (additional video track)
- Video calls (add video codec negotiation)
- Noise suppression (Krisp-like, client or server-side)
- Auto-gain control (normalize volume)
- Voice activity detection (VAD for UI indicators)
- Recording and playback (requires server-side decoding)

**Monitoring**:
- Real-time voice quality metrics (MOS score, jitter, packet loss)
- Alerting on high latency or connection failures
- Per-channel usage analytics (active users, duration)
