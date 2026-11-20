//! Voice WebSocket Message Handlers
//!
//! Handles voice signaling messages from WebSocket connections.

use std::sync::Arc;

use sqlx::PgPool;
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};
use uuid::Uuid;
use vc_common::protocol::PcType;

use super::error::VoiceError;
use super::metrics::{get_guild_id, store_metrics};
use super::rate_limit::EventClass;
use super::screen_share::{
    validate_source_label, ScreenShareError, ScreenShareInfo, ScreenShareLimiter,
};
use super::sfu::{ReplacedMedia, Room, SfuServer};
use super::stats::VoiceStats;
use super::track::spawn_subscriber_remb_reader;
use super::track_types::{LayerPreference, TrackSource};
use super::webcam::WebcamInfo;
use super::{queries, Quality};
use crate::ws::{ClientEvent, OutboundMsg, ServerEvent, VoiceParticipant};

/// Map a permission error to a voice error, logging database errors instead of masking them.
fn map_permission_error(e: crate::permissions::PermissionError, context: &str) -> VoiceError {
    match e {
        crate::permissions::PermissionError::DatabaseError(msg) => {
            error!(error = %msg, "Database error during {context}");
            // Detail stays in the log; the client gets a generic error.
            VoiceError::Internal(format!("permission check failed during {context}: {msg}"))
        }
        _ => VoiceError::Unauthorized,
    }
}

/// Handle a voice-related client event (socket without a known login
/// session; see [`handle_voice_event_for_session`]).
pub async fn handle_voice_event(
    sfu: &Arc<SfuServer>,
    pool: &PgPool,
    user_id: Uuid,
    event: ClientEvent,
    tx: &mpsc::Sender<OutboundMsg>,
    screen_share_limiter: Option<&ScreenShareLimiter>,
) -> Result<(), VoiceError> {
    handle_voice_event_for_session(sfu, pool, user_id, None, event, tx, screen_share_limiter).await
}

/// Handle a voice-related client event from a socket that authenticated
/// with login session `auth_session` (recorded on a seat it joins).
pub async fn handle_voice_event_for_session(
    sfu: &Arc<SfuServer>,
    pool: &PgPool,
    user_id: Uuid,
    auth_session: Option<Uuid>,
    event: ClientEvent,
    tx: &mpsc::Sender<OutboundMsg>,
    screen_share_limiter: Option<&ScreenShareLimiter>,
) -> Result<(), VoiceError> {
    // Per-peer, per-event-class token bucket rate limiting.
    //
    // Events that exceed the limit are dropped silently (no error returned
    // to the client) per the design in
    // `docs/superpowers/specs/2026-04-15-voice-screenshare-fixes-design.md`
    // Fix 2. VoiceJoin is handled by the separate join rate limit; VoiceStats
    // is handled by `VoiceStatsLimiter`; VoiceLeave is never limited.
    let rate_limit_class: Option<EventClass> = match &event {
        ClientEvent::VoiceIceCandidate { pc_type, .. } => Some(match pc_type {
            PcType::Publisher => EventClass::IceCandidatePublisher,
            PcType::Subscriber => EventClass::IceCandidateSubscriber,
        }),
        ClientEvent::VoicePublisherOffer { .. } => Some(EventClass::PublisherOffer),
        ClientEvent::VoiceSubscriberAnswer { .. } => Some(EventClass::SubscriberAnswer),
        ClientEvent::VoiceScreenShareStart { .. } | ClientEvent::VoiceScreenShareStop { .. } => {
            Some(EventClass::ScreenShareToggle)
        }
        ClientEvent::VoiceMute { .. }
        | ClientEvent::VoiceUnmute { .. }
        | ClientEvent::VoiceWebcamStart { .. }
        | ClientEvent::VoiceWebcamStop { .. } => Some(EventClass::MuteOrWebcamToggle),
        ClientEvent::VoiceSetLayerPreference { .. } => Some(EventClass::SetLayerPreference),
        _ => None,
    };
    if let Some(class) = rate_limit_class {
        if !sfu.voice_rate_limiter().try_acquire(user_id, class) {
            // Structured log for observability. A follow-up PR should wire
            // this into a Prometheus `voice_rate_limit_drops_total` counter.
            warn!(
                user_id = %user_id,
                event_class = class.as_scope(),
                "voice_rate_limit_drop",
            );
            // A share or webcam start is a request the client waits for
            // (C30): answer it instead of dropping it silently.
            if matches!(
                event,
                ClientEvent::VoiceScreenShareStart { .. } | ClientEvent::VoiceWebcamStart { .. }
            ) {
                return Err(VoiceError::RateLimited("media_start"));
            }
            return Ok(());
        }
    }

    match event {
        ClientEvent::VoiceJoin { channel_id } => {
            let result =
                handle_join(sfu, pool, user_id, channel_id, tx, screen_share_limiter, auth_session)
                    .await;
            crate::observability::metrics::record_voice_join(result.is_ok());
            result
        }
        ClientEvent::VoiceLeave { channel_id } => {
            handle_leave(sfu, pool, user_id, channel_id, screen_share_limiter, tx).await
        }
        ClientEvent::VoicePublisherOffer { channel_id, sdp } => {
            handle_publisher_offer(sfu, user_id, channel_id, &sdp, tx).await
        }
        ClientEvent::VoiceSubscriberAnswer { channel_id, sdp } => {
            handle_subscriber_answer(sfu, user_id, channel_id, &sdp, tx).await
        }
        ClientEvent::VoiceIceCandidate {
            channel_id,
            candidate,
            pc_type,
        } => handle_ice_candidate(sfu, user_id, channel_id, &candidate, &pc_type, tx).await,
        ClientEvent::VoiceMute { channel_id } => {
            handle_mute(sfu, user_id, channel_id, true, tx).await
        }
        ClientEvent::VoiceUnmute { channel_id } => {
            handle_mute(sfu, user_id, channel_id, false, tx).await
        }
        ClientEvent::VoiceStats {
            channel_id,
            session_id,
            latency,
            packet_loss,
            jitter,
            quality,
            timestamp,
        } => {
            let stats = VoiceStats {
                session_id,
                latency,
                packet_loss,
                jitter,
                quality,
                timestamp,
            };
            handle_voice_stats(sfu, pool, user_id, channel_id, stats, tx).await
        }
        ClientEvent::VoiceScreenShareStart {
            channel_id,
            stream_id,
            quality,
            has_audio,
            source_label,
        } => {
            handle_screen_share_start(
                sfu,
                pool,
                HandleScreenShareStartParams {
                    user_id,
                    channel_id,
                    stream_id,
                    quality,
                    has_audio,
                    source_label: &source_label,
                },
                screen_share_limiter,
                tx,
            )
            .await
        }
        ClientEvent::VoiceScreenShareStop {
            channel_id,
            stream_id,
        } => {
            handle_screen_share_stop(sfu, user_id, channel_id, stream_id, screen_share_limiter, tx)
                .await
        }
        ClientEvent::VoiceWebcamStart {
            channel_id,
            quality,
        } => handle_webcam_start(sfu, pool, user_id, channel_id, quality, tx).await,
        ClientEvent::VoiceWebcamStop { channel_id } => {
            handle_webcam_stop(sfu, user_id, channel_id, tx).await
        }
        ClientEvent::VoiceSetLayerPreference {
            channel_id,
            target_user_id,
            track_source,
            preferred_layer,
        } => {
            handle_set_layer_preference(
                sfu,
                user_id,
                channel_id,
                target_user_id,
                track_source,
                preferred_layer,
                tx,
            )
            .await
        }
        _ => Ok(()), // Non-voice events handled elsewhere
    }
}

