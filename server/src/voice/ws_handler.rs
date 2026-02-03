//! Voice WebSocket Message Handlers
//!
//! Handles voice signaling messages from WebSocket connections.

use std::sync::Arc;

use fred::clients::Client;
use sqlx::{PgPool, Row};
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};
use uuid::Uuid;
use webrtc::rtcp::payload_feedbacks::picture_loss_indication::PictureLossIndication;

use super::error::VoiceError;
use super::metrics::{finalize_session, get_guild_id, store_metrics};
use super::screen_share::{
    stop_screen_share, try_start_screen_share, validate_source_label, ScreenShareError,
    ScreenShareInfo,
};
use super::sfu::SfuServer;
use super::stats::VoiceStats;
use super::track_types::TrackSource;
use super::Quality;
use crate::ws::{ClientEvent, ServerEvent, VoiceParticipant};

/// Handle a voice-related client event.
pub async fn handle_voice_event(
    sfu: &Arc<SfuServer>,
    pool: &PgPool,
    redis: &Client,
    user_id: Uuid,
    event: ClientEvent,
    tx: &mpsc::Sender<ServerEvent>,
) -> Result<(), VoiceError> {
    match event {
        ClientEvent::VoiceJoin { channel_id } => {
            handle_join(sfu, pool, user_id, channel_id, tx).await
        }
        ClientEvent::VoiceLeave { channel_id } => {
            handle_leave(sfu, pool, redis, user_id, channel_id).await
        }
        ClientEvent::VoiceAnswer { channel_id, sdp } => {
            handle_answer(sfu, user_id, channel_id, &sdp).await
        }
        ClientEvent::VoiceIceCandidate {
            channel_id,
            candidate,
        } => handle_ice_candidate(sfu, user_id, channel_id, &candidate).await,
        ClientEvent::VoiceMute { channel_id } => handle_mute(sfu, user_id, channel_id, true).await,
        ClientEvent::VoiceUnmute { channel_id } => {
            handle_mute(sfu, user_id, channel_id, false).await
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
            handle_voice_stats(sfu, pool, user_id, channel_id, stats).await
        }
        ClientEvent::VoiceScreenShareStart {
            channel_id,
            quality,
            has_audio,
            source_label,
        } => {
            handle_screen_share_start(
                sfu,
                pool,
                redis,
                user_id,
                channel_id,
                quality,
                has_audio,
                &source_label,
            )
            .await
        }
        ClientEvent::VoiceScreenShareStop { channel_id } => {
            handle_screen_share_stop(sfu, redis, user_id, channel_id).await
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
    tx: &mpsc::Sender<ServerEvent>,
) -> Result<(), VoiceError> {
    info!(user_id = %user_id, channel_id = %channel_id, "User joining voice channel");

    // Check if user has VIEW_CHANNEL and VOICE_CONNECT permissions
    let ctx = crate::permissions::require_channel_access(pool, user_id, channel_id)
        .await
        .map_err(|_e: crate::permissions::PermissionError| VoiceError::Unauthorized)?;

    if !ctx.has_permission(crate::permissions::GuildPermissions::VOICE_CONNECT) {
        return Err(VoiceError::Unauthorized);
    }

    sfu.check_rate_limit(user_id).await?;

    let user = sqlx::query("SELECT username, display_name FROM users WHERE id = $1")
        .bind(user_id)
        .fetch_one(pool)
        .await
        .map_err(|e| VoiceError::Signaling(format!("Failed to fetch user info: {e}")))?;

    let username: String = user
        .try_get("username")
        .map_err(|e| VoiceError::Signaling(format!("Failed to get username: {e}")))?;
    let display_name: String = user
        .try_get("display_name")
        .map_err(|e| VoiceError::Signaling(format!("Failed to get display_name: {e}")))?;

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

    room.add_peer(peer.clone()).await?;

    let other_peers = room.get_other_peers(user_id).await;
    for other_peer in other_peers {
        let incoming_tracks = other_peer.incoming_tracks.read().await;
        for (source_type, track) in incoming_tracks.iter() {
            if let Ok(local_track) = room
                .track_router
                .create_subscriber_track(other_peer.user_id, *source_type, &peer, track)
                .await
            {
                if let Err(e) = peer
                    .add_outgoing_track(other_peer.user_id, *source_type, local_track)
                    .await
                {
                    warn!("Failed to add outgoing track: {}", e);
                } else if *source_type == TrackSource::ScreenVideo {
                    // Send PLI to request keyframe for late joiners
                    let pli = PictureLossIndication {
                        sender_ssrc: 0,
                        media_ssrc: track.ssrc(),
                    };
                    if let Err(e) = other_peer
                        .peer_connection
                        .write_rtcp(&[Box::new(pli)])
                        .await
                    {
                        warn!("Failed to send PLI: {}", e);
                    } else {
                        debug!("Sent PLI to source {}", other_peer.user_id);
                    }
                }
            }
        }
    }

    let offer = sfu.create_offer(&peer).await?;
    tx.send(ServerEvent::VoiceOffer {
        channel_id,
        sdp: offer.sdp,
    })
    .await
    .map_err(|e| VoiceError::Signaling(e.to_string()))?;

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
        })
        .collect();

    let screen_shares = room.get_screen_shares().await;

    tx.send(ServerEvent::VoiceRoomState {
        channel_id,
        participants,
        screen_shares,
    })
    .await
    .map_err(|e| VoiceError::Signaling(e.to_string()))?;

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

    Ok(())
}

