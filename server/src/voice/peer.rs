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
    /// Inactive m-lines on `subscriber_pc`: transceivers of removed tracks,
    /// which webrtc-rs keeps (see [`Peer::add_outgoing_track`]).
    inactive_mlines: std::sync::atomic::AtomicUsize,
    /// Local tracks that forward this peer's media to others:
    /// `(subscriber, source, track)`. Removal of this peer's media touches
    /// exactly these, never a newer peer of the same user.
    forwarded: std::sync::Mutex<Vec<(Uuid, TrackSource, Arc<TrackLocalStaticRTP>)>>,
    /// Video source bound to each simulcast publisher transceiver (`mid`) by
    /// its first layer (`None`: that layer was refused). The other layers of
    /// the transceiver use it, nothing else does.
    video_mids: std::sync::Mutex<HashMap<String, Option<TrackSource>>>,
    /// Number of the latest accepted announcement per source (a timer of an
    /// older announcement of the same source acts on nothing).
    announcements: std::sync::Mutex<HashMap<TrackSource, u64>>,
    /// Held while state is added on behalf of this seat (forwarding a track,
    /// binding it to a source) and while the seat's media is removed; see
    /// the lock rules in `sfu.rs`.
    pub(crate) media: tokio::sync::Mutex<()>,
    /// When a key frame was last requested for each of this peer's video
    /// sources on behalf of its viewers.
    keyframe_requested: std::sync::Mutex<HashMap<TrackSource, tokio::time::Instant>>,
    /// Whether an audio track was taken as this peer's microphone.
    microphone_claimed: AtomicBool,
    /// Login session of the socket that joined with this peer (set once by
    /// the join; `None` when unknown).
    auth_session: std::sync::OnceLock<Option<Uuid>>,
}

impl Drop for Peer {
    fn drop(&mut self) {
        crate::observability::metrics::voice_inactive_mlines_released(
            self.inactive_mlines
                .load(std::sync::atomic::Ordering::Relaxed) as u64,
        );
    }
}

/// Inactive m-lines on one subscriber connection at which the server warns
/// once. Offers and answers grow by roughly 0.5–1 KiB per inactive m-line;
/// the inbound frame limit (256 KiB) is reached at several hundred.
pub const INACTIVE_MLINE_WARN_THRESHOLD: usize = 60;

/// Offer/answer state of a peer's subscriber connection.
///
/// At most one server offer is outstanding: a renegotiation requested while
/// an offer waits for its answer only sets `again`, and the answer then
/// triggers exactly one new offer (coalescing). Held only over local
/// WebRTC work (create offer, set descriptions), never over a socket send.
#[derive(Debug, Default)]
pub struct SubscriberNegotiation {
    /// When the outstanding offer was sent; `None` when stable.
    pub awaiting_answer_since: Option<tokio::time::Instant>,
    /// Something changed while an offer was outstanding: offer again once
    /// the answer is in.
    pub again: bool,
    /// Number of the latest offer (its watchdog compares it).
    pub offer_seq: u64,
    /// The open offer is already a repeat of an unanswered one.
    pub repeated: bool,
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
            forwarded: std::sync::Mutex::new(Vec::new()),
            video_mids: std::sync::Mutex::new(HashMap::new()),
            announcements: std::sync::Mutex::new(HashMap::new()),
            media: tokio::sync::Mutex::new(()),
            keyframe_requested: std::sync::Mutex::new(HashMap::new()),
            inactive_mlines: std::sync::atomic::AtomicUsize::new(0),
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
        // One forwarded track per source: the track handler and the
        // first-offer subscribe path may race to add the same source; the
        // second one is refused (its caller drops its router subscription).
        let mut tracks = self.outgoing_tracks.write().await;
        if tracks.contains_key(&(source_user_id, source_type)) {
            return Err(super::error::VoiceError::Internal(format!(
                "{source_user_id}:{source_type} is already forwarded to {}",
                self.user_id
            )));
        }

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
                        } else {
                            self.count_inactive_mline();
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

    /// Remove the outgoing track of `(source_user_id, source)` only if it is
    /// exactly `track` (a newer peer of the same user keeps its own).
    pub async fn remove_outgoing_track_if(
        &self,
        source_user_id: Uuid,
        source: TrackSource,
        track: &Arc<TrackLocalStaticRTP>,
    ) -> bool {
        let same = self
            .outgoing_tracks
            .read()
            .await
            .get(&(source_user_id, source))
            .is_some_and(|t| Arc::ptr_eq(t, track));
        same && self.remove_outgoing_track(source_user_id, source).await
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

    /// Count one more inactive m-line; warn once when a long session has
    /// collected many (each makes every later offer and answer bigger).
    fn count_inactive_mline(&self) {
        crate::observability::metrics::voice_inactive_mline_added();
        let n = self
            .inactive_mlines
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            + 1;
        if n == INACTIVE_MLINE_WARN_THRESHOLD {
            tracing::warn!(
                user_id = %self.user_id,
                channel_id = %self.channel_id,
                inactive_mlines = n,
                "Voice subscriber connection keeps many inactive m-lines; rejoining resets them"
            );
        }
    }

    /// Inactive m-lines on this peer's subscriber connection.
    pub fn inactive_mlines(&self) -> usize {
        self.inactive_mlines
            .load(std::sync::atomic::Ordering::Relaxed)
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
        self.pending_track_sources
            .write()
            .await
            .retain(|s| !pred(s));
    }

    /// Record that `track` forwards this peer's `source` to `subscriber`.
    pub fn record_forwarded(
        &self,
        subscriber: Uuid,
        source: TrackSource,
        track: Arc<TrackLocalStaticRTP>,
    ) {
        self.forwarded
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push((subscriber, source, track));
    }

    /// Take the list of tracks forwarding this peer's media (on removal).
    pub fn take_forwarded(&self) -> Vec<(Uuid, TrackSource, Arc<TrackLocalStaticRTP>)> {
        std::mem::take(
            &mut *self
                .forwarded
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        )
    }

    /// Forget forwarded tracks of `source` (the source ended).
    pub fn forget_forwarded(&self, source: TrackSource) {
        self.forwarded
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .retain(|(_, s, _)| *s != source);
    }

    /// Bind the simulcast publisher transceiver `mid` to `source` (`None`:
    /// its first layer was refused).
    pub fn bind_video_mid(&self, mid: &str, source: Option<TrackSource>) {
        self.video_mids
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(mid.to_owned(), source);
    }

    /// The binding of transceiver `mid`, if its first layer was decided.
    pub fn video_mid(&self, mid: &str) -> Option<Option<TrackSource>> {
        self.video_mids
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(mid)
            .copied()
    }

    /// Record a newly accepted announcement of `source` and return its number.
    pub fn announce(&self, source: TrackSource) -> u64 {
        let mut map = self
            .announcements
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let next = map.values().copied().max().unwrap_or(0) + 1;
        map.insert(source, next);
        next
    }

    /// Whether announcement `number` is still the latest one of `source`.
    pub fn is_current_announcement(&self, source: TrackSource, number: u64) -> bool {
        self.announcements
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(&source)
            == Some(&number)
    }

    /// Whether a viewer's key-frame request for `source` may go to the
    /// publisher now (at most one per second per source, all viewers
    /// together).
    pub fn allow_keyframe_request(&self, source: TrackSource) -> bool {
        let now = tokio::time::Instant::now();
        let mut last = self
            .keyframe_requested
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        match last.get(&source) {
            Some(at) if now.duration_since(*at) < std::time::Duration::from_secs(1) => false,
            _ => {
                last.insert(source, now);
                true
            }
        }
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
