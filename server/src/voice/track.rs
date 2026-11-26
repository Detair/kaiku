//! Track Routing and RTP Forwarding
//!
//! Manages RTP packet forwarding between participants in a voice room.
//! Uses `DashMap` for lock-free concurrent access in the RTP hot path.

use std::sync::Arc;
use std::time::{Duration, Instant};

use dashmap::DashMap;
use tokio::sync::mpsc;
use tracing::{debug, warn};
use uuid::Uuid;
use webrtc::rtp::packet::Packet as RtpPacket;
use webrtc::rtp_transceiver::rtp_codec::RTCRtpCodecCapability;
use webrtc::track::track_local::track_local_static_rtp::TrackLocalStaticRTP;
use webrtc::track::track_local::TrackLocalWriter;
use webrtc::track::track_remote::TrackRemote;

use super::error::VoiceError;
use super::peer::Peer;
use super::track_types::{Layer, LayerPreference, TrackSource};

/// Subscription info for a track.
#[derive(Clone)]
struct Subscription {
    /// The subscriber's user ID.
    subscriber_id: Uuid,
    /// The local track that forwards to the subscriber.
    local_track: Arc<TrackLocalStaticRTP>,
    /// Viewer's layer preference (auto or manual ceiling).
    preferred_layer: LayerPreference,
    /// Currently active simulcast layer for this subscription.
    active_layer: Layer,
    /// Last REMB bandwidth estimate (bps) from this subscriber.
    remb_estimate: u64,
    /// Timestamp of the last layer switch (for upgrade hysteresis).
    last_layer_change: Instant,
}

// ---------------------------------------------------------------------------
// Simulcast layer selection
// ---------------------------------------------------------------------------

/// REMB threshold (bps) at or above which we select [`Layer::High`].
const REMB_THRESHOLD_HIGH: u64 = 1_500_000;

/// REMB threshold (bps) at or above which we select [`Layer::Medium`].
const REMB_THRESHOLD_MEDIUM: u64 = 400_000;

/// Minimum time between layer *upgrades* to prevent oscillation.
const UPGRADE_HYSTERESIS: Duration = Duration::from_secs(3);

/// Select the best simulcast layer given a preference and bandwidth estimate.
const fn select_layer(pref: LayerPreference, remb: u64) -> Layer {
    let bandwidth_layer = if remb >= REMB_THRESHOLD_HIGH {
        Layer::High
    } else if remb >= REMB_THRESHOLD_MEDIUM {
        Layer::Medium
    } else {
        Layer::Low
    };
    match pref.layer() {
        Some(ceiling) => layer_min(ceiling, bandwidth_layer),
        None => bandwidth_layer,
    }
}

/// Numeric ordering for layers (higher = better quality).
const fn layer_order(l: Layer) -> u8 {
    match l {
        Layer::High => 2,
        Layer::Medium => 1,
        Layer::Low => 0,
    }
}

/// Return the lower-quality of two layers.
const fn layer_min(a: Layer, b: Layer) -> Layer {
    if layer_order(a) <= layer_order(b) {
        a
    } else {
        b
    }
}

/// Check whether an upgrade from `current` to `target` is allowed given hysteresis.
fn should_upgrade(current: Layer, target: Layer, last_change: Instant) -> bool {
    layer_order(target) > layer_order(current) && last_change.elapsed() >= UPGRADE_HYSTERESIS
}

/// Check whether a downgrade from `current` to `target` should happen (always immediate).
const fn should_downgrade(current: Layer, target: Layer) -> bool {
    layer_order(target) < layer_order(current)
}

/// A forwarding task's reusable list of where a packet goes.
pub type ForwardTargets = Vec<(Uuid, Arc<TrackLocalStaticRTP>)>;

/// Manages RTP packet forwarding between participants.
///
/// Uses `DashMap` for lock-free concurrent access, which is critical for the
/// RTP hot path that processes ~50 packets/second per participant.
pub struct TrackRouter {
    /// Map: `(source_user_id, source_type)` -> list of subscriptions
    /// Using `DashMap` to avoid lock contention in the RTP forwarding hot path.
    subscriptions: DashMap<(Uuid, TrackSource), Vec<Subscription>>,
    /// Simulcast layers: `(source_user_id, source_type, layer)` -> remote track.
    /// Populated when the SFU receives tracks with an RID (simulcast layers).
    simulcast_tracks: DashMap<(Uuid, TrackSource, Layer), Arc<TrackRemote>>,
}