/// Handle a user leaving a voice channel.
async fn handle_leave(
    sfu: &Arc<SfuServer>,
    pool: &PgPool,
    redis: &Client,
    user_id: Uuid,
    channel_id: Uuid,
) -> Result<(), VoiceError> {
    info!(user_id = %user_id, channel_id = %channel_id, "User leaving voice channel");

    // Check if user has VIEW_CHANNEL permission
    crate::permissions::require_channel_access(pool, user_id, channel_id)
        .await
        .map_err(|_e: crate::permissions::PermissionError| VoiceError::Unauthorized)?;

    let room = sfu
        .get_room(channel_id)
        .await
        .ok_or(VoiceError::RoomNotFound(channel_id))?;

    // Check if sharing screen and stop it
    if room.remove_screen_share(user_id).await.is_some() {
        stop_screen_share(redis, channel_id).await;

        room.broadcast_except(
            user_id,
            ServerEvent::ScreenShareStopped {
                channel_id,
                user_id,
                reason: "disconnected".to_string(),
            },
        )
        .await;
    }

    // Remove peer from room
    if let Some(peer) = room.remove_peer(user_id).await {
        // Finalize session in background
        let guild_id = get_guild_id(pool, channel_id).await;
        let pool_clone = pool.clone();
        let session_id = peer.session_id;
        let connected_at = peer.connected_at;

        tokio::spawn(async move {
            // Retry with exponential backoff (3 attempts: 100ms, 200ms, 400ms)
            const MAX_RETRIES: u32 = 3;
            let mut delay = std::time::Duration::from_millis(100);

            for attempt in 1..=MAX_RETRIES {
                match finalize_session(
                    &pool_clone,
                    user_id,
                    session_id,
                    channel_id,
                    guild_id,
                    connected_at,
                )
                .await
                {
                    Ok(()) => {
                        if attempt > 1 {
                            info!(
                                user_id = %user_id,
                                session_id = %session_id,
                                attempt = attempt,
                                "Session finalized after retry"
                            );
                        }
                        return;
                    }
                    Err(e) if attempt < MAX_RETRIES => {
                        warn!(
                            user_id = %user_id,
                            session_id = %session_id,
                            attempt = attempt,
                            error = %e,
                            "Failed to finalize session, retrying in {:?}",
                            delay
                        );
                        tokio::time::sleep(delay).await;
                        delay *= 2; // Exponential backoff
                    }
                    Err(e) => {
                        error!(
                            user_id = %user_id,
                            session_id = %session_id,
                            "Failed to finalize session after {} attempts: {}",
                            MAX_RETRIES,
                            e
                        );
                        // Session data is lost - this should trigger an alert in production
                    }
                }
            }
        });

        // Close the peer connection
        if let Err(e) = peer.close().await {
            warn!(error = %e, "Error closing peer connection");
        }
    }

    room.broadcast_except(
        user_id,
        ServerEvent::VoiceUserLeft {
            channel_id,
            user_id,
        },
    )
    .await;

    sfu.cleanup_room_if_empty(channel_id).await;

    info!(
        user_id = %user_id,
        channel_id = %channel_id,
        "User left voice channel"
    );

    Ok(())
}

