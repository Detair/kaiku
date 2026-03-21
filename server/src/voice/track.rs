//! Track Routing and RTP Forwarding
//!
//! Manages RTP packet forwarding between participants in a voice room.
//! Uses `DashMap` for lock-free concurrent access in the RTP hot path.

use std::sync::Arc;
use std::time::{Duration, Instant};

use dashmap::DashMap;
use tokio::sync::mpsc;
use tracing::{debug, info, warn};
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
    /// Holds secondary layers (Medium/Low) that arrived before their High layer.
    pending_secondary: DashMap<(Uuid, Layer), Arc<TrackRemote>>,
}

impl TrackRouter {
    /// Create a new track router.
    pub fn new() -> Self {
        Self {
            subscriptions: DashMap::new(),
            simulcast_tracks: DashMap::new(),
            pending_secondary: DashMap::new(),
        }
    }

    /// Create a local track for forwarding media from source to subscriber.
    ///
    /// Returns the local track that should be added to the subscriber's peer connection.
    pub async fn create_subscriber_track(
        &self,
        source_user_id: Uuid,
        source_type: TrackSource,
        subscriber: &Peer,
        source_track: &TrackRemote,
    ) -> Result<Arc<TrackLocalStaticRTP>, VoiceError> {
        // Create a local track with the same codec as the source
        let local_track = Arc::new(TrackLocalStaticRTP::new(
            RTCRtpCodecCapability {
                mime_type: source_track.codec().capability.mime_type,
                clock_rate: source_track.codec().capability.clock_rate,
                channels: source_track.codec().capability.channels,
                sdp_fmtp_line: source_track.codec().capability.sdp_fmtp_line,
                rtcp_feedback: vec![],
            },
            // Track ID: "{source_user_id}:{source_type}" using Display format
            // (e.g. "uuid:microphone", "uuid:screen_video:stream_uuid", "uuid:webcam").
            // Clients split on the first colon to get [userId, sourceType].
            format!("{source_user_id}:{source_type}"),
            // Stream ID: same format so the browser groups tracks correctly.
            format!("{source_user_id}:{source_type}"),
        ));

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

        Ok(local_track)
    }