/// Numbers forwarded track instances (process-wide).
static FORWARDED_TRACKS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// A local track that forwards `source_type` of `source_user_id`.
///
/// The stream id is `{source_user_id}:{source_type}` (e.g. `uuid:microphone`,
/// `uuid:screen_video:stream_uuid`, `uuid:webcam`): clients identify the
/// person and the source by it. Each forwarded track has its own track id,
/// `{stream id}:{n}`, so a listener's session never lists the same stream
/// and track id twice when a person's source comes back (a rejoin, another
/// device, the camera off and on).
pub fn forwarded_track(
    source_user_id: Uuid,
    source_type: TrackSource,
    codec: RTCRtpCodecCapability,
) -> Arc<TrackLocalStaticRTP> {
    let stream_id = format!("{source_user_id}:{source_type}");
    let n = FORWARDED_TRACKS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    Arc::new(TrackLocalStaticRTP::new(
        codec,
        format!("{stream_id}:{n}"),
        stream_id,
    ))
}

impl TrackRouter {
    /// Create a new track router.
    pub fn new() -> Self {
        Self {
            subscriptions: DashMap::new(),
            simulcast_tracks: DashMap::new(),
        }
    }

    /// Create a local track for forwarding media from source to subscriber.
    ///
    /// Returns the local track that should be added to the subscriber's peer connection.
    pub fn create_subscriber_track(
        &self,
        source_user_id: Uuid,
        source_type: TrackSource,
        subscriber: &Peer,
        source_track: &TrackRemote,
    ) -> Result<Arc<TrackLocalStaticRTP>, VoiceError> {
        // A local track with the same codec as the source
        let codec = RTCRtpCodecCapability {
            mime_type: source_track.codec().capability.mime_type,
            clock_rate: source_track.codec().capability.clock_rate,
            channels: source_track.codec().capability.channels,
            sdp_fmtp_line: source_track.codec().capability.sdp_fmtp_line,
            rtcp_feedback: vec![],
        };
        Ok(self.subscribe_with_codec(source_user_id, source_type, subscriber, codec))
    }

    /// The part of [`Self::create_subscriber_track`] after the codec is
    /// known: the forwarded track and its subscription.
    pub(crate) fn subscribe_with_codec(
        &self,
        source_user_id: Uuid,
        source_type: TrackSource,
        subscriber: &Peer,
        codec: RTCRtpCodecCapability,
    ) -> Arc<TrackLocalStaticRTP> {
        let local_track = forwarded_track(source_user_id, source_type, codec);

        // Store subscription (default: auto layer selection, assume high bandwidth)
        let subscription = Subscription {
            subscriber_id: subscriber.user_id,
            local_track: local_track.clone(),
            preferred_layer: LayerPreference::Auto,
            active_layer: Layer::High,
            remb_estimate: u64::MAX,
            last_layer_change: Instant::now(),
        };

        self.subscriptions
            .entry((source_user_id, source_type))
            .or_default()
            .push(subscription);

        debug!(
            source = %source_user_id,
            source_type = ?source_type,
            subscriber = %subscriber.user_id,
            "Created subscriber track"
        );

        local_track
    }

    /// Forward an RTP packet from source to all subscribers whose active layer matches.
    ///
    /// This is the hot path called ~50 times/second per participant.
    /// Uses `DashMap` for lock-free concurrent reads to avoid contention.
    /// The `layer` parameter indicates which simulcast layer this packet belongs to.
    /// `targets` is the forwarding task's own buffer, reused for every packet
    /// (no allocation per packet once it has grown to the room's size).
    pub async fn forward_rtp(
        &self,
        source_user_id: Uuid,
        source_type: TrackSource,
        layer: Layer,
        rtp_packet: &RtpPacket,
        targets: &mut ForwardTargets,
    ) {
        // Check if this source has multiple simulcast layers registered.
        // Non-simulcast sources (e.g., screen share via addTrack without RID)
        // only have a single layer — forward to all subscribers regardless of
        // their active_layer setting to prevent REMB-based layer selection
        // from dropping packets when no alternative layers exist.
        let is_simulcast =
            self.simulcast_tracks
                .contains_key(&(source_user_id, source_type, Layer::Low))
                || self.simulcast_tracks.contains_key(&(
                    source_user_id,
                    source_type,
                    Layer::Medium,
                ));

        self.collect_targets((source_user_id, source_type), layer, is_simulcast, targets);
        for (subscriber_id, local_track) in targets.iter() {
            // Write RTP packet to local track (forwards to subscriber)
            if let Err(e) = local_track.write_rtp(rtp_packet).await {
                warn!(
                    subscriber = %subscriber_id,
                    error = %e,
                    "Failed to forward RTP packet"
                );
            }
        }
        let forwarded = targets.len();
        // Keep the capacity, not the tracks.
        targets.clear();
        if forwarded > 0 {
            crate::observability::metrics::record_rtp_packet_forwarded();
        }
    }