/// Handle an SDP answer from a client.
async fn handle_answer(
    sfu: &Arc<SfuServer>,
    user_id: Uuid,
    channel_id: Uuid,
    sdp: &str,
) -> Result<(), VoiceError> {
    debug!(user_id = %user_id, channel_id = %channel_id, "Received SDP answer");

    let room = sfu
        .get_room(channel_id)
        .await
        .ok_or(VoiceError::RoomNotFound(channel_id))?;

    let peer = room
        .get_peer(user_id)
        .await
        .ok_or(VoiceError::ParticipantNotFound(user_id))?;

    sfu.handle_answer(&peer, sdp).await?;

    debug!(
        user_id = %user_id,
        channel_id = %channel_id,
        "SDP answer processed"
    );

    Ok(())
}

/// Handle an ICE candidate from a client.
async fn handle_ice_candidate(
    sfu: &Arc<SfuServer>,
    user_id: Uuid,
    channel_id: Uuid,
    candidate: &str,
) -> Result<(), VoiceError> {
    debug!(user_id = %user_id, channel_id = %channel_id, "Received ICE candidate");

    let room = sfu
        .get_room(channel_id)
        .await
        .ok_or(VoiceError::RoomNotFound(channel_id))?;

    let peer = room
        .get_peer(user_id)
        .await
        .ok_or(VoiceError::ParticipantNotFound(user_id))?;

    sfu.handle_ice_candidate(&peer, candidate).await?;

    Ok(())
}

/// Handle mute/unmute.
async fn handle_mute(
    sfu: &Arc<SfuServer>,
    user_id: Uuid,
    channel_id: Uuid,
    muted: bool,
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

    let peer = room
        .get_peer(user_id)
        .await
        .ok_or(VoiceError::ParticipantNotFound(user_id))?;

    peer.set_muted(muted).await;

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
    stats: VoiceStats,
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

    // Broadcast to room participants
    let broadcast = ServerEvent::VoiceUserStats {
        channel_id,
        user_id,
        latency: stats.latency,
        packet_loss: stats.packet_loss,
        jitter: stats.jitter,
        quality: stats.quality,
    };

    if let Some(room) = sfu.get_room(channel_id).await {
        // Verify user is actually in the room before broadcasting
        if room.get_peer(user_id).await.is_none() {
            warn!(user_id = %user_id, channel_id = %channel_id, "User attempted to broadcast stats to a room they are not in");
            return Ok(());
        }
        room.broadcast_except(user_id, broadcast).await;
    }

    // Store in database (fire-and-forget)
    let guild_id = get_guild_id(pool, channel_id).await;
    let pool_clone = pool.clone();
    tokio::spawn(async move {
        store_metrics(pool_clone, stats, user_id, channel_id, guild_id).await;
    });

    Ok(())
}

/// Default max screen shares per channel.
const DEFAULT_MAX_SCREEN_SHARES: u32 = 2;