/// Handle a user joining a voice channel.
async fn handle_join(
    sfu: &Arc<SfuServer>,
    pool: &PgPool,
    user_id: Uuid,
    channel_id: Uuid,
    tx: &mpsc::Sender<OutboundMsg>,
    screen_share_limiter: Option<&ScreenShareLimiter>,
    auth_session: Option<Uuid>,
) -> Result<(), VoiceError> {
    info!(user_id = %user_id, channel_id = %channel_id, "User joining voice channel");

    // The access epochs this join is decided at (`ws::access`): checked
    // again once the peer is in the room.
    let channel = crate::db::find_channel_by_id(pool, channel_id)
        .await
        .map_err(|e| VoiceError::Internal(format!("failed to load channel: {e}")))?;
    let guild_id = channel.as_ref().and_then(|c| c.guild_id);
    // The channel's member limit (unset or 0: none). Nobody bypasses it.
    let user_limit = channel
        .as_ref()
        .and_then(|c| c.user_limit)
        .and_then(|l| usize::try_from(l).ok())
        .filter(|l| *l > 0);
    let is_dm = channel
        .as_ref()
        .is_some_and(|c| c.channel_type == crate::db::ChannelType::Dm);
    let epochs = crate::ws::access::registry().snapshot(user_id, guild_id);

    // Check if user has VIEW_CHANNEL and VOICE_CONNECT permissions
    let ctx = crate::permissions::require_channel_access(pool, user_id, channel_id)
        .await
        .map_err(|e| map_permission_error(e, "voice join"))?;

    if !ctx.has_permission(crate::permissions::GuildPermissions::VOICE_CONNECT) {
        return Err(VoiceError::Unauthorized);
    }
    if is_dm {
        ensure_no_block_in_dm(pool, user_id, channel_id).await?;
    }

    sfu.check_rate_limit(user_id).await?;
    crate::ws::test_hooks::voice_join_after_check(user_id).await;

    let (username, display_name) = queries::find_user_username_display_name(pool, user_id)
        .await
        .map_err(|e| VoiceError::Internal(format!("failed to fetch user info: {e}")))?;

    let room = sfu.get_or_create_room(channel_id).await;

    let peer = sfu
        .create_peer(
            user_id,
            username.clone(),
            display_name.clone(),
            channel_id,
            tx.clone(),
        )
        .await?;

    sfu.setup_ice_handler(&peer);
    sfu.setup_track_handler(&peer, &room);
    // What the owning socket's writes are fenced by: room content reaches it
    // only while these epochs hold, or after a new check.
    peer.set_admission(crate::ws::access::VoiceAdmission { guild_id, epochs });
    peer.set_auth_session(auth_session);

    let replaced = match room.add_peer_with_limit(peer.clone(), user_limit).await {
        Ok(replaced) => replaced,
        Err(e) => {
            // Refused (room full): the new connections are not used.
            peer.close().await;
            sfu.cleanup_room_if_empty(channel_id).await;
            return Err(e);
        }
    };
    if let Some(replaced) = replaced {
        retire_replaced_peer(pool, &room, guild_id, tx, replaced, screen_share_limiter).await;
    }

    // An access change that bumped its epoch before this point may have
    // looked at the rooms before the peer was in: re-check now. A change
    // after this point sees the peer (it bumps, then looks), so no join can
    // slip past a revocation.
    if crate::ws::access::registry().snapshot(user_id, guild_id) != epochs
        && !super::revoke::has_voice_access(pool, user_id, channel_id)
            .await
            .unwrap_or(false)
    {
        super::leave::leave_voice_internal(
            sfu,
            pool,
            user_id,
            channel_id,
            screen_share_limiter,
            Some(peer.session_id),
        )
        .await?;
        return Err(VoiceError::Unauthorized);
    }

    // In the dual-PC model, the server does NOT send an initial offer.
    // The client will send a VoicePublisherOffer after joining, which
    // triggers subscribe_to_existing_tracks() on the first offer.

    let participants: Vec<VoiceParticipant> = room
        .get_participant_info()
        .await
        .into_iter()
        .map(|p| VoiceParticipant {
            user_id: p.user_id,
            username: p.username,
            display_name: p.display_name,
            muted: p.muted,
            screen_sharing: p.screen_sharing,
            webcam_active: p.webcam_active,
        })
        .collect();

    let screen_shares = room.get_screen_shares().await;
    let webcams = room.get_webcams().await;

    crate::ws::handlers::send_outbound(
        tx,
        OutboundMsg::voice(
            channel_id,
            ServerEvent::VoiceRoomState {
                channel_id,
                participants,
                screen_shares,
                webcams,
            },
        ),
    )
    .await?;

    room.broadcast_except(
        user_id,
        ServerEvent::VoiceUserJoined {
            channel_id,
            user_id,
            username,
            display_name,
        },
    )
    .await;

    info!(
        user_id = %user_id,
        channel_id = %channel_id,
        "User joined voice channel"
    );
    crate::observability::metrics::record_voice_session_start();

    Ok(())
}