    /// Put the subscribers of `key` that get a packet of `layer` into
    /// `targets` (cleared first). Collected while the map's guard is held;
    /// the caller writes after it is released, so a slow write never holds
    /// the shard (a subscribe or removal on it would block its worker).
    fn collect_targets(
        &self,
        key: (Uuid, TrackSource),
        layer: Layer,
        is_simulcast: bool,
        targets: &mut ForwardTargets,
    ) {
        targets.clear();
        if let Some(subscribers) = self.subscriptions.get(&key) {
            targets.extend(
                subscribers
                    .value()
                    .iter()
                    .filter(|sub| sub.active_layer == layer || !is_simulcast)
                    .map(|sub| (sub.subscriber_id, sub.local_track.clone())),
            );
        }
    }

    /// Count subscribers for a specific source track (for debugging).
    pub fn subscription_count(&self, source_user_id: Uuid, source_type: TrackSource) -> usize {
        let key = (source_user_id, source_type);
        self.subscriptions.get(&key).map(|s| s.len()).unwrap_or(0)
    }

    /// Remove a subscriber from a specific source track.
    pub async fn remove_subscriber(
        &self,
        source_user_id: Uuid,
        source_type: TrackSource,
        subscriber_id: Uuid,
    ) {
        let key = (source_user_id, source_type);

        // Use entry API for atomic check-and-modify
        if let Some(mut entry) = self.subscriptions.get_mut(&key) {
            entry.retain(|s| s.subscriber_id != subscriber_id);

            // Check if we should remove the entry (do this outside the guard)
            if entry.is_empty() {
                drop(entry); // Release the mutable reference first
                self.subscriptions.remove(&key);
            }
        }

        debug!(
            source = %source_user_id,
            source_type = ?source_type,
            subscriber = %subscriber_id,
            "Removed subscriber"
        );
    }

    /// Remove all subscriptions for a source user (all tracks).
    pub async fn remove_source(&self, source_user_id: Uuid) {
        // Remove all keys where the tuple starts with source_user_id
        self.subscriptions
            .retain(|(uid, _), _| *uid != source_user_id);

        // Clean up simulcast tracks for this source
        self.simulcast_tracks
            .retain(|(uid, _, _), _| *uid != source_user_id);

        debug!(source = %source_user_id, "Removed source and all subscriptions");
    }

    /// Remove all subscriptions for a specific source track (e.g. when a user stops webcam).
    pub async fn remove_source_track(&self, source_user_id: Uuid, source_type: TrackSource) {
        self.subscriptions.remove(&(source_user_id, source_type));

        // Clean up simulcast layers for this specific track
        self.simulcast_tracks
            .retain(|(uid, st, _), _| !(*uid == source_user_id && *st == source_type));

        debug!(
            source = %source_user_id,
            source_type = ?source_type,
            "Removed source track and all subscriptions"
        );
    }

    /// Remove a subscriber from all sources (when subscriber leaves).
    pub async fn remove_subscriber_from_all(&self, subscriber_id: Uuid) {
        // First pass: remove subscriber from all entries
        for mut entry in self.subscriptions.iter_mut() {
            entry.retain(|s| s.subscriber_id != subscriber_id);
        }

        // Second pass: clean up empty entries
        self.subscriptions.retain(|_, v| !v.is_empty());

        debug!(subscriber = %subscriber_id, "Removed subscriber from all sources");
    }

    /// Remove the subscriptions that write into `tracks` (the local tracks
    /// of one subscriber peer). Matched by track identity, so a newer peer
    /// of the same user keeps its own subscriptions.
    pub fn remove_subscriptions_into(&self, tracks: &[Arc<TrackLocalStaticRTP>]) {
        if tracks.is_empty() {
            return;
        }
        for mut entry in self.subscriptions.iter_mut() {
            entry.retain(|s| !tracks.iter().any(|t| Arc::ptr_eq(t, &s.local_track)));
        }
        self.subscriptions.retain(|_, v| !v.is_empty());
    }

    /// Remove the simulcast layers that are exactly `tracks` (one seat's
    /// layers; a newer seat of the same user keeps its own).
    pub fn remove_simulcast_tracks(&self, tracks: &[Arc<TrackRemote>]) {
        if tracks.is_empty() {
            return;
        }
        self.simulcast_tracks
            .retain(|_, t| !tracks.iter().any(|x| Arc::ptr_eq(x, t)));
    }

    /// The layer `subscriber_id` currently receives of `source_user_id`'s
    /// `source_type`, if subscribed.
    pub fn active_layer(
        &self,
        subscriber_id: Uuid,
        source_user_id: Uuid,
        source_type: TrackSource,
    ) -> Option<Layer> {
        self.subscriptions
            .get(&(source_user_id, source_type))?
            .iter()
            .find(|s| s.subscriber_id == subscriber_id)
            .map(|s| s.active_layer)
    }

