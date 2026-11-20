//! Selective Forwarding Unit Implementation
//!
//! Manages voice rooms and WebRTC peer connections for real-time audio and video.
//!
//! # Locks
//!
//! Order: `SfuServer::rooms` → `Room::peers` → `Room::screen_shares` →
//! `Room::webcams` (as in [`Room::get_participant_info`]). Per-peer locks
//! (`Peer::incoming_tracks`, the outgoing track map, the pending sources) are
//! taken with no room lock held and are never held across a socket send.
//!
//! `Peer::media` (per seat, async) guards adding state on behalf of a seat:
//! the track callback binding a track to a source and forwarding it
//! (`forward_to`, `Peer::record_forwarded`), the first-offer subscribe path
//! forwarding an existing seat's tracks, and the end of a share or webcam.
//! Rule: **a retired seat adds nothing.** Whether the seat is still current
//! (`Peer::is_retired`) and its source still live is decided after taking
//! `media`, in the same critical section as the additions;
//! `Room::detach_media` sets `retired` first and then takes `media` before it
//! collects the seat's forwarded tracks, so it either sees every addition or
//! the addition sees `retired`.
//!
//! Order: `media` → room locks → leaf locks, never the reverse. At most one
//! seat's `media` is held at a time and it is never taken while a room lock
//! is held. `detach_media` and the end of a share or webcam send the
//! resulting offers while still holding `media`; each offer is bounded, so a
//! concurrent leave of the same seat waits at most that long.
//!
//! Room state a seat adds (its webcam, its screen shares, its simulcast
//! layers, the tracks it forwards) records that seat. Adding checks under the
//! room lock that the seat is still the user's current one
//! ([`Room::try_add_screen_share_for`], [`Room::add_webcam_for`]); removing on
//! behalf of a seat removes only entries that seat owns
//! ([`Room::remove_screen_share_of`], [`Room::remove_webcam_of`]). Events the
//! others get about a seat are decided under the peers lock and queued in
//! that same section without waiting ([`Room::broadcast_if_current`],
//! [`Room::announce_departure`], [`Room::announce_stopped`]), so they reach
//! every healthy queue in the order they were decided. A participant whose
//! queue is full gets the event after the lock is released (bounded wait,
//! order not guaranteed for that participant).
//!
//! `Peer::negotiation` (the subscriber offer/answer state) is a leaf lock: it
//! is taken with no room lock held, nothing else of ours is locked while it is
//! held, and it covers only local WebRTC work (create offer, set local or
//! remote description), never a network or socket wait. webrtc-rs callbacks
//! (`on_track`, ICE, connection state) run on their own tasks and reach it only
//! through [`SfuServer::renegotiate`], which never waits for a callback, so the
//! two cannot wait on each other.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

use sqlx::PgPool;
use tokio::sync::{mpsc, RwLock};
use tokio::task::JoinHandle;
use tracing::{debug, info, warn};
use uuid::Uuid;
use vc_common::protocol::PcType;
use webrtc::api::interceptor_registry::register_default_interceptors;
use webrtc::api::media_engine::MediaEngine;
use webrtc::api::setting_engine::SettingEngine;
use webrtc::api::{APIBuilder, API};
use webrtc::ice::network_type::NetworkType;
use webrtc::ice::udp_network::{EphemeralUDP, UDPNetwork};
use webrtc::interceptor::registry::Registry;
use webrtc::peer_connection::configuration::RTCConfiguration;
use webrtc::peer_connection::peer_connection_state::RTCPeerConnectionState;
use webrtc::peer_connection::sdp::session_description::RTCSessionDescription;
use webrtc::peer_connection::RTCPeerConnection;
use webrtc::rtp_transceiver::rtp_codec::{
    RTCRtpCodecCapability, RTCRtpCodecParameters, RTPCodecType,
};
use webrtc::rtp_transceiver::RTCPFeedback;

use super::error::VoiceError;
use super::peer::Peer;
use super::rate_limit::{VoiceRateLimiter, VoiceStatsLimiter};
use super::screen_share::{ScreenShareInfo, ScreenShareLimiter};
use super::track::{spawn_rtp_forwarder, spawn_subscriber_rtcp_reader, TrackRouter};
use super::track_types::{Layer, TrackSource};
use super::webcam::WebcamInfo;
use crate::config::{Config, IceIpFamily};
use crate::ratelimit::{RateLimitCategory, RateLimiter};
use crate::ws::handlers::{send_outbound, send_outbound_within, OutboundSendError};
use crate::ws::{OutboundMsg, ServerEvent};

/// How long a send to *another* peer's socket may wait for room in its queue.
/// Voice fan-out runs inside the sending user's socket loop, so a stalled peer
/// must not hold it up; the stalled peer's own socket loop tears it down.
const PEER_SIGNAL_SEND_TIMEOUT: Duration = Duration::from_millis(250);

/// Send `event` to every peer concurrently, waiting at most
/// [`PEER_SIGNAL_SEND_TIMEOUT`] per peer. Peers whose queue is closed or stays
/// full are skipped (logged), so one stalled peer never delays the others.
async fn fan_out(
    channel_id: Uuid,
    senders: Vec<(Uuid, mpsc::Sender<OutboundMsg>)>,
    event: &ServerEvent,
) {
    let sends = senders.iter().map(|(user_id, tx)| async move {
        if let Err(e) = send_outbound_within(
            tx,
            OutboundMsg::voice(channel_id, event.clone()),
            PEER_SIGNAL_SEND_TIMEOUT,
        )
        .await
        {
            warn!(user_id = %user_id, error = %e, "Skipping voice event for peer");
        }
    });
    futures::future::join_all(sends).await;
}

/// Queue `event` for each of `senders` without waiting (called under the
/// room's peers lock, so the order of events decided under it is the order
/// in every healthy queue). Returns the participants whose queue is full:
/// the caller sends to them with [`fan_out`] after releasing the lock.
fn queue_now(
    channel_id: Uuid,
    senders: impl Iterator<Item = (Uuid, mpsc::Sender<OutboundMsg>)>,
    event: &ServerEvent,
) -> Vec<(Uuid, mpsc::Sender<OutboundMsg>)> {
    let mut full = Vec::new();
    for (user_id, tx) in senders {
        match tx.try_send(OutboundMsg::voice(channel_id, event.clone())) {
            Ok(()) => {}
            Err(mpsc::error::TrySendError::Full(_)) => full.push((user_id, tx)),
            Err(mpsc::error::TrySendError::Closed(_)) => {
                debug!(user_id = %user_id, "Skipping voice event for closed peer");
            }
        }
    }
    full
}

/// How long a peer connection may stay `Failed` before the peer is removed
/// from its room. Gives ICE restarts and brief network switches a chance.
pub const PEER_FAILED_GRACE: Duration = Duration::from_secs(15);

/// A peer whose connection failed and did not recover within
/// [`PEER_FAILED_GRACE`]. Handled by the reaper from
/// [`SfuServer::spawn_stale_peer_reaper`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StalePeer {
    pub user_id: Uuid,
    pub channel_id: Uuid,
    /// Only this session is removed; a newer session of the same user stays.
    pub session_id: Uuid,
}

/// How long a video track that arrived before its announcement waits for
/// it (the rc.5 web client adds the track right before it sends the start).
pub const UNANNOUNCED_TRACK_GRACE: Duration = Duration::from_secs(3);

/// Drop a video track that has no announced source: it gets no forwarder
/// and no subscriber tracks, and the sender learns why.
async fn reject_unannounced_track(
    peer: &Peer,
    track: &webrtc::track::track_remote::TrackRemote,
    video: bool,
) {
    warn!(
        user_id = %peer.user_id,
        channel_id = %peer.channel_id,
        track_id = %track.id(),
        video,
        "Dropping track without an announced source"
    );
    let error = if video {
        VoiceError::VideoSourceUnannounced
    } else {
        VoiceError::AudioSourceUnannounced
    };
    let event = OutboundMsg::Event(crate::ws::handlers::voice_error_event(&error));
    if let Err(e) = send_outbound_within(&peer.signal_tx, event, PEER_SIGNAL_SEND_TIMEOUT).await {
        debug!(user_id = %peer.user_id, error = %e, "Could not report dropped track");
    }
}

/// The oldest announced source for a new track of `peer` whose share or
/// webcam is still live, waiting up to [`UNANNOUNCED_TRACK_GRACE`].
/// Announcements whose share or webcam ended meanwhile are discarded.
///
/// webrtc-rs runs the track callbacks of one connection one after another,
/// so this wait also holds back that participant's next track (bounded by
/// the grace).
async fn take_live_source(peer: &Peer, room: &Room, video: bool) -> Option<TrackSource> {
    let deadline = tokio::time::Instant::now() + UNANNOUNCED_TRACK_GRACE;
    loop {
        let source = peer.take_source_until(video, deadline).await?;
        if source_is_live(room, peer, source).await {
            return Some(source);
        }
        debug!(user_id = %peer.user_id, source = %source, "Discarding announcement of an ended source");
    }
}

/// Whether the share or webcam that `source` belongs to is still registered
/// for `peer` (the microphone always is).
async fn source_is_live(room: &Room, peer: &Peer, source: TrackSource) -> bool {
    match source {
        TrackSource::ScreenVideo(id) | TrackSource::ScreenAudio(id) => room
            .screen_shares
            .read()
            .await
            .get(&id)
            .is_some_and(|share| share.user_id == peer.user_id),
        TrackSource::Webcam => room.webcams.read().await.contains_key(&peer.user_id),
        TrackSource::Microphone => true,
    }
}

/// Forward `source`'s `track` of seat `source_peer` to `subscriber`: router
/// subscription, outgoing track, the source seat's forwarded list and the
/// RTCP reader for video. The caller holds `source_peer.media` and has
/// checked that the seat is current. Returns whether a track was added (the
/// subscriber then needs a renegotiation).
pub(crate) async fn forward_to(
    room: &Room,
    source_peer: &Arc<Peer>,
    source: TrackSource,
    track: &Arc<webrtc::track::track_remote::TrackRemote>,
    subscriber: &Arc<Peer>,
) -> bool {
    let source_user = source_peer.user_id;
    if subscriber.is_retired() {
        return false;
    }
    let Ok(local_track) =
        room.track_router
            .create_subscriber_track(source_user, source, subscriber, track)
    else {
        return false;
    };
    match subscriber
        .add_outgoing_track(
            source_user,
            source,
            source_peer.session_id,
            local_track.clone(),
        )
        .await
    {
        Ok((sender, replaced)) => {
            // An older seat's track that this one replaced writes nowhere.
            if let Some(old) = replaced {
                room.track_router.remove_subscriptions_into(&[old]);
            }
            // The subscriber left meanwhile: take the add back (its own
            // removal may already have run).
            if subscriber.is_retired() {
                subscriber
                    .remove_outgoing_track_if(source_user, source, &local_track)
                    .await;
                room.track_router.remove_subscriptions_into(&[local_track]);
                return false;
            }
            source_peer.record_forwarded(subscriber.user_id, source, local_track);
            if source.is_video() {
                spawn_subscriber_rtcp_reader(
                    Arc::downgrade(source_peer),
                    track.ssrc(),
                    room.track_router.clone(),
                    subscriber.user_id,
                    source_user,
                    source,
                    sender,
                    subscriber.signal_tx.clone(),
                    room.channel_id,
                );
            }
            true
        }
        Err(e) => {
            room.track_router.remove_subscriptions_into(&[local_track]);
            debug!(
                source = %source_user,
                subscriber = %subscriber.user_id,
                error = %e,
                "Track not added"
            );
            false
        }
    }
}

/// Whether an entry of user `user_id` added by seat `owner` belongs to
/// `peer`: the same user and the same seat.
fn owned_by(user_id: Uuid, owner: Uuid, peer: &Peer) -> bool {
    (user_id, owner) == (peer.user_id, peer.session_id)
}

/// Remove the simulcast layers `peer` published from the room's router.
fn room_remove_layers(room: &Room, peer: &Peer) {
    let layers = peer.take_simulcast_layers(&|_: &TrackSource| true);
    room.track_router.remove_simulcast_tracks(&layers);
}

/// Ask the publisher of `peer` for a key frame on each of `ssrcs` (PLI).
pub(crate) async fn request_keyframe(peer: &Peer, ssrcs: &[u32]) -> Result<(), webrtc::Error> {
    use webrtc::rtcp::payload_feedbacks::picture_loss_indication::PictureLossIndication;
    let packets: Vec<Box<dyn webrtc::rtcp::packet::Packet + Send + Sync>> = ssrcs
        .iter()
        .map(|ssrc| {
            Box::new(PictureLossIndication {
                sender_ssrc: 0,
                media_ssrc: *ssrc,
            }) as Box<dyn webrtc::rtcp::packet::Packet + Send + Sync>
        })
        .collect();
    peer.publisher_pc.write_rtcp(&packets).await.map(|_| ())
}

/// A session description without its simulcast lines (`a=simulcast`,
/// `a=rid`): the answer to it then accepts one stream per section.
fn without_simulcast(sdp: &str) -> String {
    let mut out = String::with_capacity(sdp.len());
    for line in sdp.split_inclusive('\n') {
        let bare = line.trim_end_matches(['\r', '\n']);
        if bare.starts_with("a=simulcast:") || bare.starts_with("a=rid:") {
            continue;
        }
        out.push_str(line);
    }
    out
}