/// How long the old connection of a replaced session may take to close
/// before it is left to the WebRTC stack (the join does not wait for it).
const REPLACED_PEER_CLOSE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// How long `voice_removed` may wait for the old socket's queue.
const REPLACED_NOTICE_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(250);

/// Tear down the peer that a new join of the same user replaced (contract
/// C31 `session_replaced`): end its screen shares and webcam (slots released,
/// the others told), remove its tracks from the other participants, tell its
/// socket `voice_removed { session_replaced }` (unless the new join came on
/// that same socket), record the session end and close its connections in
/// the background, so a slow or failing teardown never holds up the join.
async fn retire_replaced_peer(
    pool: &PgPool,
    room: &Room,
    guild_id: Option<Uuid>,
    new_tx: &mpsc::Sender<OutboundMsg>,
    replaced: ReplacedMedia,
    screen_share_limiter: Option<&ScreenShareLimiter>,
) {
    let ReplacedMedia {
        peer: old,
        screen_shares,
        webcam,
    } = replaced;
    let channel_id = room.channel_id;
    let user_id = old.user_id;

    release_replaced_media(
        room,
        user_id,
        &screen_shares,
        webcam.is_some(),
        screen_share_limiter,
    )
    .await;
    room.detach_media(&old).await;

    if !old.signal_tx.same_channel(new_tx) {
        let notice = OutboundMsg::Event(ServerEvent::VoiceRemoved {
            channel_id,
            reason: crate::ws::VoiceRemovedReason::SessionReplaced,
        });
        if let Err(e) = crate::ws::handlers::send_outbound_within(
            &old.signal_tx,
            notice,
            REPLACED_NOTICE_TIMEOUT,
        )
        .await
        {
            debug!(user_id = %user_id, error = %e, "Could not tell the replaced session");
        }
    }

    super::leave::end_session_records(pool, &old, guild_id);
    tokio::spawn(async move {
        if tokio::time::timeout(REPLACED_PEER_CLOSE_TIMEOUT, old.close())
            .await
            .is_err()
        {
            warn!(user_id = %old.user_id, "Closing a replaced voice peer timed out");
        }
    });
}

/// End the screen shares and webcam of a peer that a new join of the same
/// user replaced: release their slots and tell the other participants, so no
/// ghost share remains.
async fn release_replaced_media(
    room: &Room,
    user_id: Uuid,
    screen_shares: &[ScreenShareInfo],
    had_webcam: bool,
    screen_share_limiter: Option<&ScreenShareLimiter>,
) {
    let channel_id = room.channel_id;
    if !screen_shares.is_empty() {
        if let Some(limiter) = screen_share_limiter {
            limiter.stop_n(channel_id, screen_shares.len()).await;
        } else {
            warn!("Screen share limiter unavailable — replaced shares not released");
        }
    }
    for share in screen_shares {
        room.broadcast_except(
            user_id,
            ServerEvent::ScreenShareStopped {
                channel_id,
                user_id,
                stream_id: share.stream_id,
                reason: "replaced".to_string(),
            },
        )
        .await;
    }
    if had_webcam {
        room.broadcast_except(
            user_id,
            ServerEvent::WebcamStopped {
                channel_id,
                user_id,
                reason: "replaced".to_string(),
            },
        )
        .await;
    }
}