/// Handle starting a screen share.
#[allow(clippy::too_many_arguments)]
async fn handle_screen_share_start(
    sfu: &Arc<SfuServer>,
    pool: &PgPool,
    redis: &Client,
    user_id: Uuid,
    channel_id: Uuid,
    quality: Quality,
    has_audio: bool,
    source_label: &str,
) -> Result<(), VoiceError> {
    info!(user_id = %user_id, channel_id = %channel_id, quality = ?quality, "User starting screen share");

    // Check if user has VIEW_CHANNEL and SCREEN_SHARE permissions
    let ctx = crate::permissions::require_channel_access(pool, user_id, channel_id)
        .await
        .map_err(|_e: crate::permissions::PermissionError| VoiceError::Unauthorized)?;

    if !ctx.has_permission(crate::permissions::GuildPermissions::SCREEN_SHARE) {
        return Err(VoiceError::Unauthorized);
    }

    // Validate source label
    if let Err(e) = validate_source_label(source_label) {
        warn!(user_id = %user_id, "Invalid source label: {:?}", e);
        return Err(VoiceError::Signaling("Invalid source label".to_string()));
    }

    // Get the room
    let room = sfu
        .get_room(channel_id)
        .await
        .ok_or(VoiceError::RoomNotFound(channel_id))?;

    // Check user is in the room
    let peer = room
        .get_peer(user_id)
        .await
        .ok_or(VoiceError::ParticipantNotFound(user_id))?;

    // Check if user is already sharing
    {
        let shares = room.screen_shares.read().await;
        if shares.contains_key(&user_id) {
            return Err(VoiceError::Signaling("Already sharing screen".to_string()));
        }
    }

    // Try to reserve a slot (Redis limit check)
    // TODO: Get max_screen_shares from channel settings
    let max_shares = DEFAULT_MAX_SCREEN_SHARES;

    if let Err(e) = try_start_screen_share(redis, channel_id, max_shares).await {
        warn!(user_id = %user_id, channel_id = %channel_id, error = ?e, "Screen share limit check failed");
        return Err(VoiceError::Signaling(match e {
            ScreenShareError::LimitReached => "Screen share limit reached".to_string(),
            ScreenShareError::InternalError => "Internal error".to_string(),
            _ => format!("{e:?}"),
        }));
    }

    // Get username for the info
    let username = peer.username.clone();

    // Create screen share info
    let info = ScreenShareInfo::new(
        user_id,
        username.clone(),
        source_label.to_string(),
        has_audio,
        quality,
    );

    // Add to room's screen shares
    room.add_screen_share(info.clone()).await;

    // Broadcast to room (including the sharer, so they get confirmation)
    room.broadcast_all(ServerEvent::ScreenShareStarted {
        channel_id,
        user_id,
        username,
        source_label: source_label.to_string(),
        has_audio,
        quality,
    })
    .await;

    info!(
        user_id = %user_id,
        channel_id = %channel_id,
        quality = ?quality,
        "Screen share started"
    );

    Ok(())
}

/// Handle stopping a screen share.
async fn handle_screen_share_stop(
    sfu: &Arc<SfuServer>,
    redis: &Client,
    user_id: Uuid,
    channel_id: Uuid,
) -> Result<(), VoiceError> {
    info!(user_id = %user_id, channel_id = %channel_id, "User stopping screen share");

    // Get the room
    let room = sfu
        .get_room(channel_id)
        .await
        .ok_or(VoiceError::RoomNotFound(channel_id))?;

    // Remove screen share from room
    if room.remove_screen_share(user_id).await.is_none() {
        // User wasn't sharing, but that's okay - idempotent
        debug!(user_id = %user_id, "User tried to stop screen share but wasn't sharing");
        return Ok(());
    }

    // Decrement Redis counter
    stop_screen_share(redis, channel_id).await;

    // Broadcast to room
    room.broadcast_all(ServerEvent::ScreenShareStopped {
        channel_id,
        user_id,
        reason: "user_stopped".to_string(),
    })
    .await;

    info!(
        user_id = %user_id,
        channel_id = %channel_id,
        "Screen share stopped"
    );

    Ok(())
}

#[cfg(test)]
#[path = "ws_handler_test.rs"]
mod ws_handler_test;