    /// Store a simulcast layer of a source.
    pub fn store_simulcast_track(
        &self,
        user_id: Uuid,
        source_type: TrackSource,
        layer: Layer,
        track: Arc<TrackRemote>,
    ) {
        self.simulcast_tracks
            .insert((user_id, source_type, layer), track);
    }

    /// Get the number of subscribers for a source.
    pub fn subscriber_count(&self, source_user_id: Uuid, source_type: TrackSource) -> usize {
        self.subscriptions
            .get(&(source_user_id, source_type))
            .map_or(0, |entry| entry.value().len())
    }

    /// Update REMB bandwidth estimate for a subscriber and potentially switch layers.
    ///
    /// Returns `Some(new_layer)` if the active layer changed, `None` otherwise.
    /// Downgrades are immediate; upgrades require [`UPGRADE_HYSTERESIS`] to have elapsed
    /// since the last layer change to prevent oscillation.
    pub fn update_remb(
        &self,
        subscriber_id: Uuid,
        source_user_id: Uuid,
        source_type: TrackSource,
        remb_bps: u64,
    ) -> Option<Layer> {
        // A source without layers (one stream) has nothing to select.
        if !self
            .simulcast_tracks
            .iter()
            .any(|e| e.key().0 == source_user_id && e.key().1 == source_type)
        {
            return None;
        }
        let key = (source_user_id, source_type);
        let mut changed = None;
        if let Some(mut subs) = self.subscriptions.get_mut(&key) {
            for sub in subs.iter_mut() {
                if sub.subscriber_id == subscriber_id {
                    sub.remb_estimate = remb_bps;
                    let target = select_layer(sub.preferred_layer, remb_bps);
                    if target != sub.active_layer {
                        let switch = if should_downgrade(sub.active_layer, target) {
                            true
                        } else {
                            should_upgrade(sub.active_layer, target, sub.last_layer_change)
                        };
                        if switch {
                            sub.active_layer = target;
                            sub.last_layer_change = Instant::now();
                            changed = Some(target);
                        }
                    }
                }
            }
        }
        changed
    }

    /// Set a viewer's layer preference for a specific track subscription.
    ///
    /// Returns `Some(new_layer)` if the active layer changed, `None` otherwise.
    /// Layer changes from manual preference are applied immediately (no hysteresis).
    pub fn set_layer_preference(
        &self,
        subscriber_id: Uuid,
        source_user_id: Uuid,
        source_type: TrackSource,
        pref: LayerPreference,
    ) -> Option<Layer> {
        // A source without layers (one stream) has nothing to switch: the
        // choice is kept, no layer change happens or is reported.
        let has_layers = self
            .simulcast_tracks
            .iter()
            .any(|e| e.key().0 == source_user_id && e.key().1 == source_type);
        let key = (source_user_id, source_type);
        let mut changed = None;
        if let Some(mut subs) = self.subscriptions.get_mut(&key) {
            for sub in subs.iter_mut() {
                if sub.subscriber_id == subscriber_id {
                    sub.preferred_layer = pref;
                    if !has_layers {
                        continue;
                    }
                    let target = select_layer(pref, sub.remb_estimate);
                    if target != sub.active_layer {
                        sub.active_layer = target;
                        sub.last_layer_change = Instant::now();
                        changed = Some(target);
                    }
                }
            }
        }
        changed
    }
}

impl Default for TrackRouter {
    fn default() -> Self {
        Self::new()
    }
}