/// The direction of each media section of a session description, in order
/// (`sendrecv` when a section names none).
fn section_directions(sdp: &str) -> Vec<&'static str> {
    let mut out: Vec<&'static str> = Vec::new();
    for line in sdp.lines() {
        if line.starts_with("m=") {
            out.push("sendrecv");
        } else if let Some(last) = out.last_mut() {
            for dir in ["sendrecv", "sendonly", "recvonly", "inactive"] {
                if line.strip_prefix("a=") == Some(dir) {
                    *last = dir;
                }
            }
        }
    }
    out
}

/// Whether an answer with `answer` section directions can answer an offer
/// with `offer` directions (same sections, each direction allowed by
/// JSEP). An answer to an earlier offer fails this once a section changed.
fn answer_fits(offer: &[&str], answer: &[&str]) -> bool {
    offer.len() == answer.len()
        && offer.iter().zip(answer).all(|(o, a)| match *o {
            "sendonly" => matches!(*a, "recvonly" | "inactive"),
            "recvonly" => matches!(*a, "sendonly" | "inactive"),
            "inactive" => *a == "inactive",
            _ => true,
        })
}

/// How long a subscriber offer may wait for its answer before a new change
/// offers again regardless (a client that lost the offer would otherwise
/// never receive tracks again).
const SUBSCRIBER_ANSWER_TIMEOUT: Duration = Duration::from_secs(10);

/// Default maximum participants per room.
const DEFAULT_MAX_PARTICIPANTS: usize = 25;

/// Participant info for room state.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ParticipantInfo {
    /// User ID.
    pub user_id: Uuid,
    /// Username.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
    /// Display name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    /// Whether the user is muted.
    pub muted: bool,
    /// Whether the user is screen sharing.
    #[serde(default)]
    pub screen_sharing: bool,
    /// Whether the user has their webcam active.
    #[serde(default)]
    pub webcam_active: bool,
}

/// A peer that [`Room::add_peer`] replaced (same user, new join), with the
/// screen shares and webcam it had. The caller tears it down outside the
/// room lock.
pub struct ReplacedMedia {
    /// The replaced peer (no longer in the room, not closed yet).
    pub peer: Arc<Peer>,
    /// The replaced peer's screen shares (their slots are still counted).
    pub screen_shares: Vec<ScreenShareInfo>,
    /// The replaced peer's webcam, if it was on.
    pub webcam: Option<WebcamInfo>,
}

/// Voice channel room with all participants.
pub struct Room {
    /// Channel ID.
    pub channel_id: Uuid,
    /// Connected peers.
    pub peers: RwLock<HashMap<Uuid, Arc<Peer>>>,
    /// Track router for RTP forwarding.
    pub track_router: Arc<TrackRouter>,
    /// Maximum participants allowed.
    pub max_participants: usize,
    /// Active screen shares.
    pub screen_shares: RwLock<HashMap<Uuid, ScreenShareInfo>>,
    /// Active webcams.
    pub webcams: RwLock<HashMap<Uuid, WebcamInfo>>,
}

impl Room {
    /// Create a new room.
    #[must_use]
    pub fn new(channel_id: Uuid, max_participants: usize) -> Self {
        Self {
            channel_id,
            peers: RwLock::new(HashMap::new()),
            track_router: Arc::new(TrackRouter::new()),
            max_participants,
            screen_shares: RwLock::new(HashMap::new()),
            webcams: RwLock::new(HashMap::new()),
        }
    }

    /// Add a peer to the room (room capacity only, no channel limit).
    pub async fn add_peer(&self, peer: Arc<Peer>) -> Result<Option<ReplacedMedia>, VoiceError> {
        self.add_peer_with_limit(peer, None).await
    }

    /// Add a peer to the room, refusing it when the room holds
    /// `min(user_limit, max_participants)` other users already.
    ///
    /// If the user already has a peer here (a stale session or a join from
    /// another socket), that peer is replaced: it leaves the map in the same
    /// step (one entry per user at all times, and a replacement does not
    /// count against the limit) and is returned with its screen shares and
    /// webcam, which are removed from the room. The caller closes it; nothing
    /// slow happens under the room lock.
    pub async fn add_peer_with_limit(
        &self,
        peer: Arc<Peer>,
        user_limit: Option<usize>,
    ) -> Result<Option<ReplacedMedia>, VoiceError> {
        let mut peers = self.peers.write().await;

        let limit = user_limit.map_or(self.max_participants, |l| l.min(self.max_participants));
        let replacing = peers.contains_key(&peer.user_id);
        if !replacing && peers.len() >= limit {
            return Err(VoiceError::ChannelFull {
                max_participants: limit,
            });
        }

        let replaced = if let Some(old_peer) = peers.remove(&peer.user_id) {
            old_peer.retire();
            tracing::info!(user_id = %peer.user_id, "Replacing voice peer of the same user");
            // Lock order as in `get_participant_info`: peers, shares, webcams.
            let screen_shares = {
                let mut shares = self.screen_shares.write().await;
                let ids: Vec<Uuid> = shares
                    .values()
                    .filter(|s| s.user_id == peer.user_id)
                    .map(|s| s.stream_id)
                    .collect();
                ids.iter().filter_map(|id| shares.remove(id)).collect()
            };
            let webcam = self.webcams.write().await.remove(&peer.user_id);
            Some(ReplacedMedia {
                peer: old_peer,
                screen_shares,
                webcam,
            })
        } else {
            None
        };

        peers.insert(peer.user_id, peer);
        Ok(replaced)
    }

    /// Remove what a peer that left the room (or was replaced) still has in
    /// the room's media: its tracks at every remaining peer, with one
    /// renegotiation each, the router entries of its sources and the
    /// subscriptions that wrote into its own tracks.
    ///
    /// Must run before a replacing peer of the same user publishes (the
    /// router keys sources by user).
    pub async fn detach_media(&self, departed: &Peer) {
        // From now on its late track callbacks and forwarders do nothing.
        departed.retire();
        // Wait for a track callback that is adding state for this seat right
        // now (it checked the seat before it was retired): afterwards the
        // forwarded list below is complete, and later callbacks see `retired`.
        let _media = departed.media.lock().await;
        let user_id = departed.user_id;
        // Only this peer instance's media: a newer peer of the same user may
        // already be in the room and keeps its own.
        room_remove_layers(self, departed);
        let forwarded = departed.take_forwarded();
        let forwarded_tracks: Vec<_> = forwarded.iter().map(|(_, _, t)| t.clone()).collect();
        self.track_router
            .remove_subscriptions_into(&forwarded_tracks);
        self.track_router
            .remove_subscriptions_into(&departed.outgoing_track_list().await);
        let others = self.get_other_peers(user_id).await;
        let forwarded = &forwarded;
        let updates = others.into_iter().map(|other| async move {
            let mut removed = 0;
            for (subscriber, source, track) in forwarded {
                if *subscriber == other.user_id
                    && other
                        .remove_outgoing_track_if(user_id, *source, track)
                        .await
                {
                    removed += 1;
                }
            }
            if removed > 0 {
                if let Err(e) = SfuServer::renegotiate(&other).await {
                    warn!(
                        subscriber = %other.user_id,
                        error = %e,
                        "Renegotiation failed after removing a departed peer's tracks"
                    );
                }
            }
        });
        futures::future::join_all(updates).await;
    }

    /// Remove a peer from the room.
    pub async fn remove_peer(&self, user_id: Uuid) -> Option<Arc<Peer>> {
        self.remove_peer_matching(user_id, None).await
    }

    /// Remove the seat of `user_id` if it is still session `session_id`,
    /// together with its screen shares and webcam, all under the peers lock
    /// (as a replacement does): a seat the user takes afterwards keeps its
    /// own. The seat is retired in the same step.
    pub async fn remove_seat_if_session(
        &self,
        user_id: Uuid,
        session_id: Uuid,
    ) -> Option<ReplacedMedia> {
        let mut peers = self.peers.write().await;
        if peers.get(&user_id).map(|p| p.session_id) != Some(session_id) {
            return None;
        }
        let peer = peers.remove(&user_id)?;
        peer.retire();
        // Lock order as in `get_participant_info`: peers, shares, webcams.
        let screen_shares = {
            let mut shares = self.screen_shares.write().await;
            let ids: Vec<Uuid> = shares
                .values()
                .filter(|s| s.user_id == user_id)
                .map(|s| s.stream_id)
                .collect();
            ids.iter().filter_map(|id| shares.remove(id)).collect()
        };
        let webcam = self.webcams.write().await.remove(&user_id);
        drop(peers);
        self.track_router
            .remove_subscriptions_into(&peer.outgoing_track_list().await);
        Some(ReplacedMedia {
            peer,
            screen_shares,
            webcam,
        })
    }

    /// Remove a peer only if it is still the given session. Check and removal
    /// happen under one lock, so a newer session of the same user is never hit.
    pub async fn remove_peer_if_session(
        &self,
        user_id: Uuid,
        session_id: Uuid,
    ) -> Option<Arc<Peer>> {
        self.remove_peer_matching(user_id, Some(session_id)).await
    }

    async fn remove_peer_matching(
        &self,
        user_id: Uuid,
        session_id: Option<Uuid>,
    ) -> Option<Arc<Peer>> {
        let mut peers = self.peers.write().await;
        if let Some(expected) = session_id {
            if peers.get(&user_id).map(|p| p.session_id) != Some(expected) {
                return None;
            }
        }
        let peer = peers.remove(&user_id);

        if let Some(peer) = &peer {
            peer.retire();
            // Clean up track subscriptions
            self.track_router.remove_source(user_id).await;
            self.track_router.remove_subscriber_from_all(user_id).await;
        }

        peer
    }

    /// Get a peer by user ID.
    pub async fn get_peer(&self, user_id: Uuid) -> Option<Arc<Peer>> {
        let peers = self.peers.read().await;
        peers.get(&user_id).cloned()
    }

    /// Get all peers except one.
    pub async fn get_other_peers(&self, exclude_user_id: Uuid) -> Vec<Arc<Peer>> {
        let peers = self.peers.read().await;
        peers
            .iter()
            .filter(|(id, _)| **id != exclude_user_id)
            .map(|(_, peer)| peer.clone())
            .collect()
    }

    /// Add a screen share session (keyed by `stream_id`).
    pub async fn add_screen_share(&self, info: ScreenShareInfo) {
        let mut shares = self.screen_shares.write().await;
        shares.insert(info.stream_id, info);
    }

    /// Whether `peer` is the user's current seat in this room. Callers that
    /// need the answer to hold while they change room state use the
    /// `*_for` methods, which decide it under the same lock.
    pub async fn is_current(&self, peer: &Peer) -> bool {
        self.peers
            .read()
            .await
            .get(&peer.user_id)
            .is_some_and(|p| p.session_id == peer.session_id)
    }

    /// Register `info` as a screen share of `peer`, in one step with the
    /// check that `peer` is the user's current seat (peers lock held while
    /// the shares lock is taken) and that the stream id is free in the room.
    pub async fn try_add_screen_share_for(
        &self,
        peer: &Peer,
        mut info: ScreenShareInfo,
    ) -> Result<(), VoiceError> {
        let peers = self.peers.read().await;
        if peers
            .get(&peer.user_id)
            .is_none_or(|p| p.session_id != peer.session_id)
        {
            return Err(VoiceError::ParticipantNotFound(peer.user_id));
        }
        let mut shares = self.screen_shares.write().await;
        if shares.contains_key(&info.stream_id) {
            return Err(VoiceError::DuplicateStreamId);
        }
        info.owner = peer.session_id;
        shares.insert(info.stream_id, info);
        Ok(())
    }

    /// Register `info` as the webcam of `peer`, in one step with the check
    /// that `peer` is the user's current seat and has no webcam yet.
    pub async fn add_webcam_for(
        &self,
        peer: &Peer,
        mut info: WebcamInfo,
    ) -> Result<(), VoiceError> {
        let peers = self.peers.read().await;
        if peers
            .get(&peer.user_id)
            .is_none_or(|p| p.session_id != peer.session_id)
        {
            return Err(VoiceError::ParticipantNotFound(peer.user_id));
        }
        let mut webcams = self.webcams.write().await;
        if webcams.contains_key(&peer.user_id) {
            return Err(VoiceError::Signaling("Webcam already active".to_string()));
        }
        info.owner = peer.session_id;
        webcams.insert(peer.user_id, info);
        Ok(())
    }

    /// Remove the screen share `stream_id` only if `peer`'s seat owns it.
    pub async fn remove_screen_share_of(
        &self,
        peer: &Peer,
        stream_id: Uuid,
    ) -> Option<ScreenShareInfo> {
        let mut shares = self.screen_shares.write().await;
        if shares
            .get(&stream_id)
            .is_some_and(|s| owned_by(s.user_id, s.owner, peer))
        {
            shares.remove(&stream_id)
        } else {
            None
        }
    }

