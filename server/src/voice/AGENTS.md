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
1. `POST /api/dm/:id/call/start`: the initiator starts a call. It is refused with 403 `BLOCKED` if a DB or cached block exists in either direction (same rule as DM send).
2. Atomically with creating the call, the server publishes `incoming_call` to each DM participant except the caller, and `call_started` to all of the initiator's sessions. Both go to user channels, so a callee without the DM open still rings.
3. Recipient accepts: `POST .../call/join` (only a rung target may answer a ringing call; the initiator gets 409), then `VoiceJoin { channel_id }`.
4. Initiator and recipient both join the SFU room (the DM channel acts as the voice room).

**Wire**:
- Every call event carries `call_id`: `incoming_call`, `call_started`, `call_participant_joined`, `call_participant_left`, `call_declined` and `call_ended`.
- Every REST call response carries a top-level `call_id`.
- Clients ignore events whose `call_id` is not the call they show.
- Events go to the DM's **current** participants' user channels (never the DM channel). The `user:` branch of the WS forwarder drops call events about blocked users.

**Call end** (`call_service.rs`): a call ends, is deleted and publishes `call_ended` once in these cases:
- last decline (`all_declined`)
- ring timeout (`no_answer`; `Config::call_ring_timeout_secs`, 90 s; a tokio timer bound to the `call_id`, started inside `CallService::start_call`)
- initiator cancel or last REST leave
- last SFU peer leaves the DM room (hook in `leave::leave_voice_internal`): only if that peer connected at or after the call started. An active call ends as `last_left`. A ringing call ends as `cancelled`, and only if the leaving user started it.
- caller's last socket closes while ringing (`cancelled`): socket close checks `EXISTS call_ringing_by:{user}`. If the key exists, a detached task waits 3 s and counts sockets once with `PUBSUB NUMSUB user:{id}`. With none left, it cancels exactly the generations in the index.
- orphan: an active call whose voice room is empty and that has been idle for 30 s is ended (`last_left`) when someone starts a new call. This assumes a single node.

`end_call(state, channel_id, reason)` ends whatever call is current.

**Phone wakes**: a `CallService` built with `for_state` (or `with_push`) sends the push `call` wake (`ringing` on start, `ended` once on every end path above); see `push/AGENTS.md`, "Call wakes". `CallService::new` alone sends none.

**Concurrency**:
- **Generation:** the stream's first entry holds a random `call_id`.
- **Start:** a Lua script, "create if absent". It also SADDs `{channel}:{call_id}` to `call_ringing_by:{initiator}` and publishes `incoming_call` and `call_started`.
- **Every transition:** a Lua compare-and-set on `call_id` plus the last entry id. On success it appends the event (or DELs the stream and SREMs the index entry when the call ends) and publishes the transition's events in the same script. A call's `call_ended` therefore always precedes the next call's `incoming_call`.
- **Pinning:** every operation pins the first `call_id` it observes (or the one it was given). A retry that finds another call stops as a no-op or 404. So ending is idempotent, and a stale timer, hook or cleanup never touches a newer call.
- **TTLs:** backstops only. A ringing call gets timeout + 30 s. An active call gets 6 h, refreshed on join and leave.
- **Leaked subscriber:** if a closing socket's subscriber QUIT never completes, the user still counts as online and the call falls back to the ring timeout.

**Endpoints**: `GET /api/dm/{id}/call`, `POST /api/dm/{id}/call/{start,join,decline,leave}`. Each returns `{channel_id, call_id, status, ...state fields, capabilities}`.

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

**Removal after an access change** (`revoke.rs`, contract C31): `ws::access::access_changed` (kick, leave, ban, role and override changes, channel or server deletion), called right after the change's epoch bump and before any other post-commit work, calls `enforce_voice_access`, which re-checks the affected participants with the voice join check (`VIEW_CHANNEL` + `VOICE_CONNECT`) and removes those who fail through `leave_voice_internal` with their session id, then sends `voice_removed { channel_id, reason }` to the socket that owned the peer (`kicked`, `banned`, `permission_lost`, `channel_deleted`; the others get `voice_user_left`). A deleted channel removes everyone, a ban removes the user from every room, leaving a group DM removes the leaver from that DM's call. A database error keeps the participant in the room; the owning socket removes it itself (`remove_socket_peer`, `permission_lost`) when its revalidation — after any bump of the user's or the room's server epoch (debounced, ~150 ms) and on its backstop (≤ 60 s) — finds voice access gone.