    /// Forward an RTP packet from source to all subscribers whose active layer matches.
    ///
    /// This is the hot path called ~50 times/second per participant.
    /// Uses `DashMap` for lock-free concurrent reads to avoid contention.
    /// The `layer` parameter indicates which simulcast layer this packet belongs to.
    pub async fn forward_rtp(
        &self,
        source_user_id: Uuid,
        source_type: TrackSource,
        layer: Layer,
        rtp_packet: &RtpPacket,
    ) {
        let key = (source_user_id, source_type);
        // DashMap::get returns a guard that provides lock-free concurrent read access
        if let Some(subscribers) = self.subscriptions.get(&key) {
            crate::observability::metrics::record_rtp_packet_forwarded();
            for sub in subscribers.value() {
                if sub.active_layer == layer {
                    // Write RTP packet to local track (forwards to subscriber)
                    if let Err(e) = sub.local_track.write_rtp(rtp_packet).await {
                        warn!(
                            subscriber = %sub.subscriber_id,
                            error = %e,
                            "Failed to forward RTP packet"
                        );
                    }
                }
            }
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

        debug!(source = %source_user_id, "Removed source and all subscriptions");
    }

    /// Remove all subscriptions for a specific source track (e.g. when a user stops webcam).
    pub async fn remove_source_track(&self, source_user_id: Uuid, source_type: TrackSource) {
        self.subscriptions.remove(&(source_user_id, source_type));

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

    /// Look up the `TrackSource` for a user from a specific simulcast layer.
    ///
    /// Used by the SFU `on_track` callback: when a secondary simulcast layer
    /// (Medium/Low) arrives, we need the source type that the primary (High)
    /// layer already registered, so we don't pop from the pending queue twice.
    pub fn find_source_type_for_user(&self, user_id: Uuid, layer: Layer) -> Option<TrackSource> {
        self.simulcast_tracks.iter().find_map(|entry| {
            let (uid, src, l) = entry.key();
            if *uid == user_id && *l == layer {
                Some(*src)
            } else {
                None
            }
        })
    }

    /// Store a simulcast track and drain any pending secondary layers.
    ///
    /// When the High layer arrives first, secondary entries don't exist yet.
    /// When a secondary layer arrives first, it is stashed in `pending_secondary`.
    /// When High then arrives, this method drains those pending entries under the
    /// now-known `source_type`.
    pub fn store_simulcast_track(
        &self,
        user_id: Uuid,
        source_type: TrackSource,
        layer: Layer,
        track: Arc<TrackRemote>,
    ) {
        self.simulcast_tracks
            .insert((user_id, source_type, layer), track);

        // If this is the High layer, drain any pending secondaries for this user.
        if layer == Layer::High {
            for pending_layer in [Layer::Medium, Layer::Low] {
                if let Some((_, pending_track)) =
                    self.pending_secondary.remove(&(user_id, pending_layer))
                {
                    self.simulcast_tracks
                        .insert((user_id, source_type, pending_layer), pending_track);
                    debug!(
                        source = %user_id,
                        source_type = ?source_type,
                        layer = ?pending_layer,
                        "Drained pending secondary simulcast track"
                    );
                }
            }
        }
    }

    /// Stash a secondary simulcast layer that arrived before the High layer.
    pub fn stash_pending_secondary(&self, user_id: Uuid, layer: Layer, track: Arc<TrackRemote>) {
        self.pending_secondary.insert((user_id, layer), track);
        debug!(
            source = %user_id,
            layer = ?layer,
            "Stashed pending secondary simulcast track (High not yet received)"
        );
    }

    /// Get the number of subscribers for a source.
    pub async fn subscriber_count(&self, source_user_id: Uuid, source_type: TrackSource) -> usize {
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
        let key = (source_user_id, source_type);
        let mut changed = None;
        if let Some(mut subs) = self.subscriptions.get_mut(&key) {
            for sub in subs.iter_mut() {
                if sub.subscriber_id == subscriber_id {
                    sub.preferred_layer = pref;
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
) {
    tokio::spawn(async move {
        let mut buf = vec![0u8; 1500]; // MTU size
        let mut packet_count: u64 = 0;

        loop {
            match track.read(&mut buf).await {
                Ok((packet, _attributes)) => {
                    packet_count += 1;
                    if packet_count == 1 || packet_count.is_multiple_of(500) {
                        let sub_count = router.subscription_count(source_user_id, source_type);
                        info!(
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
                        .forward_rtp(source_user_id, source_type, layer, &packet)
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

        // Clean up this specific track when it ends
        // We can't use remove_source because that removes ALL tracks for the user
        // We need a way to remove just this track from subscriptions?
        // Actually, remove_source is fine if the user disconnects, but if they just stop screen
        // sharing? We should probably just let the subscriptions stick around or clean them
        // up specifically. For now, let's just log. The Peer cleanup handles the main
        // removal.
        debug!(
             source = %source_user_id,
             source_type = ?source_type,
             layer = ?layer,
             "RTP forwarder stopped"
        );
    });
}

/// Spawn a task to read REMB feedback from a subscriber's outgoing sender.
///
/// When the subscriber's browser reports its available bandwidth via REMB,
/// this updates the corresponding subscription in the track router and
/// notifies the subscriber if the active layer changes.
pub fn spawn_subscriber_remb_reader(
    track_router: Arc<TrackRouter>,
    subscriber_id: Uuid,
    source_user_id: Uuid,
    source_type: TrackSource,
    sender: Arc<webrtc::rtp_transceiver::rtp_sender::RTCRtpSender>,
    signal_tx: mpsc::Sender<crate::ws::OutboundMsg>,
    channel_id: Uuid,
) {
    tokio::spawn(async move {
        use webrtc::rtcp::payload_feedbacks::receiver_estimated_maximum_bitrate::ReceiverEstimatedMaximumBitrate;

        while let Ok((packets, _)) = sender.read_rtcp().await {
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
                            .send(crate::ws::OutboundMsg::Event(crate::ws::ServerEvent::VoiceLayerChanged {
                                channel_id,
                                source_user_id,
                                track_source: source_type,
                                active_layer: new_layer,
                            }))
                            .await;
                    }
                }
            }
        }

        tracing::debug!(
            subscriber = %subscriber_id,
            source = %source_user_id,
            source_type = ?source_type,
            "Subscriber REMB reader stopped"
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
        assert_eq!(
            router
                .subscriber_count(user_id, TrackSource::Microphone)
                .await,
            0
        );
        assert_eq!(
            router
                .subscriber_count(user_id, TrackSource::ScreenVideo(Uuid::nil()))
                .await,
            0
        );
        assert_eq!(
            router
                .subscriber_count(user_id, TrackSource::ScreenAudio(Uuid::nil()))
                .await,
            0
        );
        assert_eq!(
            router.subscriber_count(user_id, TrackSource::Webcam).await,
            0
        );
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
            .forward_rtp(source_id, TrackSource::Microphone, Layer::High, &rtp_packet)
            .await;
        router
            .forward_rtp(
                source_id,
                TrackSource::ScreenVideo(Uuid::nil()),
                Layer::High,
                &rtp_packet,
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
                router_clone
                    .subscriber_count(user_id, TrackSource::Microphone)
                    .await
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