    /// Remove the webcam only if `peer`'s seat owns it.
    pub async fn remove_webcam_of(&self, peer: &Peer) -> Option<WebcamInfo> {
        let mut webcams = self.webcams.write().await;
        if webcams
            .get(&peer.user_id)
            .is_some_and(|w| owned_by(w.user_id, w.owner, peer))
        {
            webcams.remove(&peer.user_id)
        } else {
            None
        }
    }

    /// Send `event` to the room (all, or all but the user) only while
    /// `peer` is the user's current seat; the decision and the queueing
    /// happen under the peers lock, so a newer seat's join cannot slip in
    /// between. Returns whether it was sent.
    pub async fn broadcast_if_current(
        &self,
        peer: &Peer,
        event: ServerEvent,
        include_self: bool,
    ) -> bool {
        let full = {
            let peers = self.peers.read().await;
            if peers
                .get(&peer.user_id)
                .is_none_or(|p| p.session_id != peer.session_id)
            {
                return false;
            }
            let senders = peers
                .iter()
                .filter(|(id, _)| include_self || **id != peer.user_id)
                .map(|(id, p)| (*id, p.signal_tx.clone()));
            queue_now(self.channel_id, senders, &event)
        };
        fan_out(self.channel_id, full, &event).await;
        true
    }

    /// Tell the others that `user_id` left, only while the user holds no
    /// seat here (decided and queued under the peers lock), and run
    /// `on_absent` in that same step. Returns whether the user was absent.
    pub async fn announce_departure(
        &self,
        user_id: Uuid,
        event: ServerEvent,
        on_absent: impl FnOnce(),
    ) -> bool {
        let full = {
            let peers = self.peers.read().await;
            if peers.contains_key(&user_id) {
                return false;
            }
            on_absent();
            let senders = peers.iter().map(|(id, p)| (*id, p.signal_tx.clone()));
            queue_now(self.channel_id, senders, &event)
        };
        fan_out(self.channel_id, full, &event).await;
        true
    }

    /// Tell the room (with or without the user) that a webcam or screen
    /// share of one of the user's seats ended, unless that webcam (by user) or share
    /// (by stream id) is live again: then it belongs to a newer seat whose
    /// start was announced, and the stop would hide it. Decided and queued
    /// under the peers lock, like the starts. Returns whether it was sent.
    pub async fn announce_stopped(&self, event: ServerEvent, include_self: bool) -> bool {
        let full = {
            let peers = self.peers.read().await;
            let (user_id, live_again) = match &event {
                ServerEvent::WebcamStopped { user_id, .. } => {
                    (*user_id, self.webcams.read().await.contains_key(user_id))
                }
                ServerEvent::ScreenShareStopped {
                    user_id, stream_id, ..
                } => (
                    *user_id,
                    self.screen_shares.read().await.contains_key(stream_id),
                ),
                _ => return false,
            };
            if live_again {
                return false;
            }
            let senders = peers
                .iter()
                .filter(|(id, _)| include_self || **id != user_id)
                .map(|(id, p)| (*id, p.signal_tx.clone()));
            queue_now(self.channel_id, senders, &event)
        };
        fan_out(self.channel_id, full, &event).await;
        true
    }

    /// Remove a single screen share session by `stream_id`.
    pub async fn remove_screen_share(&self, stream_id: Uuid) -> Option<ScreenShareInfo> {
        let mut shares = self.screen_shares.write().await;
        shares.remove(&stream_id)
    }

    /// Remove all screen share sessions belonging to a user.
    /// Returns the removed entries so callers can broadcast stop events.
    pub async fn remove_user_screen_shares(&self, user_id: Uuid) -> Vec<ScreenShareInfo> {
        let mut shares = self.screen_shares.write().await;
        let stream_ids: Vec<Uuid> = shares
            .values()
            .filter(|s| s.user_id == user_id)
            .map(|s| s.stream_id)
            .collect();
        stream_ids
            .iter()
            .filter_map(|id| shares.remove(id))
            .collect()
    }

    /// Count how many active screen share streams a user has.
    pub async fn get_user_stream_count(&self, user_id: Uuid) -> usize {
        let shares = self.screen_shares.read().await;
        shares.values().filter(|s| s.user_id == user_id).count()
    }

    /// Get all screen shares.
    pub async fn get_screen_shares(&self) -> Vec<ScreenShareInfo> {
        let shares = self.screen_shares.read().await;
        shares.values().cloned().collect()
    }

    /// Remove a webcam session.
    pub async fn remove_webcam(&self, user_id: Uuid) -> Option<WebcamInfo> {
        let mut webcams = self.webcams.write().await;
        webcams.remove(&user_id)
    }

    /// Get all active webcams.
    pub async fn get_webcams(&self) -> Vec<WebcamInfo> {
        let webcams = self.webcams.read().await;
        webcams.values().cloned().collect()
    }

    /// Get participant info for all peers.
    pub async fn get_participant_info(&self) -> Vec<ParticipantInfo> {
        let peers = self.peers.read().await;
        let shares = self.screen_shares.read().await;
        let webcams = self.webcams.read().await;
        let mut info = Vec::with_capacity(peers.len());

        for (user_id, peer) in peers.iter() {
            info.push(ParticipantInfo {
                user_id: *user_id,
                username: Some(peer.username.clone()),
                display_name: Some(peer.display_name.clone()),
                muted: peer.is_self_muted().await,
                screen_sharing: shares.values().any(|s| s.user_id == *user_id),
                webcam_active: webcams.contains_key(user_id),
            });
        }

        info
    }

    /// Broadcast an event to all peers except one.
    ///
    /// Clones the peer list before sending to avoid holding the lock during I/O,
    /// which could delay peer additions/removals during broadcasts.
    pub async fn broadcast_except(&self, exclude_user_id: Uuid, event: ServerEvent) {
        // Clone sender handles to release lock before I/O
        let senders: Vec<(Uuid, mpsc::Sender<OutboundMsg>)> = {
            let peers = self.peers.read().await;
            peers
                .iter()
                .filter(|(id, _)| **id != exclude_user_id)
                .map(|(id, peer)| (*id, peer.signal_tx.clone()))
                .collect()
        };

        // Send without holding the lock; never block on another peer's queue
        fan_out(self.channel_id, senders, &event).await;
    }

    /// Broadcast an event to all peers.
    ///
    /// Clones the peer list before sending to avoid holding the lock during I/O.
    pub async fn broadcast_all(&self, event: ServerEvent) {
        // Clone sender handles to release lock before I/O
        let senders: Vec<(Uuid, mpsc::Sender<OutboundMsg>)> = {
            let peers = self.peers.read().await;
            peers
                .iter()
                .map(|(id, peer)| (*id, peer.signal_tx.clone()))
                .collect()
        };

        // Send without holding the lock; never block on another peer's queue
        fan_out(self.channel_id, senders, &event).await;
    }

    /// Get participant count.
    pub async fn participant_count(&self) -> usize {
        self.peers.read().await.len()
    }

    /// Check if room is empty.
    pub async fn is_empty(&self) -> bool {
        self.peers.read().await.is_empty()
    }
}

/// SFU Server managing all voice rooms.
pub struct SfuServer {
    /// Active rooms.
    rooms: Arc<RwLock<HashMap<Uuid, Arc<Room>>>>,
    /// WebRTC API instance.
    api: Arc<API>,
    /// Rate limiter for voice operations (global/redis).
    rate_limiter: Option<Arc<RateLimiter>>,
    /// Rate limiter for voice stats (local/memory).
    stats_limiter: Arc<VoiceStatsLimiter>,
    /// Per-peer, per-event-class token bucket limiter for voice signaling events.
    voice_rate_limiter: Arc<VoiceRateLimiter>,
    /// Peers whose connection failed for longer than [`PEER_FAILED_GRACE`].
    stale_tx: mpsc::UnboundedSender<StalePeer>,
    /// Receiving end of `stale_tx`, taken once by the reaper.
    stale_rx: Mutex<Option<mpsc::UnboundedReceiver<StalePeer>>>,
    /// Valkey client of the DM call store, so leaving a DM room can end its
    /// call. `None` in SFU-only unit tests.
    call_redis: Option<fred::clients::Client>,
    /// Phone wakes for DM calls ended by the leave hook.
    call_push: Option<Arc<crate::push::PushService>>,
}

/// Result of combining `VOICE_ICE_IP_FAMILY` with `PUBLIC_IP`.
#[derive(Debug, PartialEq, Eq)]
struct IceNetworkPlan {
    /// Network types to set on the `SettingEngine` (`None` = webrtc-ice default).
    network_types: Option<Vec<NetworkType>>,
    /// Whether NAT 1:1 mapping with `PUBLIC_IP` should be applied.
    use_public_ip: bool,
    /// `PUBLIC_IP` was set but its family is excluded by the family filter.
    public_ip_ignored: bool,
}

/// Single place deciding family / `PUBLIC_IP` behaviour. A `PUBLIC_IP` of an
/// excluded family is ignored; an unparseable one is kept (legacy behaviour).
/// Without a family restriction, `PUBLIC_IP` keeps its previous effect of
/// enabling both Udp4 and Udp6.
fn ice_network_plan(family: IceIpFamily, public_ip: Option<&str>) -> IceNetworkPlan {
    let ip_family_ok = public_ip.is_none_or(|ip| {
        !matches!(
            (ip.parse::<std::net::IpAddr>(), family),
            (Ok(std::net::IpAddr::V4(_)), IceIpFamily::V6)
                | (Ok(std::net::IpAddr::V6(_)), IceIpFamily::V4)
        )
    });
    let network_types = match family {
        IceIpFamily::V4 => Some(vec![NetworkType::Udp4]),
        IceIpFamily::V6 => Some(vec![NetworkType::Udp6]),
        IceIpFamily::Both if public_ip.is_some() => {
            Some(vec![NetworkType::Udp4, NetworkType::Udp6])
        }
        IceIpFamily::Both => None,
    };
    IceNetworkPlan {
        network_types,
        use_public_ip: public_ip.is_some() && ip_family_ok,
        public_ip_ignored: public_ip.is_some() && !ip_family_ok,
    }
}

/// Whether `ip` cannot be reached from the internet (loopback, private,
/// link-local, shared address space, unique local).
const fn is_private_or_local(ip: std::net::IpAddr) -> bool {
    match ip {
        std::net::IpAddr::V4(v4) => {
            let o = v4.octets();
            v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                || v4.is_unspecified()
                || (o[0] == 100 && (o[1] & 0xc0) == 64) // 100.64.0.0/10
        }
        std::net::IpAddr::V6(v6) => {
            let s = v6.segments();
            v6.is_loopback()
                || v6.is_unspecified()
                || (s[0] & 0xfe00) == 0xfc00 // unique local
                || (s[0] & 0xffc0) == 0xfe80 // link-local
        }
    }
}

/// The SFU offers its own host addresses only. Without `PUBLIC_IP`, a
/// server whose addresses are all private (behind NAT, in a container) is
/// unreachable for clients outside that network: say so at start.
fn warn_if_only_private_addresses(allowed: Option<&[String]>) {
    let Ok(found) = webrtc::util::ifaces::ifaces() else {
        return;
    };
    let any_public = found
        .iter()
        .filter(|i| allowed.is_none_or(|a| a.contains(&i.name)))
        .filter_map(|i| i.addr.map(|a| a.ip()))
        .any(|ip| !is_private_or_local(ip));
    if !any_public {
        warn!(
            "PUBLIC_IP is not set and this host has only private addresses: voice clients outside \
             this network cannot reach the SFU (it uses no STUN/TURN itself). Set PUBLIC_IP to the \
             address clients reach this host on."
        );
    }
}

/// Configured interface names that do not exist on this host.
fn unmatched_interface_names<'a>(allowed: &'a [String], available: &[&str]) -> Vec<&'a str> {
    allowed
        .iter()
        .map(String::as_str)
        .filter(|name| !available.contains(name))
        .collect()
}

/// Warn about configured interface names missing on this host, and when none of
/// the allowed interface names exists on this host (a typo
/// would otherwise yield zero ICE candidates and voice joins fail silently).
fn warn_if_no_interface_matches(allowed: &[String]) {
    match webrtc::util::ifaces::ifaces() {
        Ok(found) => {
            let mut names: Vec<&str> = found.iter().map(|i| i.name.as_str()).collect();
            names.sort_unstable();
            names.dedup();
            for name in unmatched_interface_names(allowed, &names) {
                warn!(interface = %name, available = ?names,
                    "VOICE_ICE_INTERFACES entry matches no interface on this host");
            }
            if !found.iter().any(|i| allowed.contains(&i.name)) {
                warn!(configured = ?allowed, available = ?names,
                    "VOICE_ICE_INTERFACES matches no interface on this host; voice will have no ICE candidates");
            }
        }
        Err(e) => {
            warn!(error = %e, "could not list network interfaces to validate VOICE_ICE_INTERFACES");
        }
    }
}