/// Refuse a join to a DM call when the user and any other participant of
/// the DM blocked each other (either direction; same rule as starting or
/// answering a call). The error does not say who blocked whom. A failed
/// lookup refuses too.
async fn ensure_no_block_in_dm(
    pool: &PgPool,
    user_id: Uuid,
    channel_id: Uuid,
) -> Result<(), VoiceError> {
    let others: Vec<Uuid> = crate::chat::queries::list_dm_participants(pool, channel_id)
        .await
        .map_err(|e| {
            warn!(channel_id = %channel_id, error = %e, "DM participant lookup failed on voice join");
            // Still refused, but not reported as a block.
            VoiceError::Internal("DM participant lookup failed".to_string())
        })?
        .into_iter()
        .map(|p| p.user_id)
        .filter(|id| *id != user_id)
        .collect();
    match crate::chat::queries::blocked_among(pool, user_id, &others).await {
        Ok(blocked) if blocked.is_empty() => Ok(()),
        Ok(_) => Err(VoiceError::JoinRefused),
        Err(e) => {
            warn!(user_id = %user_id, error = %e, "Block lookup failed on voice join");
            Err(VoiceError::Internal("block lookup failed".to_string()))
        }
    }
}

/// Handle a user leaving a voice channel.
///
/// Leaving never requires channel access: a user whose access was revoked
/// mid-call must still be able to leave.
async fn handle_leave(
    sfu: &Arc<SfuServer>,
    pool: &PgPool,
    user_id: Uuid,
    channel_id: Uuid,
    screen_share_limiter: Option<&ScreenShareLimiter>,
    tx: &mpsc::Sender<OutboundMsg>,
) -> Result<(), VoiceError> {
    info!(user_id = %user_id, channel_id = %channel_id, "User leaving voice channel");
    // Only this socket's own seat: after the user moved to another device,
    // a leave from the old one changes nothing.
    let Some(room) = sfu.get_room(channel_id).await else {
        return Ok(());
    };
    let Ok(peer) = owned_peer(&room, user_id, tx).await else {
        debug!(user_id = %user_id, channel_id = %channel_id, "Leave from a socket without a seat ignored");
        return Ok(());
    };
    super::leave::leave_voice_internal(
        sfu,
        pool,
        user_id,
        channel_id,
        screen_share_limiter,
        Some(peer.session_id),
    )
    .await
    .map(|_| ())
}

/// The peer of `user_id` in `room` that belongs to the socket `tx`. Every
/// voice event except the join acts only on its own socket's peer: after
/// the same user joined from another socket, the old socket's events find
/// nothing (`PARTICIPANT_NOT_FOUND`) and never reach the new session.
async fn owned_peer(
    room: &Room,
    user_id: Uuid,
    tx: &mpsc::Sender<OutboundMsg>,
) -> Result<Arc<super::peer::Peer>, VoiceError> {
    room.get_peer(user_id)
        .await
        .filter(|peer| peer.signal_tx.same_channel(tx))
        .ok_or(VoiceError::ParticipantNotFound(user_id))
}

/// Handle a publisher SDP offer from the client.
///
/// The client creates offers for the publisher PC (mic, screen, webcam tracks).
/// On the first offer, we also subscribe this peer to existing peers' tracks.
async fn handle_publisher_offer(
    sfu: &Arc<SfuServer>,
    user_id: Uuid,
    channel_id: Uuid,
    sdp: &str,
    tx: &mpsc::Sender<OutboundMsg>,
) -> Result<(), VoiceError> {
    debug!(user_id = %user_id, channel_id = %channel_id, "Received publisher offer");

    let room = sfu
        .get_room(channel_id)
        .await
        .ok_or(VoiceError::RoomNotFound(channel_id))?;

    let peer = owned_peer(&room, user_id, tx).await?;

    match SfuServer::handle_publisher_offer(&peer, sdp.to_string()).await {
        Ok(answer_sdp) => {
            if let Err(e) = crate::ws::handlers::send_outbound(
                &peer.signal_tx,
                OutboundMsg::voice(
                    channel_id,
                    ServerEvent::VoicePublisherAnswer {
                        channel_id,
                        sdp: answer_sdp,
                    },
                ),
            )
            .await
            {
                tracing::error!(
                    user_id = %user_id,
                    channel_id = %channel_id,
                    error = %e,
                    "failed to send publisher answer"
                );
                return Err(e.into());
            }

            // On first publisher offer, subscribe to existing peers' tracks
            if !peer
                .has_subscribed
                .swap(true, std::sync::atomic::Ordering::Relaxed)
            {
                subscribe_to_existing_tracks(&room, &peer, user_id).await;
            }
        }
        Err(e) => {
            tracing::error!(user_id = %user_id, error = %e, "failed to handle publisher offer");
            return Err(e);
        }
    }

    Ok(())
}