/// Spawn a task to read RTP packets from a track and forward them.
///
/// The `layer` parameter identifies which simulcast layer this track carries.
/// Non-simulcast tracks should pass [`Layer::High`].
pub fn spawn_rtp_forwarder(
    source_user_id: Uuid,
    source_type: TrackSource,
    layer: Layer,
    track: Arc<TrackRemote>,
    router: Arc<TrackRouter>,
    peer: Arc<Peer>,
    ended: Arc<std::sync::atomic::AtomicBool>,
) {
    tokio::spawn(async move {
        // MTU size.
        let mut buf = vec![0u8; 1500];
        // Where each packet goes; reused for every packet of this track.
        let mut targets = ForwardTargets::new();
        let mut packet_count: u64 = 0;

        loop {
            match track.read(&mut buf).await {
                Ok((packet, _attributes)) => {
                    packet_count += 1;

                    // This forwarder serves only the binding it was started
                    // for. When the source ended (stop, expiry, leave,
                    // replacement) or the peer left or was replaced, the
                    // same key may belong to a later source or a new peer of
                    // the user.
                    if ended.load(std::sync::atomic::Ordering::Acquire) || peer.is_retired() {
                        break;
                    }

                    // Drop audio packets from muted peers. Video tracks (screen share,
                    // webcam) are not affected by mute state.
                    if source_type == TrackSource::Microphone && peer.is_effectively_muted().await {
                        continue;
                    }
                    // Again after the mute check, which may wait.
                    if ended.load(std::sync::atomic::Ordering::Acquire) || peer.is_retired() {
                        break;
                    }

                    if packet_count == 1 || packet_count.is_multiple_of(500) {
                        let sub_count = router.subscription_count(source_user_id, source_type);
                        debug!(
                            source = %source_user_id,
                            source_type = ?source_type,
                            layer = ?layer,
                            packet_count = packet_count,
                            subscribers = sub_count,
                            "RTP forwarder active"
                        );
                    }
                    // Forward the RTP packet to all subscribers whose active layer matches.
                    router
                        .forward_rtp(source_user_id, source_type, layer, &packet, &mut targets)
                        .await;
                }
                Err(e) => {
                    debug!(
                        source = %source_user_id,
                        source_type = ?source_type,
                        layer = ?layer,
                        error = %e,
                        "Track read ended"
                    );
                    break;
                }
            }
        }

        // The forwarder stops at the first packet after its binding ended,
        // or when track.read() returns an error (publisher removed the track
        // or PC closed).
        debug!(
             source = %source_user_id,
             source_type = ?source_type,
             layer = ?layer,
             "RTP forwarder stopped"
        );
    });
}

