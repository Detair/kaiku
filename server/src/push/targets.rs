//! Who gets a wake for a new message (contract C20/C21).
//!
//! Resolution runs inside a spawned task ([`spawn_wake_for_new_message`]):
//! mention targets need a channel-access check per candidate, which must never
//! delay the send response.

use uuid::Uuid;

use super::WakeSignal;
use crate::api::AppState;
use crate::chat::mentions::{extract_mentions, resolve_mention_targets};
use crate::chat::queries;
use crate::db;

/// Recipients and signals for a new message by `author_id` in `channel`.
///
/// - DM / group DM: every participant except the author and blocked users gets `dm`, encrypted or
///   not (the signal carries no content).
/// - Guild channel, not encrypted: resolved `@mention` targets get `mention`.
/// - Anything else: nobody.
pub async fn wake_targets(
    state: &AppState,
    channel: &db::Channel,
    author_id: Uuid,
    content: &str,
    encrypted: bool,
) -> Vec<(Uuid, WakeSignal)> {
    if channel.channel_type == db::ChannelType::Dm {
        let ids = match queries::list_dm_participant_ids(&state.db, channel.id).await {
            Ok(ids) => ids
                .into_iter()
                .filter(|&id| id != author_id)
                .collect::<Vec<_>>(),
            Err(e) => {
                tracing::warn!(error = %e, channel_id = %channel.id, "push: DM participants lookup failed");
                return Vec::new();
            }
        };
        // Blocked participants are never woken; a failed lookup wakes nobody.
        let blocked = match queries::blocked_among(&state.db, author_id, &ids).await {
            Ok(set) => set,
            Err(e) => {
                tracing::warn!(error = %e, channel_id = %channel.id, "push: DM block lookup failed");
                return Vec::new();
            }
        };
        return ids
            .into_iter()
            .filter(|id| !blocked.contains(id))
            .map(|id| (id, WakeSignal::dm(channel.id, 1)))
            .collect();
    }
    if channel.guild_id.is_none() || encrypted {
        return Vec::new();
    }
    let mentions = extract_mentions(content);
    if mentions.usernames.is_empty() && !mentions.everyone {
        return Vec::new();
    }
    resolve_mention_targets(state, channel, author_id, &mentions)
        .await
        .into_iter()
        .map(|id| (id, WakeSignal::mention(channel.id, 1)))
        .collect()
}

/// Resolve the wake targets of a committed message and hand each to the
/// coalescer. Returns immediately; the send response never waits on push.
pub fn spawn_wake_for_new_message(
    state: &AppState,
    channel: db::Channel,
    author_id: Uuid,
    content: String,
    encrypted: bool,
) {
    let state = state.clone();
    tokio::spawn(async move {
        for (user_id, signal) in
            wake_targets(&state, &channel, author_id, &content, encrypted).await
        {
            state.push.notify(&state, user_id, signal);
        }
    });
}