**Room events are fenced at the socket write, independent of removal** (`ws::access`): every room event (`OutboundMsg::voice`: room state, joins, departures, mute, media, SDP/ICE, layer changes) is written only if the receiving socket still owns the peer **and** its authorization for the room is current: the access epochs recorded at the join check (`Peer::set_admission`, read before the permission check) still hold, or the socket revalidates voice access before writing. From the bump of a kick on, a peer that has not been removed yet receives nothing of the room. A peer without an admission (not created by `handle_join`) receives no room events at all. Only `voice_removed` (the socket's own removal) and `voice_error` (answers to its own requests) are written without the fence.

**Join vs. revocation** (`handle_join`): the join reads the access epochs (user, and the channel's server) before its permission check, records them on the peer, and compares them again right after `Room::add_peer`. If they moved, it re-checks access and leaves through `leave_voice_internal` (with its session) when access is gone. A revocation bumps first and looks at the rooms afterwards, so a peer added after its look is caught by the join, one added before is caught by the revocation: no join slips through.

**Shutdown**: the sockets are told first; each closing socket removes its own peer through the normal leave path, and `leave::leave_all` runs concurrently for peers without a live socket (see `ws/AGENTS.md`).

**Signal send bounds**: sends to *other* peers use `renegotiate` / `fan_out` (250 ms, stalled peers are skipped). The peer of the socket handling the current event (e.g. the joiner's first subscriber offer) uses `renegotiate_own`, which gets the full slow-client bound of `send_outbound`.

### Media sources, negotiation and seats (contract C30/C31)

- **Announced or dropped** (`sfu.rs` `setup_track_handler`): a video track takes the oldest accepted announcement (`voice_screen_share_start` → `ScreenVideo(stream_id)`, `voice_webcam_start` → `Webcam`) from `Peer::pending_track_sources`; one that arrives first waits `UNANNOUNCED_TRACK_GRACE` (3 s, `Peer::take_video_source_within`), then is dropped (no forwarder, no subscriber tracks) and the sender gets `voice_error VIDEO_SOURCE_UNANNOUNCED`. There is no unnamed-share fallback any more. Audio: the microphone is the first audio section of the publisher's first offer (`Peer::set_microphone_mid` / `is_microphone_mid`, exactly one, `Peer::claim_microphone`), not whichever audio arrives first; any further audio needs an accepted share with audio, else it is dropped (`AUDIO_SOURCE_UNANNOUNCED`). A popped announcement whose share/webcam has ended is discarded (`take_live_source`); share/webcam stop removes queued announcements; a taken `(user, source)` key never gets a second track.
- **Share start** (`ws_handler.rs` `handle_screen_share_start`): permission (`SCREEN_SHARE_FORBIDDEN`), per-user and channel limits (`SCREEN_SHARE_LIMIT`) and the slot reservation all happen before the source is queued; `screen_share_started` (to everyone, sender included) is the acknowledgement. A start over the per-peer rate limit is answered `RATE_LIMITED` (other voice events over their limit are dropped silently). `SCREEN_SHARE` is in `EVERYONE_DEFAULT`/`EVERYONE_ALLOWED`.
- **Error codes:** `VoiceError::code()` is the `code` of `voice_error` and of HTTP voice errors; `client_message()` keeps internal detail out.
- **Subscriber negotiation** (`Peer::negotiation`, `SfuServer::renegotiate[_own]`, `handle_subscriber_answer`): one outstanding server offer per peer; requests while it waits set `again`, and the answer sends exactly one new offer. A lost answer is covered after `SUBSCRIBER_ANSWER_TIMEOUT` (10 s). Lock order is in the `sfu.rs` module doc. Subscriber answers have their own rate-limit class (`SubscriberAnswer`, 30/10 s⁻¹).
- **Tracks leave with their owner** (`Room::detach_media`, called by `leave_voice_internal` and on replacement): the departed peer's tracks are removed from every other peer's subscriber connection (one renegotiation each), its router sources and the subscriptions into its own tracks go away. `Peer::add_outgoing_track` always creates a new transceiver: webrtc-rs 0.17 would otherwise reuse the transceiver of a removed track with the same id (same user rejoining) and fail, because a removed sender cannot take a track again. Removed transceivers stay inactive in the SDP (one per rejoin of another participant during a session). `Peer::inactive_mlines` counts them (gauge `kaiku_voice_inactive_mlines`, one warn at `INACTIVE_MLINE_WARN_THRESHOLD` = 60); inbound WebSocket frames may be as large as messages (256 KiB) so a large answer does not end the socket.
- **Announcement lifetime:** an accepted share/webcam start without a track after `ANNOUNCEMENT_TTL` (10 s) ends itself (`no_media`, slot released). Stops are never rate-limited. `Room::try_add_screen_share` keeps stream ids unique; a start whose peer retired meanwhile takes its share back. Simulcast layers bind via the publisher transceiver's `mid` (`Peer::bind_video_mid` / `video_source_for_layer`). Simulcast is off: the media engine does not offer the mid and rid header extensions, and `SfuServer::handle_publisher_offer` drops the offer's `a=simulcast` / `a=rid` lines before applying it (`without_simulcast`), so a sender offering several encodings gets an answer without simulcast and sends one stream. Known gap: that stream is received only if the offer declares its SSRC (`a=ssrc`); a sender that declares none for the section (browsers offering simulcast) is not mapped without the mid/rid extensions: the webcam's announcement then ends with `no_media` while the microphone keeps flowing (`voice_media_flow::three_encoding_offer_without_ssrcs_sends_no_webcam_until_one_stream`) (layer forwarding is not production-ready: start layer, sequence/timestamp rewriting on a switch, extension mapping). The bookkeeping below stays for when it is switched on. Each layer is recorded on its seat (`Peer::record_simulcast_layer`) and removed with the seat or its share/webcam; an ended share/webcam forgets its `mid` binding, and a refused binding is forgotten only when a new publisher offer sends another track on that transceiver or stops sending on it.
- **Instance-scoped media:** `Peer::record_forwarded` lists the local tracks that forward a peer's media; `detach_media` removes exactly those (`remove_outgoing_track_if`), so a newer seat of the same user keeps its tracks. A leave that finds the user seated again leaves shares/webcam and sends no `voice_user_left`. `Peer::add_outgoing_track` refuses a second track of the same seat for a source (racing subscribe paths) and a track of a seat that is no longer current; a current seat's track replaces an older seat's entry (its sender is removed by identity).
- **Key frames:** `spawn_subscriber_rtcp_reader` forwards viewers' PLI/FIR to the publisher as PLI, at most one per second per source (`Peer::allow_keyframe_request`). With simulcast the request goes to the layer the viewer receives (`TrackRouter::active_layer`); the periodic request goes to every layer of the source and ends with it.
- **Socket owns its seat:** every voice event except the join resolves its peer with `owned_peer` (the peer's `signal_tx` is this socket's queue). A socket without a seat gets `PARTICIPANT_NOT_FOUND` (leave: silent no-op). A peer that left or was replaced is `retired`: its late `on_track` and its forwarders stop. What a listener receives belongs to the source seat: a subscriber's outgoing entry keeps the source seat's session, a current seat's track replaces an older seat's entry, entries are removed only by track (`Peer::add_outgoing_track`, `remove_outgoing_track_if`). A seat is added only to the room the server still lists (`SfuServer::add_peer_listed`; a join retries once). Join, mute and stats events go through `Room::broadcast_if_current`; a stop holds the source's room state until it is queued (`Room::announce_stopped`). Room state a seat adds records that seat: shares and webcam carry an `owner`, adds check the seat is current under the room lock, removals on behalf of a seat remove only its own entries, and `*Started`/`voice_user_left` are decided under the peers lock (`Room::broadcast_if_current`, `Room::announce_departure`; the latter also frees the rate-limit buckets only when the user is not seated again).
- **Seat replacement** (`Room::add_peer_with_limit` + `retire_replaced_peer`): a second join of the same user swaps the peer under the room lock (one entry per user, no double count), then outside the lock: slots released in their own task (the join does not wait for the store; until it answers, a share start in the room can still count them), the stops (`replaced`) and `voice_removed { session_replaced }` to the old socket (unless the join came on that same socket) sent together, then `detach_media` (before the join answers), session finalized, old PCs closed in a background task (5 s bound).
- **Join limits:** `channels.user_limit` (1–99, unset/0 = none) capped at the room maximum 25, checked under the room lock; nobody bypasses it (`CHANNEL_FULL`). DM rooms refuse a user who blocked or was blocked by another DM participant (Postgres `blocked_among`, lookup errors refuse; `JOIN_REFUSED`, generic text).
- **Calls after a restart** (`call_service::spawn_restart_cleanup`, started in `main.rs`): ringing calls left in Valkey end at once (`no_answer`); answered ones end as `last_left` after `RESTART_REJOIN_GRACE` (60 s) if their room is still empty. Single process assumed.
- **Server ICE:** `SfuServer::rtc_config` has no ICE servers: host candidates only (one UDP socket per connection and local address). `TURN_SERVER` (a comma-separated list, `Config::turn_servers`) is only handed to clients by `GET /api/voice/ice-servers`, as one entry with all URLs.

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
