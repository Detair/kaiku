# Dual PeerConnection Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Replace the single PeerConnection per user with a publisher + subscriber pair, enabling screen sharing via standard `addTrack` negotiation.

**Architecture:** Each user gets two WebRTC connections — a publisher PC (client offers, server answers) for sending mic/screen/webcam, and a subscriber PC (server offers, client answers) for receiving other users' tracks. This eliminates glare and makes screen sharing a trivial `addTrack` call.

**Tech Stack:** Rust (webrtc-rs, axum, tokio), TypeScript (Solid.js, Tauri), WebSocket signaling

**Design Doc:** `docs/plans/2026-03-21-dual-peerconnection-design.md`

---

## Task 1: Add New WebSocket Message Types (Server)

**Files:**
- Modify: `server/src/ws/mod.rs` (ClientEvent ~line 135, ServerEvent ~line 425)

**Step 1: Add publisher/subscriber variants to ClientEvent**

In `server/src/ws/mod.rs`, add new variants to `ClientEvent` enum alongside existing voice messages. Keep old `VoiceAnswer` temporarily for backward compat during migration.

```rust
// Add after VoiceAnswer variant (~line 150):

/// Client sends SDP offer for publisher PeerConnection (mic, screen, webcam tracks)
#[serde(rename = "voice_publisher_offer")]
VoicePublisherOffer {
    channel_id: Uuid,
    sdp: String,
},

/// Client sends SDP answer for subscriber PeerConnection (receiving other users' tracks)
#[serde(rename = "voice_subscriber_answer")]
VoiceSubscriberAnswer {
    channel_id: Uuid,
    sdp: String,
},
```

Modify `VoiceIceCandidate` to include `pc_type`:

```rust
#[serde(rename = "voice_ice_candidate")]
VoiceIceCandidate {
    channel_id: Uuid,
    candidate: String,
    /// "publisher" or "subscriber" — which PeerConnection this candidate belongs to
    #[serde(default = "default_pc_type")]
    pc_type: String,
},
```

Add the default function (for backward compat with old clients):

```rust
fn default_pc_type() -> String {
    "publisher".to_string()
}
```

**Step 2: Add publisher/subscriber variants to ServerEvent**

In `ServerEvent` enum, add new variants alongside existing `VoiceOffer`:

```rust
// Add after VoiceOffer variant (~line 430):

/// Server sends SDP answer to client's publisher offer
#[serde(rename = "voice_publisher_answer")]
VoicePublisherAnswer {
    channel_id: Uuid,
    sdp: String,
},

/// Server sends SDP offer for subscriber PeerConnection
#[serde(rename = "voice_subscriber_offer")]
VoiceSubscriberOffer {
    channel_id: Uuid,
    sdp: String,
},
```

Modify `VoiceIceCandidate` in ServerEvent to include `pc_type`:

```rust
#[serde(rename = "voice_ice_candidate")]
VoiceIceCandidate {
    channel_id: Uuid,
    candidate: String,
    pc_type: String,
},
```

**Step 3: Run compilation check**

```bash
SQLX_OFFLINE=true cargo clippy -p vc-server -- -D warnings
```

Fix any compilation errors from new variants not being handled in match arms (add `todo!()` arms temporarily).

**Step 4: Write serialization test**

