//! Track Routing and RTP Forwarding
//!
//! Manages RTP packet forwarding between participants in a voice room.

use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::RwLock;
use tracing::{debug, warn};
use uuid::Uuid;
use webrtc::{
    rtp::packet::Packet as RtpPacket,
    rtp_transceiver::rtp_codec::RTCRtpCodecCapability,
    track::track_local::{
        track_local_static_rtp::TrackLocalStaticRTP,
        TrackLocalWriter,
    },
    track::track_remote::TrackRemote,
};

use super::error::VoiceError;
use super::peer::Peer;

/// Subscription info for a track.
#[derive(Clone)]
struct Subscription {
    /// The subscriber's user ID.
    subscriber_id: Uuid,
    /// The local track that forwards to the subscriber.
    local_track: Arc<TrackLocalStaticRTP>,
}

/// Manages RTP packet forwarding between participants.
pub struct TrackRouter {
    /// Map: source_user_id -> list of subscriptions
    subscriptions: RwLock<HashMap<Uuid, Vec<Subscription>>>,
}

impl TrackRouter {
    /// Create a new track router.
    pub fn new() -> Self {
        Self {
            subscriptions: RwLock::new(HashMap::new()),
        }
    }

    /// Create a local track for forwarding audio from source to subscriber.
    ///
    /// Returns the local track that should be added to the subscriber's peer connection.
    pub async fn create_subscriber_track(
        &self,
        source_user_id: Uuid,
        subscriber: &Peer,
        source_track: &TrackRemote,
    ) -> Result<Arc<TrackLocalStaticRTP>, VoiceError> {
        // Create a local track with the same codec as the source
        let local_track = Arc::new(TrackLocalStaticRTP::new(
            RTCRtpCodecCapability {
                mime_type: source_track.codec().capability.mime_type.clone(),
                clock_rate: source_track.codec().capability.clock_rate,
                channels: source_track.codec().capability.channels,
                sdp_fmtp_line: source_track.codec().capability.sdp_fmtp_line.clone(),
                rtcp_feedback: vec![],
            },
            format!("audio-{}", source_user_id),
            format!("voice-{}-{}", source_user_id, subscriber.user_id),
        ));

        // Store subscription
        let subscription = Subscription {
            subscriber_id: subscriber.user_id,
            local_track: local_track.clone(),
        };

        let mut subs = self.subscriptions.write().await;
        subs.entry(source_user_id)
            .or_insert_with(Vec::new)
            .push(subscription);

        debug!(
            source = %source_user_id,
            subscriber = %subscriber.user_id,
            "Created subscriber track"
        );

        Ok(local_track)
    }

    /// Forward an RTP packet from source to all subscribers.
    pub async fn forward_rtp(&self, source_user_id: Uuid, rtp_packet: &RtpPacket) {
        let subs = self.subscriptions.read().await;

        if let Some(subscribers) = subs.get(&source_user_id) {
            for sub in subscribers {
                // Write RTP packet to local track (forwards to subscriber)
                if let Err(e) = sub.local_track.write_rtp(rtp_packet).await {
                    warn!(
                        source = %source_user_id,
                        subscriber = %sub.subscriber_id,
                        error = %e,
                        "Failed to forward RTP packet"
                    );
                }
            }
        }
    }

    /// Remove a subscriber from a source.
    pub async fn remove_subscriber(&self, source_user_id: Uuid, subscriber_id: Uuid) {
        let mut subs = self.subscriptions.write().await;

        if let Some(subscribers) = subs.get_mut(&source_user_id) {
            subscribers.retain(|s| s.subscriber_id != subscriber_id);

            // Remove source entry if no subscribers left
            if subscribers.is_empty() {
                subs.remove(&source_user_id);
            }
        }

        debug!(
            source = %source_user_id,
            subscriber = %subscriber_id,
            "Removed subscriber"
        );
    }

    /// Remove all subscriptions for a source (when source leaves).
    pub async fn remove_source(&self, source_user_id: Uuid) {
        let mut subs = self.subscriptions.write().await;
        subs.remove(&source_user_id);

        debug!(source = %source_user_id, "Removed source and all subscriptions");
    }

    /// Remove a subscriber from all sources (when subscriber leaves).
    pub async fn remove_subscriber_from_all(&self, subscriber_id: Uuid) {
        let mut subs = self.subscriptions.write().await;

        for (_, subscribers) in subs.iter_mut() {
            subscribers.retain(|s| s.subscriber_id != subscriber_id);
        }

        // Clean up empty entries
        subs.retain(|_, v| !v.is_empty());

        debug!(subscriber = %subscriber_id, "Removed subscriber from all sources");
    }

    /// Get the number of subscribers for a source.
    pub async fn subscriber_count(&self, source_user_id: Uuid) -> usize {
        let subs = self.subscriptions.read().await;
        subs.get(&source_user_id).map(|s| s.len()).unwrap_or(0)
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
    track: Arc<TrackRemote>,
    router: Arc<TrackRouter>,
) {
    tokio::spawn(async move {
        let mut buf = vec![0u8; 1500]; // MTU size

        loop {
            match track.read(&mut buf).await {
                Ok((packet, _attributes)) => {
                    // Forward the RTP packet to all subscribers
                    router.forward_rtp(source_user_id, &packet).await;
                }
                Err(e) => {
                    debug!(
                        source = %source_user_id,
                        error = %e,
                        "Track read ended"
                    );
                    break;
                }
            }
        }

        // Clean up when track ends
        router.remove_source(source_user_id).await;
    });
}
