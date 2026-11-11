//! Access changes on open connections.
//!
//! A socket checks channel access once, at `subscribe`. When access can
//! shrink (kick, leave, ban, a member's roles, a role's permissions, a
//! channel's permission overrides, channel or server deletion), the change
//! publishes an internal [`AccessNotice`] after its commit, like
//! [`crate::auth::revocation::SessionNotice`] and the presence audience
//! notices. The forwarder of every affected socket consumes it (it is never
//! forwarded) and re-checks its subscribed channels with the same check
//! `subscribe` uses (`require_channel_access`), dropping those that fail.
//!
//! The notice goes to:
//! - `user:{id}`: kick, leave, ban and role changes of that member;
//! - `guild:{id}`: role permission changes and server deletion (every member);
//! - `channel:{id}`: override changes and deletion of that channel. Only sockets subscribed to the
//!   channel see it, in order with the channel's own events.
//!
//! The re-check runs inside the forwarder, before the next message is
//! handled, so nothing published after the notice reaches a socket that lost
//! access. A database error ends the socket (close 1011): the client
//! reconnects and subscribes again with fresh checks. A lost notice is caught
//! by the socket's periodic re-check (at most 60 s, see `handle_socket`).

use std::collections::HashSet;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use fred::prelude::*;
use sqlx::PgPool;
use tokio::sync::RwLock;
use tracing::{debug, warn};
use uuid::Uuid;

use super::events::channels;
use crate::api::AppState;
use crate::permissions::PermissionError;

/// Upper bound for one re-check of a socket's subscriptions.
pub const ACCESS_CHECK_TIMEOUT: Duration = Duration::from_secs(5);

/// Attempts per notice publish.
const NOTICE_ATTEMPTS: u32 = 3;

/// Internal notice, consumed by the WebSocket forwarder and never forwarded.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AccessNotice {
    /// Re-check channel subscriptions: only `channel_id` if given, else all.
    AccessChanged {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        channel_id: Option<Uuid>,
    },
}

/// A change that can take channel access away from members.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccessChange {
    /// The member was kicked from the server.
    MemberKicked { guild_id: Uuid, user_id: Uuid },
    /// The member left the server.
    MemberLeft { guild_id: Uuid, user_id: Uuid },
    /// The account was banned (server-wide).
    UserBanned { user_id: Uuid },
    /// A role was given to or taken from the member.
    MemberRolesChanged { guild_id: Uuid, user_id: Uuid },
    /// A role's permissions changed, or a role was deleted.
    GuildRolesChanged { guild_id: Uuid },
    /// A channel's permission overrides changed.
    ChannelPermissionsChanged { channel_id: Uuid },
    /// The channel was deleted.
    ChannelDeleted { channel_id: Uuid },
    /// The server was deleted (with all its channels).
    GuildDeleted { guild_id: Uuid },
}

impl AccessChange {
    /// Where the notice goes and what it names.
    fn notice(self) -> (String, AccessNotice) {
        let all = AccessNotice::AccessChanged { channel_id: None };
        match self {
            Self::MemberKicked { user_id, .. }
            | Self::MemberLeft { user_id, .. }
            | Self::UserBanned { user_id }
            | Self::MemberRolesChanged { user_id, .. } => (channels::user_events(user_id), all),
            Self::GuildRolesChanged { guild_id } | Self::GuildDeleted { guild_id } => {
                (channels::guild_events(guild_id), all)
            }
            Self::ChannelPermissionsChanged { channel_id }
            | Self::ChannelDeleted { channel_id } => (
                channels::channel_events(channel_id),
                AccessNotice::AccessChanged {
                    channel_id: Some(channel_id),
                },
            ),
        }
    }
}

/// Apply `change` to open connections. Call it after the change committed;
/// it never fails the request (errors are logged, the periodic re-check
/// covers a lost notice).
#[tracing::instrument(skip(state))]
pub async fn access_changed(state: &AppState, change: AccessChange) {
    let (channel, notice) = change.notice();
    publish_notice(&state.redis, &channel, &notice).await;
}

async fn publish_notice(redis: &Client, channel: &str, notice: &AccessNotice) {
    let Ok(payload) = serde_json::to_string(notice) else {
        return;
    };
    for attempt in 1..=NOTICE_ATTEMPTS {
        let sent: Result<(), fred::error::Error> = redis.publish(channel, payload.as_str()).await;
        match sent {
            Ok(()) => return,
            Err(e) if attempt == NOTICE_ATTEMPTS => {
                warn!(
                    error = %e,
                    attempts = NOTICE_ATTEMPTS,
                    "Access notice could not be published; sockets re-check within a minute"
                );
            }
            Err(_) => tokio::time::sleep(Duration::from_millis(50 * (1 << attempt))).await,
        }
    }
}

