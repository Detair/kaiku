//! Voice WebSocket Message Handlers
//!
//! Handles voice signaling messages from WebSocket connections.

use std::sync::Arc;

use sqlx::{PgPool, Row};
use tokio::sync::mpsc;
use tracing::{debug, info, warn};
use uuid::Uuid;

use super::error::VoiceError;
use super::metrics::{finalize_session, get_guild_id, store_metrics};
use super::sfu::SfuServer;
use super::stats::VoiceStats;
use crate::ws::{ClientEvent, ServerEvent, VoiceParticipant};

/// Handle a voice-related client event.
pub async fn handle_voice_event(
    sfu: &Arc<SfuServer>,
    pool: &PgPool,
    user_id: Uuid,
    event: ClientEvent,
    tx: &mpsc::Sender<ServerEvent>,
) -> Result<(), VoiceError> {
    match event {
        ClientEvent::VoiceJoin { channel_id } => {
            handle_join(sfu, pool, user_id, channel_id, tx).await
        }
        ClientEvent::VoiceLeave { channel_id } => {
            handle_leave(sfu, pool, user_id, channel_id).await
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

    // Rate limit check (max 1 join per second per user)
    sfu.check_rate_limit(user_id).await?;

    // Fetch user info from database
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

    // Get or create the room
    let room = sfu.get_or_create_room(channel_id).await;

    // Create peer connection for this user
    let peer = sfu
        .create_peer(user_id, username, display_name, channel_id, tx.clone())
        .await?;

    // Add recvonly transceiver for receiving audio from client
    peer.add_recv_transceiver().await?;

    // Set up ICE candidate handler
    sfu.setup_ice_handler(&peer);

    // Set up track handler (will be called when client sends audio)
    sfu.setup_track_handler(&peer, &room);

    // Add peer to room
    room.add_peer(peer.clone()).await?;

    // Create and send offer to client
    let offer = sfu.create_offer(&peer).await?;
    tx.send(ServerEvent::VoiceOffer {
        channel_id,
        sdp: offer.sdp,
    })
    .await
    .map_err(|e| VoiceError::Signaling(e.to_string()))?;

    // Send current room state to joining user
    let participants: Vec<VoiceParticipant> = room
        .get_participant_info()
        .await
        .into_iter()
        .map(|p| VoiceParticipant {
            user_id: p.user_id,
            username: p.username,
            display_name: p.display_name,
            muted: p.muted,
        })
        .collect();

    tx.send(ServerEvent::VoiceRoomState {
        channel_id,
        participants,
    })
    .await
    .map_err(|e| VoiceError::Signaling(e.to_string()))?;

    // Notify other participants
    room.broadcast_except(
        user_id,
        ServerEvent::VoiceUserJoined {
            channel_id,
            user_id,
            username: peer.username.clone(),
            display_name: peer.display_name.clone(),
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
    user_id: Uuid,
    channel_id: Uuid,
) -> Result<(), VoiceError> {
    info!(user_id = %user_id, channel_id = %channel_id, "User leaving voice channel");

    let room = sfu
        .get_room(channel_id)
        .await
        .ok_or(VoiceError::RoomNotFound(channel_id))?;

    // Remove peer from room
    if let Some(peer) = room.remove_peer(user_id).await {
        // Finalize session in background
        let guild_id = get_guild_id(pool, channel_id).await;
        let pool_clone = pool.clone();
        let session_id = peer.session_id;
        let connected_at = peer.connected_at;

        tokio::spawn(async move {
            if let Err(e) = finalize_session(
                &pool_clone,
                user_id,
                session_id,
                channel_id,
                guild_id,
                connected_at,
            )
            .await
            {
                warn!(
                    user_id = %user_id,
                    session_id = %session_id,
                    "Failed to finalize session: {}",
                    e
                );
            }
        });

        // Close the peer connection
        if let Err(e) = peer.close().await {
            warn!(error = %e, "Error closing peer connection");
        }
    }

    // Notify other participants
    room.broadcast_except(
        user_id,
        ServerEvent::VoiceUserLeft {
            channel_id,
            user_id,
        },
    )
    .await;

    // Cleanup empty room
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
    if let Err(_) = sfu.check_stats_rate_limit(user_id).await {
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

#[cfg(test)]
#[path = "ws_handler_test.rs"]
mod ws_handler_test;