Add to `server/src/ws/mod.rs` (or existing test module):

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_publisher_offer_serialization() {
        let event = ClientEvent::VoicePublisherOffer {
            channel_id: Uuid::nil(),
            sdp: "v=0\r\n".to_string(),
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("voice_publisher_offer"));
        let parsed: ClientEvent = serde_json::from_str(&json).unwrap();
        match parsed {
            ClientEvent::VoicePublisherOffer { sdp, .. } => assert_eq!(sdp, "v=0\r\n"),
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn test_subscriber_offer_serialization() {
        let event = ServerEvent::VoiceSubscriberOffer {
            channel_id: Uuid::nil(),
            sdp: "v=0\r\n".to_string(),
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("voice_subscriber_offer"));
    }

    #[test]
    fn test_publisher_answer_serialization() {
        let event = ServerEvent::VoicePublisherAnswer {
            channel_id: Uuid::nil(),
            sdp: "v=0\r\n".to_string(),
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("voice_publisher_answer"));
    }

    #[test]
    fn test_ice_candidate_pc_type_default() {
        // Old clients send without pc_type — should default to "publisher"
        let json = r#"{"type":"voice_ice_candidate","channel_id":"00000000-0000-0000-0000-000000000000","candidate":"candidate:..."}"#;
        let parsed: ClientEvent = serde_json::from_str(json).unwrap();
        match parsed {
            ClientEvent::VoiceIceCandidate { pc_type, .. } => assert_eq!(pc_type, "publisher"),
            _ => panic!("wrong variant"),
        }
    }
}
```

**Step 5: Run tests**

```bash
cargo test -p vc-server -- ws::tests --nocapture
```

Expected: All 4 tests pass.

**Step 6: Commit**

```bash
git add server/src/ws/mod.rs
git commit -m "feat(ws): add publisher/subscriber offer/answer message types"
```

---

## Task 2: Refactor Peer Struct to Dual PeerConnection (Server)

**Files:**
- Modify: `server/src/voice/peer.rs` (Peer struct ~line 28, methods ~lines 61-232)

**Step 1: Split peer_connection into publisher_pc + subscriber_pc**

Update the `Peer` struct:

```rust
pub struct Peer {
    pub user_id: Uuid,
    pub username: String,
    pub display_name: String,
    pub channel_id: Uuid,

    /// Receives tracks FROM client (mic, screen, webcam). Client creates offers.
    pub publisher_pc: Arc<RTCPeerConnection>,
    /// Sends tracks TO client (other users' audio, screen shares). Server creates offers.
    pub subscriber_pc: Arc<RTCPeerConnection>,

    /// Tracks received from this peer (on publisher_pc)
    incoming_tracks: RwLock<HashMap<TrackSource, Arc<TrackRemote>>>,
    /// Tracks being forwarded to this peer (on subscriber_pc)
    outgoing_tracks: RwLock<HashMap<(Uuid, TrackSource), Arc<TrackLocalStaticRTP>>>,

    muted: RwLock<bool>,
    pub signal_tx: mpsc::Sender<OutboundMsg>,
    pub session_id: Uuid,
    pub connected_at: DateTime<Utc>,

    pending_track_sources: RwLock<Vec<TrackSource>>,
}
```

**Step 2: Update constructor to accept two PCs**

```rust
pub async fn new(
    user_id: Uuid,
    username: String,
    display_name: String,
    channel_id: Uuid,
    publisher_pc: Arc<RTCPeerConnection>,
    subscriber_pc: Arc<RTCPeerConnection>,
    signal_tx: mpsc::Sender<OutboundMsg>,
) -> Self {
    Self {
        user_id,
        username,
        display_name,
        channel_id,
        publisher_pc,
        subscriber_pc,
        incoming_tracks: RwLock::new(HashMap::new()),
        outgoing_tracks: RwLock::new(HashMap::new()),
        muted: RwLock::new(false),
        signal_tx,
        session_id: Uuid::new_v4(),
        connected_at: Utc::now(),
        pending_track_sources: RwLock::new(Vec::new()),
    }
}
```

**Step 3: Remove `add_recv_transceiver()` method**

Delete the `add_recv_transceiver` method entirely (~lines 94-129). Pre-allocated transceivers are no longer needed — the publisher PC's transceivers come from the client's offer.

**Step 4: Update `add_outgoing_track()` to use subscriber_pc**

Change `self.peer_connection` → `self.subscriber_pc` in the method body (~line 146):

```rust
pub async fn add_outgoing_track(
    &self,
    source_user_id: Uuid,
    source_type: TrackSource,
    track: Arc<TrackLocalStaticRTP>,
) -> Result<Arc<RTCRtpSender>> {
    let sender = self
        .subscriber_pc  // was: self.peer_connection
        .add_track(track.clone() as Arc<dyn TrackLocal + Send + Sync>)
        .await?;
    // ... rest unchanged
}
```

**Step 5: Update `remove_outgoing_track()` to use subscriber_pc**

Change `self.peer_connection` → `self.subscriber_pc` (~line 170):

```rust
// In remove_outgoing_track, find the line that iterates senders:
let senders = self.subscriber_pc.get_senders().await;  // was: self.peer_connection
```

**Step 6: Update `is_connected()` to check both PCs**

```rust
pub fn is_connected(&self) -> bool {
    self.publisher_pc.connection_state() == RTCPeerConnectionState::Connected
        || self.subscriber_pc.connection_state() == RTCPeerConnectionState::Connected
}
```

**Step 7: Update `close()` to close both PCs**

```rust
pub async fn close(&self) -> Result<()> {
    let _ = self.publisher_pc.close().await;
    let _ = self.subscriber_pc.close().await;
    Ok(())
}
```

**Step 8: Run compilation check**

```bash
SQLX_OFFLINE=true cargo clippy -p vc-server -- -D warnings
```

This will show errors in `sfu.rs` where `Peer::new()` and `peer.peer_connection` are used. That's expected — Task 3 fixes those.

**Step 9: Commit (WIP, won't compile yet)**

```bash
git add server/src/voice/peer.rs
git commit -m "refactor(voice): split Peer into publisher_pc + subscriber_pc

WIP: sfu.rs not yet updated to use new Peer API"
```

---

## Task 3: Update SfuServer for Dual PeerConnection (Server)

**Files:**
- Modify: `server/src/voice/sfu.rs` (~lines 524-842)

**Step 1: Update `create_peer()` to create two PeerConnections**

The existing `create_peer()` creates one PC and adds recv transceivers. Update it to create two PCs with no pre-allocated transceivers:

```rust
pub async fn create_peer(
    &self,
    user_id: Uuid,
    username: String,
    display_name: String,
    channel_id: Uuid,
    signal_tx: mpsc::Sender<OutboundMsg>,
) -> Result<Arc<Peer>> {
    let config = RTCConfiguration {
        ice_servers: self.config.ice_servers(),
        ..Default::default()
    };

    let publisher_pc = Arc::new(self.api.new_peer_connection(config.clone()).await?);
    let subscriber_pc = Arc::new(self.api.new_peer_connection(config).await?);

    let peer = Arc::new(
        Peer::new(
            user_id,
            username,
            display_name,
            channel_id,
            publisher_pc,
            subscriber_pc,
            signal_tx,
        )
        .await,
    );

    // Set up connection state handler on BOTH PCs
    Self::setup_connection_state_handler(&peer, "publisher").await;
    Self::setup_connection_state_handler(&peer, "subscriber").await;

    Ok(peer)
}
```

Extract the connection state handler into a reusable method:

```rust
async fn setup_connection_state_handler(peer: &Arc<Peer>, pc_label: &str) {
    let peer_weak = Arc::downgrade(peer);
    let label = pc_label.to_string();
    let pc = if pc_label == "publisher" {
        &peer.publisher_pc
    } else {
        &peer.subscriber_pc
    };

    pc.on_peer_connection_state_change(Box::new(move |state| {
        let peer_weak = peer_weak.clone();
        let label = label.clone();
        Box::pin(async move {
            if let Some(peer) = peer_weak.upgrade() {
                tracing::debug!(
                    user_id = %peer.user_id,
                    pc = %label,
                    state = ?state,
                    "peer connection state changed"
                );
            }
        })
    }));
}
```

**Step 2: Update `setup_track_handler()` to use publisher_pc**

Change `peer.peer_connection.on_track(...)` → `peer.publisher_pc.on_track(...)` (~line 590):

```rust
pub fn setup_track_handler(peer: Arc<Peer>, room: Arc<Room>) {
    let peer_clone = peer.clone();
    let room_clone = room.clone();

    peer.publisher_pc.on_track(Box::new(move |track, receiver, transceiver| {
        // ... existing handler body unchanged
    }));
}
```

**Step 3: Update `setup_ice_handler()` for both PCs with pc_type**

```rust
pub fn setup_ice_handler(peer: Arc<Peer>) {
    // Publisher ICE
    let peer_pub = peer.clone();
    peer.publisher_pc.on_ice_candidate(Box::new(move |candidate| {
        let peer = peer_pub.clone();
        Box::pin(async move {
            if let Some(candidate) = candidate {
                let json = match candidate.to_json() {
                    Ok(json) => json,
                    Err(e) => {
                        tracing::warn!(error = %e, "failed to serialize publisher ICE candidate");
                        return;
                    }
                };
                let candidate_str = serde_json::to_string(&json).unwrap_or_default();
                let _ = peer.signal_tx.send(OutboundMsg::Event(ServerEvent::VoiceIceCandidate {
                    channel_id: peer.channel_id,
                    candidate: candidate_str,
                    pc_type: "publisher".to_string(),
                })).await;
            }
        })
    }));

    // Subscriber ICE
    let peer_sub = peer.clone();
    peer.subscriber_pc.on_ice_candidate(Box::new(move |candidate| {
        let peer = peer_sub.clone();
        Box::pin(async move {
            if let Some(candidate) = candidate {
                let json = match candidate.to_json() {
                    Ok(json) => json,
                    Err(e) => {
                        tracing::warn!(error = %e, "failed to serialize subscriber ICE candidate");
                        return;
                    }
                };
                let candidate_str = serde_json::to_string(&json).unwrap_or_default();
                let _ = peer.signal_tx.send(OutboundMsg::Event(ServerEvent::VoiceIceCandidate {
                    channel_id: peer.channel_id,
                    candidate: candidate_str,
                    pc_type: "subscriber".to_string(),
                })).await;
            }
        })
    }));
}
```

**Step 4: Rename and update offer/answer methods**

Replace existing `create_offer`, `handle_answer`, `renegotiate`:

```rust
/// Handle a publisher offer from the client — server creates answer
pub async fn handle_publisher_offer(peer: &Arc<Peer>, sdp: String) -> Result<String> {
    let offer = RTCSessionDescription::offer(sdp)?;
    peer.publisher_pc.set_remote_description(offer).await?;

    let answer = peer.publisher_pc.create_answer(None).await?;
    peer.publisher_pc.set_local_description(answer.clone()).await?;

    Ok(answer.sdp)
}

/// Handle a subscriber answer from the client
pub async fn handle_subscriber_answer(peer: &Arc<Peer>, sdp: String) -> Result<()> {
    let answer = RTCSessionDescription::answer(sdp)?;
    peer.subscriber_pc.set_remote_description(answer).await?;
    Ok(())
}

/// Create subscriber offer after adding outgoing tracks, send to client
pub async fn renegotiate(peer: &Arc<Peer>) -> Result<()> {
    let offer = peer.subscriber_pc.create_offer(None).await?;
    peer.subscriber_pc.set_local_description(offer.clone()).await?;

    let _ = peer.signal_tx.send(OutboundMsg::Event(ServerEvent::VoiceSubscriberOffer {
        channel_id: peer.channel_id,
        sdp: offer.sdp,
    })).await;

    Ok(())
}

/// Handle ICE candidate routed to correct PC
pub async fn handle_ice_candidate(peer: &Arc<Peer>, candidate_str: String, pc_type: &str) -> Result<()> {
    let candidate: RTCIceCandidateInit = serde_json::from_str(&candidate_str)?;
    let pc = if pc_type == "subscriber" {
        &peer.subscriber_pc
    } else {
        &peer.publisher_pc
    };
    pc.add_ice_candidate(candidate).await?;
    Ok(())
}
```

Remove the old `create_offer()` and `handle_answer()` methods.

**Step 5: Run compilation check**

```bash
SQLX_OFFLINE=true cargo clippy -p vc-server -- -D warnings
```

Expect errors in `ws_handler.rs` — that's Task 4.

**Step 6: Commit**

```bash
git add server/src/voice/sfu.rs
git commit -m "refactor(voice): update SfuServer for dual PeerConnection

WIP: ws_handler.rs not yet updated to use new SfuServer API"
```

---

## Task 4: Update WebSocket Handlers (Server)

**Files:**
- Modify: `server/src/voice/ws_handler.rs` (~lines 39-503)

**Step 1: Add match arms for new message types**

In `handle_voice_event()`, add new arms:

```rust
ClientEvent::VoicePublisherOffer { channel_id, sdp } => {
    handle_publisher_offer(sfu, user_id, channel_id, &sdp).await;
}
ClientEvent::VoiceSubscriberAnswer { channel_id, sdp } => {
    handle_subscriber_answer(sfu, user_id, channel_id, &sdp).await;
}
```

Update ICE candidate arm to pass `pc_type`:

```rust
ClientEvent::VoiceIceCandidate { channel_id, candidate, pc_type } => {
    handle_ice_candidate(sfu, user_id, channel_id, &candidate, &pc_type).await;
}
```

**Step 2: Update `handle_join()` — don't create initial offer**

The key change: on join, the server creates PCs but does NOT send an initial offer. Instead, it waits for the client's publisher offer.

Remove the `create_offer` + `VoiceOffer` send at the end of `handle_join()` (~lines 270-290). The server just:
1. Creates peer (two PCs)
2. Sets up track handler on publisher_pc
3. Sets up ICE handler on both PCs
4. Adds peer to room
5. Sends `VoiceRoomState` (existing)
6. Does NOT send offer — waits for client's `VoicePublisherOffer`

For existing peers' tracks: subscribe the new user to existing tracks AFTER receiving the first publisher offer. This can be done in `handle_publisher_offer()` by checking if this is the first offer (no outgoing tracks yet).

**Step 3: Write `handle_publisher_offer()` handler**

```rust
async fn handle_publisher_offer(
    sfu: &Arc<SfuServer>,
    user_id: Uuid,
    channel_id: Uuid,
    sdp: &str,
) {
    let room = match sfu.get_room(channel_id).await {
        Some(room) => room,
        None => return,
    };
    let peer = match room.get_peer(user_id).await {
        Some(peer) => peer,
        None => return,
    };

    match SfuServer::handle_publisher_offer(&peer, sdp.to_string()).await {
        Ok(answer_sdp) => {
            let _ = peer.signal_tx.send(OutboundMsg::Event(
                ServerEvent::VoicePublisherAnswer {
                    channel_id,
                    sdp: answer_sdp,
                },
            )).await;

            // On first publisher offer, subscribe to existing peers' tracks
            let needs_subscription = peer.outgoing_tracks_count().await == 0;
            if needs_subscription {
                subscribe_to_existing_tracks(sfu, &room, &peer, user_id).await;
            }
        }
        Err(e) => {
            tracing::error!(user_id = %user_id, error = %e, "failed to handle publisher offer");
        }
    }
}
```

Add helper `subscribe_to_existing_tracks()`:

```rust
async fn subscribe_to_existing_tracks(
    sfu: &Arc<SfuServer>,
    room: &Arc<Room>,
    peer: &Arc<Peer>,
    user_id: Uuid,
) {
    let peers = room.peers.read().await;
    for (other_id, other_peer) in peers.iter() {
        if *other_id == user_id {
            continue;
        }
        let incoming = other_peer.incoming_tracks.read().await;
        for (source, _track) in incoming.iter() {
            if let Some(subscriber_track) = room.track_router.create_subscriber_track(
                *other_id,
                source.clone(),
            ).await {
                if let Ok(sender) = peer.add_outgoing_track(
                    *other_id,
                    source.clone(),
                    subscriber_track.clone(),
                ).await {
                    room.track_router.spawn_subscriber_remb_reader(
                        user_id,
                        *other_id,
                        source.clone(),
                        sender,
                    );
                }
            }
        }
    }
    drop(peers);

    // Renegotiate subscriber PC to send the offer with new tracks
    if let Err(e) = SfuServer::renegotiate(&peer).await {
        tracing::error!(user_id = %user_id, error = %e, "failed to renegotiate subscriber after join");
    }
}
```

**Step 4: Write `handle_subscriber_answer()` handler**

```rust
async fn handle_subscriber_answer(
    sfu: &Arc<SfuServer>,
    user_id: Uuid,
    channel_id: Uuid,
    sdp: &str,
) {
    let room = match sfu.get_room(channel_id).await {
        Some(room) => room,
        None => return,
    };
    let peer = match room.get_peer(user_id).await {
        Some(peer) => peer,
        None => return,
    };

    if let Err(e) = SfuServer::handle_subscriber_answer(&peer, sdp.to_string()).await {
        tracing::error!(user_id = %user_id, error = %e, "failed to handle subscriber answer");
    }
}
```

**Step 5: Update `handle_ice_candidate()` to route by pc_type**

```rust
async fn handle_ice_candidate(
    sfu: &Arc<SfuServer>,
    user_id: Uuid,
    channel_id: Uuid,
    candidate: &str,
    pc_type: &str,
) {
    // ... get room and peer as before ...
    if let Err(e) = SfuServer::handle_ice_candidate(&peer, candidate.to_string(), pc_type).await {
        tracing::error!(user_id = %user_id, pc_type = %pc_type, error = %e, "failed to add ICE candidate");
    }
}
```

**Step 6: Remove old `handle_answer()` and update old `VoiceAnswer` arm**

Either remove the `VoiceAnswer` match arm or redirect it to `handle_subscriber_answer` temporarily:

```rust
// Keep for backward compat during migration, remove in Task 8
ClientEvent::VoiceAnswer { channel_id, sdp } => {
    tracing::warn!("received legacy VoiceAnswer, treating as subscriber answer");
    handle_subscriber_answer(sfu, user_id, channel_id, &sdp).await;
}
```

**Step 7: Compile and run tests**

```bash
SQLX_OFFLINE=true cargo clippy -p vc-server -- -D warnings
cargo test -p vc-server --nocapture
```

Expected: Compiles, existing tests pass. Functional testing requires client changes (Task 5).

**Step 8: Commit**

```bash
git add server/src/voice/ws_handler.rs
git commit -m "feat(voice): add publisher/subscriber offer handlers

Server now accepts client-initiated publisher offers and creates
subscriber offers for outgoing tracks. Legacy VoiceAnswer redirected
to subscriber answer for backward compat."
```

---

## Task 5: Client — Dual PeerConnection Adapter (Core)

**Files:**
- Modify: `client/src/lib/webrtc/browser.ts` (~lines 49-400)
- Modify: `client/src/lib/webrtc/types.ts` (VoiceAdapter interface)

**Step 1: Update VoiceAdapter interface**

In `types.ts`, replace `handleOffer` with new methods:

```typescript
interface VoiceAdapter {
  // ... existing methods ...

  // Replace handleOffer with:
  handlePublisherAnswer(channelId: string, sdp: string): Promise<VoiceResult<void>>;
  handleSubscriberOffer(channelId: string, sdp: string): Promise<VoiceResult<string>>; // returns answer SDP

  // Keep handleIceCandidate but it now needs pc_type routing internally
  handleIceCandidate(channelId: string, candidate: string, pcType?: string): Promise<VoiceResult<void>>;
}
```

**Step 2: Refactor BrowserVoiceAdapter properties**

Replace `peerConnection` with two PCs, remove pending screen share state:

```typescript
// Replace:
private peerConnection: RTCPeerConnection | null = null;

// With:
private publisherPC: RTCPeerConnection | null = null;
private subscriberPC: RTCPeerConnection | null = null;
```

Remove `pendingScreenShares` map entirely.

**Step 3: Rewrite `join()` for dual PC**

```typescript
async join(channelId: string): Promise<VoiceResult<void>> {
  try {
    // 1. Get microphone
    this.localStream = await navigator.mediaDevices.getUserMedia({
      audio: this.getAudioConstraints(),
      video: false,
    });

    // 2. Create publisher PC
    this.publisherPC = new RTCPeerConnection(this.getRTCConfig());
    this.setupPublisherPC(channelId);

    // 3. Create subscriber PC
    this.subscriberPC = new RTCPeerConnection(this.getRTCConfig());
    this.setupSubscriberPC(channelId);

    // 4. Add mic track to publisher
    for (const track of this.localStream.getAudioTracks()) {
      this.publisherPC.addTrack(track, this.localStream);
    }

    // 5. Notify server (triggers room join, but no server offer)
    // The publisher's onnegotiationneeded will fire and send the first offer
    this.sendWsEvent('voice_join', { channel_id: channelId });

    return { ok: true, value: undefined };
  } catch (error) {
    return { ok: false, error: String(error) };
  }
}
```

**Step 4: Implement `setupPublisherPC()`**

```typescript
private setupPublisherPC(channelId: string) {
  if (!this.publisherPC) return;

  // onnegotiationneeded — create and send offer
  this.publisherPC.onnegotiationneeded = async () => {
    if (!this.publisherPC) return;
    try {
      const offer = await this.publisherPC.createOffer();
      await this.publisherPC.setLocalDescription(offer);
      this.sendWsEvent('voice_publisher_offer', {
        channel_id: channelId,
        sdp: offer.sdp,
      });
    } catch (e) {
      console.error('[WebRTC] publisher negotiation failed:', e);
    }
  };

  // ICE candidates
  this.publisherPC.onicecandidate = (event) => {
    if (event.candidate) {
      this.sendWsEvent('voice_ice_candidate', {
        channel_id: channelId,
        candidate: JSON.stringify(event.candidate.toJSON()),
        pc_type: 'publisher',
      });
    }
  };

  // Connection state logging
  this.publisherPC.onconnectionstatechange = () => {
    console.log('[WebRTC] publisher connection state:', this.publisherPC?.connectionState);
  };
}
```

**Step 5: Implement `setupSubscriberPC()`**

```typescript
private setupSubscriberPC(channelId: string) {
  if (!this.subscriberPC) return;

  // Receive remote tracks
  this.subscriberPC.ontrack = (event) => {
    const stream = event.streams[0];
    if (stream) {
      this.remoteStreams.set(stream.id, stream);
      this.onRemoteTrack?.(event.track, stream);
    }
  };

  // ICE candidates
  this.subscriberPC.onicecandidate = (event) => {
    if (event.candidate) {
      this.sendWsEvent('voice_ice_candidate', {
        channel_id: channelId,
        candidate: JSON.stringify(event.candidate.toJSON()),
        pc_type: 'subscriber',
      });
    }
  };

  // Connection state logging
  this.subscriberPC.onconnectionstatechange = () => {
    console.log('[WebRTC] subscriber connection state:', this.subscriberPC?.connectionState);
  };
}
```

**Step 6: Implement `handlePublisherAnswer()`**

```typescript
async handlePublisherAnswer(channelId: string, sdp: string): Promise<VoiceResult<void>> {
  if (!this.publisherPC) {
    return { ok: false, error: 'Publisher PC not initialized' };
  }
  try {
    await this.publisherPC.setRemoteDescription(
      new RTCSessionDescription({ type: 'answer', sdp })
    );
    return { ok: true, value: undefined };
  } catch (error) {
    return { ok: false, error: String(error) };
  }
}
```

**Step 7: Implement `handleSubscriberOffer()`**

```typescript
async handleSubscriberOffer(channelId: string, sdp: string): Promise<VoiceResult<string>> {
  if (!this.subscriberPC) {
    return { ok: false, error: 'Subscriber PC not initialized' };
  }
  try {
    await this.subscriberPC.setRemoteDescription(
      new RTCSessionDescription({ type: 'offer', sdp })
    );
    const answer = await this.subscriberPC.createAnswer();
    await this.subscriberPC.setLocalDescription(answer);
    return { ok: true, value: answer.sdp! };
  } catch (error) {
    return { ok: false, error: String(error) };
  }
}
```

**Step 8: Update `handleIceCandidate()` for pc_type routing**

```typescript
async handleIceCandidate(
  channelId: string,
  candidate: string,
  pcType: string = 'publisher',
): Promise<VoiceResult<void>> {
  const pc = pcType === 'subscriber' ? this.subscriberPC : this.publisherPC;
  if (!pc) {
    return { ok: false, error: `${pcType} PC not initialized` };
  }
  try {
    const parsed = JSON.parse(candidate);
    await pc.addIceCandidate(new RTCIceCandidate(parsed));
    return { ok: true, value: undefined };
  } catch (error) {
    return { ok: false, error: String(error) };
  }
}
```

**Step 9: Update `leave()` to close both PCs**

```typescript
async leave(): Promise<VoiceResult<void>> {
  this.publisherPC?.close();
  this.publisherPC = null;
  this.subscriberPC?.close();
  this.subscriberPC = null;
  this.localStream?.getTracks().forEach(t => t.stop());
  this.localStream = null;
  this.remoteStreams.clear();
  this.screenShares.clear();
  // ... send voice_leave, etc.
}
```

**Step 10: Remove old `handleOffer()` method**

Delete the entire `handleOffer()` method.

**Step 11: Compile check**

```bash
cd client && bun run typecheck
```

Fix type errors from removed/changed interface methods (voice store and websocket store will have errors — Task 6).

**Step 12: Commit**

```bash
git add client/src/lib/webrtc/browser.ts client/src/lib/webrtc/types.ts
git commit -m "refactor(client): dual PeerConnection in BrowserVoiceAdapter

Publisher PC for sending (client offers), subscriber PC for receiving
(server offers). Removes handleOffer, adds handlePublisherAnswer and
handleSubscriberOffer.

WIP: voice store and websocket store not yet updated."
```

---

## Task 6: Client — WebSocket + Voice Store Integration

**Files:**
- Modify: `client/src/stores/websocket.ts` (~lines 1683-1745)
- Modify: `client/src/stores/voice.ts` (~lines 401-548)

**Step 1: Update WebSocket incoming message handlers**

Replace `handleVoiceOffer` with two handlers:

```typescript
async function handleVoicePublisherAnswer(channelId: string, sdp: string) {
  const adapter = getVoiceAdapter();
  if (!adapter) return;

  const result = await adapter.handlePublisherAnswer(channelId, sdp);
  if (!result.ok) {
    console.error('[WS] handlePublisherAnswer failed:', result.error);
  }
}

async function handleVoiceSubscriberOffer(channelId: string, sdp: string) {
  const adapter = getVoiceAdapter();
  if (!adapter) return;

  const result = await adapter.handleSubscriberOffer(channelId, sdp);
  if (!result.ok) {
    console.error('[WS] handleSubscriberOffer failed:', result.error);
    return;
  }

  // Send subscriber answer back to server
  wsSend({
    type: 'voice_subscriber_answer',
    channel_id: channelId,
    sdp: result.value,
  });
}
```

**Step 2: Update incoming ICE candidate handler**

```typescript
async function handleVoiceIceCandidate(channelId: string, candidate: string, pcType: string = 'publisher') {
  const adapter = getVoiceAdapter();
  if (!adapter) return;

  const result = await adapter.handleIceCandidate(channelId, candidate, pcType);
  if (!result.ok) {
    console.error(`[WS] handleIceCandidate (${pcType}) failed:`, result.error);
  }
}
```

**Step 3: Update the message dispatcher**

In the server event handler switch/if-else block, add new event types and update ICE:

```typescript
case 'voice_publisher_answer':
  handleVoicePublisherAnswer(event.channel_id, event.sdp);
  break;

case 'voice_subscriber_offer':
  handleVoiceSubscriberOffer(event.channel_id, event.sdp);
  break;

case 'voice_ice_candidate':
  handleVoiceIceCandidate(event.channel_id, event.candidate, event.pc_type || 'publisher');
  break;
```

Remove the `voice_offer` handler (old `handleVoiceOffer`).

**Step 4: Update voice store `joinVoice()` adapter setup**

In `voice.ts`, the `joinVoice()` function sets up adapter event handlers. Remove the offer handler setup, the adapter now handles negotiation internally via `onnegotiationneeded`.

**Step 5: Type check and test**

```bash
cd client && bun run typecheck
bun run test:run
```

**Step 6: Commit**

```bash
git add client/src/stores/websocket.ts client/src/stores/voice.ts
git commit -m "feat(client): wire publisher/subscriber signaling in stores

WebSocket store handles voice_publisher_answer and voice_subscriber_offer.
Voice store updated for new adapter interface."
```

---

## Task 7: Audio Integration Test

**Files:** None new — functional test with running server + client

**Step 1: Start dev environment**

```bash
podman compose -f docker-compose.dev.yml --profile storage up -d
DATABASE_URL="postgresql://voicechat:voicechat_dev@localhost:5433/voicechat" sqlx migrate run --source server/migrations
```

**Step 2: Start server**

```bash
cd server && cargo run
```

**Step 3: Start client**

```bash
cd client && bun run dev
```

**Step 4: Test audio flow**

1. Open two browser tabs, log in as different users
2. Both join the same voice channel
3. Verify: both users can hear each other (mic audio flows)
4. Check browser console: should see `[WebRTC] publisher connection state: connected` and `[WebRTC] subscriber connection state: connected`
5. Check server logs: should see publisher offers being answered and subscriber offers being sent

**Step 5: Debug if needed**

Common issues:
- **webrtc-rs can't answer client offer**: Check server logs for SDP parsing errors. May need to adjust codec negotiation in `create_peer()`.
- **ICE fails on one PC**: Verify both PCs get ICE candidates routed correctly (check `pc_type` in messages).
- **No remote audio**: Verify subscriber PC's `ontrack` fires. Check that `subscribe_to_existing_tracks()` is called after the first publisher offer.

**Step 6: Commit (if any debug fixes needed)**

```bash
git add -A
git commit -m "fix(voice): debug fixes for dual PeerConnection audio flow"
```

---

## Task 8: Simplify Screen Share (Client)

**Files:**
- Modify: `client/src/lib/webrtc/browser.ts` (startScreenShare ~line 752, stopScreenShare ~line 923)

**Step 1: Rewrite `startScreenShare()`**

The entire method becomes trivially simple:

```typescript
async startScreenShare(options?: ScreenShareOptions): Promise<VoiceResult<ScreenShareResult>> {
  if (!this.publisherPC) {
    return { ok: false, error: 'Not connected' };
  }

  try {
    const streamId = options?.streamId || crypto.randomUUID();

    // Capture screen
    const stream = await navigator.mediaDevices.getDisplayMedia({
      video: this.getScreenShareConstraints(options?.quality),
      audio: options?.withAudio || false,
    });

    // Add video track to publisher PC
    const videoTrack = stream.getVideoTracks()[0];
    const videoSender = this.publisherPC.addTrack(videoTrack, stream);

    // Add audio track if present
    let audioSender: RTCRtpSender | null = null;
    const audioTrack = stream.getAudioTracks()[0];
    if (audioTrack) {
      audioSender = this.publisherPC.addTrack(audioTrack, stream);
    }

    // onnegotiationneeded fires automatically → creates offer → server answers → RTP flows

    // Handle user stopping share via browser UI
    videoTrack.onended = () => {
      this.stopScreenShare(streamId);
    };

    // Store for cleanup
    this.screenShares.set(streamId, { stream, videoSender, audioSender });

    return {
      ok: true,
      value: { streamId, stream, hasAudio: !!audioTrack },
    };
  } catch (error) {
    return { ok: false, error: String(error) };
  }
}
```

**Step 2: Rewrite `stopScreenShare()`**

```typescript
async stopScreenShare(streamId?: string): Promise<VoiceResult<void>> {
  if (!this.publisherPC) {
    return { ok: false, error: 'Not connected' };
  }

  const targetId = streamId || this.screenShares.keys().next().value;
  if (!targetId) {
    return { ok: false, error: 'No active screen share' };
  }

  const share = this.screenShares.get(targetId);
  if (!share) {
    return { ok: false, error: `Screen share ${targetId} not found` };
  }

  // Remove tracks from publisher PC
  if (share.videoSender) {
    this.publisherPC.removeTrack(share.videoSender);
  }
  if (share.audioSender) {
    this.publisherPC.removeTrack(share.audioSender);
  }

  // Stop media tracks
  share.stream.getTracks().forEach(t => t.stop());

  // onnegotiationneeded fires automatically → renegotiates

  this.screenShares.delete(targetId);

  return { ok: true, value: undefined };
}
```

**Step 3: Remove all pending screen share infrastructure**

Delete `pendingScreenShares` property and any code referencing it. Delete video transceiver search logic. Delete `replaceTrack` usage for screen shares.

**Step 4: Type check**

```bash
cd client && bun run typecheck
```

**Step 5: Commit**

```bash
git add client/src/lib/webrtc/browser.ts
git commit -m "feat(voice): simplify screen share to addTrack on publisher PC

Screen sharing is now just addTrack + onnegotiationneeded. Removes
replaceTrack hacks, pendingScreenShares, transceiver search logic."
```

---

## Task 9: Screen Share Integration Test

**Step 1: Test single screen share**

1. Two users in voice channel
2. User A starts screen share
3. Check: User A's browser console shows publisher renegotiation (new offer sent)
4. Check: Server logs show on_track fire for video
5. Check: User B receives screen share video

**Step 2: Test multiple screen shares**

1. Three users in voice channel
2. User A and User B both start screen shares
3. Check: User C sees both screen shares
4. Check: Each screen share has independent stream ID

**Step 3: Test screen share stop**

1. User A stops screen share
2. Check: Publisher renegotiates (removeTrack → onnegotiationneeded)
3. Check: Server removes subscriber tracks, renegotiates with viewers
4. Check: User B stops seeing User A's screen share

**Step 4: Commit any fixes**

```bash
git add -A
git commit -m "fix(voice): screen share integration fixes"
```

---

## Task 10: Cleanup Legacy Code

**Files:**
- Modify: `server/src/ws/mod.rs` — remove `VoiceOffer` and old `VoiceAnswer` variants
- Modify: `server/src/voice/ws_handler.rs` — remove legacy `VoiceAnswer` backward compat arm
- Modify: `server/src/voice/peer.rs` — remove any remaining `peer_connection` references
- Modify: `client/src/stores/websocket.ts` — remove `handleVoiceOffer` function
- Modify: `client/src/lib/webrtc/browser.ts` — remove any remaining single-PC references

**Step 1: Remove `VoiceOffer` from ServerEvent**

Remove the `VoiceOffer` variant from `ServerEvent` enum in `server/src/ws/mod.rs`.

**Step 2: Remove legacy `VoiceAnswer` handling**

Remove the `VoiceAnswer` match arm from `handle_voice_event()` and the `VoiceAnswer` variant from `ClientEvent` (or mark deprecated).

**Step 3: Remove client `handleVoiceOffer`**

Delete the `handleVoiceOffer()` function from `websocket.ts` and its dispatcher entry.

**Step 4: Compile and test**

```bash
SQLX_OFFLINE=true cargo clippy -p vc-server -- -D warnings
cd client && bun run typecheck && bun run test:run
```

**Step 5: Commit**

```bash
git add -A
git commit -m "refactor(voice): remove legacy single-PC signaling

Removes VoiceOffer, VoiceAnswer, handleVoiceOffer, and all remaining
single-PeerConnection code paths."
```

---

## Task 11: Update CHANGELOG

**Files:**
- Modify: `CHANGELOG.md`

Add under `[Unreleased]`:

```markdown
### Changed
- Voice connections now use dual PeerConnection architecture (publisher + subscriber) for better reliability and screen share support
- Screen sharing uses standard WebRTC addTrack negotiation instead of replaceTrack workarounds

### Fixed
- Screen sharing video now flows between users (was broken due to single-PC offer model)
```

**Commit:**

```bash
git add CHANGELOG.md
git commit -m "docs: update changelog for dual PeerConnection screen share"
```

---

## Summary

| Task | Files | Effort | Risk |
|------|-------|--------|------|
| 1. WS message types | `ws/mod.rs` | 30min | None |
| 2. Peer struct refactor | `peer.rs` | 30min | Low |
| 3. SfuServer methods | `sfu.rs` | 1h | Medium |
| 4. WS handlers | `ws_handler.rs` | 1h | Medium |
| 5. Client adapter | `browser.ts`, `types.ts` | 1.5h | Medium |
| 6. Client stores | `websocket.ts`, `voice.ts` | 45min | Low |
| 7. Audio integration test | — | 1h | **Key validation** |
| 8. Screen share simplify | `browser.ts` | 30min | Low |
| 9. Screen share test | — | 1h | Medium |
| 10. Cleanup | Multiple | 30min | Low |
| 11. Changelog | `CHANGELOG.md` | 5min | None |
| **Total** | | **~8-9h** | |

Tasks 1-4 (server) and 5-6 (client) can be developed in parallel by tracking the shared WS message contract. Task 7 is the critical validation point — if audio works with dual PCs, screen share (Task 8) is almost guaranteed to work.
