//! Voice WebSocket Message Handlers
//!
//! Handles voice signaling messages from WebSocket connections.

use std::sync::Arc;

use sqlx::PgPool;
use tokio::sync::mpsc;
use tracing::{debug, info, warn};
use uuid::Uuid;

use super::error::VoiceError;
use super::sfu::SfuServer;
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
            handle_leave(sfu, user_id, channel_id).await
        }
        ClientEvent::VoiceAnswer { channel_id, sdp } => {
            handle_answer(sfu, user_id, channel_id, &sdp).await
        }
        ClientEvent::VoiceIceCandidate { channel_id, candidate } => {
            handle_ice_candidate(sfu, user_id, channel_id, &candidate).await
        }
        ClientEvent::VoiceMute { channel_id } => {
            handle_mute(sfu, user_id, channel_id, true).await
        }
        ClientEvent::VoiceUnmute { channel_id } => {
            handle_mute(sfu, user_id, channel_id, false).await
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

    // Fetch user info from database
    let user = sqlx::query!(
        "SELECT username, display_name FROM users WHERE id = $1",
        user_id
    )
    .fetch_one(pool)
    .await
    .map_err(|e| VoiceError::Signaling(format!("Failed to fetch user info: {}", e)))?;

    let username = user.username;
    let display_name = user.display_name;

    // Get or create the room
    let room = sfu.get_or_create_room(channel_id).await;

    // Create peer connection for this user
    let peer = sfu.create_peer(user_id, username, display_name, channel_id, tx.clone()).await?;

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

