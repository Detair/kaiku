//! WebRTC Peer Connection Management
//!
//! Each participant has two `PeerConnection`s:
//! - **`publisher_pc`**: receives tracks FROM the client (mic, screen, webcam). The client creates
//!   offers for this connection.
//! - **`subscriber_pc`**: sends tracks TO the client (other users' audio, screen shares, webcams).
//!   The server creates offers for this connection.

use std::collections::HashMap;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use tokio::sync::{mpsc, RwLock};
use uuid::Uuid;
use webrtc::peer_connection::peer_connection_state::RTCPeerConnectionState;
use webrtc::peer_connection::RTCPeerConnection;
use webrtc::rtp_transceiver::rtp_sender::RTCRtpSender;
use webrtc::rtp_transceiver::rtp_transceiver_direction::RTCRtpTransceiverDirection;
use webrtc::rtp_transceiver::RTCRtpTransceiverInit;
use webrtc::track::track_local::track_local_static_rtp::TrackLocalStaticRTP;
use webrtc::track::track_local::TrackLocal;
use webrtc::track::track_remote::TrackRemote;

use super::track_types::TrackSource;
use crate::ws::OutboundMsg;

/// Represents a user's WebRTC connection to the SFU.
///
/// Uses two `PeerConnection`s (dual-PC architecture) to cleanly separate
/// upstream (publish) and downstream (subscribe) media flows.
pub struct Peer {
    /// User ID.
    pub user_id: Uuid,
    /// Username.
    pub username: String,
    /// Display name.
    pub display_name: String,
    /// Channel ID the peer is connected to.
    pub channel_id: Uuid,

    /// Receives tracks FROM client (mic, screen, webcam). Client creates offers.
    pub publisher_pc: Arc<RTCPeerConnection>,
    /// Sends tracks TO client (other users' audio, screen shares). Server creates offers.
    pub subscriber_pc: Arc<RTCPeerConnection>,

    /// Tracks received from this peer (on `publisher_pc`).
    /// Map: `TrackSource` -> remote track
    pub incoming_tracks: RwLock<HashMap<TrackSource, Arc<TrackRemote>>>,
    /// Tracks being forwarded to this peer (on `subscriber_pc`).
    /// Map: `(source_user_id, source_type)` -> local track
    outgoing_tracks: RwLock<HashMap<(Uuid, TrackSource), Arc<TrackLocalStaticRTP>>>,

    /// Whether the user muted themselves.
    self_muted: RwLock<bool>,
    /// Whether a moderator muted the user. Set only by future moderation events.
    #[allow(dead_code)]
    server_muted: RwLock<bool>,
    /// Channel to send signaling messages back to the user.
    pub signal_tx: mpsc::Sender<OutboundMsg>,
    /// Unique session identifier for this connection.
    pub session_id: Uuid,
    /// Timestamp when this peer connected.
    pub connected_at: DateTime<Utc>,
    /// Pending track sources queued by the client before tracks arrive.
    /// The client sends e.g. `VoiceWebcamStart` before `addTrack()`, so the
    /// server can pop from this queue when `on_track` fires to identify the source.
    pending_track_sources: RwLock<Vec<TrackSource>>,
    /// Woken whenever a source is announced (queued), for a track that
    /// arrived before its announcement.
    announced: tokio::sync::Notify,
    /// Whether this peer has already subscribed to existing tracks.
    /// Used for robust first-offer detection instead of checking `outgoing_tracks_count`.
    pub has_subscribed: AtomicBool,
    /// The access epochs the join check let this peer in at (set once, by
    /// the join, before the peer enters its room). The owning socket writes
    /// room content only while they hold or after a new check
    /// (`ws::access`); a peer without it gets none.
    admission: std::sync::OnceLock<crate::ws::access::VoiceAdmission>,
    /// Offer/answer state of `subscriber_pc` (the server offers). See
    /// [`SubscriberNegotiation`]; only `SfuServer` touches it.
    pub(crate) negotiation: tokio::sync::Mutex<SubscriberNegotiation>,
    /// Set once the peer left its room or was replaced: its late track
    /// callbacks and forwarders must not touch the room any more.
    retired: AtomicBool,
    /// Whether an audio track was taken as this peer's microphone.
    microphone_claimed: AtomicBool,
    /// Login session of the socket that joined with this peer (set once by
    /// the join; `None` when unknown).
    auth_session: std::sync::OnceLock<Option<Uuid>>,
}

