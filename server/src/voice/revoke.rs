//! Removing members from voice when they lose access (contract C31).
//!
//! After a change that can take access away (see [`crate::ws::access`]), the
//! affected voice rooms are checked: every participant must still have
//! `VIEW_CHANNEL` and `VOICE_CONNECT` (the voice join check). Whoever fails
//! leaves through [`leave_voice_internal`] (screen share, webcam and DM call
//! bookkeeping, `voice_user_left` to the others) and gets `voice_removed` on
//! the socket that owned the voice peer. A deleted channel removes everyone,
//! a ban removes the user from every room.
//!
//! Rooms live in this process (single node), so this runs synchronously in
//! the request that made the change. A database error keeps the participant;
//! the socket's periodic re-check catches up within a minute.

use std::sync::Arc;

use sqlx::PgPool;
use tokio::sync::mpsc;
use tracing::{info, warn};
use uuid::Uuid;

use super::leave::leave_voice_internal;
use super::screen_share::ScreenShareLimiter;
use super::sfu::{Room, SfuServer};
use crate::permissions::{GuildPermissions, PermissionError};
use crate::ws::{OutboundMsg, ServerEvent, VoiceRemovedReason};

/// How long the `voice_removed` send may wait for the owner's queue.
const REMOVED_SEND_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(250);

/// Which voice rooms and participants to check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VoiceAccessScope {
    /// One user, in the rooms of one server (`Some`) or in every room
    /// (`None`; removed without a check, used for bans).
    User {
        user_id: Uuid,
        guild_id: Option<Uuid>,
    },
    /// Everyone in the rooms of one server.
    Guild(Uuid),
    /// Everyone in one room.
    Channel(Uuid),
    /// Everyone in rooms whose channel no longer exists.
    DeletedChannels,
}

/// Whether `user_id` may (still) be in the voice room of `channel_id`: the
/// voice join check (`VIEW_CHANNEL` via `require_channel_access`, then
/// `VOICE_CONNECT`). A database error is returned as `Err`.
pub async fn has_voice_access(
    pool: &PgPool,
    user_id: Uuid,
    channel_id: Uuid,
) -> Result<bool, PermissionError> {
    match crate::permissions::require_channel_access(pool, user_id, channel_id).await {
        Ok(ctx) => Ok(ctx.has_permission(GuildPermissions::VOICE_CONNECT)),
        Err(e @ PermissionError::DatabaseError(_)) => Err(e),
        Err(_) => Ok(false),
    }
}

/// Remove everyone in `scope` who lost voice access, telling each removed
/// user `voice_removed` with `reason`. Returns how many were removed.
pub async fn enforce_voice_access(
    sfu: &Arc<SfuServer>,
    pool: &PgPool,
    limiter: Option<&ScreenShareLimiter>,
    scope: VoiceAccessScope,
    reason: VoiceRemovedReason,
) -> usize {
    let rooms = sfu.rooms_snapshot().await;
    if rooms.is_empty() {
        return 0;
    }
    let room_ids: Vec<Uuid> = rooms.iter().map(|r| r.channel_id).collect();

    // (room, users to look at, removed without a check)
    let mut candidates: Vec<(Arc<Room>, Vec<Uuid>, bool)> = Vec::new();
    match scope {
        VoiceAccessScope::Channel(channel_id) => {
            let unconditional = reason == VoiceRemovedReason::ChannelDeleted;
            for room in rooms.into_iter().filter(|r| r.channel_id == channel_id) {
                let users = participants(&room).await;
                candidates.push((room, users, unconditional));
            }
        }
        VoiceAccessScope::Guild(guild_id) => {
            let Some(in_guild) = channels_of_guild(pool, guild_id, &room_ids).await else {
                return 0;
            };
            for room in rooms
                .into_iter()
                .filter(|r| in_guild.contains(&r.channel_id))
            {
                let users = participants(&room).await;
                candidates.push((room, users, false));
            }
        }
        VoiceAccessScope::User {
            user_id,
            guild_id: Some(guild_id),
        } => {
            let Some(in_guild) = channels_of_guild(pool, guild_id, &room_ids).await else {
                return 0;
            };
            for room in rooms
                .into_iter()
                .filter(|r| in_guild.contains(&r.channel_id))
            {
                if room.get_peer(user_id).await.is_some() {
                    candidates.push((room, vec![user_id], false));
                }
            }
        }
        VoiceAccessScope::User {
            user_id,
            guild_id: None,
        } => {
            for room in rooms {
                if room.get_peer(user_id).await.is_some() {
                    candidates.push((room, vec![user_id], true));
                }
            }
        }
        VoiceAccessScope::DeletedChannels => {
            let existing: Vec<Uuid> =
                match sqlx::query_scalar("SELECT id FROM channels WHERE id = ANY($1)")
                    .bind(&room_ids)
                    .fetch_all(pool)
                    .await
                {
                    Ok(ids) => ids,
                    Err(e) => {
                        warn!(error = %e, "Voice access check: channel lookup failed");
                        return 0;
                    }
                };
            for room in rooms
                .into_iter()
                .filter(|r| !existing.contains(&r.channel_id))
            {
                let users = participants(&room).await;
                candidates.push((room, users, true));
            }
        }
    }

    let mut removed = 0;
    for (room, users, unconditional) in candidates {
        for user_id in users {
            if !unconditional {
                match has_voice_access(pool, user_id, room.channel_id).await {
                    Ok(true) => continue,
                    Ok(false) => {}
                    Err(e) => {
                        warn!(
                            user_id = %user_id,
                            channel_id = %room.channel_id,
                            error = %e,
                            "Voice access check failed; participant kept until the next re-check"
                        );
                        continue;
                    }
                }
            }
            if remove_with_notice(sfu, pool, limiter, &room, user_id, reason).await {
                removed += 1;
            }
        }
    }
    removed
}

