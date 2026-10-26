//! Removing a user from a voice room.
//!
//! Shared by the explicit `VoiceLeave` event, WebSocket close and the
//! stale-peer reaper, so every path has the same side effects: screen share
//! and webcam stop broadcasts, session finalization, rate-limit cleanup,
//! `VoiceUserLeft`, empty-room cleanup and ending the DM call of a room whose
//! last peer left.

use std::sync::Arc;

use sqlx::PgPool;
use tokio::sync::mpsc;
use tracing::{error, info, warn};
use uuid::Uuid;

use super::error::VoiceError;
use super::metrics::{finalize_session, get_guild_id};
use super::screen_share::ScreenShareLimiter;
use super::sfu::SfuServer;
use crate::ws::{OutboundMsg, ServerEvent};

/// Remove `user_id` from the voice room of `channel_id`.
///
/// Does not check channel access: leaving is always allowed.
///
/// With `expected_session = Some(s)` only the peer of session `s` is removed;
/// if the user has no peer in the room or a different (newer) session, nothing
/// happens and `Ok(false)` is returned. With `None` the call behaves like an
/// explicit leave and fails with [`VoiceError::RoomNotFound`] if the room does
/// not exist.
///
/// Returns whether a peer was removed.
pub async fn leave_voice_internal(
    sfu: &Arc<SfuServer>,
    pool: &PgPool,
    user_id: Uuid,
    channel_id: Uuid,
    screen_share_limiter: Option<&ScreenShareLimiter>,
    expected_session: Option<Uuid>,
) -> Result<bool, VoiceError> {
    let Some(room) = sfu.get_room(channel_id).await else {
        return match expected_session {
            Some(_) => Ok(false),
            None => Err(VoiceError::RoomNotFound(channel_id)),
        };
    };

    // A session-bound leave removes the peer first, atomically with the
    // session check, so a newer session's screen shares and webcam stay.
    let session_peer = match expected_session {
        Some(session_id) => match room.remove_peer_if_session(user_id, session_id).await {
            Some(peer) => Some(peer),
            None => return Ok(false),
        },
        None => None,
    };

    // Remove all screen shares for this user
    let removed_shares = room.remove_user_screen_shares(user_id).await;
    if !removed_shares.is_empty() {
        if let Some(limiter) = screen_share_limiter {
            limiter.stop_n(channel_id, removed_shares.len()).await;
        } else {
            warn!("Screen share limiter unavailable during leave — counter not decremented");
        }

        for share in &removed_shares {
            room.broadcast_except(
                user_id,
                ServerEvent::ScreenShareStopped {
                    channel_id,
                    user_id,
                    stream_id: share.stream_id,
                    reason: "disconnected".to_string(),
                },
            )
            .await;
        }
    }

    // Check if webcam is active and stop it
    if room.remove_webcam(user_id).await.is_some() {
        room.broadcast_except(
            user_id,
            ServerEvent::WebcamStopped {
                channel_id,
                user_id,
                reason: "disconnected".to_string(),
            },
        )
        .await;
    }

    // Remove peer from room
    let removed_peer = match session_peer {
        Some(peer) => Some(peer),
        None => room.remove_peer(user_id).await,
    };
    let removed = removed_peer.is_some();
    let removed_connected_at = removed_peer.as_ref().map(|p| p.connected_at);
    if let Some(peer) = removed_peer {
        // Record voice session end metric
        let duration_s = (chrono::Utc::now() - peer.connected_at)
            .num_milliseconds()
            .max(0) as f64
            / 1000.0;
        crate::observability::metrics::record_voice_session_end(duration_s);

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
        peer.close().await;
    }

    // Free per-peer rate-limit buckets so they don't accumulate across reconnects.
    sfu.voice_rate_limiter().forget_peer(user_id);

    // Only announce a departure that actually happened; a leave for a user who
    // is not in the room must not tell the others anything.
    if removed {
        room.broadcast_except(
            user_id,
            ServerEvent::VoiceUserLeft {
                channel_id,
                user_id,
            },
        )
        .await;
    }

    // The last peer of a DM room left: its call is over.
    if let (Some(connected_at), Some(redis)) = (removed_connected_at, sfu.call_redis()) {
        if room.is_empty().await {
            if let Err(e) =
                super::call_service::last_voice_peer_left(redis, channel_id, connected_at).await
            {
                warn!(channel_id = %channel_id, error = %e, "Failed to end DM call after last peer left");
            }
        }
    }

    sfu.cleanup_room_if_empty(channel_id).await;

    info!(
        user_id = %user_id,
        channel_id = %channel_id,
        "User left voice channel"
    );

    Ok(removed)
}

/// Remove the voice peer that belongs to a closing WebSocket.
///
/// Only the peer whose signaling queue is `tx` is removed, so closing one of
/// several sockets of the same user does not end a call running on another.
/// Errors are logged; socket cleanup always continues.
pub async fn leave_voice_on_socket_close(
    sfu: &Arc<SfuServer>,
    pool: &PgPool,
    user_id: Uuid,
    tx: &mpsc::Sender<OutboundMsg>,
    limiter: Option<&ScreenShareLimiter>,
) {
    let Some((channel_id, session_id)) = sfu.find_peer_owned_by(user_id, tx).await else {
        return;
    };
    info!(
        user_id = %user_id,
        channel_id = %channel_id,
        "Removing voice peer of closed socket"
    );
    if let Err(e) =
        leave_voice_internal(sfu, pool, user_id, channel_id, limiter, Some(session_id)).await
    {
        warn!(
            user_id = %user_id,
            channel_id = %channel_id,
            error = %e,
            "Failed to remove voice peer on socket close"
        );
    }
}