impl SfuServer {
    /// Create a new SFU server.
    pub fn new(config: Arc<Config>, rate_limiter: Option<RateLimiter>) -> Result<Self, VoiceError> {
        // Configure MediaEngine with Opus audio codec
        let mut media_engine = MediaEngine::default();

        // Register Opus codec for audio
        media_engine
            .register_codec(
                RTCRtpCodecParameters {
                    capability: RTCRtpCodecCapability {
                        mime_type: "audio/opus".to_string(),
                        clock_rate: 48000,
                        channels: 2,
                        sdp_fmtp_line: "minptime=10;useinbandfec=1".to_string(),
                        rtcp_feedback: vec![],
                    },
                    payload_type: 111,
                    ..Default::default()
                },
                RTPCodecType::Audio,
            )
            .map_err(|e| VoiceError::WebRtc(e.to_string()))?;

        // Register VP8 video codec (only video codec — ensures consistent PT across sessions)
        media_engine
            .register_codec(
                RTCRtpCodecParameters {
                    capability: RTCRtpCodecCapability {
                        mime_type: "video/VP8".to_string(),
                        clock_rate: 90000,
                        channels: 0,
                        sdp_fmtp_line: String::new(),
                        rtcp_feedback: vec![
                            RTCPFeedback {
                                typ: "goog-remb".to_string(),
                                parameter: String::new(),
                            },
                            RTCPFeedback {
                                typ: "ccm".to_string(),
                                parameter: "fir".to_string(),
                            },
                            RTCPFeedback {
                                typ: "nack".to_string(),
                                parameter: String::new(),
                            },
                            RTCPFeedback {
                                typ: "nack".to_string(),
                                parameter: "pli".to_string(),
                            },
                        ],
                    },
                    payload_type: 96,
                    ..Default::default()
                },
                RTPCodecType::Video,
            )
            .map_err(|e| VoiceError::WebRtc(e.to_string()))?;

        // The media-id and layer-id (rid) header extensions are not offered
        // on purpose: without them a sender that offers several encodings
        // gets an answer without simulcast and sends one stream. Forwarding
        // between simulcast layers is not ready for production yet.

        // Create interceptor registry
        let mut registry = Registry::new();
        registry = register_default_interceptors(registry, &mut media_engine)
            .map_err(|e| VoiceError::WebRtc(e.to_string()))?;

        // Configure SettingEngine for NAT traversal behind Docker/VPS
        let mut setting_engine = SettingEngine::default();
        // Family / PUBLIC_IP interaction lives in `ice_network_plan`.
        let plan = ice_network_plan(config.voice_ice_ip_family, config.public_ip.as_deref());
        if plan.public_ip_ignored {
            warn!(public_ip = ?config.public_ip, family = ?config.voice_ice_ip_family,
                "PUBLIC_IP ignored: address family excluded by VOICE_ICE_IP_FAMILY");
        }
        if let Some(types) = plan.network_types {
            setting_engine.set_network_types(types);
        }
        if let (true, Some(public_ip)) = (plan.use_public_ip, config.public_ip.as_deref()) {
            setting_engine.set_nat_1to1_ips(
                vec![public_ip.to_owned()],
                webrtc::ice_transport::ice_candidate_type::RTCIceCandidateType::Host,
            );
            info!(public_ip = %public_ip, "SFU NAT 1:1 IP configured");
        }

        if config.public_ip.is_none() {
            warn_if_only_private_addresses(config.voice_ice_interfaces.as_deref());
        }

        // Restrict ICE to the configured interfaces.
        if let Some(allowed) = config.voice_ice_interfaces.clone() {
            info!(interfaces = ?allowed, "SFU ICE interface filter configured");
            warn_if_no_interface_matches(&allowed);
            setting_engine
                .set_interface_filter(Box::new(move |name| allowed.iter().any(|a| a == name)));
        }

        // Pin ICE/media to the configured UDP range (firewall-friendly).
        if let (Some(min), Some(max)) = (config.voice_udp_port_min, config.voice_udp_port_max) {
            let ephemeral = EphemeralUDP::new(min, max)
                .map_err(|e| VoiceError::WebRtc(format!("invalid UDP port range: {e}")))?;
            setting_engine.set_udp_network(UDPNetwork::Ephemeral(ephemeral));
            info!(min, max, "SFU UDP port range configured");
        }

        // Build WebRTC API
        let api = APIBuilder::new()
            .with_media_engine(media_engine)
            .with_interceptor_registry(registry)
            .with_setting_engine(setting_engine)
            .build();

        info!("SFU server initialized");

        let (stale_tx, stale_rx) = mpsc::unbounded_channel();

        Ok(Self {
            rooms: Arc::new(RwLock::new(HashMap::new())),
            api: Arc::new(api),
            rate_limiter: rate_limiter.map(Arc::new),
            stats_limiter: Arc::new(VoiceStatsLimiter::default()),
            voice_rate_limiter: Arc::new(VoiceRateLimiter::new()),
            stale_tx,
            stale_rx: Mutex::new(Some(stale_rx)),
            call_redis: None,
            call_push: None,
        })
    }

    /// Attach the DM call store: when the last peer leaves a room, that
    /// channel's DM call ends (see [`super::leave::leave_voice_internal`]).
    #[must_use]
    pub fn with_call_redis(mut self, redis: fred::clients::Client) -> Self {
        self.call_redis = Some(redis);
        self
    }

    /// Attach the push service, so a DM call ended by the leave hook wakes
    /// the callees' phones (`ended`).
    #[must_use]
    pub fn with_call_push(mut self, push: Arc<crate::push::PushService>) -> Self {
        self.call_push = Some(push);
        self
    }

    /// The push service for DM call wakes, if attached.
    #[must_use]
    pub const fn call_push(&self) -> Option<&Arc<crate::push::PushService>> {
        self.call_push.as_ref()
    }

    /// The DM call store, if attached.
    #[must_use]
    pub const fn call_redis(&self) -> Option<&fred::clients::Client> {
        self.call_redis.as_ref()
    }

    /// Sender for reporting stale peers to the reaper.
    #[must_use]
    pub fn stale_peer_sender(&self) -> mpsc::UnboundedSender<StalePeer> {
        self.stale_tx.clone()
    }

    /// Start the task that removes peers reported as stale (see
    /// [`PEER_FAILED_GRACE`]). Call once after startup; later calls only log.
    /// The task holds a weak reference, so it ends when the SFU is dropped.
    pub fn spawn_stale_peer_reaper(
        self: &Arc<Self>,
        pool: PgPool,
        limiter: Option<ScreenShareLimiter>,
    ) -> JoinHandle<()> {
        let rx = self
            .stale_rx
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
        let sfu = Arc::downgrade(self);
        tokio::spawn(async move {
            let Some(mut rx) = rx else {
                warn!("Stale voice peer reaper already running");
                return;
            };
            while let Some(stale) = rx.recv().await {
                let Some(sfu) = sfu.upgrade() else { break };
                match super::leave::leave_voice_internal(
                    &sfu,
                    &pool,
                    stale.user_id,
                    stale.channel_id,
                    limiter.as_ref(),
                    Some(stale.session_id),
                )
                .await
                {
                    Ok(true) => info!(
                        user_id = %stale.user_id,
                        channel_id = %stale.channel_id,
                        "Removed voice peer after failed connection"
                    ),
                    Ok(false) => debug!(
                        user_id = %stale.user_id,
                        session_id = %stale.session_id,
                        "Stale voice peer already gone or replaced"
                    ),
                    Err(e) => warn!(
                        user_id = %stale.user_id,
                        channel_id = %stale.channel_id,
                        error = %e,
                        "Failed to remove stale voice peer"
                    ),
                }
            }
        })
    }

    /// Find the voice peer of `user_id` whose signaling queue is `tx`, i.e. the
    /// peer that belongs to that WebSocket. Returns `(channel_id, session_id)`.
    pub async fn find_peer_owned_by(
        &self,
        user_id: Uuid,
        tx: &mpsc::Sender<OutboundMsg>,
    ) -> Option<(Uuid, Uuid)> {
        let rooms: Vec<Arc<Room>> = self.rooms.read().await.values().cloned().collect();
        for room in rooms {
            if let Some(peer) = room.get_peer(user_id).await {
                if peer.signal_tx.same_channel(tx) {
                    return Some((room.channel_id, peer.session_id));
                }
            }
        }
        None
    }

    /// Start background cleanup task for voice stats rate limiter.
    /// This should be called once after server initialization to prevent memory leaks.
    /// Returns a handle to the spawned task.
    pub fn start_cleanup_task(&self) -> tokio::task::JoinHandle<()> {
        self.stats_limiter.start_cleanup_task()
    }

    /// `RTCConfiguration` of the server's own peer connections: no ICE
    /// servers. The SFU gathers host candidates only (its address is
    /// reachable, or `PUBLIC_IP` maps it); without STUN/TURN gathering each
    /// connection binds one UDP socket per local address instead of two
    /// (plus a relay allocation). Clients still get STUN/TURN from
    /// `GET /api/voice/ice-servers` and reach the SFU through a relay of
    /// their own when they need one.
    #[must_use]
    pub fn rtc_config(&self) -> RTCConfiguration {
        RTCConfiguration::default()
    }

    /// Get or create a room for a channel.
    pub async fn get_or_create_room(&self, channel_id: Uuid) -> Arc<Room> {
        let mut rooms = self.rooms.write().await;

        if let Some(room) = rooms.get(&channel_id) {
            return room.clone();
        }

        let room = Arc::new(Room::new(channel_id, DEFAULT_MAX_PARTICIPANTS));
        rooms.insert(channel_id, room.clone());

        debug!(channel_id = %channel_id, "Created new voice room");

        room
    }

    /// All current rooms.
    pub async fn rooms_snapshot(&self) -> Vec<Arc<Room>> {
        self.rooms.read().await.values().cloned().collect()
    }

    /// Get a room by channel ID.
    pub async fn get_room(&self, channel_id: Uuid) -> Option<Arc<Room>> {
        let rooms = self.rooms.read().await;
        rooms.get(&channel_id).cloned()
    }

    /// Remove a room if empty.
    pub async fn cleanup_room_if_empty(&self, channel_id: Uuid) {
        let mut rooms = self.rooms.write().await;

        if let Some(room) = rooms.get(&channel_id) {
            if room.is_empty().await {
                rooms.remove(&channel_id);
                debug!(channel_id = %channel_id, "Removed empty voice room");
            }
        }
    }

    /// Create a new peer with two `PeerConnection`s (publisher + subscriber).
    pub async fn create_peer(
        &self,
        user_id: Uuid,
        username: String,
        display_name: String,
        channel_id: Uuid,
        signal_tx: mpsc::Sender<OutboundMsg>,
    ) -> Result<Arc<Peer>, VoiceError> {
        let config = self.rtc_config();

        // Create two PeerConnections: one for publishing, one for subscribing
        let publisher_pc = Arc::new(
            self.api
                .new_peer_connection(config.clone())
                .await
                .map_err(|e| VoiceError::WebRtc(e.to_string()))?,
        );
        let subscriber_pc = Arc::new(
            self.api
                .new_peer_connection(config)
                .await
                .map_err(|e| VoiceError::WebRtc(e.to_string()))?,
        );

        let peer = Arc::new(Peer::new(
            user_id,
            username,
            display_name,
            channel_id,
            publisher_pc.clone(),
            subscriber_pc.clone(),
            signal_tx,
        ));

        // Set up connection state handlers for both PCs
        Self::register_pc_state_handler(&publisher_pc, &peer, self.stale_tx.clone(), "publisher");
        Self::register_pc_state_handler(&subscriber_pc, &peer, self.stale_tx.clone(), "subscriber");

        Ok(peer)
    }