/// Re-check the voice peer owned by the socket with queue `tx` (the socket's
/// periodic backstop); remove it with `permission_lost` if access is gone.
pub async fn recheck_socket_peer(
    sfu: &Arc<SfuServer>,
    pool: &PgPool,
    limiter: Option<&ScreenShareLimiter>,
    user_id: Uuid,
    tx: &mpsc::Sender<OutboundMsg>,
) {
    let Some((channel_id, _)) = sfu.find_peer_owned_by(user_id, tx).await else {
        return;
    };
    if matches!(has_voice_access(pool, user_id, channel_id).await, Ok(false)) {
        if let Some(room) = sfu.get_room(channel_id).await {
            remove_with_notice(
                sfu,
                pool,
                limiter,
                &room,
                user_id,
                VoiceRemovedReason::PermissionLost,
            )
            .await;
        }
    }
}

async fn participants(room: &Room) -> Vec<Uuid> {
    room.peers.read().await.keys().copied().collect()
}

/// The rooms among `room_ids` whose channel belongs to `guild_id`, or `None`
/// on a database error (logged).
async fn channels_of_guild(pool: &PgPool, guild_id: Uuid, room_ids: &[Uuid]) -> Option<Vec<Uuid>> {
    match sqlx::query_scalar("SELECT id FROM channels WHERE guild_id = $1 AND id = ANY($2)")
        .bind(guild_id)
        .bind(room_ids)
        .fetch_all(pool)
        .await
    {
        Ok(ids) => Some(ids),
        Err(e) => {
            warn!(error = %e, guild_id = %guild_id, "Voice access check: channel lookup failed");
            None
        }
    }
}

/// Remove `user_id`'s current peer from `room` and send `voice_removed` to
/// the socket that owned it. Returns whether a peer was removed.
async fn remove_with_notice(
    sfu: &Arc<SfuServer>,
    pool: &PgPool,
    limiter: Option<&ScreenShareLimiter>,
    room: &Room,
    user_id: Uuid,
    reason: VoiceRemovedReason,
) -> bool {
    let channel_id = room.channel_id;
    let Some(peer) = room.get_peer(user_id).await else {
        return false;
    };
    let owner = peer.signal_tx.clone();
    let session_id = peer.session_id;
    drop(peer);

    // Only this session: a newer join of the user is a separate decision.
    match leave_voice_internal(sfu, pool, user_id, channel_id, limiter, Some(session_id)).await {
        Ok(true) => {}
        Ok(false) => return false,
        Err(e) => {
            warn!(user_id = %user_id, channel_id = %channel_id, error = %e, "Failed to remove voice peer");
            return false;
        }
    }
    info!(
        user_id = %user_id,
        channel_id = %channel_id,
        reason = reason.as_str(),
        "Removed voice participant after access change"
    );
    let event = OutboundMsg::Event(ServerEvent::VoiceRemoved { channel_id, reason });
    if let Err(e) =
        crate::ws::handlers::send_outbound_within(&owner, event, REMOVED_SEND_TIMEOUT).await
    {
        // The socket is closing or stalled; it tears itself down.
        warn!(user_id = %user_id, error = %e, "Could not deliver voice_removed");
    }
    true
}
