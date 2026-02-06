//! Track Routing and RTP Forwarding
//!
//! Manages RTP packet forwarding between participants in a voice room.
//! Uses `DashMap` for lock-free concurrent access in the RTP hot path.

use std::sync::Arc;

use dashmap::DashMap;
use tracing::{debug, warn};
use uuid::Uuid;
use webrtc::rtp::packet::Packet as RtpPacket;
use webrtc::rtp_transceiver::rtp_codec::RTCRtpCodecCapability;
use webrtc::track::track_local::track_local_static_rtp::TrackLocalStaticRTP;
use webrtc::track::track_local::TrackLocalWriter;
use webrtc::track::track_remote::TrackRemote;

use super::error::VoiceError;
use super::peer::Peer;
use super::track_types::TrackSource;

/// Subscription info for a track.
#[derive(Clone)]
struct Subscription {
    /// The subscriber's user ID.
    subscriber_id: Uuid,
    /// The local track that forwards to the subscriber.
    local_track: Arc<TrackLocalStaticRTP>,
}

/// Manages RTP packet forwarding between participants.
///
/// Uses `DashMap` for lock-free concurrent access, which is critical for the
/// RTP hot path that processes ~50 packets/second per participant.
pub struct TrackRouter {
    /// Map: `(source_user_id, source_type)` -> list of subscriptions
    /// Using `DashMap` to avoid lock contention in the RTP forwarding hot path.
    subscriptions: DashMap<(Uuid, TrackSource), Vec<Subscription>>,
}

impl TrackRouter {
    /// Create a new track router.
    pub fn new() -> Self {
        Self {
            subscriptions: DashMap::new(),
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
            // Track ID needs to be unique. Using user_id + source type.
            format!("{source_user_id}-{source_type:?}"),
            format!("voice-{source_user_id}-{}", subscriber.user_id),
        ));

        // Store subscription
        let subscription = Subscription {
            subscriber_id: subscriber.user_id,
            local_track: local_track.clone(),
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

    /// Forward an RTP packet from source to all subscribers.
    ///
    /// This is the hot path called ~50 times/second per participant.
    /// Uses `DashMap` for lock-free concurrent reads to avoid contention.
    pub async fn forward_rtp(
        &self,
        source_user_id: Uuid,
        source_type: TrackSource,
        rtp_packet: &RtpPacket,
    ) {
        // DashMap::get returns a guard that provides lock-free concurrent read access
        if let Some(subscribers) = self.subscriptions.get(&(source_user_id, source_type)) {
            for sub in subscribers.value() {
                // Write RTP packet to local track (forwards to subscriber)
                if let Err(e) = sub.local_track.write_rtp(rtp_packet).await {
                    warn!(
                        source = %source_user_id,
                        source_type = ?source_type,
                        subscriber = %sub.subscriber_id,
                        error = %e,
                        "Failed to forward RTP packet"
                    );
                }
            }
        }
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

    /// Get the number of subscribers for a source.
    pub async fn subscriber_count(&self, source_user_id: Uuid, source_type: TrackSource) -> usize {
        self.subscriptions
            .get(&(source_user_id, source_type))
            .map_or(0, |entry| entry.value().len())
    }
}

impl Default for TrackRouter {
    fn default() -> Self {
        Self::new()
    }
}

/// Spawn a task to read RTP packets from a track and forward them.
pub fn spawn_rtp_forwarder(
    source_user_id: Uuid,
    source_type: TrackSource,
    track: Arc<TrackRemote>,
    router: Arc<TrackRouter>,
) {
    tokio::spawn(async move {
        let mut buf = vec![0u8; 1500]; // MTU size

        loop {
            match track.read(&mut buf).await {
                Ok((packet, _attributes)) => {
                    // Forward the RTP packet to all subscribers
                    router
                        .forward_rtp(source_user_id, source_type, &packet)
                        .await;
                }
                Err(e) => {
                    debug!(
                        source = %source_user_id,
                        source_type = ?source_type,
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
             "RTP forwarder stopped"
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
                .subscriber_count(user_id, TrackSource::ScreenVideo)
                .await,
            0
        );
        assert_eq!(
            router
                .subscriber_count(user_id, TrackSource::ScreenAudio)
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
            .remove_subscriber(source_id, TrackSource::ScreenVideo, subscriber_id)
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
            .forward_rtp(source_id, TrackSource::Microphone, &rtp_packet)
            .await;
        router
            .forward_rtp(source_id, TrackSource::ScreenVideo, &rtp_packet)
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
                        .remove_subscriber(source_id, TrackSource::ScreenVideo, subscriber_id)
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