/// The subscriptions could not be re-checked (database error or timeout).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AccessCheckFailed;

/// Re-check `user_id`'s access to the subscribed channels (only `only`, if
/// given and subscribed) and drop the ones that fail. Returns the dropped
/// channels.
#[allow(clippy::implicit_hasher)]
pub async fn revalidate_subscriptions(
    db: &PgPool,
    user_id: Uuid,
    subscribed: &RwLock<HashSet<Uuid>>,
    only: Option<Uuid>,
) -> Result<Vec<Uuid>, AccessCheckFailed> {
    let channels: Vec<Uuid> = {
        let set = subscribed.read().await;
        match only {
            Some(channel) if set.contains(&channel) => vec![channel],
            Some(_) => Vec::new(),
            None => set.iter().copied().collect(),
        }
    };
    if channels.is_empty() {
        return Ok(Vec::new());
    }

    let check = async {
        let mut lost = Vec::new();
        for channel in channels {
            match crate::permissions::require_channel_access(db, user_id, channel).await {
                Ok(_) => {}
                Err(PermissionError::DatabaseError(e)) => {
                    warn!(error = %e, user_id = %user_id, "Subscription re-check failed");
                    return Err(AccessCheckFailed);
                }
                Err(_) => lost.push(channel),
            }
        }
        Ok(lost)
    };
    let lost = tokio::time::timeout(ACCESS_CHECK_TIMEOUT, check)
        .await
        .map_err(|_| AccessCheckFailed)??;

    if !lost.is_empty() {
        let mut set = subscribed.write().await;
        for channel in &lost {
            set.remove(channel);
        }
        debug!(user_id = %user_id, dropped = ?lost, "Dropped subscriptions after access change");
    }
    Ok(lost)
}

/// Counts access notices a socket has processed.
///
/// `subscribe` reads it before its access check; if it moved by the time the
/// channel is added, a notice may have been handled in between without seeing
/// the new subscription, so `subscribe` checks once more.
#[derive(Debug, Default)]
pub struct AccessEpoch(AtomicU64);

impl AccessEpoch {
    /// The current value.
    pub fn current(&self) -> u64 {
        self.0.load(Ordering::SeqCst)
    }

    /// Record that a notice is being processed (before its re-check reads
    /// the subscriptions).
    pub fn bump(&self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notice_wire_format() {
        let all = AccessNotice::AccessChanged { channel_id: None };
        assert_eq!(
            serde_json::to_string(&all).unwrap(),
            r#"{"type":"access_changed"}"#
        );
        let one = AccessNotice::AccessChanged {
            channel_id: Some(Uuid::nil()),
        };
        let json = serde_json::to_string(&one).unwrap();
        assert_eq!(
            json,
            r#"{"type":"access_changed","channel_id":"00000000-0000-0000-0000-000000000000"}"#
        );
        assert_eq!(serde_json::from_str::<AccessNotice>(&json).unwrap(), one);
    }

    #[test]
    fn notice_is_not_a_server_event() {
        let json =
            serde_json::to_string(&AccessNotice::AccessChanged { channel_id: None }).unwrap();
        assert!(serde_json::from_str::<crate::ws::ServerEvent>(&json).is_err());
        assert!(
            serde_json::from_str::<crate::presence::connections::PresenceNotice>(&json).is_err()
        );
        assert!(serde_json::from_str::<crate::auth::revocation::SessionNotice>(&json).is_err());
    }

    #[test]
    fn notices_go_where_the_affected_sockets_listen() {
        let (u, g, c) = (Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4());
        let user = |change: AccessChange| change.notice().0;
        assert_eq!(
            user(AccessChange::MemberKicked {
                guild_id: g,
                user_id: u
            }),
            channels::user_events(u)
        );
        assert_eq!(
            user(AccessChange::GuildRolesChanged { guild_id: g }),
            channels::guild_events(g)
        );
        let (channel, notice) = AccessChange::ChannelDeleted { channel_id: c }.notice();
        assert_eq!(channel, channels::channel_events(c));
        assert_eq!(
            notice,
            AccessNotice::AccessChanged {
                channel_id: Some(c)
            }
        );
    }
}