/// Subscribe a newly joined peer to all existing peers' incoming tracks.
///
/// Creates subscriber tracks via the `TrackRouter`, adds them to the new peer's
/// subscriber PC, and renegotiates so the client receives the offer with new tracks.
async fn subscribe_to_existing_tracks(
    room: &Arc<super::sfu::Room>,
    peer: &Arc<super::peer::Peer>,
    user_id: Uuid,
) {
    // Collect peer refs under read lock, then release it before async I/O
    let other_peers: Vec<(Uuid, Arc<super::peer::Peer>)> = {
        let peers = room.peers.read().await;
        peers
            .iter()
            .filter(|(id, _)| **id != user_id)
            .map(|(id, p)| (*id, p.clone()))
            .collect()
    };

    for (other_id, other_peer) in &other_peers {
        let incoming = other_peer.incoming_tracks.read().await;
        for (source_type, track) in incoming.iter() {
            match room
                .track_router
                .create_subscriber_track(*other_id, *source_type, peer, track)
            {
                Ok(local_track) => {
                    match peer
                        .add_outgoing_track(*other_id, *source_type, local_track.clone())
                        .await
                    {
                        Ok(sender) => {
                            if source_type.is_video() {
                                spawn_subscriber_remb_reader(
                                    room.track_router.clone(),
                                    user_id,
                                    *other_id,
                                    *source_type,
                                    sender,
                                    peer.signal_tx.clone(),
                                    room.channel_id,
                                );
                            }
                            // PLI for screen shares is sent AFTER renegotiation
                            // (see below) so the keyframe arrives when the subscriber
                            // is actually ready to decode.
                        }
                        Err(e) => {
                            room.track_router.remove_subscriptions_into(&[local_track]);
                            warn!(
                                source = %other_id,
                                subscriber = %user_id,
                                error = %e,
                                "Failed to add outgoing track for existing peer"
                            );
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!(
                        user_id = %user_id,
                        source_user = %other_id,
                        source = ?source_type,
                        error = %e,
                        "failed to create subscriber track"
                    );
                }
            }
        }
    }

    // Renegotiate subscriber PC to send the offer with new tracks
    if peer.outgoing_tracks_count().await > 0 {
        if let Err(e) = SfuServer::renegotiate_own(peer).await {
            error!(user_id = %user_id, error = %e, "failed to renegotiate subscriber after join");
        }
    }

    // PLI for screen shares is sent from handle_subscriber_answer,
    // which is the definitive moment the subscriber PC is ready to decode.
}

/// Handle a subscriber SDP answer from the client.
async fn handle_subscriber_answer(
    sfu: &Arc<SfuServer>,
    user_id: Uuid,
    channel_id: Uuid,
    sdp: &str,
    tx: &mpsc::Sender<OutboundMsg>,
) -> Result<(), VoiceError> {
    debug!(user_id = %user_id, channel_id = %channel_id, "Received subscriber answer");

    let room = sfu
        .get_room(channel_id)
        .await
        .ok_or(VoiceError::RoomNotFound(channel_id))?;

    let peer = owned_peer(&room, user_id, tx).await?;

    SfuServer::handle_subscriber_answer(&peer, sdp.to_string())
        .await
        .map_err(|e| {
            error!(user_id = %user_id, channel_id = %channel_id, error = %e, "failed to handle subscriber answer");
            e
        })?;

    // Keyframes are handled by the interval PLI task in setup_track_handler
    // (sends PLI every 3s per video track). No manual PLI needed here.

    Ok(())
}

/// Handle an ICE candidate from a client, routed by `pc_type`.
async fn handle_ice_candidate(
    sfu: &Arc<SfuServer>,
    user_id: Uuid,
    channel_id: Uuid,
    candidate: &str,
    pc_type: &PcType,
    tx: &mpsc::Sender<OutboundMsg>,
) -> Result<(), VoiceError> {
    debug!(user_id = %user_id, channel_id = %channel_id, pc_type = %pc_type, "Received ICE candidate");

    let room = sfu
        .get_room(channel_id)
        .await
        .ok_or(VoiceError::RoomNotFound(channel_id))?;

    let peer = owned_peer(&room, user_id, tx).await?;

    if let Err(e) = SfuServer::handle_ice_candidate(&peer, candidate, pc_type).await {
        error!(user_id = %user_id, channel_id = %channel_id, pc_type = %pc_type, error = %e, "failed to add ICE candidate");
    }

    Ok(())
}

/// Handle mute/unmute.
async fn handle_mute(
    sfu: &Arc<SfuServer>,
    user_id: Uuid,
    channel_id: Uuid,
    muted: bool,
    tx: &mpsc::Sender<OutboundMsg>,
) -> Result<(), VoiceError> {
    debug!(
        user_id = %user_id,
        channel_id = %channel_id,
        muted = muted,
        "Mute state changed"
    );

    let room = sfu
        .get_room(channel_id)
        .await
        .ok_or(VoiceError::RoomNotFound(channel_id))?;

    let peer = owned_peer(&room, user_id, tx).await?;

    peer.set_self_muted(muted).await;

    // Notify other participants
    let event = if muted {
        ServerEvent::VoiceUserMuted {
            channel_id,
            user_id,
        }
    } else {
        ServerEvent::VoiceUserUnmuted {
            channel_id,
            user_id,
        }
    };

    room.broadcast_except(user_id, event).await;

    Ok(())
}

/// Handle voice quality statistics from a client.
///
/// This broadcasts the stats to other participants in the room
/// and stores them in the database for historical analysis.
async fn handle_voice_stats(
    sfu: &Arc<SfuServer>,
    pool: &PgPool,
    user_id: Uuid,
    channel_id: Uuid,
    mut stats: VoiceStats,
    tx: &mpsc::Sender<OutboundMsg>,
) -> Result<(), VoiceError> {
    // Rate limit check
    if sfu.check_stats_rate_limit(user_id).await.is_err() {
        warn!(user_id = %user_id, "User sent voice stats too frequently, dropping");
        return Ok(());
    }

    // Validate stats
    if let Err(reason) = stats.validate() {
        warn!(user_id = %user_id, "Invalid voice stats: {}", reason);
        return Ok(());
    }

    // The peer must be in the room; bail before storing anything otherwise.
    // We also take the SERVER-tracked session id from the peer rather than
    // trusting the client-supplied `stats.session_id` — the client value is
    // unauthenticated and a wrong/forged id would file metrics against a
    // session that never gets finalized (or another user's session).
    let Some(room) = sfu.get_room(channel_id).await else {
        return Ok(());
    };
    let Ok(peer) = owned_peer(&room, user_id, tx).await else {
        warn!(user_id = %user_id, channel_id = %channel_id, "User sent voice stats for a room they are not in");
        return Ok(());
    };
    stats.session_id = peer.session_id;

    // Broadcast to room participants
    room.broadcast_except(
        user_id,
        ServerEvent::VoiceUserStats {
            channel_id,
            user_id,
            latency: stats.latency,
            packet_loss: stats.packet_loss,
            jitter: stats.jitter,
            quality: stats.quality,
        },
    )
    .await;

    // Store in database (fire-and-forget)
    let guild_id = get_guild_id(pool, channel_id).await;
    let pool_clone = pool.clone();
    tokio::spawn(async move {
        store_metrics(pool_clone, stats, user_id, channel_id, guild_id).await;
    });

    Ok(())
}

/// Parameters for starting a screen share.
struct HandleScreenShareStartParams<'a> {
    user_id: Uuid,
    channel_id: Uuid,
    stream_id: Uuid,
    quality: Quality,
    has_audio: bool,
    source_label: &'a str,
}

/// Handle starting a screen share.
async fn handle_screen_share_start(
    sfu: &Arc<SfuServer>,
    pool: &PgPool,
    params: HandleScreenShareStartParams<'_>,
    screen_share_limiter: Option<&ScreenShareLimiter>,
    tx: &mpsc::Sender<OutboundMsg>,
) -> Result<(), VoiceError> {
    info!(user_id = %params.user_id, channel_id = %params.channel_id, quality = ?params.quality, "User starting screen share");

    // Check if user has VIEW_CHANNEL and SCREEN_SHARE permissions
    let ctx = crate::permissions::require_channel_access(pool, params.user_id, params.channel_id)
        .await
        .map_err(|e| map_permission_error(e, "screen share"))?;

    if !ctx.has_permission(crate::permissions::GuildPermissions::SCREEN_SHARE) {
        return Err(VoiceError::ScreenShareForbidden);
    }

    // Validate source label
    if let Err(e) = validate_source_label(params.source_label) {
        warn!(user_id = %params.user_id, "Invalid source label: {:?}", e);
        return Err(VoiceError::Signaling("Invalid source label".to_string()));
    }

    // Get the room
    let room = sfu
        .get_room(params.channel_id)
        .await
        .ok_or(VoiceError::RoomNotFound(params.channel_id))?;

    // Check user is in the room
    let peer = owned_peer(&room, params.user_id, tx).await?;

    // Check if user has reached the per-user stream limit (max 3 concurrent streams)
    {
        const MAX_STREAMS_PER_USER: usize = 3;
        let count = room.get_user_stream_count(params.user_id).await;
        if count >= MAX_STREAMS_PER_USER {
            return Err(VoiceError::ScreenShareLimit(
                "Maximum 3 concurrent screen shares per user",
            ));
        }
    }

    // Fetch max_screen_shares from channel settings
    let max_screen_shares: i32 = queries::find_channel_max_screen_shares(pool, params.channel_id)
        .await
        .inspect_err(|e| {
            warn!(channel_id = %params.channel_id, error = %e, "Failed to query max_screen_shares, using default");
        })
        .ok()
        .flatten()
        .unwrap_or(6);
    // Use the stream_id provided by the client.
    let stream_id = params.stream_id;

    // Check for duplicate stream_id FIRST — before reserving the channel slot,
    // otherwise a duplicate leaks the slot.
    if room.screen_shares.read().await.contains_key(&stream_id) {
        return Err(VoiceError::DuplicateStreamId);
    }

    let max_shares: u32 = max_screen_shares.try_into().unwrap_or(6);

    // Try to reserve a slot via limiter
    let limiter = screen_share_limiter
        .ok_or_else(|| VoiceError::Signaling("Screen share limiter unavailable".to_string()))?;
    if let Err(e) = limiter.start(params.channel_id, max_shares).await {
        warn!(user_id = %params.user_id, channel_id = %params.channel_id, error = ?e, "Screen share limit check failed");
        return Err(match e {
            ScreenShareError::LimitReached => {
                VoiceError::ScreenShareLimit("Screen share limit reached")
            }
            other => VoiceError::Internal(format!("screen share limiter: {other:?}")),
        });
    }

    // Get username for the info
    let username = peer.username.clone();

    // Create screen share info
    let info = ScreenShareInfo::new(
        stream_id,
        params.user_id,
        username.clone(),
        params.source_label.to_string(),
        params.has_audio,
        params.quality,
    );

    // Add to the room's screen shares; a concurrent start with the same
    // `stream_id` that won the race keeps its entry, and this one gives its
    // slot back.
    if !room.try_add_screen_share(info.clone()).await {
        limiter.stop(params.channel_id).await;
        return Err(VoiceError::DuplicateStreamId);
    }

    // Queue pending track sources so setup_track_handler can identify them
    // (only once the share is live; a track never binds to a dead share).
    peer.push_pending_source(TrackSource::ScreenVideo(stream_id))
        .await;
    if params.has_audio {
        peer.push_pending_source(TrackSource::ScreenAudio(stream_id))
            .await;
    }

    // Broadcast to room (including the sharer, so they get confirmation)
    room.broadcast_all(ServerEvent::ScreenShareStarted {
        channel_id: params.channel_id,
        user_id: params.user_id,
        stream_id,
        username,
        source_label: params.source_label.to_string(),
        has_audio: params.has_audio,
        quality: params.quality,
        started_at: info.started_at.to_rfc3339(),
    })
    .await;

    info!(
        user_id = %params.user_id,
        channel_id = %params.channel_id,
        quality = ?params.quality,
        "Screen share started"
    );

    Ok(())
}

/// Handle stopping a screen share.
async fn handle_screen_share_stop(
    sfu: &Arc<SfuServer>,
    user_id: Uuid,
    channel_id: Uuid,
    stream_id: Uuid,
    screen_share_limiter: Option<&ScreenShareLimiter>,
    tx: &mpsc::Sender<OutboundMsg>,
) -> Result<(), VoiceError> {
    info!(user_id = %user_id, channel_id = %channel_id, stream_id = %stream_id, "User stopping screen share");

    // Get the room
    let room = sfu
        .get_room(channel_id)
        .await
        .ok_or(VoiceError::RoomNotFound(channel_id))?;
    let peer = owned_peer(&room, user_id, tx).await?;
    // An announcement this stop ends must never bind a later track.
    peer.remove_pending_sources(|s| s.stream_id() == Some(stream_id))
        .await;

    // Look up the specific stream by stream_id and verify ownership.
    let share_info = {
        let shares = room.screen_shares.read().await;
        shares.get(&stream_id).cloned()
    };

    if let Some(info) = share_info {
        // Verify the requesting user owns this stream
        if info.user_id != user_id {
            warn!(user_id = %user_id, stream_id = %stream_id, "User tried to stop a screen share they don't own");
            return Err(VoiceError::Signaling("Not your screen share".to_string()));
        }
        room.remove_screen_share(stream_id).await;
    } else {
        // User wasn't sharing, but that's okay - idempotent
        debug!(user_id = %user_id, "User tried to stop screen share but wasn't sharing");
        return Ok(());
    }

    // Decrement Redis counter
    if let Some(limiter) = screen_share_limiter {
        limiter.stop(channel_id).await;
    } else {
        tracing::warn!("Screen share limiter unavailable during stop — counter not decremented");
    }

    // Collect only the track sources belonging to this specific stream_id
    let screen_sources: Vec<TrackSource> = {
        let incoming = peer.incoming_tracks.read().await;
        incoming
            .keys()
            .filter(|s| s.stream_id() == Some(stream_id))
            .copied()
            .collect()
    };

    // Clean up screen share tracks from the track router
    for source in &screen_sources {
        room.track_router
            .remove_source_track(user_id, *source)
            .await;
    }

    // Remove incoming tracks from the peer
    {
        let mut incoming = peer.incoming_tracks.write().await;
        for source in &screen_sources {
            incoming.remove(source);
        }
    }

    // Remove outgoing tracks from all subscribers and renegotiate
    let other_peers = room.get_other_peers(user_id).await;
    for other_peer in &other_peers {
        let mut any_removed = false;
        for source in &screen_sources {
            if other_peer.remove_outgoing_track(user_id, *source).await {
                any_removed = true;
            }
        }
        if any_removed {
            if let Err(e) = SfuServer::renegotiate(other_peer).await {
                warn!(
                    subscriber = %other_peer.user_id,
                    error = %e,
                    "Failed to renegotiate after screen share track removal"
                );
            }
        }
    }

    // Broadcast to room
    room.broadcast_all(ServerEvent::ScreenShareStopped {
        channel_id,
        user_id,
        stream_id,
        reason: "user_stopped".to_string(),
    })
    .await;

    info!(
        user_id = %user_id,
        channel_id = %channel_id,
        stream_id = %stream_id,
        "Screen share stopped"
    );

    Ok(())
}

/// Handle starting a webcam.
async fn handle_webcam_start(
    sfu: &Arc<SfuServer>,
    pool: &PgPool,
    user_id: Uuid,
    channel_id: Uuid,
    quality: Quality,
    tx: &mpsc::Sender<OutboundMsg>,
) -> Result<(), VoiceError> {
    info!(user_id = %user_id, channel_id = %channel_id, quality = ?quality, "User starting webcam");

    // Rate limit check
    sfu.check_rate_limit(user_id).await?;

    // Check if user has VIEW_CHANNEL permission
    crate::permissions::require_channel_access(pool, user_id, channel_id)
        .await
        .map_err(|e| map_permission_error(e, "webcam start"))?;

    // Get the room
    let room = sfu
        .get_room(channel_id)
        .await
        .ok_or(VoiceError::RoomNotFound(channel_id))?;

    // Check user is in the room
    let peer = owned_peer(&room, user_id, tx).await?;

    // Check if user already has webcam active
    {
        let webcams = room.webcams.read().await;
        if webcams.contains_key(&user_id) {
            return Err(VoiceError::Signaling("Webcam already active".to_string()));
        }
    }

    // Queue pending track source so setup_track_handler identifies it as Webcam
    peer.push_pending_source(TrackSource::Webcam).await;

    // In the dual-PC model, the client adds tracks to the publisher PC
    // and sends a new offer. No server-side transceiver allocation needed.

    // Get username for the info
    let username = peer.username.clone();

    // Create webcam info and add to room
    let info = WebcamInfo::new(user_id, username.clone(), quality);
    room.add_webcam(info).await;

    // Broadcast to room (including the user, so they get confirmation)
    room.broadcast_all(ServerEvent::WebcamStarted {
        channel_id,
        user_id,
        username,
        quality,
    })
    .await;

    info!(
        user_id = %user_id,
        channel_id = %channel_id,
        quality = ?quality,
        "Webcam started"
    );

    Ok(())
}

/// Handle stopping a webcam.
async fn handle_webcam_stop(
    sfu: &Arc<SfuServer>,
    user_id: Uuid,
    channel_id: Uuid,
    tx: &mpsc::Sender<OutboundMsg>,
) -> Result<(), VoiceError> {
    info!(user_id = %user_id, channel_id = %channel_id, "User stopping webcam");

    // Get the room
    let room = sfu
        .get_room(channel_id)
        .await
        .ok_or(VoiceError::RoomNotFound(channel_id))?;
    let peer = owned_peer(&room, user_id, tx).await?;
    // A webcam announcement this stop ends must never bind a later track.
    peer.remove_pending_sources(|s| *s == TrackSource::Webcam)
        .await;

    // Remove webcam from room
    if room.remove_webcam(user_id).await.is_none() {
        // User didn't have webcam active, but that's okay - idempotent
        debug!(user_id = %user_id, "User tried to stop webcam but wasn't active");
        return Ok(());
    }

    // Clean up the webcam track from the track router
    // This removes all subscribers of this user's Webcam track
    room.track_router
        .remove_source_track(user_id, TrackSource::Webcam)
        .await;

    // Remove the incoming webcam track from the peer and clean up outgoing tracks
    // on all other peers
    peer.incoming_tracks
        .write()
        .await
        .remove(&TrackSource::Webcam);

    // Remove outgoing tracks from all subscribers and renegotiate
    let other_peers = room.get_other_peers(user_id).await;
    for other_peer in &other_peers {
        if other_peer
            .remove_outgoing_track(user_id, TrackSource::Webcam)
            .await
        {
            // Renegotiate so subscriber's SDP reflects the removed track
            if let Err(e) = SfuServer::renegotiate(other_peer).await {
                warn!(
                    subscriber = %other_peer.user_id,
                    error = %e,
                    "Failed to renegotiate after webcam track removal"
                );
            }
        }
    }

    // Broadcast to room
    room.broadcast_all(ServerEvent::WebcamStopped {
        channel_id,
        user_id,
        reason: "user_stopped".to_string(),
    })
    .await;

    info!(
        user_id = %user_id,
        channel_id = %channel_id,
        "Webcam stopped"
    );

    Ok(())
}

/// Handle setting a layer preference for a simulcast track subscription.
async fn handle_set_layer_preference(
    sfu: &Arc<SfuServer>,
    user_id: Uuid,
    channel_id: Uuid,
    target_user_id: Uuid,
    track_source: TrackSource,
    preferred_layer: LayerPreference,
    tx: &mpsc::Sender<OutboundMsg>,
) -> Result<(), VoiceError> {
    debug!(
        user_id = %user_id,
        channel_id = %channel_id,
        target_user_id = %target_user_id,
        track_source = %track_source,
        preferred_layer = ?preferred_layer,
        "Setting layer preference"
    );

    let room = sfu
        .get_room(channel_id)
        .await
        .ok_or(VoiceError::RoomNotFound(channel_id))?;

    // Verify the requesting user is in the room
    owned_peer(&room, user_id, tx).await?;

    if let Some(new_layer) = room.track_router.set_layer_preference(
        user_id,
        target_user_id,
        track_source,
        preferred_layer,
    ) {
        // Notify the viewer of the layer change
        crate::ws::handlers::send_outbound(
            tx,
            OutboundMsg::voice(
                channel_id,
                ServerEvent::VoiceLayerChanged {
                    channel_id,
                    source_user_id: target_user_id,
                    track_source,
                    active_layer: new_layer,
                },
            ),
        )
        .await?;
    }

    Ok(())
}

#[cfg(test)]
#[path = "ws_handler_test.rs"]
mod ws_handler_test;