    /// Set up track handling for a peer.
    pub fn setup_track_handler(&self, peer: &Arc<Peer>, room: &Arc<Room>) {
        let peer_weak = Arc::downgrade(peer);
        let room_weak = Arc::downgrade(room);
        let user_id = peer.user_id;
        let channel_id = peer.channel_id;

        peer.publisher_pc
            .on_track(Box::new(move |track, _receiver, transceiver| {
                let pw = peer_weak.clone();
                let rw = room_weak.clone();
                let uid = user_id;
                let cid = channel_id;

                Box::pin(async move {
                    // Parse simulcast RID — empty string means non-simulcast.
                    let rid = track.rid().to_string();
                    let layer = Layer::from_rid(&rid).unwrap_or(Layer::High);

                    info!(
                        user_id = %uid,
                        channel_id = %cid,
                        track_id = %track.id(),
                        kind = ?track.kind(),
                        rid = %rid,
                        layer = ?layer,
                        "Received track from peer"
                    );

                    // Upgrade weak references once — use for both source resolution and track setup
                    let (peer, room) = match (pw.upgrade(), rw.upgrade()) {
                        (Some(p), Some(r)) => (p, r),
                        _ => return,
                    };
                    // A seat that left or was replaced takes no new track.
                    if peer.is_retired() {
                        return;
                    }

                    // A simulcast track arrives as one `on_track` per layer
                    // (rid), all on one publisher transceiver (`mid`). The first
                    // layer to arrive binds the transceiver to its announcement
                    // and is the primary one (incoming track, subscriber
                    // tracks); the other layers only forward under that binding.
                    // webrtc-rs runs the callbacks of one connection in order,
                    // so the binding is in place before the next layer's turn.
                    let mid = transceiver.mid().map(|m| m.to_string()).unwrap_or_default();
                    let simulcast = !rid.is_empty() && track.kind() == RTPCodecType::Video;
                    let binding = if simulcast { peer.video_mid(&mid) } else { None };
                    let is_secondary_simulcast = binding.is_some();
                    let source_type = if let Some(bound) = binding {
                        // The transceiver's first layer was refused: so are its
                        // other layers (refusal already reported).
                        let super::peer::MidBinding::Source(st) = bound else {
                            return;
                        };
                        st
                    } else {
                        // Every track needs a source the server accepted
                        // (contract C30): the first audio track is the
                        // microphone (announced by the join); any further
                        // audio needs an accepted share with audio, video an
                        // accepted share or webcam start. One that stays
                        // unannounced is never forwarded.
                        // The microphone is the audio negotiated as such when
                        // joining (the first audio section of the first
                        // publisher offer), not whichever audio sends first.
                        let video = match track.kind() {
                            RTPCodecType::Audio
                                if peer.is_microphone_mid(Some(&mid)) && peer.claim_microphone() =>
                            {
                                None
                            }
                            RTPCodecType::Audio => Some(false),
                            RTPCodecType::Video => Some(true),
                            RTPCodecType::Unspecified => {
                                warn!("Unspecified track kind: {:?}", track.kind());
                                return;
                            }
                        };
                        let resolved = match video {
                            None => Some(TrackSource::Microphone),
                            Some(video) => take_live_source(&peer, &room, video).await,
                        };
                        if simulcast {
                            peer.bind_video_mid(&mid, resolved);
                        }
                        let Some(source) = resolved else {
                            // A retired seat's socket no longer cares.
                            if !peer.is_retired() {
                                reject_unannounced_track(&peer, &track, video == Some(true)).await;
                            }
                            return;
                        };
                        source
                    };

                    // Everything below adds state on behalf of this seat, so it
                    // runs under the seat's media lock and only while the seat
                    // is current and its source is live (see the module doc).
                    let media = peer.media.lock().await;
                    if peer.is_retired() || !source_is_live(&room, &peer, source_type).await {
                        debug!(user_id = %uid, source = %source_type, "Not forwarding: seat or source ended");
                        return;
                    }
                    crate::ws::test_hooks::track_before_forwarding(uid).await;
                    // One track per source: a key that is taken stays with its
                    // first track (no second forwarder into its subscribers).
                    if !is_secondary_simulcast
                        && peer.incoming_tracks.read().await.contains_key(&source_type)
                    {
                        warn!(user_id = %uid, source = %source_type, "Dropping a second track for a taken source");
                        return;
                    }

                    if simulcast {
                        peer.record_simulcast_layer(source_type, layer, track.clone());
                        room.track_router.store_simulcast_track(
                            uid,
                            source_type,
                            layer,
                            track.clone(),
                        );
                        debug!(
                            source = %uid,
                            source_type = ?source_type,
                            layer = ?layer,
                            "Stored simulcast track"
                        );
                    }

                    // Start RTP forwarder for every layer (each sends packets
                    // tagged with its layer so forward_rtp can filter by active_layer).
                    spawn_rtp_forwarder(
                        uid,
                        source_type,
                        layer,
                        track.clone(),
                        room.track_router.clone(),
                        peer.clone(),
                    );

                    // Secondary layers share the primary layer's subscriber
                    // tracks: they are only forwarded when a subscriber's active
                    // layer matches.
                    if is_secondary_simulcast {
                        return;
                    }

                    // For video tracks: send PLI every 3 seconds so subscribers
                    // always get a fresh keyframe. This is the standard Pion SFU
                    // pattern — webrtc-rs doesn't include an interval PLI interceptor
                    // by default, and PeerConnection::write_rtcp() doesn't reliably
                    // deliver PLI to the remote browser.
                    // The request goes to every layer of the source (a viewer
                    // may watch any) and ends with the source or the seat.
                    if source_type.is_video() {
                        let peer_weak = Arc::downgrade(&peer);
                        let primary_ssrc = track.ssrc();
                        // The source is live while its incoming track is
                        // still this one: a source ended and started again
                        // has a new track (and its own request task).
                        let own_track = Arc::downgrade(&track);
                        tokio::spawn(async move {
                            let mut interval = tokio::time::interval(std::time::Duration::from_secs(3));
                            let mut first = true;
                            loop {
                                interval.tick().await;
                                let Some(peer) = peer_weak.upgrade() else { break };
                                let live = peer
                                    .incoming_tracks
                                    .read()
                                    .await
                                    .get(&source_type)
                                    .is_some_and(|t| std::ptr::eq(Arc::as_ptr(t), own_track.as_ptr()));
                                // Only the immediate first tick may come before
                                // the track is stored.
                                if peer.is_retired() || (!live && !first) {
                                    break;
                                }
                                first = false;
                                let mut ssrcs: Vec<u32> = peer
                                    .simulcast_ssrcs(source_type)
                                    .into_iter()
                                    .map(|(_, ssrc)| ssrc)
                                    .collect();
                                if ssrcs.is_empty() {
                                    ssrcs.push(primary_ssrc);
                                }
                                if request_keyframe(&peer, &ssrcs).await.is_err() {
                                    break;
                                }
                            }
                        });
                    }

                    // Store incoming track (only for the primary / non-simulcast track)
                    peer.set_incoming_track(source_type, track.clone()).await;

                    // Create subscriber tracks for all existing peers
                    let other_peers = room.get_other_peers(uid).await;
                    info!(
                        source = %uid,
                        source_type = ?source_type,
                        other_peer_count = other_peers.len(),
                        "Forwarding track to subscribers"
                    );
                    let mut changed = Vec::new();
                    for other_peer in other_peers {
                        if forward_to(&room, &peer, source_type, &track, &other_peer).await {
                            changed.push(other_peer);
                        }
                    }
                    // Renegotiate outside the media lock (a slow socket must not
                    // hold up a leave or replacement of this seat).
                    drop(media);
                    for other_peer in changed {
                        if let Err(e) = Self::renegotiate(&other_peer).await {
                            warn!(
                                subscriber = %other_peer.user_id,
                                error = %e,
                                "Renegotiation failed after track add"
                            );
                        }
                    }
                })
            }));
    }

    /// Set up ICE candidate handlers for both publisher and subscriber PCs.
    pub fn setup_ice_handler(&self, peer: &Arc<Peer>) {
        Self::register_ice_callback(
            &peer.publisher_pc,
            peer.signal_tx.clone(),
            peer.channel_id,
            "publisher",
        );
        Self::register_ice_callback(
            &peer.subscriber_pc,
            peer.signal_tx.clone(),
            peer.channel_id,
            "subscriber",
        );
    }

    /// Register an ICE candidate callback on a single `PeerConnection`.
    fn register_ice_callback(
        pc: &Arc<RTCPeerConnection>,
        signal_tx: mpsc::Sender<OutboundMsg>,
        channel_id: Uuid,
        pc_label: &'static str,
    ) {
        pc.on_ice_candidate(Box::new(move |candidate| {
            let tx = signal_tx.clone();
            Box::pin(async move {
                let Some(c) = candidate else { return };
                let json = match c.to_json() {
                    Ok(j) => j,
                    Err(e) => {
                        warn!(pc = pc_label, error = %e, "Failed to convert ICE candidate to JSON");
                        return;
                    }
                };
                let candidate_str = match serde_json::to_string(&json) {
                    Ok(s) => s,
                    Err(e) => {
                        tracing::error!(pc = pc_label, error = %e, "Failed to serialize ICE candidate");
                        return;
                    }
                };
                if let Err(e) = tx
                    .send(OutboundMsg::voice(channel_id, ServerEvent::VoiceIceCandidate {
                        channel_id,
                        candidate: candidate_str,
                        pc_type: if pc_label == "subscriber" { PcType::Subscriber } else { PcType::Publisher },
                    }))
                    .await
                {
                    tracing::error!(
                        channel_id = %channel_id,
                        pc = pc_label,
                        error = %e,
                        "Failed to send ICE candidate - connection may fail"
                    );
                }
            })
        }));
    }

    /// Register a connection state change handler on a single `PeerConnection`.
    ///
    /// `Failed` starts a [`PEER_FAILED_GRACE`] timer; if the connection is
    /// still `Failed` afterwards, the peer is reported to the stale-peer
    /// reaper. `Disconnected` is only logged (it often recovers by itself).
    /// The callback holds only weak references to the peer and the PC, so it
    /// does not keep either alive.
    fn register_pc_state_handler(
        pc: &Arc<RTCPeerConnection>,
        peer: &Arc<Peer>,
        stale_tx: mpsc::UnboundedSender<StalePeer>,
        pc_label: &'static str,
    ) {
        let peer_weak = Arc::downgrade(peer);
        let pc_weak = Arc::downgrade(pc);
        let user_id = peer.user_id;
        let channel_id = peer.channel_id;
        let session_id = peer.session_id;
        pc.on_peer_connection_state_change(Box::new(move |state: RTCPeerConnectionState| {
            let pw = peer_weak.clone();
            let pcw = pc_weak.clone();
            let stale_tx = stale_tx.clone();
            Box::pin(async move {
                debug!(
                    user_id = %user_id,
                    channel_id = %channel_id,
                    pc = pc_label,
                    state = ?state,
                    "Peer connection state changed"
                );

                if pw.upgrade().is_none() {
                    return;
                }
                match state {
                    RTCPeerConnectionState::Failed => {
                        warn!(user_id = %user_id, pc = pc_label, "Peer connection failed");
                        let stale = StalePeer {
                            user_id,
                            channel_id,
                            session_id,
                        };
                        tokio::spawn(report_if_still_failed(pw, pcw, stale_tx, stale, pc_label));
                    }
                    RTCPeerConnectionState::Disconnected => {
                        warn!(user_id = %user_id, pc = pc_label, "Peer connection disconnected");
                    }
                    _ => {}
                }
            })
        }));
    }

    /// Handle a publisher offer from the client -- server creates answer.
    pub async fn handle_publisher_offer(
        peer: &Arc<Peer>,
        sdp: String,
    ) -> Result<String, VoiceError> {
        // Simulcast is not accepted (the server cannot receive it): without
        // the layer lines in the offer, the answer has none either, so the
        // sender falls back to one stream (its first encoding).
        let offer = RTCSessionDescription::offer(without_simulcast(&sdp))
            .map_err(|e| VoiceError::Signaling(e.to_string()))?;
        peer.publisher_pc.set_remote_description(offer).await?;
        let answer = peer.publisher_pc.create_answer(None).await?;
        peer.publisher_pc
            .set_local_description(answer.clone())
            .await?;
        Ok(answer.sdp)
    }

    /// Handle a subscriber answer from the client. If tracks changed while
    /// the answered offer was outstanding, the next offer goes out now.
    pub async fn handle_subscriber_answer(peer: &Arc<Peer>, sdp: String) -> Result<(), VoiceError> {
        let answer_directions = section_directions(&sdp);
        let answer =
            RTCSessionDescription::answer(sdp).map_err(|e| VoiceError::Signaling(e.to_string()))?;
        let (next, applied) = {
            let mut state = peer.negotiation.lock().await;
            if state.awaiting_answer_since.is_none() {
                // No offer is open (a repeated or late answer): nothing to do.
                debug!(user_id = %peer.user_id, "Ignoring a subscriber answer without an open offer");
                return Ok(());
            }
            let open_directions = peer
                .subscriber_pc
                .pending_local_description()
                .await
                .map(|d| section_directions(&d.sdp));
            if open_directions.is_some_and(|offer| !answer_fits(&offer, &answer_directions)) {
                // Answers an older offer than the open one (another number of
                // sections, or a section whose direction changed since).
                debug!(user_id = %peer.user_id, "Ignoring an answer to a superseded subscriber offer");
                return Ok(());
            }
            let applied = peer.subscriber_pc.set_remote_description(answer).await;
            state.awaiting_answer_since = None;
            state.repeated = false;
            // A change waited for this answer, or the answer did not fit:
            // offer (again) now.
            let next = if state.again || applied.is_err() {
                match Self::open_offer(peer, &mut state).await {
                    Ok(next) => Some(next),
                    Err(e) => {
                        // Not lost: try again shortly.
                        state.again = true;
                        Self::retry_offer_later(peer);
                        return Err(e);
                    }
                }
            } else {
                None
            };
            (next, applied)
        };
        if let Some((event, seq)) = next {
            // This runs in the peer's own socket loop.
            Self::send_offer(peer, event, seq, None).await?;
        }
        applied.map_err(VoiceError::from)
    }

    /// Renegotiate the subscriber connection of *another* peer after its
    /// tracks changed. Runs from the sender's socket loop, so the send is
    /// bounded by the short per-peer [`PEER_SIGNAL_SEND_TIMEOUT`].
    ///
    /// Serialized per peer (see [`super::peer::SubscriberNegotiation`]): while
    /// an offer waits for its answer, this only records that another offer
    /// is due; the answer sends it.
    pub async fn renegotiate(peer: &Arc<Peer>) -> Result<(), VoiceError> {
        Self::negotiate(peer, Some(PEER_SIGNAL_SEND_TIMEOUT)).await
    }