/// Spawn a task that reads the RTCP feedback of a subscriber's outgoing
/// sender: REMB updates the subscription's layer (and tells the subscriber
/// when it changes); a key-frame request (PLI or FIR) goes to the publisher
/// of the track as a PLI for `media_ssrc`, at most once per second per track
/// across all its viewers.
#[allow(clippy::too_many_arguments)]
pub fn spawn_subscriber_rtcp_reader(
    publisher: std::sync::Weak<Peer>,
    media_ssrc: u32,
    track_router: Arc<TrackRouter>,
    subscriber_id: Uuid,
    source_user_id: Uuid,
    source_type: TrackSource,
    sender: Arc<webrtc::rtp_transceiver::rtp_sender::RTCRtpSender>,
    signal_tx: mpsc::Sender<crate::ws::OutboundMsg>,
    channel_id: Uuid,
) {
    tokio::spawn(async move {
        use webrtc::rtcp::payload_feedbacks::full_intra_request::FullIntraRequest;
        use webrtc::rtcp::payload_feedbacks::picture_loss_indication::PictureLossIndication;
        use webrtc::rtcp::payload_feedbacks::receiver_estimated_maximum_bitrate::ReceiverEstimatedMaximumBitrate;

        while let Ok((packets, _)) = sender.read_rtcp().await {
            let wants_keyframe = packets.iter().any(|p| {
                let any = p.as_any();
                any.downcast_ref::<PictureLossIndication>().is_some()
                    || any.downcast_ref::<FullIntraRequest>().is_some()
            });
            if wants_keyframe {
                if let Some(publisher) = publisher.upgrade() {
                    if !publisher.is_retired() && publisher.allow_keyframe_request(source_type) {
                        // The layer this viewer receives (simulcast), else the
                        // track's own SSRC.
                        let layer =
                            track_router.active_layer(subscriber_id, source_user_id, source_type);
                        let ssrc = layer
                            .and_then(|layer| {
                                publisher
                                    .simulcast_ssrcs(source_type)
                                    .into_iter()
                                    .find(|(l, _)| *l == layer)
                                    .map(|(_, ssrc)| ssrc)
                            })
                            .unwrap_or(media_ssrc);
                        let _ = crate::voice::sfu::request_keyframe(&publisher, &[ssrc]).await;
                    }
                }
            }
            for pkt in &packets {
                if let Some(remb) = pkt
                    .as_any()
                    .downcast_ref::<ReceiverEstimatedMaximumBitrate>()
                {
                    if !remb.bitrate.is_finite() || remb.bitrate < 0.0 {
                        continue;
                    }
                    let bitrate = remb.bitrate as u64;
                    tracing::trace!(
                        subscriber = %subscriber_id,
                        source = %source_user_id,
                        source_type = ?source_type,
                        bitrate,
                        "Subscriber REMB received"
                    );

                    if let Some(new_layer) = track_router.update_remb(
                        subscriber_id,
                        source_user_id,
                        source_type,
                        bitrate,
                    ) {
                        tracing::info!(
                            subscriber = %subscriber_id,
                            source = %source_user_id,
                            source_type = ?source_type,
                            new_layer = ?new_layer,
                            bitrate,
                            "Auto-switched simulcast layer"
                        );

                        let _ = signal_tx
                            .send(crate::ws::OutboundMsg::voice(
                                channel_id,
                                crate::ws::ServerEvent::VoiceLayerChanged {
                                    channel_id,
                                    source_user_id,
                                    track_source: source_type,
                                    active_layer: new_layer,
                                },
                            ))
                            .await;
                    }
                }
            }
        }

        tracing::debug!(
            subscriber = %subscriber_id,
            source = %source_user_id,
            source_type = ?source_type,
            "Subscriber RTCP reader stopped"
        );
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    // =========================================================================
    // TrackRouter Construction Tests
    // =========================================================================

    #[test]
    fn test_track_router_new() {
        let router = TrackRouter::new();
        // Router should be empty initially
        assert!(router.subscriptions.is_empty());
    }

    #[test]
    fn test_track_router_default() {
        let router = TrackRouter::default();
        assert!(router.subscriptions.is_empty());
    }

    // =========================================================================
    // Empty Router Tests
    // =========================================================================

    #[tokio::test]
    async fn test_subscriber_count_empty_router() {
        let router = TrackRouter::new();
        let user_id = Uuid::new_v4();

        // All track sources should have 0 subscribers on empty router
        assert_eq!(router.subscriber_count(user_id, TrackSource::Microphone), 0);
        assert_eq!(
            router.subscriber_count(user_id, TrackSource::ScreenVideo(Uuid::nil())),
            0
        );
        assert_eq!(
            router.subscriber_count(user_id, TrackSource::ScreenAudio(Uuid::nil())),
            0
        );
        assert_eq!(router.subscriber_count(user_id, TrackSource::Webcam), 0);
    }

    #[tokio::test]
    async fn test_remove_subscriber_empty_router_does_not_panic() {
        let router = TrackRouter::new();
        let source_id = Uuid::new_v4();
        let subscriber_id = Uuid::new_v4();

        // Should not panic on empty router
        router
            .remove_subscriber(source_id, TrackSource::Microphone, subscriber_id)
            .await;
        router
            .remove_subscriber(
                source_id,
                TrackSource::ScreenVideo(Uuid::nil()),
                subscriber_id,
            )
            .await;
    }

    #[tokio::test]
    async fn test_remove_source_empty_router_does_not_panic() {
        let router = TrackRouter::new();
        let source_id = Uuid::new_v4();

        // Should not panic on empty router
        router.remove_source(source_id).await;
    }

    #[tokio::test]
    async fn test_remove_subscriber_from_all_empty_router_does_not_panic() {
        let router = TrackRouter::new();
        let subscriber_id = Uuid::new_v4();

        // Should not panic on empty router
        router.remove_subscriber_from_all(subscriber_id).await;
    }

    // =========================================================================
    // Forward RTP Tests (edge cases)
    // =========================================================================

    #[tokio::test]
    async fn test_forward_rtp_no_subscribers_does_not_panic() {
        let router = TrackRouter::new();
        let source_id = Uuid::new_v4();

        // Create a minimal RTP packet
        let rtp_packet = RtpPacket {
            header: webrtc::rtp::header::Header {
                version: 2,
                padding: false,
                extension: false,
                marker: false,
                payload_type: 96,
                sequence_number: 1,
                timestamp: 0,
                ssrc: 12345,
                csrc: vec![],
                extension_profile: 0,
                extensions: vec![],
                extensions_padding: 0,
            },
            payload: bytes::Bytes::from_static(&[0u8; 160]),
        };

        // Should not panic when no subscribers exist
        router
            .forward_rtp(
                source_id,
                TrackSource::Microphone,
                Layer::High,
                &rtp_packet,
                &mut Vec::new(),
            )
            .await;
        router
            .forward_rtp(
                source_id,
                TrackSource::ScreenVideo(Uuid::nil()),
                Layer::High,
                &rtp_packet,
                &mut Vec::new(),
            )
            .await;
    }

    // =========================================================================
    // Concurrent Access Tests (DashMap should handle these without deadlocks)
    // =========================================================================

    #[tokio::test]
    async fn test_concurrent_subscriber_count_reads() {
        let router = Arc::new(TrackRouter::new());
        let user_id = Uuid::new_v4();

        // Spawn multiple concurrent reads - DashMap handles this lock-free
        let mut handles = vec![];
        for _ in 0..10 {
            let router_clone = router.clone();
            handles.push(tokio::spawn(async move {
                router_clone.subscriber_count(user_id, TrackSource::Microphone)
            }));
        }

        // All should complete without deadlock
        for handle in handles {
            let count = handle.await.unwrap();
            assert_eq!(count, 0);
        }
    }

    #[tokio::test]
    async fn test_concurrent_remove_operations() {
        let router = Arc::new(TrackRouter::new());

        // Spawn multiple concurrent remove operations - DashMap handles concurrent writes
        let mut handles = vec![];
        for i in 0..10 {
            let router_clone = router.clone();
            let source_id = Uuid::new_v4();
            let subscriber_id = Uuid::new_v4();

            handles.push(tokio::spawn(async move {
                if i % 3 == 0 {
                    router_clone.remove_source(source_id).await;
                } else if i % 3 == 1 {
                    router_clone
                        .remove_subscriber(
                            source_id,
                            TrackSource::ScreenVideo(Uuid::nil()),
                            subscriber_id,
                        )
                        .await;
                } else {
                    router_clone.remove_subscriber_from_all(subscriber_id).await;
                }
            }));
        }

        // All should complete without deadlock or panic
        for handle in handles {
            handle.await.unwrap();
        }
    }
}

#[cfg(test)]
mod simulcast_tests {
    use super::*;

    /// Two packets through `forward_rtp` with one buffer: the buffer holds
    /// nothing after each packet, and a subscriber removed between them is
    /// not written to (nor kept) for the second.
    #[tokio::test]
    async fn forward_rtp_reuses_its_buffer_without_stale_targets() {
        let router = TrackRouter::new();
        let source = Uuid::new_v4();
        let (a, b) = (Uuid::new_v4(), Uuid::new_v4());
        let key = (source, TrackSource::Microphone);
        let track_of = |id: &str| {
            Arc::new(TrackLocalStaticRTP::new(
                RTCRtpCodecCapability {
                    mime_type: "audio/opus".into(),
                    clock_rate: 48_000,
                    channels: 2,
                    ..Default::default()
                },
                id.into(),
                "s".into(),
            ))
        };
        let removed_track = track_of("b");
        for (viewer, track) in [(a, track_of("a")), (b, removed_track.clone())] {
            router
                .subscriptions
                .entry(key)
                .or_default()
                .push(Subscription {
                    subscriber_id: viewer,
                    local_track: track,
                    preferred_layer: LayerPreference::Auto,
                    active_layer: Layer::High,
                    remb_estimate: u64::MAX,
                    last_layer_change: Instant::now(),
                });
        }
        let packet = RtpPacket {
            header: webrtc::rtp::header::Header {
                version: 2,
                payload_type: 111,
                ssrc: 1,
                ..Default::default()
            },
            payload: bytes::Bytes::from_static(&[0u8; 20]),
        };
        let mut targets = ForwardTargets::new();

        router
            .forward_rtp(
                source,
                TrackSource::Microphone,
                Layer::High,
                &packet,
                &mut targets,
            )
            .await;
        assert!(targets.is_empty(), "targets kept after the first packet");

        router
            .remove_subscriber(source, TrackSource::Microphone, b)
            .await;
        router
            .forward_rtp(
                source,
                TrackSource::Microphone,
                Layer::High,
                &packet,
                &mut targets,
            )
            .await;
        assert!(targets.is_empty(), "targets kept after the second packet");
        assert_eq!(
            Arc::strong_count(&removed_track),
            1,
            "the removed subscriber's track is still held for forwarding"
        );
    }

    /// Forwarding reaches every subscriber of the packet's layer and none of
    /// another layer (with layers), and a subscriber removed between two
    /// packets gets nothing more; the task's buffer is reused.
    #[tokio::test]
    async fn forwarding_targets_follow_layers_and_removals() {
        let router = TrackRouter::new();
        let source = Uuid::new_v4();
        let (a, b, c) = (Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4());
        let key = (source, TrackSource::Webcam);
        for (viewer, layer) in [(a, Layer::High), (b, Layer::Low), (c, Layer::High)] {
            router
                .subscriptions
                .entry(key)
                .or_default()
                .push(Subscription {
                    subscriber_id: viewer,
                    local_track: Arc::new(TrackLocalStaticRTP::new(
                        RTCRtpCodecCapability {
                            mime_type: "video/VP8".into(),
                            clock_rate: 90_000,
                            ..Default::default()
                        },
                        "t".into(),
                        "s".into(),
                    )),
                    preferred_layer: LayerPreference::Auto,
                    active_layer: layer,
                    remb_estimate: u64::MAX,
                    last_layer_change: Instant::now(),
                });
        }
        let ids = |t: &ForwardTargets| t.iter().map(|(id, _)| *id).collect::<Vec<_>>();
        let mut targets = ForwardTargets::new();

        router.collect_targets(key, Layer::High, true, &mut targets);
        assert_eq!(
            ids(&targets),
            vec![a, c],
            "the high layer's subscribers only"
        );
        router.collect_targets(key, Layer::Low, false, &mut targets);
        assert_eq!(ids(&targets), vec![a, b, c], "one stream: everyone");

        router
            .remove_subscriber(source, TrackSource::Webcam, c)
            .await;
        router.collect_targets(key, Layer::High, true, &mut targets);
        assert_eq!(
            ids(&targets),
            vec![a],
            "a removed subscriber gets nothing more"
        );
    }

    /// A viewer's own layer choice on a source without layers reports no
    /// layer change (it receives the one stream either way).
    #[test]
    fn a_layer_choice_on_a_source_without_layers_reports_nothing() {
        let router = TrackRouter::new();
        let (source, viewer) = (Uuid::new_v4(), Uuid::new_v4());
        router
            .subscriptions
            .entry((source, TrackSource::Webcam))
            .or_default()
            .push(Subscription {
                subscriber_id: viewer,
                local_track: Arc::new(TrackLocalStaticRTP::new(
                    RTCRtpCodecCapability {
                        mime_type: "video/VP8".into(),
                        clock_rate: 90_000,
                        ..Default::default()
                    },
                    "t".into(),
                    "s".into(),
                )),
                preferred_layer: LayerPreference::Auto,
                active_layer: Layer::High,
                remb_estimate: u64::MAX,
                last_layer_change: Instant::now(),
            });
        assert_eq!(
            router.set_layer_preference(viewer, source, TrackSource::Webcam, LayerPreference::Low),
            None
        );
    }

    /// A source without simulcast layers (one stream, e.g. a screen share)
    /// gets no layer selection from a viewer's bandwidth estimate: no layer
    /// change is reported for it.
    #[test]
    fn remb_selects_no_layer_for_a_source_without_layers() {
        let router = TrackRouter::new();
        let (source, viewer) = (Uuid::new_v4(), Uuid::new_v4());
        router
            .subscriptions
            .entry((source, TrackSource::Webcam))
            .or_default()
            .push(Subscription {
                subscriber_id: viewer,
                local_track: Arc::new(TrackLocalStaticRTP::new(
                    RTCRtpCodecCapability {
                        mime_type: "video/VP8".into(),
                        clock_rate: 90_000,
                        ..Default::default()
                    },
                    "t".into(),
                    "s".into(),
                )),
                preferred_layer: LayerPreference::Auto,
                active_layer: Layer::High,
                remb_estimate: u64::MAX,
                last_layer_change: Instant::now(),
            });
        assert_eq!(
            router.update_remb(viewer, source, TrackSource::Webcam, 300_000),
            None
        );
    }

    #[test]
    fn test_select_layer_auto_high_bandwidth() {
        assert_eq!(select_layer(LayerPreference::Auto, 2_000_000), Layer::High);
    }

    #[test]
    fn test_select_layer_auto_medium_bandwidth() {
        assert_eq!(select_layer(LayerPreference::Auto, 800_000), Layer::Medium);
    }

    #[test]
    fn test_select_layer_auto_low_bandwidth() {
        assert_eq!(select_layer(LayerPreference::Auto, 200_000), Layer::Low);
    }

    #[test]
    fn test_select_layer_manual_ceiling() {
        assert_eq!(
            select_layer(LayerPreference::Medium, 2_000_000),
            Layer::Medium
        );
    }

    #[test]
    fn test_select_layer_manual_drops_below_ceiling() {
        assert_eq!(select_layer(LayerPreference::High, 200_000), Layer::Low);
    }

    #[test]
    fn test_hysteresis_prevents_immediate_upgrade() {
        let recent = Instant::now().checked_sub(Duration::from_secs(1)).unwrap();
        assert!(!should_upgrade(Layer::Medium, Layer::High, recent));
    }

    #[test]
    fn test_hysteresis_allows_upgrade_after_delay() {
        let old = Instant::now().checked_sub(Duration::from_secs(4)).unwrap();
        assert!(should_upgrade(Layer::Medium, Layer::High, old));
    }

    #[test]
    fn test_downgrade_is_immediate() {
        assert!(should_downgrade(Layer::High, Layer::Low));
        assert!(!should_downgrade(Layer::Low, Layer::High));
    }

    #[test]
    fn test_select_layer_at_boundaries() {
        assert_eq!(
            select_layer(LayerPreference::Auto, REMB_THRESHOLD_HIGH),
            Layer::High
        );
        assert_eq!(
            select_layer(LayerPreference::Auto, REMB_THRESHOLD_HIGH - 1),
            Layer::Medium
        );
        assert_eq!(
            select_layer(LayerPreference::Auto, REMB_THRESHOLD_MEDIUM),
            Layer::Medium
        );
        assert_eq!(
            select_layer(LayerPreference::Auto, REMB_THRESHOLD_MEDIUM - 1),
            Layer::Low
        );
    }
}