/// Offer/answer state of a peer's subscriber connection.
///
/// At most one server offer is outstanding: a renegotiation requested while
/// an offer waits for its answer only sets `again`, and the answer then
/// triggers exactly one new offer (coalescing). Held only over local
/// WebRTC work (create offer, set descriptions), never over a socket send.
#[derive(Debug, Default)]
pub struct SubscriberNegotiation {
    /// When the outstanding offer was sent; `None` when stable.
    pub awaiting_answer_since: Option<std::time::Instant>,
    /// Something changed while an offer was outstanding: offer again once
    /// the answer is in.
    pub again: bool,
}

impl Peer {
    /// Create a new peer with two WebRTC connections (publisher + subscriber).
    pub fn new(
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
            self_muted: RwLock::new(false),
            server_muted: RwLock::new(false),
            signal_tx,
            session_id: Uuid::now_v7(),
            connected_at: Utc::now(),
            pending_track_sources: RwLock::new(Vec::new()),
            announced: tokio::sync::Notify::new(),
            has_subscribed: AtomicBool::new(false),
            admission: std::sync::OnceLock::new(),
            negotiation: tokio::sync::Mutex::new(SubscriberNegotiation::default()),
            retired: AtomicBool::new(false),
            microphone_claimed: AtomicBool::new(false),
            auth_session: std::sync::OnceLock::new(),
        }
    }

    /// Record the login session of the joining socket (first call wins).
    pub fn set_auth_session(&self, session: Option<Uuid>) {
        let _ = self.auth_session.set(session);
    }

    /// Login session of the socket that holds this seat, if known.
    pub fn auth_session(&self) -> Option<Uuid> {
        self.auth_session.get().copied().flatten()
    }

    /// Mark the peer as gone from its room (left or replaced).
    pub fn retire(&self) {
        self.retired
            .store(true, std::sync::atomic::Ordering::Release);
    }

    /// Whether the peer left its room or was replaced.
    pub fn is_retired(&self) -> bool {
        self.retired.load(std::sync::atomic::Ordering::Acquire)
    }

    /// Record the access epochs of the join check (first call wins).
    pub fn set_admission(&self, admission: crate::ws::access::VoiceAdmission) {
        let _ = self.admission.set(admission);
    }

    /// The access epochs of the join check, if the peer came through it.
    pub fn admission(&self) -> Option<crate::ws::access::VoiceAdmission> {
        self.admission.get().copied()
    }

    /// Set an incoming track from this peer.
    pub async fn set_incoming_track(&self, source: TrackSource, track: Arc<TrackRemote>) {
        let mut incoming = self.incoming_tracks.write().await;
        incoming.insert(source, track);
    }

    /// Add an outgoing track to forward media from another user.
    /// The track is added to the subscriber `PeerConnection`.
    /// Returns the `RTCRtpSender` so callers can read RTCP feedback (e.g. REMB).
    pub async fn add_outgoing_track(
        &self,
        source_user_id: Uuid,
        source_type: TrackSource,
        track: Arc<TrackLocalStaticRTP>,
    ) -> Result<Arc<RTCRtpSender>, super::error::VoiceError> {
        // Always a new transceiver: `add_track` would reuse the transceiver
        // of a removed track with the same id (the same user rejoining), and
        // in webrtc-rs a removed (stopped) sender cannot take a track again,
        // so the re-add would fail. A removed track's transceiver stays
        // inactive in the session description.
        let transceiver = self
            .subscriber_pc
            .add_transceiver_from_track(
                track.clone() as Arc<dyn TrackLocal + Send + Sync>,
                Some(RTCRtpTransceiverInit {
                    direction: RTCRtpTransceiverDirection::Sendrecv,
                    send_encodings: Vec::new(),
                }),
            )
            .await?;
        let sender = transceiver.sender().await;

        // Store reference
        let mut tracks = self.outgoing_tracks.write().await;
        tracks.insert((source_user_id, source_type), track);

        Ok(sender)
    }

    /// Remove an outgoing track, also removing it from the subscriber peer connection.
    pub async fn remove_outgoing_track(
        &self,
        source_user_id: Uuid,
        source_type: TrackSource,
    ) -> bool {
        let mut tracks = self.outgoing_tracks.write().await;
        if let Some(track) = tracks.remove(&(source_user_id, source_type)) {
            // Remove from subscriber PeerConnection so the subscriber stops receiving it
            let senders = self.subscriber_pc.get_senders().await;
            for sender in senders {
                if let Some(t) = sender.track().await {
                    if t.id() == track.id() {
                        if let Err(e) = self.subscriber_pc.remove_track(&sender).await {
                            tracing::warn!(
                                user_id = %self.user_id,
                                error = %e,
                                "failed to remove track from subscriber PC"
                            );
                        }
                        break;
                    }
                }
            }
            true
        } else {
            false
        }
    }

    /// Remove every outgoing track that forwards media of `source_user_id`
    /// from the subscriber connection. Returns how many were removed.
    pub async fn remove_outgoing_tracks_from(&self, source_user_id: Uuid) -> usize {
        let sources: Vec<TrackSource> = self
            .outgoing_tracks
            .read()
            .await
            .keys()
            .filter(|(user, _)| *user == source_user_id)
            .map(|(_, source)| *source)
            .collect();
        let mut removed = 0;
        for source in sources {
            if self.remove_outgoing_track(source_user_id, source).await {
                removed += 1;
            }
        }
        removed
    }

    /// The local tracks currently forwarded to this peer.
    pub async fn outgoing_track_list(&self) -> Vec<Arc<TrackLocalStaticRTP>> {
        self.outgoing_tracks
            .read()
            .await
            .values()
            .cloned()
            .collect()
    }

    /// Returns the number of outgoing tracks on the subscriber `PeerConnection`.
    pub async fn outgoing_tracks_count(&self) -> usize {
        self.outgoing_tracks.read().await.len()
    }

    /// Enqueue an expected track source. Called before the client's `addTrack()`
    /// so that `on_track` can identify the source correctly.
    pub async fn push_pending_source(&self, source: TrackSource) {
        let mut pending = self.pending_track_sources.write().await;
        pending.push(source);
        drop(pending);
        self.announced.notify_waiters();
    }

    /// Drop queued announcements matching `pred` (their share or webcam
    /// ended before a track came).
    pub async fn remove_pending_sources(&self, pred: impl Fn(&TrackSource) -> bool) {
        self.pending_track_sources.write().await.retain(|s| !pred(s));
    }

    /// Claim the microphone: `true` for the first audio track of this peer
    /// only (a peer has exactly one microphone, announced by its join).
    pub fn claim_microphone(&self) -> bool {
        !self
            .microphone_claimed
            .swap(true, std::sync::atomic::Ordering::AcqRel)
    }

    /// Take the oldest announced source of the track's kind (video: screen
    /// share or webcam; audio: share audio); if none is queued, wait until
    /// `deadline` for an announcement. `None`: the track stays unannounced.
    ///
    /// A track is matched to an announcement by arrival order, whichever of
    /// the two comes first (contract C30).
    pub async fn take_source_until(
        &self,
        video: bool,
        deadline: tokio::time::Instant,
    ) -> Option<TrackSource> {
        loop {
            // Register for the wake-up before looking, so an announcement
            // between the look and the wait is not missed.
            let notified = self.announced.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            if let Some(source) = self.pop_pending(video).await {
                return Some(source);
            }
            if tokio::time::timeout_at(deadline, notified).await.is_err() {
                return self.pop_pending(video).await;
            }
        }
    }

    async fn pop_pending(&self, video: bool) -> Option<TrackSource> {
        if video {
            self.pop_pending_video_source().await
        } else {
            self.pop_pending_audio_source().await
        }
    }

    /// Dequeue the first pending video source, if any.
    pub async fn pop_pending_video_source(&self) -> Option<TrackSource> {
        let mut pending = self.pending_track_sources.write().await;
        pending
            .iter()
            .position(|s| s.is_video())
            .map(|pos| pending.remove(pos))
    }

    /// Dequeue the first pending audio source, if any.
    pub async fn pop_pending_audio_source(&self) -> Option<TrackSource> {
        let mut pending = self.pending_track_sources.write().await;
        pending
            .iter()
            .position(|s| s.is_audio())
            .map(|pos| pending.remove(pos))
    }

    /// Check if either peer connection is connected.
    pub fn is_connected(&self) -> bool {
        self.publisher_pc.connection_state() == RTCPeerConnectionState::Connected
            || self.subscriber_pc.connection_state() == RTCPeerConnectionState::Connected
    }

    /// Set the user's own mute state (does not affect `server_muted`).
    pub async fn set_self_muted(&self, muted: bool) {
        let mut m = self.self_muted.write().await;
        *m = muted;
    }

    /// Returns true if the user has muted themselves (ignores moderator mute).
    pub async fn is_self_muted(&self) -> bool {
        *self.self_muted.read().await
    }

    /// Returns true if either the user self-muted or a moderator muted them.
    pub async fn is_effectively_muted(&self) -> bool {
        *self.self_muted.read().await || *self.server_muted.read().await
    }

    /// Close both peer connections.
    pub async fn close(&self) {
        if let Err(e) = self.publisher_pc.close().await {
            tracing::warn!(user_id = %self.user_id, error = %e, "failed to close publisher PC");
        }
        if let Err(e) = self.subscriber_pc.close().await {
            tracing::warn!(user_id = %self.user_id, error = %e, "failed to close subscriber PC");
        }
    }
}