    /// [`Self::renegotiate`] for the peer of the socket that is handling the
    /// current event (e.g. the joiner's first subscriber offer). Uses the
    /// socket's own full slow-client bound, so a briefly busy queue does not
    /// drop the offer.
    pub async fn renegotiate_own(peer: &Arc<Peer>) -> Result<(), VoiceError> {
        Self::negotiate(peer, None).await
    }

    async fn negotiate(peer: &Arc<Peer>, bound: Option<Duration>) -> Result<(), VoiceError> {
        let (event, seq) = {
            let mut state = peer.negotiation.lock().await;
            if let Some(since) = state.awaiting_answer_since {
                if since.elapsed() < SUBSCRIBER_ANSWER_TIMEOUT {
                    state.again = true;
                    return Ok(());
                }
                // The answer is overdue: offer again instead of waiting forever.
                warn!(user_id = %peer.user_id, "Subscriber answer overdue, offering again");
            }
            state.repeated = false;
            Self::open_offer(peer, &mut state).await?
        };
        Self::send_offer(peer, event, seq, bound).await
    }

    /// Create and set a new subscriber offer under the negotiation lock and
    /// mark it open. Returns the event to send and the offer's number.
    async fn open_offer(
        peer: &Arc<Peer>,
        state: &mut super::peer::SubscriberNegotiation,
    ) -> Result<(OutboundMsg, u64), VoiceError> {
        let event = Self::create_subscriber_offer(peer).await?;
        state.awaiting_answer_since = Some(tokio::time::Instant::now());
        state.again = false;
        state.offer_seq += 1;
        Ok((event, state.offer_seq))
    }

    /// After [`SUBSCRIBER_ANSWER_TIMEOUT`], if offer `seq` is still open and
    /// unanswered: repeat it once (a lost offer or answer would otherwise
    /// block every later change), or, if it already is the repeat, release
    /// the open state so the next change offers at once.
    fn watch_offer(peer: &Arc<Peer>, seq: u64) {
        let peer = Arc::downgrade(peer);
        tokio::spawn(async move {
            tokio::time::sleep(SUBSCRIBER_ANSWER_TIMEOUT).await;
            let Some(peer) = peer.upgrade() else { return };
            let repeat = {
                let mut state = peer.negotiation.lock().await;
                if state.offer_seq != seq || state.awaiting_answer_since.is_none() {
                    return;
                }
                if state.repeated && !state.again {
                    debug!(user_id = %peer.user_id, "Repeated subscriber offer unanswered; releasing");
                    state.awaiting_answer_since = None;
                    return;
                }
                // Repeat once; a change that came in meanwhile is offered now
                // (with a repeat of its own) instead of being dropped. Changes
                // are only ever dropped with the seat.
                state.repeated = !state.again;
                match Self::open_offer(&peer, &mut state).await {
                    Ok(next) => next,
                    Err(e) => {
                        debug!(user_id = %peer.user_id, error = %e, "Repeating an unanswered offer failed");
                        state.awaiting_answer_since = None;
                        // The change it carried is not lost: try again shortly.
                        state.again = true;
                        Self::retry_offer_later(&peer);
                        return;
                    }
                }
            };
            let (event, seq) = repeat;
            let _ = Self::send_offer(&peer, event, seq, Some(PEER_SIGNAL_SEND_TIMEOUT)).await;
        });
    }

    /// Offer once more after a short pause if a change is still pending and
    /// no offer is open.
    fn retry_offer_later(peer: &Arc<Peer>) {
        let peer = Arc::downgrade(peer);
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_secs(1)).await;
            let Some(peer) = peer.upgrade() else { return };
            let pending = {
                let state = peer.negotiation.lock().await;
                state.again && state.awaiting_answer_since.is_none()
            };
            if pending && !peer.is_retired() {
                let _ = Self::negotiate(&peer, Some(PEER_SIGNAL_SEND_TIMEOUT)).await;
            }
        });
    }

    /// Send an offer created under the negotiation lock and watch it. If it
    /// cannot be queued, the client never answers it: forget that it is
    /// outstanding, so the next change offers again.
    async fn send_offer(
        peer: &Arc<Peer>,
        event: OutboundMsg,
        seq: u64,
        bound: Option<Duration>,
    ) -> Result<(), VoiceError> {
        let sent = match bound {
            Some(bound) => send_outbound_within(&peer.signal_tx, event, bound).await,
            None => send_outbound(&peer.signal_tx, event).await,
        };
        if sent.is_err() {
            let mut state = peer.negotiation.lock().await;
            if state.offer_seq == seq {
                state.awaiting_answer_since = None;
                // Neither the change this offer carried nor one that waited
                // for it may be lost: offer again shortly (the queue may only
                // have been busy; a closed socket ends the seat).
                state.again = true;
                Self::retry_offer_later(peer);
            }
        } else {
            Self::watch_offer(peer, seq);
        }
        Self::check_offer_sent(peer, sent)
    }

    async fn create_subscriber_offer(peer: &Arc<Peer>) -> Result<OutboundMsg, VoiceError> {
        let offer = peer.subscriber_pc.create_offer(None).await?;
        peer.subscriber_pc
            .set_local_description(offer.clone())
            .await?;
        Ok(OutboundMsg::voice(
            peer.channel_id,
            ServerEvent::VoiceSubscriberOffer {
                channel_id: peer.channel_id,
                sdp: offer.sdp,
            },
        ))
    }

    fn check_offer_sent(
        peer: &Arc<Peer>,
        sent: Result<(), OutboundSendError>,
    ) -> Result<(), VoiceError> {
        sent.map_err(|e| {
            tracing::error!(
                user_id = %peer.user_id,
                channel_id = %peer.channel_id,
                error = %e,
                "failed to send subscriber offer — client will not receive tracks"
            );
            VoiceError::Outbound(e)
        })?;
        Ok(())
    }

    /// Handle an ICE candidate from a peer, routed by `pc_type`.
    pub async fn handle_ice_candidate(
        peer: &Arc<Peer>,
        candidate_str: &str,
        pc_type: &PcType,
    ) -> Result<(), VoiceError> {
        let candidate: webrtc::ice_transport::ice_candidate::RTCIceCandidateInit =
            serde_json::from_str(candidate_str)
                .map_err(|e| VoiceError::Signaling(format!("Invalid ICE candidate: {e}")))?;

        let pc = match pc_type {
            PcType::Subscriber => &peer.subscriber_pc,
            PcType::Publisher => &peer.publisher_pc,
        };
        pc.add_ice_candidate(candidate).await?;

        Ok(())
    }

    /// Check if a user can join voice (rate limit check).
    pub async fn check_rate_limit(&self, user_id: Uuid) -> Result<(), VoiceError> {
        if let Some(limiter) = &self.rate_limiter {
            // Note: We use user_id as identifier (stringified)
            let result = match limiter
                .check(RateLimitCategory::VoiceJoin, &user_id.to_string())
                .await
            {
                Ok(result) => result,
                // Voice join is a WebSocket limit: fail open while Valkey is
                // unavailable.
                Err(_) if limiter.allow_during_outage(RateLimitCategory::VoiceJoin) => {
                    return Ok(());
                }
                Err(e) => return Err(VoiceError::Internal(e.to_string())),
            };

            if !result.allowed {
                return Err(VoiceError::RateLimited("voice_join"));
            }
        }
        Ok(())
    }

    /// Check if a user can report voice stats (rate limit check).
    pub async fn check_stats_rate_limit(&self, user_id: Uuid) -> Result<(), VoiceError> {
        self.stats_limiter.check_stats(user_id).await
    }

    /// Access the per-peer voice signaling rate limiter.
    #[must_use]
    pub const fn voice_rate_limiter(&self) -> &Arc<VoiceRateLimiter> {
        &self.voice_rate_limiter
    }

    /// Get active room count.
    pub async fn room_count(&self) -> usize {
        self.rooms.read().await.len()
    }
}

/// After [`PEER_FAILED_GRACE`], report the peer as stale if it still exists
/// and this PC is still `Failed`.
async fn report_if_still_failed(
    peer: Weak<Peer>,
    pc: Weak<RTCPeerConnection>,
    stale_tx: mpsc::UnboundedSender<StalePeer>,
    stale: StalePeer,
    pc_label: &'static str,
) {
    tokio::time::sleep(PEER_FAILED_GRACE).await;
    if peer.upgrade().is_none() {
        return;
    }
    let still_failed = pc
        .upgrade()
        .is_some_and(|pc| pc.connection_state() == RTCPeerConnectionState::Failed);
    if !still_failed {
        return;
    }
    warn!(
        user_id = %stale.user_id,
        channel_id = %stale.channel_id,
        pc = pc_label,
        "Peer connection did not recover, removing peer"
    );
    if stale_tx.send(stale).is_err() {
        warn!(user_id = %stale.user_id, "Stale voice peer reaper is not running");
    }
}

#[cfg(test)]
mod fan_out_tests {
    use super::*;

    fn joined_event(user_id: Uuid) -> ServerEvent {
        ServerEvent::VoiceUserLeft {
            channel_id: Uuid::nil(),
            user_id,
        }
    }

    #[tokio::test(start_paused = true)]
    async fn fan_out_skips_stalled_peer_without_blocking_others() {
        let stalled_id = Uuid::from_u128(1);
        let (stalled_tx, mut stalled_rx) = mpsc::channel(1);
        stalled_tx
            .try_send(OutboundMsg::Ping)
            .expect("fill stalled queue"); // never drained during the call
        let (healthy_a_tx, mut healthy_a_rx) = mpsc::channel(8);
        let (healthy_b_tx, mut healthy_b_rx) = mpsc::channel(8);

        let started = tokio::time::Instant::now();
        tokio::time::timeout(
            PEER_SIGNAL_SEND_TIMEOUT * 2,
            fan_out(
                Uuid::nil(),
                vec![
                    (Uuid::from_u128(2), healthy_a_tx),
                    (stalled_id, stalled_tx),
                    (Uuid::from_u128(3), healthy_b_tx),
                ],
                &joined_event(stalled_id),
            ),
        )
        .await
        .expect("fan_out must not block on a stalled peer");
        assert!(started.elapsed() <= PEER_SIGNAL_SEND_TIMEOUT);

        for rx in [&mut healthy_a_rx, &mut healthy_b_rx] {
            assert!(matches!(
                rx.try_recv(),
                Ok(OutboundMsg::Voice {
                    event: ServerEvent::VoiceUserLeft { .. },
                    ..
                })
            ));
        }
        // The stalled peer only holds what it had before; its event was skipped.
        assert!(matches!(stalled_rx.try_recv(), Ok(OutboundMsg::Ping)));
        assert!(stalled_rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn fan_out_skips_closed_peer() {
        let (closed_tx, closed_rx) = mpsc::channel(1);
        drop(closed_rx);
        let (healthy_tx, mut healthy_rx) = mpsc::channel(8);
        fan_out(
            Uuid::nil(),
            vec![
                (Uuid::from_u128(1), closed_tx),
                (Uuid::from_u128(2), healthy_tx),
            ],
            &joined_event(Uuid::from_u128(9)),
        )
        .await;
        assert!(healthy_rx.try_recv().is_ok());
    }
}

#[cfg(test)]
mod renegotiate_tests {
    use super::*;

    /// The joiner's own subscriber offer must survive a briefly busy queue:
    /// it gets the full slow-client bound, not the short fan-out bound.
    #[tokio::test]
    async fn own_offer_waits_past_fan_out_timeout() {
        let sfu = SfuServer::new(Arc::new(Config::default_for_test()), None).expect("sfu");
        let (tx, mut rx) = mpsc::channel(1);
        let peer = sfu
            .create_peer(
                Uuid::from_u128(1),
                "joiner".into(),
                "Joiner".into(),
                Uuid::from_u128(2),
                tx.clone(),
            )
            .await
            .expect("peer");
        tx.try_send(OutboundMsg::Ping).expect("fill queue");

        // Drain the queue only after the fan-out bound has passed.
        let drain = tokio::spawn(async move {
            tokio::time::sleep(PEER_SIGNAL_SEND_TIMEOUT * 2).await;
            assert!(matches!(rx.recv().await, Some(OutboundMsg::Ping)));
            rx.recv().await
        });

        SfuServer::renegotiate_own(&peer)
            .await
            .expect("own offer must not be dropped by the fan-out bound");
        assert!(matches!(
            drain.await.expect("drain task"),
            Some(OutboundMsg::Voice {
                event: ServerEvent::VoiceSubscriberOffer { .. },
                ..
            })
        ));
        peer.close().await;
    }
}

#[cfg(test)]
mod negotiation_tests {
    use webrtc::track::track_local::track_local_static_rtp::TrackLocalStaticRTP;

    use super::*;

    fn audio_track(id: &str) -> Arc<TrackLocalStaticRTP> {
        Arc::new(TrackLocalStaticRTP::new(
            RTCRtpCodecCapability {
                mime_type: "audio/opus".into(),
                clock_rate: 48000,
                channels: 2,
                ..Default::default()
            },
            id.into(),
            id.into(),
        ))
    }

    fn offer_sdp(msg: OutboundMsg) -> String {
        match msg {
            OutboundMsg::Voice {
                event: ServerEvent::VoiceSubscriberOffer { sdp, .. },
                ..
            } => sdp,
            other => panic!("expected a subscriber offer, got {other:?}"),
        }
    }

    /// While an offer waits for its answer, further changes send nothing;
    /// the answer triggers exactly one new offer that contains them all.
    #[tokio::test]
    async fn changes_during_an_open_offer_coalesce_into_one_offer() {
        let mut config = Config::default_for_test();
        config.stun_server = "stun:127.0.0.1:3478".into();
        let sfu = SfuServer::new(Arc::new(config), None).expect("sfu");
        let (tx, mut rx) = mpsc::channel(16);
        let peer = sfu
            .create_peer(
                Uuid::from_u128(1),
                "a".into(),
                "A".into(),
                Uuid::from_u128(2),
                tx,
            )
            .await
            .expect("peer");
        let client = sfu
            .api
            .new_peer_connection(RTCConfiguration::default())
            .await
            .expect("client pc");

        peer.add_outgoing_track(
            Uuid::from_u128(3),
            TrackSource::Microphone,
            Uuid::from_u128(100),
            audio_track("u3"),
        )
        .await
        .expect("track 1");
        SfuServer::renegotiate(&peer).await.expect("offer 1");
        let first = offer_sdp(rx.try_recv().expect("first offer sent"));

        for n in 4..6 {
            peer.add_outgoing_track(
                Uuid::from_u128(n),
                TrackSource::Microphone,
                Uuid::from_u128(100),
                audio_track(&format!("u{n}")),
            )
            .await
            .expect("more tracks");
            SfuServer::renegotiate(&peer).await.expect("coalesced");
        }
        assert!(
            rx.try_recv().is_err(),
            "a second offer went out before the answer"
        );

        client
            .set_remote_description(RTCSessionDescription::offer(first).unwrap())
            .await
            .expect("client applies offer 1");
        let answer = client.create_answer(None).await.expect("answer");
        client
            .set_local_description(answer.clone())
            .await
            .expect("client local answer");
        SfuServer::handle_subscriber_answer(&peer, answer.sdp)
            .await
            .expect("server applies answer 1");

        let second = offer_sdp(rx.try_recv().expect("the pending change was offered"));
        assert_eq!(second.matches("m=audio").count(), 3, "{second}");
        assert!(rx.try_recv().is_err(), "more than one follow-up offer");

        let _ = client.close().await;
        peer.close().await;
    }

    async fn setup() -> (
        SfuServer,
        Arc<Peer>,
        mpsc::Receiver<OutboundMsg>,
        Arc<RTCPeerConnection>,
    ) {
        let mut config = Config::default_for_test();
        config.stun_server = "stun:127.0.0.1:3478".into();
        let sfu = SfuServer::new(Arc::new(config), None).expect("sfu");
        let (tx, rx) = mpsc::channel(16);
        let peer = sfu
            .create_peer(
                Uuid::from_u128(1),
                "a".into(),
                "A".into(),
                Uuid::from_u128(2),
                tx,
            )
            .await
            .expect("peer");
        let client = Arc::new(
            sfu.api
                .new_peer_connection(RTCConfiguration::default())
                .await
                .expect("client pc"),
        );
        (sfu, peer, rx, client)
    }

    async fn answer_for(client: &RTCPeerConnection, offer: String) -> String {
        client
            .set_remote_description(RTCSessionDescription::offer(offer).unwrap())
            .await
            .expect("client applies offer");
        let answer = client.create_answer(None).await.expect("answer");
        client
            .set_local_description(answer.clone())
            .await
            .expect("client local answer");
        answer.sdp
    }

    /// An answer that arrives while no offer is open is ignored, not an
    /// error that could disturb the connection.
    #[tokio::test]
    async fn stray_answer_is_ignored() {
        let (_sfu, peer, mut rx, client) = setup().await;
        peer.add_outgoing_track(
            Uuid::from_u128(3),
            TrackSource::Microphone,
            Uuid::from_u128(100),
            audio_track("u3"),
        )
        .await
        .expect("track");
        SfuServer::renegotiate(&peer).await.expect("offer");
        let offer = offer_sdp(rx.try_recv().expect("offer sent"));
        let answer = answer_for(&client, offer).await;
        SfuServer::handle_subscriber_answer(&peer, answer.clone())
            .await
            .expect("answer applied");
        SfuServer::handle_subscriber_answer(&peer, answer)
            .await
            .expect("a repeated answer is ignored");
        assert!(rx.try_recv().is_err());
        let _ = client.close().await;
        peer.close().await;
    }

    /// An answer to an older offer (other number of m-lines than the open
    /// one) is ignored; the open offer still takes its own answer.
    #[tokio::test]
    async fn answer_to_a_superseded_offer_is_ignored() {
        let (_sfu, peer, mut rx, client) = setup().await;
        let old_client = setup().await.3;
        peer.add_outgoing_track(
            Uuid::from_u128(3),
            TrackSource::Microphone,
            Uuid::from_u128(100),
            audio_track("u3"),
        )
        .await
        .expect("track");
        SfuServer::renegotiate(&peer).await.expect("offer");
        let first = offer_sdp(rx.try_recv().expect("offer sent"));
        // The open offer is replaced (as after the answer timeout) by one
        // with two m-lines.
        peer.add_outgoing_track(
            Uuid::from_u128(4),
            TrackSource::Microphone,
            Uuid::from_u128(100),
            audio_track("u4"),
        )
        .await
        .expect("track 2");
        peer.negotiation.lock().await.awaiting_answer_since =
            Some(tokio::time::Instant::now() - SUBSCRIBER_ANSWER_TIMEOUT);
        SfuServer::renegotiate(&peer).await.expect("re-offer");
        let second = offer_sdp(rx.try_recv().expect("re-offer sent"));

        let stale = answer_for(&old_client, first).await;
        SfuServer::handle_subscriber_answer(&peer, stale)
            .await
            .expect("stale answer ignored");
        assert!(peer
            .negotiation
            .lock()
            .await
            .awaiting_answer_since
            .is_some());

        let answer = answer_for(&client, second).await;
        SfuServer::handle_subscriber_answer(&peer, answer)
            .await
            .expect("current answer applied");
        assert!(peer
            .negotiation
            .lock()
            .await
            .awaiting_answer_since
            .is_none());
        let _ = client.close().await;
        let _ = old_client.close().await;
        peer.close().await;
    }

    /// An offer that could not be sent is offered again shortly, even with
    /// no other change pending (the change it carried must not be lost).
    #[tokio::test(start_paused = true)]
    async fn an_offer_that_could_not_be_sent_is_offered_again() {
        let (_sfu, peer, mut rx, _client) = setup().await;
        peer.add_outgoing_track(
            Uuid::from_u128(3),
            TrackSource::Microphone,
            Uuid::from_u128(100),
            audio_track("u3"),
        )
        .await
        .expect("track");
        // A full queue: the offer cannot be sent within its bound.
        for _ in 0..16 {
            if peer.signal_tx.try_send(OutboundMsg::Ping).is_err() {
                break;
            }
        }
        assert!(SfuServer::renegotiate(&peer).await.is_err());
        while rx.try_recv().is_ok() {}
        tokio::time::sleep(Duration::from_secs(2)).await;
        offer_sdp(rx.try_recv().expect("the change was offered again"));
        peer.close().await;
    }

    /// A change made while a repeated offer waits is offered when the repeat
    /// times out, not dropped with it.
    #[tokio::test(start_paused = true)]
    async fn a_change_during_the_repeat_is_offered_after_it() {
        let (_sfu, peer, mut rx, _client) = setup().await;
        peer.add_outgoing_track(
            Uuid::from_u128(3),
            TrackSource::Microphone,
            Uuid::from_u128(100),
            audio_track("u3"),
        )
        .await
        .expect("track");
        SfuServer::renegotiate(&peer).await.expect("offer");
        offer_sdp(rx.try_recv().expect("offer sent"));
        tokio::time::sleep(SUBSCRIBER_ANSWER_TIMEOUT + Duration::from_secs(1)).await;
        offer_sdp(rx.try_recv().expect("repeat sent"));

        peer.add_outgoing_track(
            Uuid::from_u128(4),
            TrackSource::Microphone,
            Uuid::from_u128(100),
            audio_track("u4"),
        )
        .await
        .expect("track 2");
        SfuServer::renegotiate(&peer).await.expect("coalesced");
        assert!(rx.try_recv().is_err());

        tokio::time::sleep(SUBSCRIBER_ANSWER_TIMEOUT + Duration::from_secs(1)).await;
        let next = offer_sdp(rx.try_recv().expect("the change was offered"));
        assert_eq!(next.matches("m=audio").count(), 2, "{next}");
        peer.close().await;
    }

    /// Telling the room about a seat never holds the room's peer list while
    /// a participant's queue is full: a join or leave is not held up.
    #[tokio::test]
    async fn room_events_do_not_hold_the_peer_list_for_a_full_queue() {
        let (sfu, me, mut rx, _client) = setup().await;
        let room = Room::new(me.channel_id, 10);
        let (slow_tx, _slow_rx) = mpsc::channel(1);
        slow_tx.try_send(OutboundMsg::Ping).expect("fill the queue");
        let slow = sfu
            .create_peer(
                Uuid::from_u128(3),
                "s".into(),
                "S".into(),
                me.channel_id,
                slow_tx,
            )
            .await
            .expect("peer");
        room.add_peer(slow).await.expect("add");
        room.add_peer(me.clone()).await.expect("add");
        let event = ServerEvent::VoiceUserLeft {
            channel_id: me.channel_id,
            user_id: Uuid::from_u128(9),
        };
        let (sent, free) = tokio::join!(room.broadcast_if_current(&me, event, true), async {
            tokio::task::yield_now().await;
            tokio::time::timeout(PEER_SIGNAL_SEND_TIMEOUT / 5, room.peers.write())
                .await
                .map(drop)
        });
        assert!(sent);
        assert!(free.is_ok(), "the peer list was held for the full queue");
        assert!(rx.try_recv().is_ok(), "the seat's own queue got the event");
    }

    /// A refused simulcast transceiver stays refused across a new offer
    /// that still sends the same track on it (its late layers must not bind
    /// to a newer announcement); it is forgotten once the offer sends
    /// another track there or stops sending.
    #[tokio::test]
    async fn a_refused_transceiver_is_forgotten_only_when_its_track_changes() {
        let (_sfu, peer, _rx, _client) = setup().await;
        let offer = |msid: &str, dir: &str| {
            format!(
                "v=0\r\nm=audio 9 UDP/TLS/RTP/SAVPF 111\r\na=mid:0\r\na=sendonly\r\n\
                 m=video 9 UDP/TLS/RTP/SAVPF 96\r\na=mid:1\r\na={dir}\r\na=msid:{msid}\r\n"
            )
        };
        peer.forget_refused_mids(&offer("s cam", "sendonly"));
        peer.bind_video_mid("1", None);
        peer.forget_refused_mids(&offer("s cam", "sendonly"));
        assert_eq!(
            peer.video_mid("1"),
            Some(crate::voice::peer::MidBinding::Refused),
            "same track, still refused"
        );
        peer.forget_refused_mids(&offer("s cam2", "sendonly"));
        assert_eq!(
            peer.video_mid("1"),
            None,
            "another track on the transceiver"
        );
        peer.bind_video_mid("1", None);
        peer.forget_refused_mids(&offer("s cam2", "inactive"));
        assert_eq!(peer.video_mid("1"), None, "the transceiver stopped sending");
    }

    /// What a listener receives from a person belongs to that person's seat:
    /// a track of the person's new seat replaces the old seat's entry (also
    /// while the old seat's teardown has not removed it yet), a second one of
    /// the same seat is refused, and the old seat's late removal by track
    /// leaves the new one.
    #[tokio::test]
    async fn a_new_seats_track_replaces_the_old_seats_entry() {
        use webrtc::track::track_local::TrackLocal;
        let (_sfu, listener, _rx, _client) = setup().await;
        let user = Uuid::from_u128(7);
        let (old_seat, new_seat) = (Uuid::from_u128(70), Uuid::from_u128(71));
        let shared = Uuid::from_u128(72);
        for source in [
            TrackSource::Microphone,
            TrackSource::Webcam,
            TrackSource::ScreenVideo(shared),
        ] {
            let old = audio_track(&format!("old-{source}"));
            let new = audio_track(&format!("new-{source}"));
            listener
                .add_outgoing_track(user, source, old_seat, old.clone())
                .await
                .expect("old seat's track");
            listener
                .add_outgoing_track(user, source, new_seat, new.clone())
                .await
                .expect("the new seat's track replaces the old one");
            assert!(
                listener
                    .add_outgoing_track(user, source, new_seat, audio_track("dup"))
                    .await
                    .is_err(),
                "a second track of the same seat is refused"
            );
            assert!(
                !listener.remove_outgoing_track_if(user, source, &old).await,
                "the old seat's late removal finds nothing of its own"
            );
        }
        let kept = listener.outgoing_track_list().await;
        assert_eq!(kept.len(), 3, "exactly one track per source");
        assert!(
            kept.iter().all(|t| t.id().starts_with("new-")),
            "{:?}",
            kept.iter().map(|t| t.id().to_owned()).collect::<Vec<_>>()
        );
    }

    /// A track waiting for its announcement on a seat that is retired stops
    /// waiting at once (no grace period, nothing bound).
    #[tokio::test]
    async fn retiring_a_seat_ends_its_wait_for_an_announcement() {
        let (_sfu, peer, _rx, _client) = setup().await;
        let waiting = {
            let peer = peer.clone();
            tokio::spawn(async move {
                let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(30);
                peer.take_source_until(true, deadline).await
            })
        };
        tokio::task::yield_now().await;
        peer.retire();
        let got = tokio::time::timeout(std::time::Duration::from_secs(2), waiting)
            .await
            .expect("the wait ended with the seat")
            .expect("join");
        assert!(got.is_none(), "a retired seat bound {got:?}");
    }

    /// An answer to an older offer with the same number of media sections
    /// (a track was removed in between, so a section changed direction) is
    /// ignored too.
    #[tokio::test]
    async fn answer_to_an_offer_before_a_removal_is_ignored() {
        let (_sfu, peer, mut rx, client) = setup().await;
        let track = audio_track("u3");
        peer.add_outgoing_track(
            Uuid::from_u128(3),
            TrackSource::Microphone,
            Uuid::from_u128(100),
            track.clone(),
        )
        .await
        .expect("track");
        SfuServer::renegotiate(&peer).await.expect("offer");
        let first = offer_sdp(rx.try_recv().expect("offer sent"));
        let stale = answer_for(&client, first).await;

        assert!(
            peer.remove_outgoing_track_if(Uuid::from_u128(3), TrackSource::Microphone, &track)
                .await
        );
        peer.negotiation.lock().await.awaiting_answer_since =
            Some(tokio::time::Instant::now() - SUBSCRIBER_ANSWER_TIMEOUT);
        SfuServer::renegotiate(&peer).await.expect("re-offer");
        let second = offer_sdp(rx.try_recv().expect("re-offer sent"));
        assert_eq!(second.matches("m=audio").count(), 1, "{second}");

        SfuServer::handle_subscriber_answer(&peer, stale)
            .await
            .expect("stale answer ignored");
        assert!(
            peer.negotiation
                .lock()
                .await
                .awaiting_answer_since
                .is_some(),
            "an answer to the earlier offer was taken for the open one"
        );
        let _ = client.close().await;
        peer.close().await;
    }

    /// An offer nobody answers is sent once more after the answer timeout;
    /// if that one stays unanswered too, the open state is released so the
    /// next change offers right away.
    #[tokio::test(start_paused = true)]
    async fn unanswered_offer_is_repeated_once_then_released() {
        let (_sfu, peer, mut rx, _client) = setup().await;
        peer.add_outgoing_track(
            Uuid::from_u128(3),
            TrackSource::Microphone,
            Uuid::from_u128(100),
            audio_track("u3"),
        )
        .await
        .expect("track");
        SfuServer::renegotiate(&peer).await.expect("offer");
        offer_sdp(rx.try_recv().expect("offer sent"));

        tokio::time::sleep(SUBSCRIBER_ANSWER_TIMEOUT + Duration::from_secs(1)).await;
        offer_sdp(rx.try_recv().expect("the unanswered offer is repeated"));

        tokio::time::sleep(SUBSCRIBER_ANSWER_TIMEOUT + Duration::from_secs(1)).await;
        assert!(rx.try_recv().is_err(), "repeated more than once");
        assert!(peer
            .negotiation
            .lock()
            .await
            .awaiting_answer_since
            .is_none());

        SfuServer::renegotiate(&peer).await.expect("next change");
        offer_sdp(rx.try_recv().expect("next change offers at once"));
        peer.close().await;
    }
}

#[cfg(test)]
mod host_candidate_tests {
    use super::*;

    /// With STUN and TURN configured (for clients), the server's own peer
    /// connections still use no ICE server and gather host candidates only.
    #[tokio::test]
    async fn server_peer_connections_gather_host_candidates_only() {
        let mut config = Config::default_for_test();
        config.stun_server = "stun:127.0.0.1:3478".into();
        config.turn_servers = vec!["turn:127.0.0.1:3478".into()];
        config.turn_shared_secret = Some("secret".into());
        config.voice_udp_port_min = Some(50100);
        config.voice_udp_port_max = Some(50120);
        let sfu = SfuServer::new(Arc::new(config), None).expect("sfu");
        assert!(sfu.rtc_config().ice_servers.is_empty());

        let (tx, _rx) = mpsc::channel(16);
        let peer = sfu
            .create_peer(
                Uuid::from_u128(1),
                "a".into(),
                "A".into(),
                Uuid::from_u128(2),
                tx,
            )
            .await
            .expect("peer");
        let pc = &peer.subscriber_pc;
        pc.create_data_channel("probe", None).await.expect("dc");
        let offer = pc.create_offer(None).await.expect("offer");
        let mut gathered = pc.gathering_complete_promise().await;
        pc.set_local_description(offer).await.expect("local desc");
        let _ = tokio::time::timeout(Duration::from_secs(10), gathered.recv()).await;
        let sdp = pc.local_description().await.expect("desc").sdp;
        peer.close().await;
        let candidates: Vec<&str> = sdp
            .lines()
            .filter(|l| l.starts_with("a=candidate:"))
            .collect();
        assert!(!candidates.is_empty(), "no candidates gathered:\n{sdp}");
        for line in candidates {
            assert!(line.contains(" typ host"), "non-host candidate: {line}");
        }
    }
}

#[cfg(test)]
mod udp_range_tests {
    use super::*;

    #[tokio::test]
    async fn sfu_candidates_use_configured_port_range() {
        let mut config = Config::default_for_test();
        config.voice_udp_port_min = Some(50000);
        config.voice_udp_port_max = Some(50010);
        let sfu = SfuServer::new(Arc::new(config), None).expect("sfu");

        let pc = sfu
            .api
            .new_peer_connection(RTCConfiguration::default())
            .await
            .expect("pc");
        pc.create_data_channel("probe", None).await.expect("dc");
        let offer = pc.create_offer(None).await.expect("offer");
        let mut gathered = pc.gathering_complete_promise().await;
        pc.set_local_description(offer).await.expect("local desc");
        let _ = tokio::time::timeout(Duration::from_secs(10), gathered.recv()).await;

        let sdp = pc.local_description().await.expect("local description").sdp;
        let ports: Vec<u16> = sdp
            .lines()
            .filter_map(|l| l.strip_prefix("a=candidate:"))
            .filter_map(|l| {
                let f: Vec<&str> = l.split_whitespace().collect();
                (f.get(2).is_some_and(|p| p.eq_ignore_ascii_case("udp")))
                    .then(|| f.get(5)?.parse().ok())
                    .flatten()
            })
            .collect();
        let _ = pc.close().await;
        assert!(!ports.is_empty(), "no UDP host candidates gathered:\n{sdp}");
        for p in ports {
            assert!(
                (50000..=50010).contains(&p),
                "candidate port {p} outside range"
            );
        }
    }
}

#[cfg(test)]
mod ice_filter_tests {
    use super::*;

    /// Gather host candidate IPs for an SFU built from `config`.
    async fn gather_ips(config: Config) -> Vec<std::net::IpAddr> {
        let sfu = SfuServer::new(Arc::new(config), None).expect("sfu");
        let pc = sfu
            .api
            .new_peer_connection(RTCConfiguration::default())
            .await
            .expect("pc");
        pc.create_data_channel("probe", None).await.expect("dc");
        let offer = pc.create_offer(None).await.expect("offer");
        let mut gathered = pc.gathering_complete_promise().await;
        pc.set_local_description(offer).await.expect("local desc");
        let _ = tokio::time::timeout(Duration::from_secs(10), gathered.recv()).await;
        let sdp = pc.local_description().await.expect("desc").sdp;
        let _ = pc.close().await;
        sdp.lines()
            .filter_map(|l| l.strip_prefix("a=candidate:"))
            .filter_map(|l| l.split_whitespace().nth(4)?.parse().ok())
            .collect()
    }

    /// Does the host have a non-loopback address of the given family?
    async fn has_family(v6: bool) -> bool {
        webrtc::util::vnet::net::Net::new(None)
            .get_interfaces()
            .await
            .iter()
            .flat_map(|i| i.addrs().iter().map(|a| a.addr()))
            .any(|a| !a.is_loopback() && a.is_ipv6() == v6)
    }

    #[tokio::test]
    async fn sfu_candidates_respect_ip_family() {
        let mut config = Config::default_for_test();
        config.voice_ice_ip_family = IceIpFamily::V4;
        let ips = gather_ips(config).await;
        if has_family(false).await {
            assert!(!ips.is_empty(), "host has IPv4 but no candidates gathered");
        } else {
            eprintln!("SKIP non-empty v4 assertion: host has no non-loopback IPv4");
        }
        assert!(ips.iter().all(std::net::IpAddr::is_ipv4), "{ips:?}");

        // Opposite family: nothing of the excluded family may appear (on a
        // host without global IPv6 this also yields no candidates at all).
        let mut config = Config::default_for_test();
        config.voice_ice_ip_family = IceIpFamily::V6;
        let ips = gather_ips(config).await;
        assert!(ips.iter().all(std::net::IpAddr::is_ipv6), "{ips:?}");
    }

    /// Approach: loopback is suppressed by webrtc-ice, so pick a real
    /// interface from the host (the first one that has a non-loopback IPv4
    /// address) and assert all candidate IPs belong to it; a nonexistent
    /// name must yield no host candidates.
    #[tokio::test]
    async fn sfu_candidates_respect_interface_filter() {
        let ifaces = webrtc::util::vnet::net::Net::new(None)
            .get_interfaces()
            .await;
        let Some(chosen) = ifaces.iter().find(|i| {
            i.addrs()
                .iter()
                .any(|a| a.addr().is_ipv4() && !a.addr().is_loopback())
        }) else {
            eprintln!("SKIP: host has no non-loopback IPv4 interface");
            return;
        };
        let own: Vec<std::net::IpAddr> = chosen.addrs().iter().map(|a| a.addr()).collect();

        let mut config = Config::default_for_test();
        config.voice_ice_interfaces = Some(vec![chosen.name().to_owned()]);
        let ips = gather_ips(config).await;
        assert!(!ips.is_empty(), "expected candidates on {}", chosen.name());
        assert!(
            ips.iter().all(|ip| own.contains(ip)),
            "{ips:?} not in {own:?}"
        );

        let mut config = Config::default_for_test();
        config.voice_ice_interfaces = Some(vec!["no-such-if0".to_owned()]);
        let ips = gather_ips(config).await;
        assert!(ips.is_empty(), "{ips:?}");
    }

    #[test]
    fn private_and_local_addresses_are_recognised() {
        for ip in [
            "10.1.2.3",
            "172.16.0.1",
            "192.168.1.1",
            "127.0.0.1",
            "169.254.1.1",
            "100.64.0.1",
            "fd00::1",
            "fe80::1",
            "::1",
        ] {
            assert!(is_private_or_local(ip.parse().unwrap()), "{ip}");
        }
        for ip in ["203.0.113.5", "8.8.8.8", "2001:db8::1", "100.128.0.1"] {
            assert!(!is_private_or_local(ip.parse().unwrap()), "{ip}");
        }
    }

    #[test]
    fn unmatched_interface_names_lists_each_missing_name() {
        let allowed = vec!["eth0".to_string(), "typo0".to_string(), "wg9".to_string()];
        assert_eq!(
            unmatched_interface_names(&allowed, &["eth0", "lo"]),
            vec!["typo0", "wg9"]
        );
        assert!(unmatched_interface_names(&allowed, &["eth0", "typo0", "wg9"]).is_empty());
    }

    #[test]
    fn network_plan_covers_family_and_public_ip() {
        use NetworkType::{Udp4, Udp6};
        let v4 = Some("203.0.113.5");
        let v6 = Some("2001:db8::1");
        let plan = |t: Option<Vec<NetworkType>>, use_ip, ign| IceNetworkPlan {
            network_types: t,
            use_public_ip: use_ip,
            public_ip_ignored: ign,
        };
        // Both
        assert_eq!(
            ice_network_plan(IceIpFamily::Both, None),
            plan(None, false, false)
        );
        let both = Some(vec![Udp4, Udp6]);
        assert_eq!(
            ice_network_plan(IceIpFamily::Both, v4),
            plan(both.clone(), true, false)
        );
        assert_eq!(
            ice_network_plan(IceIpFamily::Both, v6),
            plan(both, true, false)
        );
        // V4
        let t4 = Some(vec![Udp4]);
        assert_eq!(
            ice_network_plan(IceIpFamily::V4, None),
            plan(t4.clone(), false, false)
        );
        assert_eq!(
            ice_network_plan(IceIpFamily::V4, v4),
            plan(t4.clone(), true, false)
        );
        assert_eq!(ice_network_plan(IceIpFamily::V4, v6), plan(t4, false, true));
        // V6
        let t6 = Some(vec![Udp6]);
        assert_eq!(
            ice_network_plan(IceIpFamily::V6, None),
            plan(t6.clone(), false, false)
        );
        assert_eq!(
            ice_network_plan(IceIpFamily::V6, v6),
            plan(t6.clone(), true, false)
        );
        assert_eq!(ice_network_plan(IceIpFamily::V6, v4), plan(t6, false, true));
    }
}
