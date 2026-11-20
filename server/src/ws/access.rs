//! Access changes on open connections.
//!
//! A socket keeps two sets of channels:
//! - **requested**: what the client asked for with `subscribe` (and DMs it was auto-subscribed to);
//!   only `unsubscribe` removes a channel from it;
//! - **allowed**: the requested channels the user may currently view.
//!
//! Only events of allowed channels reach the client. A revalidation
//! recomputes allowed from requested in both directions: a channel whose
//! access was lost leaves allowed, a channel whose access came back (re-invite,
//! role granted again, override removed) returns to it without the client
//! doing anything.
//!
//! # Access epochs
//!
//! The server runs as a single node (like the voice SFU and the connection
//! caps), so access changes are fenced in process: [`AccessRegistry`] keeps a
//! monotonically increasing epoch per server (guild) and per user.
//! - **Guild epoch**: role permissions, role deletion, channel overrides, channel deletion, server
//!   deletion. Affects every member's channels of that server.
//! - **User epoch**: membership of that user (join, leave, kick, ban), the user's roles (incl.
//!   reaction roles), group-DM membership, admin elevation, account deletion. Affects all of that
//!   user's channels and the admin event subscription.
//!
//! Every write that can change access goes through [`bump_after`] or
//! [`bump_after_if`]: the epoch is bumped in the poll in which the write's
//! future completes, with no `.await` in between, and also when that future
//! is dropped while the commit is in flight or the commit fails (outcome
//! unknown). Only they return the [`Bumped`] proof that [`access_changed`]
//! (voice enforcement, cross-node notice) takes, and handlers call it right
//! after, before any other post-commit work.
//!
//! # What is fenced
//!
//! Each socket ([`SocketAccess`]) keeps **scopes** and decides every frame
//! when it is **written** (the sender task), not when it is queued:
//!
//! | Frame (`OutboundMsg`) | Depends on | Decision |
//! |---|---|---|
//! | `Channel` | user scope + the channel's server scope | [`SocketAccess::admit`]: in the allowed set |
//! | `ChannelBound` (a bot's answer to the user's command, from the user's own feed) | as `Channel`; for a channel that was never requested: one read at epochs that must still hold afterwards | [`SocketAccess::admit_bound`] |
//! | `Guild` (a server's own feed, an event reminder) | user scope + that server's scope | [`SocketAccess::admit_guild`]: member |
//! | `Voice` (everything of a voice room) | the voice scope: user epoch + the room's server epoch as of the join check | [`SocketAccess::admit_voice`]: the socket owns the peer and voice access holds |
//! | `Admin` | the admin scope: user epoch; a "yes" is never reused (see The admin stream) | [`SocketAccess::admit_admin`]: requested (until `admin_unsubscribe`), and the session is an elevated admin's — read again for every frame of an allowed socket, a "no" cached per epoch; admin events are rare and elevated sockets few |
//!
//! There are four kinds of scope — the **user**, each **server**, the
//! **voice room**, the **admin stream** — and one state machine for all of
//! them. A scope is **current** (computed at the epoch that still holds),
//! **stale** (the epoch moved, or it was invalidated) or **failing** (see
//! below). A frame of a current scope is decided from memory: atomic loads
//! and a lookup. A frame of a stale scope waits for a revalidation, which
//! reads the epochs first, then the database, and counts only if the epochs
//! are still the same afterwards. So after a bump nothing of that scope is
//! written on old authorization: neither events published after the commit
//! nor events that were already queued.
//!
//! Every frame goes through one decision loop and leaves it in one of three
//! ways: **admitted** (its scope is current and allows it), **denied** (its
//! scope is current and does not, or it was never asked for) or
//! **undecided** — and an undecided outbound frame is always recorded on its
//! scope (see Failure). There is no fourth way.
//!
//! `subscribe` uses the same epochs: it reads them, checks access and records
//! the result only if they are unchanged, otherwise it checks again.
//!
//! # The admin stream
//!
//! An elevation also ends without any epoch moving: it expires, and the
//! admin CLI demotes from another process. So the admin scope has one rule
//! of its own — a "yes" is never reused — and these invariants:
//!
//! 1. **One writer.** Whether the stream is allowed is written in one place, the revalidation
//!    (`apply`), from a read of the committed rows: live full session, system-admin role, an active
//!    elevation of this session, not banned. Revalidations are serialised, so answers are applied
//!    in the order they were read. `admin_subscribe` ([`SocketAccess::subscribe_admin`]) decides
//!    nothing itself: it records the request and answers it as (6) says.
//! 2. **An answer counts only if it is the newest.** It is applied only if the user epoch is
//!    unchanged and nothing invalidated the admin scope since the read was planned
//!    (`admin_invalidations`, counted for this scope alone: an admin frame taken, an
//!    `admin_subscribe`, the backstop). An answer that does not count changes nothing on the scope;
//!    it is read again.
//! 3. **A frame is pinned in one critical section.** [`SocketAccess::admit_admin`] either refuses
//!    the frame outright — the stream is not requested, or the scope is a current "no" (cached per
//!    user epoch: no read) — or invalidates the scope, in that same critical section. From there
//!    only (1) and (2) make the scope current again, so the frame is written only on a "yes" whose
//!    read was planned after the frame was taken from the queue.
//! 4. **The request stays, and the decision outlasts it.** The stream is requested from
//!    `admin_subscribe` until `admin_unsubscribe` or the socket's end. A "no" only makes it not
//!    allowed; the next elevation bumps the user epoch and delivery comes back without the client
//!    doing anything. The decision is kept when the client unsubscribes (nothing is written or read
//!    for a stream that is not requested), so asking again does not start from nothing.
//! 5. **A refused socket is told nothing.** A read that fails drops the frame and makes the scope
//!    failing; the recovery asks for a resync only if it finds the stream allowed.
//! 6. **The answer is the socket's own, and asking costs a bounded number of reads.** The answer to
//!    `admin_subscribe` depends on the user epoch, this socket's earlier answers and the result of
//!    a read the request itself asked for — never on what a frame's read wrote. A refusal stands
//!    for its user epoch: asking again is answered without a read. A request that would read a
//!    "yes" again (for a fresh answer; frames are read anyway) does so at most once per
//!    [`ADMIN_SUBSCRIBE_COOLDOWN`] and socket; meanwhile the socket's earlier answer at the same
//!    user epoch stands, and without one it is "not decided yet". So within the cooldown an
//!    elevation that ran out keeps its earlier "yes": nothing is written on it, every frame is read
//!    on its own. The reads it asks for run in a pool of their own
//!    ([`ADMIN_SUBSCRIBE_CONCURRENCY`]) and never take one of the permits other revalidations wait
//!    for. It takes its socket's revalidation gate first and a permit of that pool second, under
//!    one deadline for both waits, and lets go of the gate when the deadline passes: no task holds
//!    a permit while it waits for a gate. Meanwhile frames queued on that socket wait behind it (at
//!    most the deadline). After the deadline it answers "not decided yet".
//!
//! # Revalidation cost
//!
//! One revalidation ([`SocketAccess::refresh`]) covers every scope of the
//! socket that is not current, with batched reads: the account's ban state,
//! one [`viewable_and_existing_channels`] call for the stale servers'
//! channels, the user's memberships, after a user bump one query for the
//! requested DMs, the voice check if the voice scope is stale, and the
//! elevation check if the admin scope is. At most
//! [`REVALIDATION_CONCURRENCY`] run at once process-wide (and
//! [`ADMIN_SUBSCRIBE_CONCURRENCY`] more for what `admin_subscribe` asks
//! for). Bumps arriving
//! while one runs coalesce into one follow-up. Idle sockets are woken by the
//! bump and revalidate after [`WAKE_DEBOUNCE`], so voice and the admin flag
//! do not wait for the next event. A periodic backstop (about every minute,
//! with jitter) invalidates every scope and revalidates.
//!
//! # Failure
//!
//! From the moment a revalidation attempt for a scope fails (database error,
//! timeout) or cannot be made (the socket is in backoff) until one succeeds,
//! the scope is **failing**: nothing of it is written, and every event
//! dropped for that reason is recorded on the scope. The revalidation is
//! retried with backoff; the socket stays open. When the scope recovers and
//! something was dropped, the socket closes with 1011 `RESYNC` (spread over a
//! few seconds) so the client reconnects and catches up. Only after failing
//! for [`FAILURE_CLOSE_AFTER`] does the socket close with 1011 regardless —
//! when the socket's own revalidation fails then, and when a read an
//! `admin_subscribe` asked for does ([`AdminSubscription::Failed`]).
//! The forwarder never drops for a stale or failing scope; the writer does,
//! in one place ([`SocketAccess`]'s decision loop).
//!
//! The backoff (since when attempts fail, how many, when the next one may be
//! made) is one per socket, and two things hold for it:
//!
//! 1. **It outlasts the scopes.** A failed attempt sets it; until its retry time no read is
//!    attempted for this socket, whatever goes away or comes back meanwhile (an `admin_unsubscribe`
//!    and a new `admin_subscribe`, a voice peer that left and rejoined). It ends when the database
//!    answers, or when no failure renewed it for [`FAILURE_CLOSE_AFTER`] after its retry time: a
//!    failure that old says nothing about now, and the next one counts from itself.
//! 2. **Only a scope that is not current waits for the retry.** A socket with nothing left to
//!    revalidate has no retry time to wake for ([`SocketAccess::retry_at`] is `None`) and is not
//!    failing ([`SocketAccess::failing_for`]), also while its backoff still runs; a revalidation
//!    that finds nothing to do is not a failed one.
//!
//! # Bounds
//!
//! A socket keeps at most [`MAX_REQUESTED_CHANNELS`] requested server
//! channels and [`MAX_REQUESTED_DIRECT`] requested DMs (counted apart, so a
//! new DM always finds room unless the socket is full of DMs — then it
//! closes for a resync instead of filtering the DM), one scope per server
//! with a requested channel or a feed event, one voice scope and one admin
//! scope. The registry keeps one cell per server and user that exists.
//!
//! # Several nodes
//!
//! [`access_changed`] also publishes an [`AccessNotice`] on
//! [`NOTICE_CHANNEL`] (detached, short timeout). Nothing subscribes to it
//! today; a multi-node setup would run one subscriber per process that bumps
//! the local registry for notices whose `origin` is another process.
//!
//! [`viewable_and_existing_channels`]: crate::permissions::viewable_and_existing_channels

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, LazyLock, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use dashmap::DashMap;
use fred::prelude::*;
use sqlx::PgPool;
use tokio::sync::{watch, Notify};
use tokio::time::Instant;
use tracing::{debug, warn};
use uuid::Uuid;

use super::events::VoiceRemovedReason;
use crate::api::AppState;
use crate::voice::revoke::VoiceAccessScope;

/// Delay between a bump and the revalidation of an idle socket. Bumps within
/// this window coalesce.
pub const WAKE_DEBOUNCE: Duration = Duration::from_millis(150);

/// A socket whose revalidations keep failing for this long closes with 1011.
pub const FAILURE_CLOSE_AFTER: Duration = Duration::from_secs(30);

/// Upper bound for one revalidation (its database reads).
pub const REFRESH_TIMEOUT: Duration = Duration::from_secs(5);

/// First retry delay after a failed revalidation; doubles per failure.
const RETRY_BASE: Duration = Duration::from_millis(250);
/// Longest retry delay after failed revalidations.
const RETRY_MAX: Duration = Duration::from_secs(5);

/// Revalidation reads running at once, process-wide.
///
/// The database pool has
/// 20 connections (`db::create_pool`); a server-wide bump must leave most of
/// them to requests, so at most 4 are used for revalidation and the other
/// sockets queue for a permit.
pub const REVALIDATION_CONCURRENCY: usize = 4;

static REVALIDATION_PERMITS: tokio::sync::Semaphore =
    tokio::sync::Semaphore::const_new(REVALIDATION_CONCURRENCY);

/// Reads asked for by `admin_subscribe` running at once, process-wide.
///
/// Any client can send `admin_subscribe`, so these reads have a pool of
/// their own and never take one of the [`REVALIDATION_CONCURRENCY`] permits
/// that other sockets' revalidations (and with them their writers) wait
/// for. One more database connection at most.
pub const ADMIN_SUBSCRIBE_CONCURRENCY: usize = 1;

/// Longest an `admin_subscribe` waits for its socket's revalidation gate
/// and a permit of that pool together (one deadline for both). After it the
/// request is answered "not decided yet"; frames decide on their own reads.
const ADMIN_SUBSCRIBE_WAIT: Duration = Duration::from_secs(1);

/// How soon after an `admin_subscribe` had the admin stream read, the next
/// one on the same socket may do so again.
///
/// Until then it is answered as the socket was answered before at the same
/// user epoch (every admin frame is read on its own anyway), or "not
/// decided yet" without such an answer.
pub const ADMIN_SUBSCRIBE_COOLDOWN: Duration = Duration::from_secs(10);

/// The pool the reads of one socket's `admin_subscribe` run in.
fn admin_subscribe_pool() -> Arc<tokio::sync::Semaphore> {
    static POOL: LazyLock<Arc<tokio::sync::Semaphore>> =
        LazyLock::new(|| Arc::new(tokio::sync::Semaphore::new(ADMIN_SUBSCRIBE_CONCURRENCY)));
    if cfg!(test) {
        // Unit tests share one process, and some hold a read on purpose:
        // each socket gets a pool of its own there.
        Arc::new(tokio::sync::Semaphore::new(ADMIN_SUBSCRIBE_CONCURRENCY))
    } else {
        Arc::clone(&POOL)
    }
}

/// Most server channels one socket can have requested at a time.
///
/// A `subscribe` beyond it is refused (`TOO_MANY_SUBSCRIPTIONS`); the client
/// unsubscribes from something first. Bounds the per-socket state and the
/// size of one revalidation.
pub const MAX_REQUESTED_CHANNELS: usize = 2_000;

/// Most DMs and group DMs one socket can have requested at a time.
///
/// Counted apart from server channels, so that the automatic subscription of
/// a new DM (`dm_created`) is never refused because of server channels. A
/// `subscribe` beyond it is refused like one beyond
/// [`MAX_REQUESTED_CHANNELS`]; a new DM that does not fit ends the socket
/// with a resynchronising close instead of going unheard.
pub const MAX_REQUESTED_DIRECT: usize = 2_000;

/// Revalidations one delivery decision waits for before it gives up
/// (epochs kept moving); the decision is evaluated after each of them.
const DECISION_REVALIDATIONS: usize = 4;

/// Servers whose feed (`guild:{id}`) a socket keeps a scope for without
/// having requested one of their channels; beyond it the set starts over
/// (the next feed event of a server costs one read again).
const MAX_FEED_GUILDS: usize = 256;

/// Bound for publishing the cross-node notice.
const NOTICE_TIMEOUT: Duration = Duration::from_millis(500);

/// Valkey channel of the cross-node [`AccessNotice`].
pub const NOTICE_CHANNEL: &str = "access:changed";

// ---------------------------------------------------------------------------
// Registry
// ---------------------------------------------------------------------------

/// Process-wide access epochs (see the module docs).
pub struct AccessRegistry {
    guilds: DashMap<Uuid, Arc<AtomicU64>>,
    users: DashMap<Uuid, Arc<AtomicU64>>,
    /// Ticks on every bump; sockets watch it to revalidate while idle.
    wake: watch::Sender<u64>,
    /// Identifies this process in [`AccessNotice::origin`].
    origin: Uuid,
}

static REGISTRY: LazyLock<AccessRegistry> = LazyLock::new(|| AccessRegistry {
    guilds: DashMap::new(),
    users: DashMap::new(),
    wake: watch::channel(0).0,
    origin: Uuid::new_v4(),
});

/// The process-wide registry.
pub fn registry() -> &'static AccessRegistry {
    &REGISTRY
}

/// The epochs one access decision was made at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EpochSnapshot {
    /// The user's epoch.
    pub user: u64,
    /// The server's epoch (`None` for DMs).
    pub guild: Option<u64>,
}

/// One process-wide sequence for all epoch values: a cell that is removed and
/// created again never repeats a value a live socket may have stored, and 0
/// is never an epoch (sockets use it as "never computed").
static EPOCH_SEQ: AtomicU64 = AtomicU64::new(1);

fn next_epoch() -> u64 {
    EPOCH_SEQ.fetch_add(1, Ordering::SeqCst) + 1
}

fn cell(map: &DashMap<Uuid, Arc<AtomicU64>>, id: Uuid) -> Arc<AtomicU64> {
    map.entry(id)
        .or_insert_with(|| Arc::new(AtomicU64::new(next_epoch())))
        .clone()
}

impl AccessRegistry {
    /// The epoch cell of `guild_id` (created on first use).
    pub fn guild_epoch(&self, guild_id: Uuid) -> Arc<AtomicU64> {
        cell(&self.guilds, guild_id)
    }

    /// The epoch cell of `user_id` (created on first use).
    pub fn user_epoch(&self, user_id: Uuid) -> Arc<AtomicU64> {
        cell(&self.users, user_id)
    }

    /// Drop the cell of a deleted server (after its final bump). Sockets that
    /// still hold the old cell see it moved and revalidate.
    fn forget_guild(&self, guild_id: Uuid) {
        self.guilds.remove(&guild_id);
    }

    /// Drop the cell of a deleted account (after its final bump; sockets
    /// that still hold the old cell see it moved).
    fn forget_user(&self, user_id: Uuid) {
        if let Some((_, cell)) = self.users.remove(&user_id) {
            cell.store(next_epoch(), Ordering::SeqCst);
        }
    }

    /// Number of cells (tests).
    #[doc(hidden)]
    pub fn cells(&self) -> (usize, usize) {
        (self.guilds.len(), self.users.len())
    }

    /// Current epochs of `user_id` and (if given) `guild_id`.
    pub fn snapshot(&self, user_id: Uuid, guild_id: Option<Uuid>) -> EpochSnapshot {
        EpochSnapshot {
            user: self.user_epoch(user_id).load(Ordering::SeqCst),
            guild: guild_id.map(|g| self.guild_epoch(g).load(Ordering::SeqCst)),
        }
    }

    /// Advance `guild_id`'s epoch and wake the sockets (only through
    /// [`bump_after`] / [`bump_after_if`]).
    fn bump_guild(&self, guild_id: Uuid) {
        self.guild_epoch(guild_id)
            .store(next_epoch(), Ordering::SeqCst);
        self.wake.send_modify(|n| *n = n.wrapping_add(1));
    }

    /// Advance `user_id`'s epoch and wake the sockets (only through
    /// [`bump_after`] / [`bump_after_if`]).
    fn bump_user(&self, user_id: Uuid) {
        self.user_epoch(user_id)
            .store(next_epoch(), Ordering::SeqCst);
        self.wake.send_modify(|n| *n = n.wrapping_add(1));
    }

    /// A receiver that changes on every bump.
    pub fn wake_receiver(&self) -> watch::Receiver<u64> {
        self.wake.subscribe()
    }
}

// ---------------------------------------------------------------------------
// Triggers
// ---------------------------------------------------------------------------

/// A change that can take channel access away from members, or give it back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccessChange {
    /// The member was kicked from the server.
    MemberKicked { guild_id: Uuid, user_id: Uuid },
    /// The member left the server.
    MemberLeft { guild_id: Uuid, user_id: Uuid },
    /// The user joined the server or became an active member (invite,
    /// discovery, admin, screening accepted).
    MemberJoined { guild_id: Uuid, user_id: Uuid },
    /// The account was banned (server-wide).
    UserBanned { user_id: Uuid },
    /// A role was given to or taken from the member (incl. reaction roles).
    MemberRolesChanged { guild_id: Uuid, user_id: Uuid },
    /// A role's permissions changed, or a role was deleted.
    GuildRolesChanged { guild_id: Uuid },
    /// A channel's permission overrides changed.
    ChannelPermissionsChanged { guild_id: Uuid, channel_id: Uuid },
    /// The channel was deleted (`guild_id` is `None` for a DM).
    ChannelDeleted {
        guild_id: Option<Uuid>,
        channel_id: Uuid,
    },
    /// The server was deleted (with all its channels).
    GuildDeleted { guild_id: Uuid },
    /// The user left a group DM.
    DmLeft { user_id: Uuid, channel_id: Uuid },
    /// The user's admin elevation began or ended (admin event
    /// subscription).
    AdminAccessChanged { user_id: Uuid },
    /// The account was deleted (memberships, roles and DM participations
    /// went with it; its sessions ended).
    AccountDeleted { user_id: Uuid },
}

/// Which epoch a change bumps.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NoticeScope {
    Guild,
    User,
}

/// Cross-node notice for one bump (see the module docs).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct AccessNotice {
    pub scope: NoticeScope,
    pub id: Uuid,
    /// The process that bumped its own registry already.
    pub origin: Uuid,
}

impl AccessChange {
    /// The epoch this change bumps, if any.
    pub const fn scope(self) -> Option<(NoticeScope, Uuid)> {
        match self {
            Self::MemberKicked { user_id, .. }
            | Self::MemberLeft { user_id, .. }
            | Self::MemberJoined { user_id, .. }
            | Self::UserBanned { user_id }
            | Self::MemberRolesChanged { user_id, .. }
            | Self::DmLeft { user_id, .. }
            | Self::AdminAccessChanged { user_id }
            | Self::AccountDeleted { user_id } => Some((NoticeScope::User, user_id)),
            Self::GuildRolesChanged { guild_id }
            | Self::ChannelPermissionsChanged { guild_id, .. }
            | Self::GuildDeleted { guild_id }
            | Self::ChannelDeleted {
                guild_id: Some(guild_id),
                ..
            } => Some((NoticeScope::Guild, guild_id)),
            // A DM is deleted when its last participant left (who bumped).
            Self::ChannelDeleted { guild_id: None, .. } => None,
        }
    }

    /// Which voice participants to check, and the reason they are told.
    const fn voice(self) -> Option<(VoiceAccessScope, VoiceRemovedReason)> {
        Some(match self {
            Self::MemberKicked { guild_id, user_id } => (
                VoiceAccessScope::User {
                    user_id,
                    guild_id: Some(guild_id),
                },
                VoiceRemovedReason::Kicked,
            ),
            Self::MemberLeft { guild_id, user_id }
            | Self::MemberRolesChanged { guild_id, user_id } => (
                VoiceAccessScope::User {
                    user_id,
                    guild_id: Some(guild_id),
                },
                VoiceRemovedReason::PermissionLost,
            ),
            Self::UserBanned { user_id } => (
                VoiceAccessScope::User {
                    user_id,
                    guild_id: None,
                },
                VoiceRemovedReason::Banned,
            ),
            Self::AccountDeleted { user_id } => (
                VoiceAccessScope::User {
                    user_id,
                    guild_id: None,
                },
                VoiceRemovedReason::PermissionLost,
            ),
            Self::GuildRolesChanged { guild_id } => (
                VoiceAccessScope::Guild(guild_id),
                VoiceRemovedReason::PermissionLost,
            ),
            Self::ChannelPermissionsChanged { channel_id, .. } => (
                VoiceAccessScope::Channel(channel_id),
                VoiceRemovedReason::PermissionLost,
            ),
            Self::ChannelDeleted { channel_id, .. } => (
                VoiceAccessScope::Channel(channel_id),
                VoiceRemovedReason::ChannelDeleted,
            ),
            Self::GuildDeleted { .. } => (
                VoiceAccessScope::DeletedChannels,
                VoiceRemovedReason::ChannelDeleted,
            ),
            Self::DmLeft {
                user_id,
                channel_id,
            } => (
                VoiceAccessScope::UserInChannel {
                    user_id,
                    channel_id,
                },
                VoiceRemovedReason::PermissionLost,
            ),
            Self::MemberJoined { .. } | Self::AdminAccessChanged { .. } => return None,
        })
    }

    /// Advance this change's epoch (no proof, registry cell kept): what a
    /// write of unknown outcome does.
    fn bump_epoch(self) {
        match self.scope() {
            Some((NoticeScope::Guild, id)) => registry().bump_guild(id),
            Some((NoticeScope::User, id)) => registry().bump_user(id),
            None => {}
        }
    }

    /// Bump this change's epoch for a write that is known to have committed
    /// and return the proof for [`access_changed`].
    ///
    /// Private on purpose: only [`bump_after`] and [`bump_after_if`] call it,
    /// in the poll in which their write completed, so a [`Bumped`] cannot be
    /// made anywhere else.
    fn bump(self) -> Bumped {
        self.bump_epoch();
        match self {
            Self::GuildDeleted { guild_id } => registry().forget_guild(guild_id),
            Self::AccountDeleted { user_id } => registry().forget_user(user_id),
            _ => {}
        }
        Bumped(self)
    }
}

/// Proof that a change's epoch was bumped in the poll in which its write
/// completed; the only way to call [`access_changed`]. It cannot be built
/// outside this module.
#[derive(Debug)]
#[must_use = "pass it to access_changed so voice and other nodes follow"]
pub struct Bumped(AccessChange);

impl Bumped {
    /// The change.
    pub const fn change(&self) -> AccessChange {
        self.0
    }
}

/// A write in flight whose outcome is not known yet.
///
/// If it is dropped in that state — the request was cancelled while its
/// commit was on the wire, or the commit's answer was an error (which a lost
/// connection also produces after the database applied it) — the epoch is
/// bumped anyway: a bump without a change costs one revalidation, a change
/// without a bump would leave open connections on old authorization.
struct PendingBump(Option<AccessChange>);

impl Drop for PendingBump {
    fn drop(&mut self) {
        if let Some(change) = self.0.take() {
            change.bump_epoch();
        }
    }
}

/// Run the committing `write`, then bump `change`'s epoch in the same poll.
///
/// There is no `.await` between the commit and the bump, and no cancellation
/// point either: if this future is dropped while the write is in flight, or
/// the write fails, the epoch is bumped as well (see [`PendingBump`]). Every
/// access trigger writes through this (or [`bump_after_if`]), so no
/// post-commit work can delay the fence. `write` must be the committing
/// future itself (one statement on the pool, or `tx.commit()`), nothing that
/// awaits anything after its commit.
pub async fn bump_after<T, E>(
    change: AccessChange,
    write: impl std::future::Future<Output = Result<T, E>>,
) -> Result<(T, Bumped), E> {
    let mut pending = PendingBump(Some(change));
    let value = write.await?;
    pending.0 = None;
    let bumped = change.bump();
    crate::ws::test_hooks::after_access_bump(change).await;
    Ok((value, bumped))
}

/// [`bump_after`] for writes whose result says whether they changed anything
/// (e.g. "a row was removed").
///
/// `change` is what the write changes when it applies (`None`: this request
/// cannot change access); `applies` reads that off the result. A write of
/// unknown outcome (cancelled, failed) bumps like in [`bump_after`].
pub async fn bump_after_if<T, E>(
    change: Option<AccessChange>,
    write: impl std::future::Future<Output = Result<T, E>>,
    applies: impl FnOnce(&T) -> bool,
) -> Result<(T, Option<Bumped>), E> {
    let mut pending = PendingBump(change);
    let value = write.await?;
    let change = pending.0.take().filter(|_| applies(&value));
    let bumped = change.map(AccessChange::bump);
    if let Some(change) = change {
        crate::ws::test_hooks::after_access_bump(change).await;
    }
    Ok((value, bumped))
}

/// Apply a bumped change to the rest of the system.
///
/// That is the cross-node notice (detached, so a Valkey outage never delays the request) and voice
/// participants who lost access (removed before this returns; rooms are in
/// this process). It never fails the request. Call it right after the bump,
/// before any other post-commit work.
#[tracing::instrument(skip(state))]
pub async fn access_changed(state: &AppState, bumped: Bumped) {
    let change = bumped.0;
    if let Some((scope, id)) = change.scope() {
        publish_notice_detached(&state.redis, scope, id);
    }

    if let Some((scope, reason)) = change.voice() {
        crate::voice::revoke::enforce_voice_access(
            &state.sfu,
            &state.db,
            state.screen_share_limiter.as_ref(),
            scope,
            reason,
        )
        .await;
    }
}

fn publish_notice_detached(redis: &Client, scope: NoticeScope, id: Uuid) {
    let notice = AccessNotice {
        scope,
        id,
        origin: registry().origin,
    };
    let Ok(payload) = serde_json::to_string(&notice) else {
        return;
    };
    let redis = redis.clone();
    tokio::spawn(async move {
        let publish = redis.publish::<(), _, _>(NOTICE_CHANNEL, payload);
        match tokio::time::timeout(NOTICE_TIMEOUT, publish).await {
            Ok(Ok(())) => {}
            Ok(Err(e)) => debug!(error = %e, "Access notice not published"),
            Err(_) => debug!("Access notice not published (timeout)"),
        }
    });
}

// ---------------------------------------------------------------------------
// Per-socket state
// ---------------------------------------------------------------------------

/// What a channel's access depends on besides the user.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChannelScope {
    /// A server channel.
    Guild(Uuid),
    /// A DM or group DM (participation; user epoch only).
    Direct,
}

/// A revalidation could not be completed (database error, timeout, or still
/// in backoff after such an error).
///
/// As a client request's error it means that the socket's access checks
/// have kept failing for [`FAILURE_CLOSE_AFTER`]: the socket ends (1011).
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("access could not be checked for too long")]
pub struct AccessCheckFailed;

/// What a socket knows about one scope: the user (membership, roles, DMs),
/// one server (role permissions, overrides, channels), or its voice room.
///
/// | State | Events of the scope | Leaves it when |
/// |---|---|---|
/// | `Checked`, epoch unchanged (**current**) | decided from what was computed | the epoch moves, or the scope is invalidated |
/// | `Checked` with a moved epoch, or `Unchecked` (**stale**) | wait for a revalidation | it succeeds (current) or fails (failing) |
/// | `Failing { dropped }` | dropped, and `dropped` is set | a revalidation succeeds (current); if `dropped`, the socket resynchronises |
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ScopeState {
    /// Decisions were computed at the recorded epoch.
    Checked,
    /// Must be recomputed whatever the epoch says (backstop, a channel
    /// recorded without a decision).
    Unchecked,
    /// A revalidation attempt for this scope failed, or could not be made,
    /// and none has succeeded since. `dropped`: an event of the scope was
    /// dropped meanwhile.
    Failing { dropped: bool },
}

/// How a scope stands right now (see [`ScopeState`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Phase {
    Current,
    Stale,
    Failing,
}

impl ScopeState {
    /// The phase, given whether the recorded epoch(s) still hold.
    const fn phase(self, epoch_unchanged: bool) -> Phase {
        match self {
            Self::Failing { .. } => Phase::Failing,
            Self::Checked if epoch_unchanged => Phase::Current,
            Self::Checked | Self::Unchecked => Phase::Stale,
        }
    }

    /// Mark for recomputation; a failing scope stays failing.
    const fn invalidate(&mut self) {
        if !matches!(self, Self::Failing { .. }) {
            *self = Self::Unchecked;
        }
    }

    /// A revalidation attempt failed (or could not be made).
    const fn fail(&mut self) {
        if !matches!(self, Self::Failing { .. }) {
            *self = Self::Failing { dropped: false };
        }
    }

    /// An event of this scope is dropped without a decision.
    const fn drop_event(&mut self) {
        *self = Self::Failing { dropped: true };
    }

    /// A revalidation succeeded. Returns whether an event was dropped while
    /// it was failing. `fresh`: nothing invalidated the scope since the
    /// revalidation was planned.
    const fn settle(&mut self, fresh: bool) -> bool {
        let dropped = matches!(self, Self::Failing { dropped: true });
        *self = if fresh {
            Self::Checked
        } else {
            Self::Unchecked
        };
        dropped
    }
}

struct UserScope {
    seen: u64,
    state: ScopeState,
}

struct GuildScope {
    epoch: Arc<AtomicU64>,
    seen: u64,
    state: ScopeState,
    /// Whether the user is a member of this server, as read at `seen`
    /// (`None`: not read yet under this scope; only its feed needs it).
    member: Option<bool>,
}

impl GuildScope {
    fn phase(&self) -> Phase {
        self.state
            .phase(self.seen == self.epoch.load(Ordering::SeqCst))
    }
}

/// The epochs a voice peer was let into its room at (the join check).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VoiceAdmission {
    /// The server of the room's channel (`None` for a DM call).
    pub guild_id: Option<Uuid>,
    /// Read before the join's permission check.
    pub epochs: EpochSnapshot,
}

/// The voice peer a socket owns, as the SFU holds it now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OwnedVoicePeer {
    pub channel_id: Uuid,
    pub session_id: Uuid,
    /// `None`: the peer was not created by the join check; it gets no room
    /// content.
    pub admission: Option<VoiceAdmission>,
}

/// The socket's authorization for the voice room of the peer it owns.
struct VoiceScope {
    session_id: Uuid,
    channel_id: Uuid,
    guild: Option<Arc<AtomicU64>>,
    seen_user: u64,
    seen_guild: u64,
    state: ScopeState,
    /// Whether the last check found voice access.
    allowed: bool,
    /// The socket loop was told to remove the peer (once per finding).
    denial_taken: bool,
}

impl VoiceScope {
    /// The scope of a peer the join check let in at `admission`.
    fn new(peer: &OwnedVoicePeer, admission: VoiceAdmission) -> Self {
        Self {
            session_id: peer.session_id,
            channel_id: peer.channel_id,
            guild: admission.guild_id.map(|g| registry().guild_epoch(g)),
            seen_user: admission.epochs.user,
            seen_guild: admission.epochs.guild.unwrap_or(0),
            state: ScopeState::Checked,
            allowed: true,
            denial_taken: false,
        }
    }

    fn guild_now(&self) -> u64 {
        self.guild
            .as_ref()
            .map_or(0, |cell| cell.load(Ordering::SeqCst))
    }
}

/// The admin event stream on this socket: whether the client asked for it
/// (`requested`, from `admin_subscribe` until `admin_unsubscribe` or the
/// socket's end, like a requested channel), and whether its session is an
/// elevated system admin's, as read at user epoch `seen`. A "not elevated"
/// answer only clears `allowed`; the next elevation bumps the epoch and
/// restores delivery.
///
/// `allowed`, `seen` and `decided` are written by a revalidation only
/// ([`SocketAccess::apply`]); everything else only invalidates the scope or
/// changes whether it is requested (see the module docs, The admin stream).
/// The decision is kept while the stream is not requested: nothing is read
/// or written for it then, and a new request starts from it.
///
/// `reply` is not a decision: it is what this socket's last
/// `admin_subscribe` was answered, written by
/// [`SocketAccess::subscribe_admin`] only, and the one thing besides the
/// user epoch and a read of its own that the next answer depends on.
struct AdminScope {
    requested: bool,
    seen: u64,
    state: ScopeState,
    allowed: bool,
    /// [`Inner::admin_invalidations`] as of the plan of the read that wrote
    /// `allowed` (0: no read yet).
    decided: u64,
    /// When an `admin_subscribe` last asked for this scope to be read
    /// ([`ADMIN_SUBSCRIBE_COOLDOWN`]).
    read_asked: Option<Instant>,
    /// The last `admin_subscribe`'s answer, and the user epoch it holds
    /// for.
    reply: Option<(u64, AdminSubscription)>,
}

/// What an `admin_subscribe` needs ([`SocketAccess::request_admin`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AdminRequest {
    /// Answered as this socket was answered before, at the same user
    /// epoch: a refusal, or within the cooldown an earlier answer.
    Answered(AdminSubscription),
    /// A read: an answer planned at or after this invalidation count
    /// answers it.
    Read(u64),
    /// Not decided, and no read now (cooldown, or the socket is in
    /// backoff).
    Later,
}

/// Answer to an `admin_subscribe` ([`SocketAccess::subscribe_admin`]). The
/// stream is requested in every case, until `admin_unsubscribe`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdminSubscription {
    /// The session is an elevated system admin's: admin events follow.
    Allowed,
    /// It is not; a later elevation restores delivery.
    Refused,
    /// Not decided yet: the read failed or was not made (the socket is in
    /// backoff after a failed one, or in the cooldown without an earlier
    /// answer at this user epoch, or the socket's revalidation or the pool
    /// for these reads stayed busy past the deadline), or the epoch kept
    /// moving. Given on purpose in those cases: it does not take what a
    /// frame's read settled meanwhile. Frames decide on their own reads.
    Undecided,
    /// The read it asked for failed, and the socket's access checks have
    /// been failing for [`FAILURE_CLOSE_AFTER`]: the socket ends, as on its
    /// own revalidation.
    Failed,
}

struct Failure {
    since: Instant,
    attempts: u32,
    retry_at: Instant,
}

struct Inner {
    requested: HashMap<Uuid, ChannelScope>,
    allowed: HashSet<Uuid>,
    user: UserScope,
    guilds: HashMap<Uuid, GuildScope>,
    /// Servers whose feed events this socket has been sent: their scope is
    /// kept although no channel of theirs is requested (bounded by
    /// [`MAX_FEED_GUILDS`]).
    feed_guilds: HashSet<Uuid>,
    voice: Option<VoiceScope>,
    admin: Option<AdminScope>,
    /// Requested DMs and group DMs (the rest of `requested` are server
    /// channels); kept by [`Inner::request`] and [`Inner::forget`].
    requested_direct: usize,
    /// Backoff after failed revalidations (the database is shared, so one
    /// for the socket). It outlasts the scopes: only a successful read or
    /// its age ends it (see the module docs, Failure).
    failure: Option<Failure>,
    /// A scope recovered after dropping events: the socket must make the
    /// client resynchronise (close 1011, see `ws::handlers`).
    resync: bool,
    /// Counts explicit invalidations; a revalidation planned before one
    /// must not mark its scopes checked.
    invalidations: u64,
    /// Counts invalidations of the admin scope alone (never reset, also not
    /// by `admin_unsubscribe`): a read planned before one does not decide
    /// the admin stream.
    admin_invalidations: u64,
}

/// What an event's delivery depends on.
#[derive(Clone, Copy)]
enum Target {
    /// A channel's events; a server's own feed depends on the same scopes
    /// as its channels (`ChannelScope::Guild`).
    Channel(ChannelScope),
    Voice,
    /// The admin event stream.
    Admin,
}

impl Inner {
    fn requested_guild(&self) -> usize {
        self.requested.len() - self.requested_direct
    }

    /// Whether `channel_id` (of `scope`) can be in the requested set: it is
    /// already, or its kind is below its cap.
    fn has_room(&self, channel_id: Uuid, scope: ChannelScope) -> bool {
        self.requested.contains_key(&channel_id)
            || match scope {
                ChannelScope::Direct => self.requested_direct < MAX_REQUESTED_DIRECT,
                ChannelScope::Guild(_) => self.requested_guild() < MAX_REQUESTED_CHANNELS,
            }
    }

    /// Put `channel_id` into the requested set (the only place that does).
    fn request(&mut self, channel_id: Uuid, scope: ChannelScope) {
        let before = self.requested.insert(channel_id, scope);
        let was_direct = before == Some(ChannelScope::Direct);
        let is_direct = scope == ChannelScope::Direct;
        if is_direct && !was_direct {
            self.requested_direct += 1;
        } else if was_direct && !is_direct {
            self.requested_direct -= 1;
        }
    }

    /// Take `channel_id` out of the requested set (the only place that does).
    fn forget(&mut self, channel_id: Uuid) {
        if self.requested.remove(&channel_id) == Some(ChannelScope::Direct) {
            self.requested_direct -= 1;
        }
    }
}

/// What one revalidation read found.
struct ReadResult {
    viewable: HashSet<Uuid>,
    existing: HashSet<Uuid>,
    direct_ok: HashSet<Uuid>,
    direct_existing: HashSet<Uuid>,
    /// The user's servers; read whenever a server scope is revalidated.
    member_of: Option<HashSet<Uuid>>,
    voice_ok: Option<bool>,
    admin_ok: Option<bool>,
}

/// One step of a scripted revalidation read (tests): optionally bump an
/// epoch while the read is "in flight", then fail or answer.
#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ScriptStep {
    bump: bool,
    /// `None`: the read fails; `Some(allowed)`: everything asked is allowed
    /// (or nothing is).
    outcome: Option<bool>,
}

/// Replaces the database in [`SocketAccess::read`] (tests).
#[cfg(test)]
struct Script {
    steps: std::collections::VecDeque<ScriptStep>,
    /// The answer once the steps are used up.
    then: bool,
    /// The server whose epoch a step's bump moves (`None`: the user's).
    bump_guild: Option<Uuid>,
    /// The last read that was answered.
    last_served: Option<ScriptStep>,
    /// What that read said about the admin stream: `None` if it did not
    /// cover it, `Some(None)` if it failed.
    last_admin: Option<Option<bool>>,
    /// Set by a test that explores interleavings (see [`Seams`]).
    seams: Option<Arc<Seams>>,
}

/// Lets a test run the tasks of one socket step by step: every scripted
/// read waits (its answer already fixed) until the test lets it return, and
/// every [`SocketAccess::seam`] hands control back to the test once.
#[cfg(test)]
struct Seams {
    /// Reads that have their answer and wait to return.
    parked: std::sync::atomic::AtomicUsize,
    /// The task stopped at a seam (not at a read, not for the gate).
    yielded: std::sync::atomic::AtomicBool,
    /// One permit lets one parked read return.
    release: tokio::sync::Semaphore,
}

/// One socket's requested and allowed channels, server memberships and voice
/// authorization, and the epochs they were computed at (see the module
/// docs).
pub struct SocketAccess {
    user_id: Uuid,
    db: PgPool,
    user_epoch: Arc<AtomicU64>,
    inner: Mutex<Inner>,
    /// Serialises revalidations (the only lock held across `.await`).
    refresh_gate: tokio::sync::Mutex<()>,
    /// The pool this socket's `admin_subscribe` reads run in.
    subscribe_permits: Arc<tokio::sync::Semaphore>,
    /// Wakes the socket loop: a revalidation failed (retry), a scope needs
    /// one, the client must resynchronise, or the voice peer lost access.
    attention: Notify,
    /// The session the socket authenticated with (an admin elevation belongs
    /// to one session).
    session_id: Option<Uuid>,
    #[cfg(test)]
    script: Mutex<Option<Script>>,
}

struct VoicePlan {
    session_id: Uuid,
    channel_id: Uuid,
    user_now: u64,
    guild_now: u64,
}

struct Plan {
    invalidations: u64,
    full: bool,
    user_now: u64,
    guild_targets: Vec<(Uuid, u64)>,
    guild_channels: Vec<(Uuid, Uuid)>,
    direct: Vec<Uuid>,
    voice: Option<VoicePlan>,
    /// The admin scope is read: [`Inner::admin_invalidations`] as of this
    /// plan.
    admin: Option<u64>,
}

enum Planned {
    /// Every scope is current.
    Nothing,
    /// Still in backoff after a failure: no attempt now.
    Backoff,
    Run(Plan),
}

/// Outcome of [`SocketAccess::commit_subscribe`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubscribeCommit {
    /// The decision was recorded.
    Recorded,
    /// An epoch moved since the check: check again.
    Moved,
    /// The socket already has as many channels of this kind as it may
    /// ([`MAX_REQUESTED_CHANNELS`], [`MAX_REQUESTED_DIRECT`]).
    Full,
}

impl SocketAccess {
    /// A socket of `user_id` with nothing requested and no session (admin
    /// events are then never allowed).
    pub fn new(user_id: Uuid, db: PgPool) -> Arc<Self> {
        Self::for_session(user_id, None, db)
    }

    /// A socket of `user_id` that authenticated with session `session_id`.
    pub fn for_session(user_id: Uuid, session_id: Option<Uuid>, db: PgPool) -> Arc<Self> {
        let user_epoch = registry().user_epoch(user_id);
        let now = user_epoch.load(Ordering::SeqCst);
        Arc::new(Self {
            user_id,
            db,
            user_epoch,
            inner: Mutex::new(Inner {
                requested: HashMap::new(),
                allowed: HashSet::new(),
                user: UserScope {
                    seen: now,
                    state: ScopeState::Checked,
                },
                guilds: HashMap::new(),
                feed_guilds: HashSet::new(),
                voice: None,
                admin: None,
                requested_direct: 0,
                failure: None,
                resync: false,
                invalidations: 0,
                admin_invalidations: 0,
            }),
            refresh_gate: tokio::sync::Mutex::new(()),
            subscribe_permits: admin_subscribe_pool(),
            attention: Notify::new(),
            session_id,
            #[cfg(test)]
            script: Mutex::new(None),
        })
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn user_phase(&self, inner: &Inner) -> Phase {
        inner
            .user
            .state
            .phase(inner.user.seen == self.user_epoch.load(Ordering::SeqCst))
    }

    fn voice_phase(&self, voice: &VoiceScope) -> Phase {
        voice.state.phase(
            voice.seen_user == self.user_epoch.load(Ordering::SeqCst)
                && voice.seen_guild == voice.guild_now(),
        )
    }

    /// The phase of everything `target` depends on (the worst one).
    fn phase(&self, inner: &Inner, target: Target) -> Phase {
        match target {
            Target::Channel(ChannelScope::Direct) => self.user_phase(inner),
            Target::Channel(ChannelScope::Guild(g)) => self.user_phase(inner).max(
                inner
                    .guilds
                    .get(&g)
                    // No scope yet: never computed.
                    .map_or(Phase::Stale, GuildScope::phase),
            ),
            Target::Voice => inner
                .voice
                .as_ref()
                .map_or(Phase::Stale, |v| self.voice_phase(v)),
            Target::Admin => inner
                .admin
                .as_ref()
                .map_or(Phase::Stale, |a| self.admin_phase(a)),
        }
    }

    fn admin_phase(&self, admin: &AdminScope) -> Phase {
        admin
            .state
            .phase(admin.seen == self.user_epoch.load(Ordering::SeqCst))
    }

    /// An event of `target` is dropped without a decision: every scope it
    /// depends on that is not current becomes `Failing { dropped: true }`,
    /// so its recovery makes the client resynchronise.
    fn record_drop(&self, inner: &mut Inner, target: Target) {
        let user_current = self.user_phase(inner) == Phase::Current;
        match target {
            Target::Channel(ChannelScope::Direct) => inner.user.state.drop_event(),
            Target::Channel(ChannelScope::Guild(g)) => {
                if !user_current {
                    inner.user.state.drop_event();
                }
                match inner.guilds.get_mut(&g) {
                    Some(gs) if gs.phase() != Phase::Current => gs.state.drop_event(),
                    Some(_) => {}
                    None => inner.user.state.drop_event(),
                }
            }
            Target::Voice => {
                if let Some(voice) = inner.voice.as_mut() {
                    voice.state.drop_event();
                }
            }
            Target::Admin => {
                if let Some(admin) = inner.admin.as_mut() {
                    admin.state.drop_event();
                }
            }
        }
        self.attention.notify_one();
    }

    fn in_backoff(inner: &Inner) -> bool {
        inner
            .failure
            .as_ref()
            .is_some_and(|f| Instant::now() < f.retry_at)
    }

    /// Whether the client asked for `channel_id` (and has not unsubscribed).
    pub fn is_requested(&self, channel_id: Uuid) -> bool {
        self.lock().requested.contains_key(&channel_id)
    }

    /// Whether `channel_id` is in the allowed set as last computed. Not the
    /// delivery decision: that is [`Self::admit`], which also asks the
    /// scope's state.
    pub fn is_allowed(&self, channel_id: Uuid) -> bool {
        self.lock().allowed.contains(&channel_id)
    }

    #[cfg(test)]
    fn requested_len(&self) -> usize {
        self.lock().requested.len()
    }

    /// Cheap pre-filter for the forwarder: `false` only on a decision (not
    /// requested, or current and not allowed); `true` if the event must be
    /// queued for the write-time decision — also while the scope is stale or
    /// failing, so that every drop without a decision happens in one place
    /// ([`Self::admit`]) and is recorded.
    pub fn may_forward(&self, channel_id: Uuid) -> bool {
        let inner = self.lock();
        let Some(&scope) = inner.requested.get(&channel_id) else {
            return false;
        };
        self.phase(&inner, Target::Channel(scope)) != Phase::Current
            || inner.allowed.contains(&channel_id)
    }

    /// Delivery decision for an event of `channel_id`, taken when its frame
    /// is written: revalidates first if the scope is stale. Fails closed, and
    /// a drop that is not a decision is recorded (see [`ScopeState`]).
    pub async fn admit(&self, channel_id: Uuid) -> bool {
        self.decide_channel(channel_id, true).await
    }

    /// Whether the client may act in `channel_id` now (typing): the same
    /// decision as [`Self::admit`], but refusing it drops nothing the client
    /// should have received.
    pub async fn may_act(&self, channel_id: Uuid) -> bool {
        self.decide_channel(channel_id, false).await
    }

    async fn decide_channel(&self, channel_id: Uuid, outbound: bool) -> bool {
        self.decide(outbound, |inner| {
            let &scope = inner.requested.get(&channel_id)?;
            Some((
                Target::Channel(scope),
                inner.allowed.contains(&channel_id).then_some(true),
            ))
        })
        .await
    }

    /// Delivery decision for an event that is bound to `channel_id` but came
    /// on the user's own feed (a bot's answer to the user's command).
    ///
    /// A channel the client asked for is decided like its own events
    /// ([`Self::admit`]). For any other channel this is a one-off decision
    /// under the same rules — the epochs are read, then access, and the
    /// answer counts only if the epochs are unchanged — and the channel does
    /// not become requested. A decision that cannot be made (database
    /// error, epochs kept moving, the socket in backoff) drops the event and
    /// records it on the user scope, so the recovery resynchronises.
    pub async fn admit_bound(&self, channel_id: Uuid) -> bool {
        if self.is_requested(channel_id) {
            return self.admit(channel_id).await;
        }
        for _ in 0..DECISION_REVALIDATIONS {
            if Self::in_backoff(&self.lock()) {
                break;
            }
            let read = tokio::time::timeout(REFRESH_TIMEOUT, self.read_bound(channel_id)).await;
            match read {
                // The channel is gone: nothing to deliver for it.
                Ok(Ok(None)) => return false,
                Ok(Ok(Some((scope, snap, granted)))) => {
                    if self.snapshot(scope) == snap {
                        return granted;
                    }
                }
                Ok(Err(e)) => {
                    warn!(error = %e, user_id = %self.user_id, "Channel access check failed");
                    break;
                }
                Err(_) => {
                    warn!(user_id = %self.user_id, "Channel access check timed out");
                    break;
                }
            }
        }
        let mut inner = self.lock();
        inner.user.state.drop_event();
        drop(inner);
        self.attention.notify_one();
        false
    }

    /// One access read for [`Self::admit_bound`]: the channel's scope, the
    /// epochs read before the check, and the answer (`None`: no such
    /// channel).
    async fn read_bound(
        &self,
        channel_id: Uuid,
    ) -> Result<Option<(ChannelScope, EpochSnapshot, bool)>, String> {
        if crate::ws::test_hooks::access_read_fails(self.user_id) {
            return Err(sqlx::Error::PoolTimedOut.to_string());
        }
        let Some(channel) = crate::db::find_channel_by_id(&self.db, channel_id)
            .await
            .map_err(|e| e.to_string())?
        else {
            return Ok(None);
        };
        let scope = channel
            .guild_id
            .map_or(ChannelScope::Direct, ChannelScope::Guild);
        let snap = self.snapshot(scope);
        if self.banned().await? {
            return Ok(Some((scope, snap, false)));
        }
        let granted =
            match crate::permissions::require_channel_access(&self.db, self.user_id, channel_id)
                .await
            {
                Ok(_) => true,
                Err(crate::permissions::PermissionError::DatabaseError(e)) => return Err(e),
                Err(_) => false,
            };
        Ok(Some((scope, snap, granted)))
    }

    /// Whether the account is banned now (a banned account keeps its rows —
    /// membership, roles, DMs — but may see nothing).
    async fn banned(&self) -> Result<bool, String> {
        sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM global_bans gb
                            WHERE gb.user_id = $1
                              AND (gb.expires_at IS NULL OR gb.expires_at > NOW()))",
        )
        .bind(self.user_id)
        .fetch_one(&self.db)
        .await
        .map_err(|e| e.to_string())
    }

    /// Delivery decision for an event of the server `guild_id`'s own feed
    /// (`guild:{id}`): the user must be a member, as read at the current
    /// user epoch and the current epoch of that server. The first feed event
    /// of a server on this socket is decided by a read.
    pub async fn admit_guild(&self, guild_id: Uuid) -> bool {
        self.decide(true, |inner| {
            if inner.feed_guilds.len() >= MAX_FEED_GUILDS && !inner.feed_guilds.contains(&guild_id)
            {
                inner.feed_guilds.clear();
            }
            inner.feed_guilds.insert(guild_id);
            let scope = inner.guilds.entry(guild_id).or_insert_with(|| GuildScope {
                epoch: registry().guild_epoch(guild_id),
                seen: 0,
                state: ScopeState::Unchecked,
                member: None,
            });
            if scope.member.is_none() {
                // Membership was not read under this scope yet (it was
                // created for a channel): the next read does.
                scope.state.invalidate();
            }
            Some((
                Target::Channel(ChannelScope::Guild(guild_id)),
                scope.member.filter(|member| *member).map(|_| true),
            ))
        })
        .await
    }

    /// Delivery decision for content of the voice room of `peer`, the peer
    /// this socket owns there (`None`: it owns none, nothing is written).
    /// The socket's authorization for the room must be current: the epochs of
    /// the join check still hold, or it is revalidated now.
    pub async fn admit_voice(&self, peer: Option<&OwnedVoicePeer>) -> bool {
        // No peer, or one that never passed the join check: nothing, and
        // nothing to revalidate.
        let Some(peer) = peer.filter(|p| p.admission.is_some()) else {
            return false;
        };
        self.decide(true, |inner| {
            Self::track_voice_in(inner, Some(peer));
            let voice = inner.voice.as_ref()?;
            Some((Target::Voice, voice.allowed.then_some(true)))
        })
        .await
    }

    /// Delivery decision for an admin event, taken after its frame was taken
    /// from the queue: the socket requested the admin stream and its session
    /// is an elevated system admin's.
    ///
    /// A "yes" is never reused (an elevation also ends without any epoch
    /// moving), so a frame that is not refused outright is decided by a read
    /// planned after this call began: one elevation read per admin frame
    /// for an allowed socket. A current "no" is cached per user epoch and
    /// costs no read. See the module docs (The admin stream).
    pub async fn admit_admin(&self) -> bool {
        if !self.admin_frame_taken() {
            return false;
        }
        #[cfg(test)]
        self.seam().await;
        self.decide_admin().await
    }

    /// The one critical section that settles what an admin frame needs:
    /// `false` = refused outright (the stream is not requested, or the scope
    /// is a current "no"); `true` = a read of its own, and the scope was
    /// invalidated here, under the same lock, so no answer planned earlier
    /// can make it current again.
    fn admin_frame_taken(&self) -> bool {
        let mut inner = self.lock();
        let Some(admin) = inner.admin.as_ref().filter(|a| a.requested) else {
            return false;
        };
        if !admin.allowed && self.admin_phase(admin) == Phase::Current {
            return false;
        }
        Self::invalidate_admin(&mut inner);
        true
    }

    /// The decision loop for an admin frame whose scope
    /// [`Self::admin_frame_taken`] invalidated.
    async fn decide_admin(&self) -> bool {
        self.decide(true, |inner| {
            let admin = inner.admin.as_ref().filter(|a| a.requested)?;
            Some((Target::Admin, admin.allowed.then_some(true)))
        })
        .await
    }

    /// Invalidate the admin scope: an answer whose read was planned before
    /// this no longer counts for it (a failing scope stays failing). Returns
    /// the scope's invalidation count from here on.
    const fn invalidate_admin(inner: &mut Inner) -> u64 {
        inner.admin_invalidations += 1;
        if let Some(admin) = inner.admin.as_mut() {
            admin.state.invalidate();
        }
        inner.admin_invalidations
    }

    /// Pre-filter for the forwarder, like [`Self::may_forward`]: `false`
    /// only on a decision (not subscribed, or current and not allowed).
    pub fn may_forward_admin(&self) -> bool {
        let inner = self.lock();
        inner
            .admin
            .as_ref()
            .is_some_and(|a| a.requested && (self.admin_phase(a) != Phase::Current || a.allowed))
    }

    /// The decision loop shared by every kind of frame.
    ///
    /// `look` names what the frame depends on and, if that is current, the
    /// answer: `None` = refused outright (not requested, no peer, not
    /// subscribed), `Some((target, Some(true)))` = allowed when `target` is
    /// current, `Some((target, None))` = not allowed as computed.
    ///
    /// It leaves in exactly one of three ways:
    /// 1. **admitted** — `target` is current and allows it;
    /// 2. **denied** — refused outright, or `target` is current and does not allow it;
    /// 3. **undecided** — `target` is not current and cannot be made so now (a revalidation failed,
    ///    the socket is in backoff, or epochs moved under [`DECISION_REVALIDATIONS`] of them): the
    ///    one place where a frame is dropped without a decision, and for an outbound frame the drop
    ///    is recorded on its scope ([`Self::record_drop`]).
    ///
    /// The answer is looked at again after every revalidation, the last one
    /// included.
    async fn decide(
        &self,
        outbound: bool,
        mut look: impl FnMut(&mut Inner) -> Option<(Target, Option<bool>)>,
    ) -> bool {
        let mut revalidations = 0;
        loop {
            {
                let mut inner = self.lock();
                let Some((target, answer)) = look(&mut inner) else {
                    return false;
                };
                if self.phase(&inner, target) == Phase::Current {
                    return answer.unwrap_or(false);
                }
                if Self::in_backoff(&inner) || revalidations == DECISION_REVALIDATIONS {
                    if outbound {
                        self.record_drop(&mut inner, target);
                    }
                    return false;
                }
            }
            revalidations += 1;
            // A failure leaves the scope failing and the socket in backoff:
            // the next pass ends as undecided.
            let _ = self.refresh().await;
            #[cfg(test)]
            self.seam().await;
        }
    }

    /// Whether the client must resynchronise (an event was dropped while a
    /// scope was failing, and the scope has recovered since). Clears it.
    pub fn take_resync(&self) -> bool {
        std::mem::take(&mut self.lock().resync)
    }

    /// The epochs a `subscribe` of a channel in `scope` checks against.
    pub fn snapshot(&self, scope: ChannelScope) -> EpochSnapshot {
        EpochSnapshot {
            user: self.user_epoch.load(Ordering::SeqCst),
            guild: match scope {
                ChannelScope::Guild(g) => Some(registry().guild_epoch(g).load(Ordering::SeqCst)),
                ChannelScope::Direct => None,
            },
        }
    }

    /// Whether a `subscribe` of `channel_id` can be recorded at all, asked
    /// before any database work (its kind is not known yet: refused only if
    /// neither kind has room).
    pub fn has_room_for(&self, channel_id: Uuid) -> bool {
        let inner = self.lock();
        inner.has_room(channel_id, ChannelScope::Direct)
            || inner.requested_guild() < MAX_REQUESTED_CHANNELS
    }

    /// Record a `subscribe` decision taken at `snap`.
    ///
    /// Changes nothing if an epoch moved since (the caller checks again) or
    /// the socket is full. A granted channel becomes requested and allowed;
    /// a refused one leaves allowed but stays requested, so it comes back
    /// with access.
    pub fn commit_subscribe(
        &self,
        channel_id: Uuid,
        scope: ChannelScope,
        snap: EpochSnapshot,
        granted: bool,
    ) -> SubscribeCommit {
        let mut inner = self.lock();
        if self.snapshot(scope) != snap {
            return SubscribeCommit::Moved;
        }
        if !inner.has_room(channel_id, scope) {
            return SubscribeCommit::Full;
        }
        if let (ChannelScope::Guild(g), Some(seen)) = (scope, snap.guild) {
            inner.guilds.entry(g).or_insert_with(|| GuildScope {
                epoch: registry().guild_epoch(g),
                seen,
                state: ScopeState::Checked,
                member: None,
            });
        }
        // Recorded either way: the client keeps the id and will not ask
        // again, so a refused channel must come back when access does.
        inner.request(channel_id, scope);
        if granted {
            inner.allowed.insert(channel_id);
        } else {
            inner.allowed.remove(&channel_id);
        }
        SubscribeCommit::Recorded
    }

    /// Record `channel_id` as requested without a decision (the check failed
    /// with a database error, or a `dm_created` arrived): not allowed, and
    /// its scope is invalidated so the next delivery or wake revalidates it.
    /// Returns `false` (nothing recorded) if the socket is full.
    pub fn record_unchecked(&self, channel_id: Uuid, scope: ChannelScope) -> bool {
        let mut inner = self.lock();
        if !inner.has_room(channel_id, scope) {
            return false;
        }
        inner.request(channel_id, scope);
        inner.allowed.remove(&channel_id);
        inner.invalidations += 1;
        match scope {
            ChannelScope::Direct => inner.user.state.invalidate(),
            ChannelScope::Guild(g) => inner
                .guilds
                .entry(g)
                .or_insert_with(|| GuildScope {
                    epoch: registry().guild_epoch(g),
                    seen: 0,
                    state: ScopeState::Unchecked,
                    member: None,
                })
                .state
                .invalidate(),
        }
        drop(inner);
        self.attention.notify_one();
        true
    }

    /// The client unsubscribed: drop `channel_id` from both sets.
    pub fn unsubscribe(&self, channel_id: Uuid) {
        let mut inner = self.lock();
        inner.forget(channel_id);
        inner.allowed.remove(&channel_id);
    }

    /// A DM the user was just added to (`dm_created`): requested, and
    /// allowed only after a revalidation confirms participation (the user
    /// may have left again before this socket processed the event).
    /// `false`: the socket holds [`MAX_REQUESTED_DIRECT`] DMs already and
    /// cannot follow this one — the caller ends the socket so the client
    /// resynchronises, it must not go on without the DM's messages.
    pub fn insert_direct(&self, channel_id: Uuid) -> bool {
        self.record_unchecked(channel_id, ChannelScope::Direct)
    }

    fn track_voice_in(inner: &mut Inner, peer: Option<&OwnedVoicePeer>) {
        match peer.and_then(|p| p.admission.map(|a| (p, a))) {
            None => inner.voice = None,
            Some((peer, admission)) => {
                if inner
                    .voice
                    .as_ref()
                    .is_none_or(|v| v.session_id != peer.session_id)
                {
                    inner.voice = Some(VoiceScope::new(peer, admission));
                }
            }
        }
    }

    /// Align the voice scope with the peer this socket owns now (`None`: it
    /// owns none). A new peer starts from its join check's epochs.
    pub fn track_voice(&self, peer: Option<&OwnedVoicePeer>) {
        Self::track_voice_in(&mut self.lock(), peer);
    }

    /// The voice peer `(channel, session)` whose access a revalidation found
    /// gone, once per finding: the socket removes it (`voice_removed`).
    pub fn take_voice_denied(&self) -> Option<(Uuid, Uuid)> {
        let mut inner = self.lock();
        let phase = inner.voice.as_ref().map(|v| self.voice_phase(v));
        let voice = inner.voice.as_mut()?;
        if phase != Some(Phase::Current) || voice.allowed || voice.denial_taken {
            return None;
        }
        voice.denial_taken = true;
        Some((voice.channel_id, voice.session_id))
    }

    /// Whether the socket loop has work: a scope is not current (or a full
    /// revalidation was requested), or the voice peer is to be removed.
    pub fn is_stale(&self) -> bool {
        let inner = self.lock();
        self.needs_revalidation(&inner)
            || inner
                .voice
                .as_ref()
                .is_some_and(|v| !v.allowed && !v.denial_taken)
    }

    /// Whether any scope is not current (stale or failing): what a
    /// revalidation would read.
    fn needs_revalidation(&self, inner: &Inner) -> bool {
        self.user_phase(inner) != Phase::Current
            || inner.guilds.values().any(|gs| gs.phase() != Phase::Current)
            || inner
                .voice
                .as_ref()
                .is_some_and(|v| self.voice_phase(v) != Phase::Current)
            || inner
                .admin
                .as_ref()
                .is_some_and(|a| a.requested && self.admin_phase(a) != Phase::Current)
    }

    /// Make the next [`Self::refresh`] re-check everything (backstop): every
    /// scope is invalidated, so events wait for that revalidation.
    pub fn force_full(&self) {
        let mut inner = self.lock();
        inner.invalidations += 1;
        inner.user.state.invalidate();
        for gs in inner.guilds.values_mut() {
            gs.state.invalidate();
        }
        if let Some(voice) = inner.voice.as_mut() {
            voice.state.invalidate();
        }
        Self::invalidate_admin(&mut inner);
    }

    /// When the socket should try again after a failed revalidation: `None`
    /// unless a scope is waiting for it. The backoff itself outlasts the
    /// scopes; a socket with nothing to revalidate just has nothing to wake
    /// for.
    pub fn retry_at(&self) -> Option<Instant> {
        self.waiting_failure(|failure| failure.retry_at)
    }

    /// How long revalidations have been failing, if a scope is still
    /// waiting for one to succeed.
    pub fn failing_for(&self) -> Option<Duration> {
        self.waiting_failure(|failure| failure.since.elapsed())
    }

    /// Something of the backoff, if a scope is not current: only then is
    /// the socket waiting for a retry.
    fn waiting_failure<T>(&self, of: impl FnOnce(&Failure) -> T) -> Option<T> {
        let inner = self.lock();
        inner
            .failure
            .as_ref()
            .filter(|_| self.needs_revalidation(&inner))
            .map(of)
    }

    /// Resolves when the socket loop should look at this socket's access
    /// (see the `attention` field).
    pub async fn attention(&self) {
        self.attention.notified().await;
    }

    /// Whether this socket's session is an elevated system admin's right
    /// now, read from the committed rows (not the cache: a de-elevation
    /// bumps the user's epoch before it updates the cache). Part of a
    /// revalidation's read ([`Self::read`]), nowhere else.
    async fn read_admin(&self) -> Result<bool, String> {
        let Some(session_id) = self.session_id else {
            return Ok(false);
        };
        // A live full session of this user (a probation session never
        // subscribes, even with an old elevation).
        let live = crate::auth::session_check::load_live(&self.db, session_id)
            .await
            .map_err(|e| e.to_string())?;
        let full = live.as_ref().is_some_and(|s| {
            s.user.id == self.user_id
                && crate::auth::SessionMeta::from(s).assurance
                    == crate::auth::assurance::Assurance::Full
        });
        if !full {
            return Ok(false);
        }
        let admin = crate::permissions::queries::get_system_admin(&self.db, self.user_id)
            .await
            .map_err(|e| e.to_string())?
            .is_some();
        if !admin {
            return Ok(false);
        }
        crate::admin::queries::has_active_elevated_session(&self.db, self.user_id, session_id)
            .await
            .map_err(|e| e.to_string())
    }

    /// The client asked for the admin stream (`admin_subscribe`).
    ///
    /// Records the request and answers it; it decides nothing itself (the
    /// revalidation writes the decision). The answer depends on the user
    /// epoch, this socket's earlier answers and a read this request asks
    /// for, never on what a frame's read wrote. The request stays whatever
    /// the answer (like a refused `subscribe`): a later elevation restores
    /// delivery.
    ///
    /// What it costs is bounded (see the module docs, The admin stream): a
    /// refusal at the current user epoch answers at once; otherwise a read
    /// planned after the request answers it, at most one such read per
    /// socket and [`ADMIN_SUBSCRIBE_COOLDOWN`], in a pool of its own.
    pub async fn subscribe_admin(&self) -> AdminSubscription {
        let epoch = self.user_epoch.load(Ordering::SeqCst);
        let asked = match self.request_admin() {
            AdminRequest::Answered(reply) => return reply,
            AdminRequest::Later => return self.reply_admin(epoch, AdminSubscription::Undecided),
            AdminRequest::Read(asked) => asked,
        };
        for _ in 0..DECISION_REVALIDATIONS {
            #[cfg(test)]
            self.seam().await;
            // This socket's revalidation first (a frame's read under way
            // is waited for, without a deadline: the answer is then this
            // request's own read, only later), then a permit of the pool,
            // for at most [`ADMIN_SUBSCRIBE_WAIT`]; the gate is let go when
            // that passes. No permit of the pool is held while waiting for
            // a socket, and never one of the permits other sockets'
            // revalidations wait for.
            let gate = self.refresh_gate.lock().await;
            let pool = Arc::clone(&self.subscribe_permits);
            let Ok(Ok(permit)) =
                tokio::time::timeout(ADMIN_SUBSCRIBE_WAIT, pool.acquire_owned()).await
            else {
                break;
            };
            let attempt = self.revalidate(&gate, Some(permit)).await;
            drop(gate);
            #[cfg(test)]
            self.seam().await;
            match self.admin_answer(asked) {
                Some((at, true)) => return self.reply_admin(at, AdminSubscription::Allowed),
                Some((at, false)) => return self.reply_admin(at, AdminSubscription::Refused),
                None => {}
            }
            // Failed or in backoff: undecided, unless the checks have been
            // failing for so long that the socket ends (the same rule as
            // for the socket's own revalidation).
            if attempt.is_err() {
                if self
                    .failing_for()
                    .is_some_and(|failing| failing >= FAILURE_CLOSE_AFTER)
                {
                    return AdminSubscription::Failed;
                }
                break;
            }
        }
        self.attention.notify_one();
        self.reply_admin(epoch, AdminSubscription::Undecided)
    }

    /// Remember `reply` as this socket's answer at user epoch `epoch` (the
    /// one place that writes it) and return it.
    fn reply_admin(&self, epoch: u64, reply: AdminSubscription) -> AdminSubscription {
        if let Some(admin) = self.lock().admin.as_mut() {
            admin.reply = Some((epoch, reply));
        }
        reply
    }

    /// Record the request for the admin stream and say what answers it.
    ///
    /// The socket's own earlier answer at the current user epoch does if it
    /// was a refusal (it stands for its epoch), or within the cooldown.
    /// Otherwise the scope is to be read: not now while the cooldown or a
    /// backoff runs ("not decided yet"), else by a read asked for here —
    /// the scope is invalidated, so that an answer planned earlier does not
    /// count (a failing scope stays failing, drops kept). What frames'
    /// reads settled is not looked at.
    fn request_admin(&self) -> AdminRequest {
        let mut inner = self.lock();
        let now = Instant::now();
        let backing_off = Self::in_backoff(&inner);
        let epoch = self.user_epoch.load(Ordering::SeqCst);
        let admin = inner.admin.get_or_insert(AdminScope {
            requested: false,
            seen: 0,
            state: ScopeState::Unchecked,
            allowed: false,
            decided: 0,
            read_asked: None,
            reply: None,
        });
        admin.requested = true;
        let cooling = admin
            .read_asked
            .is_some_and(|at| now < at + ADMIN_SUBSCRIBE_COOLDOWN);
        // Only this socket's own answers count here, never what a frame's
        // read wrote: a refusal stands for its user epoch, and within the
        // cooldown so does an earlier "yes".
        match admin.reply.filter(|(at, _)| *at == epoch).map(|(_, r)| r) {
            Some(AdminSubscription::Refused) => {
                return AdminRequest::Answered(AdminSubscription::Refused);
            }
            Some(AdminSubscription::Allowed) if cooling => {
                return AdminRequest::Answered(AdminSubscription::Allowed);
            }
            _ => {}
        }
        if cooling || backing_off {
            drop(inner);
            self.attention.notify_one();
            return AdminRequest::Later;
        }
        admin.read_asked = Some(now);
        AdminRequest::Read(Self::invalidate_admin(&mut inner))
    }

    /// What a revalidation planned at or after `asked` wrote for the admin
    /// stream, and the user epoch it was read at (`None`: none has yet).
    fn admin_answer(&self, asked: u64) -> Option<(u64, bool)> {
        let inner = self.lock();
        inner
            .admin
            .as_ref()
            .filter(|admin| admin.decided >= asked)
            .map(|admin| (admin.seen, admin.allowed))
    }

    /// The client unsubscribed from admin events: the stream is no longer
    /// requested. The decision stays (a new `admin_subscribe` starts from
    /// it); what the stream missed while it was failing is no longer owed.
    pub fn unsubscribe_admin(&self) {
        if let Some(admin) = self.lock().admin.as_mut() {
            admin.requested = false;
            if admin.state == (ScopeState::Failing { dropped: true }) {
                admin.state = ScopeState::Failing { dropped: false };
            }
        }
    }

    /// Whether the socket has requested admin events (allowed, refused or
    /// still to be decided): until the client unsubscribes.
    pub fn admin_subscribed(&self) -> bool {
        self.lock().admin.as_ref().is_some_and(|a| a.requested)
    }

    /// Whether the requested admin stream is allowed as last computed. Not
    /// the delivery decision: that is [`Self::admit_admin`].
    pub fn admin_allowed(&self) -> bool {
        let inner = self.lock();
        inner
            .admin
            .as_ref()
            .is_some_and(|a| a.requested && a.allowed)
    }

    /// Revalidate every scope that is not current — all of them after a user
    /// bump or [`Self::force_full`] — with batched reads. Concurrent calls
    /// are serialised; bumps during a run make the next run.
    ///
    /// Nothing is consumed before the read succeeded: dropping this future
    /// leaves every scope as it was.
    pub async fn refresh(&self) -> Result<(), AccessCheckFailed> {
        #[cfg(test)]
        self.seam().await;
        let gate = self.refresh_gate.lock().await;
        self.revalidate(&gate, None).await
    }

    /// [`Self::refresh`] under the socket's revalidation gate, which the
    /// caller holds (`_gate`), with the permit its read runs under: an
    /// `admin_subscribe` brings one of its own pool (`own`); every other
    /// revalidation waits for one of the process-wide ones. Every caller
    /// takes the gate before any permit.
    async fn revalidate(
        &self,
        _gate: &tokio::sync::MutexGuard<'_, ()>,
        own: Option<tokio::sync::OwnedSemaphorePermit>,
    ) -> Result<(), AccessCheckFailed> {
        let plan = match self.plan() {
            Planned::Nothing => return Ok(()),
            Planned::Backoff => return Err(AccessCheckFailed),
            Planned::Run(plan) => plan,
        };

        crate::ws::test_hooks::count_access_read(
            self.user_id,
            usize::from(!plan.guild_channels.is_empty()) + usize::from(!plan.direct.is_empty()),
        );
        // Queue for a permit outside the timeout: waiting behind other
        // sockets is not a failure (the semaphore is never closed, so this
        // cannot fail either).
        let _permit = match own {
            Some(own) => (Some(own), None),
            None => (None, REVALIDATION_PERMITS.acquire().await.ok()),
        };
        let read = tokio::time::timeout(REFRESH_TIMEOUT, self.read(&plan)).await;
        match read {
            Ok(Ok(found)) => {
                self.apply(&plan, found);
                Ok(())
            }
            Ok(Err(e)) => {
                warn!(error = %e, user_id = %self.user_id, "Channel access revalidation failed");
                self.fail(&plan);
                Err(AccessCheckFailed)
            }
            Err(_) => {
                warn!(user_id = %self.user_id, "Channel access revalidation timed out");
                self.fail(&plan);
                Err(AccessCheckFailed)
            }
        }
    }

    fn plan(&self) -> Planned {
        let mut inner = self.lock();
        // A failure that nothing renewed for this long after its retry was
        // due is over: the next one counts from itself.
        if inner
            .failure
            .as_ref()
            .is_some_and(|f| Instant::now() >= f.retry_at + FAILURE_CLOSE_AFTER)
        {
            inner.failure = None;
        }
        let user_now = self.user_epoch.load(Ordering::SeqCst);
        let full = self.user_phase(&inner) != Phase::Current;
        let guild_targets: Vec<(Uuid, u64)> = inner
            .guilds
            .iter()
            .filter(|(_, gs)| full || gs.phase() != Phase::Current)
            .map(|(g, gs)| (*g, gs.epoch.load(Ordering::SeqCst)))
            .collect();
        let voice = inner
            .voice
            .as_ref()
            .filter(|v| self.voice_phase(v) != Phase::Current)
            .map(|v| VoicePlan {
                session_id: v.session_id,
                channel_id: v.channel_id,
                user_now,
                guild_now: v.guild_now(),
            });
        let admin = inner
            .admin
            .as_ref()
            .filter(|a| a.requested && self.admin_phase(a) != Phase::Current)
            .map(|_| inner.admin_invalidations);
        if !full && guild_targets.is_empty() && voice.is_none() && admin.is_none() {
            // Nothing is waiting for a read: not a failed attempt, also
            // while a backoff runs (it stays for whatever comes next).
            return Planned::Nothing;
        }
        if Self::in_backoff(&inner) {
            return Planned::Backoff;
        }
        let stale: HashSet<Uuid> = guild_targets.iter().map(|(g, _)| *g).collect();
        let mut guild_channels = Vec::new();
        let mut direct = Vec::new();
        for (&channel, &scope) in &inner.requested {
            match scope {
                ChannelScope::Guild(g) if stale.contains(&g) => guild_channels.push((g, channel)),
                ChannelScope::Direct if full => direct.push(channel),
                _ => {}
            }
        }
        Planned::Run(Plan {
            invalidations: inner.invalidations,
            full,
            user_now,
            guild_targets,
            guild_channels,
            direct,
            voice,
            admin,
        })
    }

    async fn read(&self, plan: &Plan) -> Result<ReadResult, String> {
        #[cfg(test)]
        if let Some(scripted) = self.scripted_read(plan) {
            if let Some(seams) = self.seams() {
                seams.parked.fetch_add(1, Ordering::SeqCst);
                if let Ok(permit) = seams.release.acquire().await {
                    permit.forget();
                }
                seams.parked.fetch_sub(1, Ordering::SeqCst);
            }
            return scripted;
        }
        if crate::ws::test_hooks::access_read_fails(self.user_id) {
            return Err(sqlx::Error::PoolTimedOut.to_string());
        }
        // A banned account's sessions end with the ban; until each socket
        // has closed, nothing more is written to it.
        let banned = self.banned().await?;
        if banned {
            return Ok(ReadResult {
                viewable: HashSet::new(),
                // Nothing is known to be deleted: the requests stay.
                existing: plan.guild_channels.iter().map(|(_, c)| *c).collect(),
                direct_ok: HashSet::new(),
                direct_existing: plan.direct.iter().copied().collect(),
                member_of: Some(HashSet::new()),
                voice_ok: plan.voice.as_ref().map(|_| false),
                admin_ok: plan.admin.map(|_| false),
            });
        }
        let (viewable, existing) = if plan.guild_channels.is_empty() {
            (HashSet::new(), HashSet::new())
        } else {
            crate::permissions::viewable_and_existing_channels(
                &self.db,
                self.user_id,
                &plan.guild_channels,
            )
            .await
            .map_err(|e| e.to_string())?
        };
        let mut direct_ok = HashSet::new();
        let mut direct_existing = HashSet::new();
        if !plan.direct.is_empty() {
            let rows: Vec<(Uuid, bool)> = sqlx::query_as(
                "SELECT c.id, EXISTS (SELECT 1 FROM dm_participants dp
                                       WHERE dp.channel_id = c.id AND dp.user_id = $1)
                 FROM channels c WHERE c.id = ANY($2)",
            )
            .bind(self.user_id)
            .bind(&plan.direct)
            .fetch_all(&self.db)
            .await
            .map_err(|e| e.to_string())?;
            for (channel, participant) in rows {
                direct_existing.insert(channel);
                if participant {
                    direct_ok.insert(channel);
                }
            }
        }
        let member_of = if plan.full || !plan.guild_targets.is_empty() {
            Some(
                crate::db::get_user_guild_ids(&self.db, self.user_id)
                    .await
                    .map_err(|e| e.to_string())?
                    .into_iter()
                    .collect(),
            )
        } else {
            None
        };
        let voice_ok = match &plan.voice {
            Some(voice) => Some(
                crate::voice::revoke::has_voice_access(&self.db, self.user_id, voice.channel_id)
                    .await
                    .map_err(|e| e.to_string())?,
            ),
            None => None,
        };
        let admin_ok = if plan.admin.is_some() {
            Some(self.read_admin().await?)
        } else {
            None
        };
        Ok(ReadResult {
            viewable,
            existing,
            direct_ok,
            direct_existing,
            member_of,
            voice_ok,
            admin_ok,
        })
    }

    #[cfg(test)]
    fn seams(&self) -> Option<Arc<Seams>> {
        let script = self.script.lock().unwrap_or_else(PoisonError::into_inner);
        script.as_ref().and_then(|script| script.seams.clone())
    }

    /// A point between two steps of one task at which another task of the
    /// socket can run. Nothing happens here unless a test explores
    /// interleavings ([`Seams`]): then the task stops once.
    #[cfg(test)]
    async fn seam(&self) {
        if let Some(seams) = self.seams() {
            seams.yielded.store(true, Ordering::SeqCst);
            tokio::task::yield_now().await;
        }
    }

    /// The scripted answer to a read, if a script is installed (tests).
    #[cfg(test)]
    fn scripted_read(&self, plan: &Plan) -> Option<Result<ReadResult, String>> {
        let mut guard = self.script.lock().unwrap_or_else(PoisonError::into_inner);
        let script = guard.as_mut()?;
        let step = script.steps.pop_front().unwrap_or(ScriptStep {
            bump: false,
            outcome: Some(script.then),
        });
        script.last_served = Some(step);
        script.last_admin = plan.admin.map(|_| step.outcome);
        if step.bump {
            match script.bump_guild {
                Some(g) => registry().bump_guild(g),
                None => registry().bump_user(self.user_id),
            }
        }
        let Some(allowed) = step.outcome else {
            return Some(Err("scripted failure".to_string()));
        };
        let all = |ids: Vec<Uuid>| -> HashSet<Uuid> {
            if allowed {
                ids.into_iter().collect()
            } else {
                HashSet::new()
            }
        };
        Some(Ok(ReadResult {
            viewable: all(plan.guild_channels.iter().map(|(_, c)| *c).collect()),
            existing: plan.guild_channels.iter().map(|(_, c)| *c).collect(),
            direct_ok: all(plan.direct.clone()),
            direct_existing: plan.direct.iter().copied().collect(),
            member_of: Some(all(plan.guild_targets.iter().map(|(g, _)| *g).collect())),
            voice_ok: plan.voice.as_ref().map(|_| allowed),
            admin_ok: plan.admin.map(|_| allowed),
        }))
    }

    /// Apply what a revalidation read found.
    ///
    /// An answer — a yes as much as a no — is applied to a scope only if the
    /// epochs that were read before the read are still that scope's epochs.
    /// Whatever a moved epoch touches is left exactly as it was (stale or
    /// failing) and read again: nothing is decided, and nothing that cannot
    /// be undone (forgetting a deleted channel, asking for a voice peer's
    /// removal) is done, on an
    /// answer that is no longer the current one.
    fn apply(&self, plan: &Plan, found: ReadResult) {
        let mut guard = self.lock();
        let inner = &mut *guard;
        // The database answered: the next attempt need not wait.
        inner.failure = None;
        // Every scope depends on the user's epoch.
        if self.user_epoch.load(Ordering::SeqCst) != plan.user_now {
            drop(guard);
            self.attention.notify_one();
            return;
        }
        let held: HashSet<Uuid> = plan
            .guild_targets
            .iter()
            .filter(|(g, planned)| {
                inner
                    .guilds
                    .get(g)
                    .is_some_and(|gs| gs.epoch.load(Ordering::SeqCst) == *planned)
            })
            .map(|(g, _)| *g)
            .collect();
        let mut reread = held.len() != plan.guild_targets.len();

        let mut lost = Vec::new();
        let mut regained = Vec::new();
        // allowed = requested ∩ existing ∩ permitted; a deleted channel can
        // never come back, so it also leaves requested.
        for &(g, channel) in &plan.guild_channels {
            // Only channels still requested in the same scope (an
            // unsubscribe or a fresh subscribe during the read wins).
            if held.contains(&g) && inner.requested.get(&channel) == Some(&ChannelScope::Guild(g)) {
                if !found.existing.contains(&channel) {
                    inner.forget(channel);
                }
                let ok = found.viewable.contains(&channel);
                set_allowed(&mut inner.allowed, channel, ok, &mut lost, &mut regained);
            }
        }
        for &channel in &plan.direct {
            if inner.requested.get(&channel) == Some(&ChannelScope::Direct) {
                if !found.direct_existing.contains(&channel) {
                    inner.forget(channel);
                }
                let ok = found.direct_ok.contains(&channel);
                set_allowed(&mut inner.allowed, channel, ok, &mut lost, &mut regained);
            }
        }
        // Something invalidated during the read (a channel recorded
        // unchecked, a backstop) was not in the plan: the scopes are settled
        // (no longer failing) but stay to be recomputed.
        let fresh = inner.invalidations == plan.invalidations;
        let mut resync = false;
        if plan.full {
            resync |= inner.user.state.settle(fresh);
            if fresh {
                inner.user.seen = plan.user_now;
            }
        }
        for &(g, planned) in &plan.guild_targets {
            if !held.contains(&g) {
                continue;
            }
            if let Some(gs) = inner.guilds.get_mut(&g) {
                resync |= gs.state.settle(fresh);
                if fresh {
                    gs.seen = planned;
                }
                if let Some(member_of) = &found.member_of {
                    gs.member = Some(member_of.contains(&g));
                }
            }
        }
        let mut voice_denied = false;
        if let (Some(planned), Some(ok)) = (&plan.voice, found.voice_ok) {
            if let Some(voice) = inner
                .voice
                .as_mut()
                .filter(|v| v.session_id == planned.session_id)
            {
                if voice.guild_now() == planned.guild_now {
                    resync |= voice.state.settle(fresh);
                    if fresh {
                        voice.seen_user = planned.user_now;
                        voice.seen_guild = planned.guild_now;
                    }
                    voice.allowed = ok;
                    voice_denied = !ok && !voice.denial_taken;
                } else {
                    reread = true;
                }
            }
        }
        if let (Some(planned), Some(ok)) = (plan.admin, found.admin_ok) {
            // The one place that decides the admin stream. Like a channel:
            // "not elevated" makes the requested stream not allowed, it does
            // not forget the request, so the next elevation (an epoch bump)
            // restores delivery.
            let newest = inner.admin_invalidations == planned;
            if let Some(admin) = inner.admin.as_mut() {
                if newest {
                    // A recovery that confirms "not allowed" owes nothing:
                    // the admin events dropped meanwhile were not this
                    // socket's, and a resync would tell a refused
                    // subscriber that they flow.
                    let dropped = admin.state.settle(true);
                    resync |= dropped && ok;
                    if admin.allowed != ok {
                        debug!(user_id = %self.user_id, elevated = ok, "Admin events revalidated");
                    }
                    admin.seen = plan.user_now;
                    admin.allowed = ok;
                    admin.decided = planned;
                } else {
                    // Invalidated since this read was planned (an admin
                    // frame taken, a new request, the backstop): the answer
                    // is older than what asked for it. The scope stays as
                    // it is and is read again.
                    reread = true;
                }
            }
        }
        // Servers without requested channels or feed events need no scope.
        let (requested, feed_guilds) = (&inner.requested, &inner.feed_guilds);
        inner.guilds.retain(|g, _| {
            feed_guilds.contains(g)
                || requested
                    .values()
                    .any(|scope| *scope == ChannelScope::Guild(*g))
        });
        if resync {
            inner.resync = true;
        }
        drop(guard);
        if resync || voice_denied || !fresh || reread {
            self.attention.notify_one();
        }
        if !lost.is_empty() || !regained.is_empty() {
            debug!(
                user_id = %self.user_id,
                lost = ?lost,
                regained = ?regained,
                "Channel access revalidated"
            );
        }
    }

    /// A revalidation attempt failed: every scope it covered is failing
    /// until one succeeds (nothing of it is delivered meanwhile).
    fn fail(&self, plan: &Plan) {
        crate::observability::metrics::record_ws_access_check_failure();
        let mut inner = self.lock();
        if plan.full {
            inner.user.state.fail();
        }
        for (g, _) in &plan.guild_targets {
            if let Some(gs) = inner.guilds.get_mut(g) {
                gs.state.fail();
            }
        }
        if let Some(planned) = &plan.voice {
            if let Some(voice) = inner
                .voice
                .as_mut()
                .filter(|v| v.session_id == planned.session_id)
            {
                voice.state.fail();
            }
        }
        if plan.admin.is_some() {
            if let Some(admin) = inner.admin.as_mut() {
                admin.state.fail();
            }
        }
        let now = Instant::now();
        let (since, attempts) = inner
            .failure
            .as_ref()
            .map_or((now, 0), |f| (f.since, f.attempts));
        let delay = RETRY_BASE
            .saturating_mul(1 << attempts.min(5))
            .min(RETRY_MAX);
        inner.failure = Some(Failure {
            since,
            attempts: attempts.saturating_add(1),
            retry_at: now + delay,
        });
        drop(inner);
        self.attention.notify_one();
    }

    /// The phase of the scopes `channel_id` depends on.
    #[cfg(test)]
    fn channel_phase(&self, channel_id: Uuid) -> Option<Phase> {
        let inner = self.lock();
        let &scope = inner.requested.get(&channel_id)?;
        Some(self.phase(&inner, Target::Channel(scope)))
    }
}

fn set_allowed(
    allowed: &mut HashSet<Uuid>,
    channel: Uuid,
    ok: bool,
    lost: &mut Vec<Uuid>,
    regained: &mut Vec<Uuid>,
) {
    if ok {
        if allowed.insert(channel) {
            regained.push(channel);
        }
    } else if allowed.remove(&channel) {
        lost.push(channel);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lazy_pool() -> PgPool {
        // Never connected: these tests do not revalidate.
        PgPool::connect_lazy("postgres://localhost/unused").expect("lazy pool")
    }

    #[test]
    fn notice_wire_format() {
        let notice = AccessNotice {
            scope: NoticeScope::Guild,
            id: Uuid::nil(),
            origin: Uuid::nil(),
        };
        assert_eq!(
            serde_json::to_value(&notice).unwrap(),
            serde_json::json!({
                "scope": "guild",
                "id": "00000000-0000-0000-0000-000000000000",
                "origin": "00000000-0000-0000-0000-000000000000",
            })
        );
    }

    #[test]
    fn changes_bump_the_scope_they_affect() {
        let (u, g, c) = (Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4());
        assert_eq!(
            AccessChange::MemberKicked {
                guild_id: g,
                user_id: u
            }
            .scope(),
            Some((NoticeScope::User, u))
        );
        assert_eq!(
            AccessChange::ChannelPermissionsChanged {
                guild_id: g,
                channel_id: c
            }
            .scope(),
            Some((NoticeScope::Guild, g))
        );
        assert_eq!(
            AccessChange::DmLeft {
                user_id: u,
                channel_id: c
            }
            .scope(),
            Some((NoticeScope::User, u))
        );
        assert_eq!(
            AccessChange::ChannelDeleted {
                guild_id: None,
                channel_id: c
            }
            .scope(),
            None
        );
        assert_eq!(
            AccessChange::AccountDeleted { user_id: u }.scope(),
            Some((NoticeScope::User, u))
        );

        let before = registry().snapshot(u, Some(g));
        let _ = AccessChange::GuildRolesChanged { guild_id: g }.bump();
        let after = registry().snapshot(u, Some(g));
        assert_eq!(after.user, before.user);
        assert!(after.guild > before.guild);
    }

    #[test]
    fn deleted_cells_go_and_never_repeat_a_value() {
        let (g, u) = (Uuid::new_v4(), Uuid::new_v4());
        let old_guild = registry().guild_epoch(g);
        let old_user = registry().user_epoch(u);
        let seen_guild = old_guild.load(Ordering::SeqCst);
        let seen_user = old_user.load(Ordering::SeqCst);
        let _ = AccessChange::GuildDeleted { guild_id: g }.bump();
        assert!(
            !registry().guilds.contains_key(&g),
            "cell of a deleted server kept"
        );
        assert_ne!(
            old_guild.load(Ordering::SeqCst),
            seen_guild,
            "holders see the bump"
        );
        let _ = AccessChange::AccountDeleted { user_id: u }.bump();
        assert!(
            !registry().users.contains_key(&u),
            "cell of a deleted account kept"
        );
        assert_ne!(old_user.load(Ordering::SeqCst), seen_user);
        // A re-created cell starts at a value nobody has stored.
        let again = registry().guild_epoch(g).load(Ordering::SeqCst);
        assert!(again > seen_guild && again != old_guild.load(Ordering::SeqCst));
        assert!(registry().user_epoch(u).load(Ordering::SeqCst) > seen_user);
    }

    #[tokio::test]
    async fn refused_and_unchecked_channels_stay_requested() {
        let (u, g, c, d) = (
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
        );
        let access = SocketAccess::new(u, lazy_pool());
        let scope = ChannelScope::Guild(g);
        assert_eq!(
            access.commit_subscribe(c, scope, access.snapshot(scope), false),
            SubscribeCommit::Recorded
        );
        assert!(access.is_requested(c) && !access.is_allowed(c));
        assert!(!access.is_stale());
        assert!(access.record_unchecked(d, ChannelScope::Direct));
        assert!(access.is_requested(d) && !access.is_allowed(d));
        assert!(
            access.is_stale(),
            "an unchecked channel must be revalidated"
        );
    }

    #[tokio::test]
    async fn steady_state_needs_no_revalidation() {
        let (u, g, c) = (Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4());
        let access = SocketAccess::new(u, lazy_pool());
        let scope = ChannelScope::Guild(g);
        assert_eq!(
            access.commit_subscribe(c, scope, access.snapshot(scope), true),
            SubscribeCommit::Recorded
        );
        assert!(!access.is_stale());
        assert!(access.may_forward(c));
        assert!(access.admit(c).await);
        assert!(!access.may_forward(Uuid::new_v4()));
    }

    #[tokio::test]
    async fn subscribe_after_a_bump_is_refused_for_a_new_check() {
        let (u, g, c) = (Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4());
        let access = SocketAccess::new(u, lazy_pool());
        let scope = ChannelScope::Guild(g);
        let snap = access.snapshot(scope);
        registry().bump_guild(g);
        assert_eq!(
            access.commit_subscribe(c, scope, snap, true),
            SubscribeCommit::Moved
        );
        assert!(!access.is_requested(c));
        registry().bump_user(u);
        assert_eq!(
            access.commit_subscribe(c, scope, access.snapshot(ChannelScope::Direct), true),
            SubscribeCommit::Moved
        );
    }

    #[tokio::test]
    async fn a_bump_makes_the_socket_stale_and_unsubscribe_clears_both_sets() {
        let (u, g, c) = (Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4());
        let access = SocketAccess::new(u, lazy_pool());
        let scope = ChannelScope::Guild(g);
        assert_eq!(
            access.commit_subscribe(c, scope, access.snapshot(scope), true),
            SubscribeCommit::Recorded
        );
        registry().bump_guild(Uuid::new_v4());
        assert!(!access.is_stale(), "another server's bump");
        registry().bump_guild(g);
        assert!(access.is_stale());
        // Stale: the forwarder queues, the writer decides.
        assert!(access.may_forward(c));
        access.unsubscribe(c);
        assert!(!access.is_requested(c));
        assert!(!access.is_allowed(c));
        assert!(!access.admit(c).await);
    }

    fn epoch_of_user(u: Uuid) -> u64 {
        registry().snapshot(u, None).user
    }

    /// A write whose outcome is not known (the request was
    /// dropped while its commit was in flight, or the commit answered with
    /// an error) bumps as well; a write that did not apply does not.
    #[tokio::test]
    async fn a_write_of_unknown_outcome_still_bumps() {
        let u = Uuid::new_v4();
        let change = AccessChange::MemberKicked {
            guild_id: Uuid::new_v4(),
            user_id: u,
        };
        let before = epoch_of_user(u);
        {
            let write = std::future::pending::<Result<(), ()>>();
            let fut = bump_after(change, write);
            tokio::pin!(fut);
            assert!(futures::poll!(&mut fut).is_pending());
            assert_eq!(epoch_of_user(u), before, "bumped before the commit");
        }
        let dropped = epoch_of_user(u);
        assert!(dropped > before, "a cancelled commit left the epoch alone");

        let failed = bump_after(change, async { Err::<(), _>("connection lost") }).await;
        assert!(failed.is_err());
        let after_error = epoch_of_user(u);
        assert!(
            after_error > dropped,
            "a failed commit left the epoch alone"
        );

        let (_, bumped) = bump_after_if(Some(change), async { Ok::<_, ()>(0_u64) }, |n| *n > 0)
            .await
            .unwrap();
        assert!(bumped.is_none());
        assert_eq!(epoch_of_user(u), after_error, "nothing changed, yet bumped");

        let (_, bumped) = bump_after_if(Some(change), async { Ok::<_, ()>(1_u64) }, |n| *n > 0)
            .await
            .unwrap();
        assert_eq!(bumped.map(|b| b.change()), Some(change));
        let applied = epoch_of_user(u);
        assert!(applied > after_error);

        {
            let write = std::future::pending::<Result<u64, ()>>();
            let fut = bump_after_if(Some(change), write, |n| *n > 0);
            tokio::pin!(fut);
            assert!(futures::poll!(&mut fut).is_pending());
        }
        assert!(epoch_of_user(u) > applied);

        // A request that cannot change access bumps nothing either way.
        let now = epoch_of_user(u);
        let (_, bumped) = bump_after_if(None, async { Ok::<_, ()>(1_u64) }, |_| true)
            .await
            .unwrap();
        assert!(bumped.is_none());
        assert_eq!(epoch_of_user(u), now);
    }

    /// A write of unknown outcome must not drop a live server's epoch cell
    /// (sockets hold the cell; a new one would never tell them anything).
    #[tokio::test]
    async fn an_unknown_server_deletion_keeps_the_cell() {
        let g = Uuid::new_v4();
        let cell = registry().guild_epoch(g);
        let seen = cell.load(Ordering::SeqCst);
        let failed = bump_after(AccessChange::GuildDeleted { guild_id: g }, async {
            Err::<(), _>("connection lost")
        })
        .await;
        assert!(failed.is_err());
        assert!(cell.load(Ordering::SeqCst) > seen);
        assert!(Arc::ptr_eq(&cell, &registry().guild_epoch(g)));
    }

    /// The requested set is bounded, and DMs have their own room: server
    /// channels can never crowd out the automatic subscription of a new DM.
    #[tokio::test]
    async fn requested_channels_are_capped_per_kind() {
        let (u, g) = (Uuid::new_v4(), Uuid::new_v4());
        let access = SocketAccess::new(u, lazy_pool());
        let scope = ChannelScope::Guild(g);
        let first = Uuid::new_v4();
        for i in 0..MAX_REQUESTED_CHANNELS {
            let channel = if i == 0 { first } else { Uuid::new_v4() };
            assert_eq!(
                access.commit_subscribe(channel, scope, access.snapshot(scope), false),
                SubscribeCommit::Recorded
            );
        }
        let extra = Uuid::new_v4();
        assert_eq!(
            access.commit_subscribe(extra, scope, access.snapshot(scope), true),
            SubscribeCommit::Full
        );
        assert!(!access.record_unchecked(extra, scope));
        assert!(!access.is_requested(extra));
        assert_eq!(access.requested_len(), MAX_REQUESTED_CHANNELS);
        // A channel that is requested already can still be decided again.
        assert_eq!(
            access.commit_subscribe(first, scope, access.snapshot(scope), true),
            SubscribeCommit::Recorded
        );

        // A full set of server channels leaves every DM slot free.
        assert!(access.has_room_for(extra), "a DM would still fit");
        let first_dm = Uuid::new_v4();
        assert!(access.insert_direct(first_dm));
        for _ in 1..MAX_REQUESTED_DIRECT {
            assert!(access.insert_direct(Uuid::new_v4()));
        }
        assert_eq!(
            access.requested_len(),
            MAX_REQUESTED_CHANNELS + MAX_REQUESTED_DIRECT
        );
        assert!(!access.insert_direct(extra), "one DM too many was taken");
        assert!(!access.has_room_for(extra));
        let direct = ChannelScope::Direct;
        assert_eq!(
            access.commit_subscribe(extra, direct, access.snapshot(direct), true),
            SubscribeCommit::Full
        );
        // Unsubscribing makes room, per kind.
        access.unsubscribe(first_dm);
        assert!(access.insert_direct(extra));
        access.unsubscribe(first);
        assert_eq!(
            access.commit_subscribe(Uuid::new_v4(), scope, access.snapshot(scope), true),
            SubscribeCommit::Recorded
        );
        assert_eq!(
            access.requested_len(),
            MAX_REQUESTED_CHANNELS + MAX_REQUESTED_DIRECT
        );
    }

    fn all_found(channels: &[Uuid]) -> ReadResult {
        ReadResult {
            viewable: channels.iter().copied().collect(),
            existing: channels.iter().copied().collect(),
            direct_ok: HashSet::new(),
            direct_existing: HashSet::new(),
            member_of: Some(HashSet::new()),
            voice_ok: None,
            admin_ok: None,
        }
    }

    fn dropped(access: &SocketAccess) -> bool {
        let inner = access.lock();
        let is = |s: ScopeState| s == ScopeState::Failing { dropped: true };
        is(inner.user.state)
            || inner.guilds.values().any(|gs| is(gs.state))
            || inner.voice.as_ref().is_some_and(|v| is(v.state))
            || inner.admin.as_ref().is_some_and(|a| is(a.state))
    }

    /// A forced (backstop) revalidation that fails **without**
    /// any epoch moving makes the scopes failing; every event dropped then
    /// is recorded, whoever drops it, and the recovery asks for a resync —
    /// only if something was dropped.
    #[tokio::test(start_paused = true)]
    async fn a_failed_forced_revalidation_records_every_drop() {
        for drop_something in [true, false] {
            let (u, g, c) = (Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4());
            let access = SocketAccess::new(u, lazy_pool());
            let scope = ChannelScope::Guild(g);
            assert_eq!(
                access.commit_subscribe(c, scope, access.snapshot(scope), true),
                SubscribeCommit::Recorded
            );
            assert_eq!(access.channel_phase(c), Some(Phase::Current));

            crate::ws::test_hooks::fail_access_reads(u, 1);
            access.force_full();
            assert_eq!(access.channel_phase(c), Some(Phase::Stale));
            assert!(access.may_forward(c), "stale: the writer decides");
            assert!(access.refresh().await.is_err());
            assert_eq!(access.channel_phase(c), Some(Phase::Failing));
            assert!(!dropped(&access));

            // What the client sends is refused, but nothing it should have
            // received was dropped.
            assert!(!access.may_act(c).await);
            assert!(!dropped(&access));
            if drop_something {
                // The forwarder leaves the decision to the writer; the
                // writer drops and records.
                assert!(access.may_forward(c));
                assert!(!access.admit(c).await);
                assert!(dropped(&access), "silent drop while failing");
            }
            assert!(!access.take_resync(), "resync before the recovery");

            // The retry succeeds.
            tokio::time::advance(RETRY_BASE + Duration::from_millis(1)).await;
            let Planned::Run(plan) = access.plan() else {
                panic!("no retry planned");
            };
            access.apply(&plan, all_found(&[c]));
            assert_eq!(access.channel_phase(c), Some(Phase::Current));
            assert!(access.admit(c).await);
            assert_eq!(access.take_resync(), drop_something);
        }
    }

    /// An event that cannot be decided because the socket is in backoff
    /// (another scope failed) is recorded on its own scope too.
    #[tokio::test(start_paused = true)]
    async fn a_drop_during_backoff_is_recorded_on_its_scope() {
        let (u, g1, g2, c1, c2) = (
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
        );
        let access = SocketAccess::new(u, lazy_pool());
        for (g, c) in [(g1, c1), (g2, c2)] {
            let scope = ChannelScope::Guild(g);
            assert_eq!(
                access.commit_subscribe(c, scope, access.snapshot(scope), true),
                SubscribeCommit::Recorded
            );
        }
        crate::ws::test_hooks::fail_access_reads(u, 1);
        registry().bump_guild(g1);
        assert!(access.refresh().await.is_err());
        assert_eq!(access.channel_phase(c1), Some(Phase::Failing));
        assert_eq!(access.channel_phase(c2), Some(Phase::Current));
        assert!(access.admit(c2).await, "an unaffected server keeps flowing");

        // The second server changes while the socket is in backoff.
        registry().bump_guild(g2);
        assert_eq!(access.channel_phase(c2), Some(Phase::Stale));
        assert!(!access.admit(c2).await);
        assert_eq!(access.channel_phase(c2), Some(Phase::Failing));

        tokio::time::advance(RETRY_BASE + Duration::from_millis(1)).await;
        let Planned::Run(plan) = access.plan() else {
            panic!("no retry planned");
        };
        assert_eq!(plan.guild_targets.len(), 2);
        access.apply(&plan, all_found(&[c1, c2]));
        assert!(access.take_resync());
    }

    /// Room content needs an owned peer **and** current
    /// authorization for the room; a peer that did not come through the join
    /// check gets nothing.
    #[tokio::test]
    async fn voice_content_needs_current_authorization() {
        let (u, g, room) = (Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4());
        let access = SocketAccess::new(u, lazy_pool());
        let peer = OwnedVoicePeer {
            channel_id: room,
            session_id: Uuid::new_v4(),
            admission: Some(VoiceAdmission {
                guild_id: Some(g),
                epochs: registry().snapshot(u, Some(g)),
            }),
        };
        assert!(!access.admit_voice(None).await, "no peer owned");
        assert!(access.admit_voice(Some(&peer)).await);
        assert!(!access.is_stale());

        let unadmitted = OwnedVoicePeer {
            session_id: Uuid::new_v4(),
            admission: None,
            ..peer
        };
        assert!(!access.admit_voice(Some(&unadmitted)).await);
        assert!(access.take_voice_denied().is_none());

        // The user's access changed: the next write revalidates; while that
        // fails, nothing is written and the drop is recorded.
        assert!(access.admit_voice(Some(&peer)).await);
        crate::ws::test_hooks::fail_access_reads(u, 1);
        registry().bump_user(u);
        assert!(access.is_stale());
        assert!(!access.admit_voice(Some(&peer)).await);
        assert!(dropped(&access));

        // A server-wide change is seen as well.
        let (u2, g2) = (Uuid::new_v4(), Uuid::new_v4());
        let access = SocketAccess::new(u2, lazy_pool());
        let peer = OwnedVoicePeer {
            channel_id: room,
            session_id: Uuid::new_v4(),
            admission: Some(VoiceAdmission {
                guild_id: Some(g2),
                epochs: registry().snapshot(u2, Some(g2)),
            }),
        };
        assert!(access.admit_voice(Some(&peer)).await);
        crate::ws::test_hooks::fail_access_reads(u2, 1);
        registry().bump_guild(g2);
        assert!(!access.admit_voice(Some(&peer)).await);
    }

    /// What a frame of the enumeration below depends on.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Kind {
        GuildChannel,
        Direct,
        Feed,
        Voice,
        Admin,
    }

    /// How the scope stands when the frame arrives.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Start {
        Current,
        Bumped,
        Invalidated,
    }

    fn install(access: &SocketAccess, steps: &[ScriptStep], then: bool, bump_guild: Option<Uuid>) {
        *access.script.lock().unwrap() = Some(Script {
            steps: steps.iter().copied().collect(),
            then,
            bump_guild,
            last_served: None,
            last_admin: None,
            seams: None,
        });
    }

    /// A read that fails.
    const FAILS: ScriptStep = ScriptStep {
        bump: false,
        outcome: None,
    };

    /// A socket whose `admin_subscribe` was answered by a read that said
    /// `elevated`; the script keeps answering the same.
    async fn admin_socket(u: Uuid, elevated: bool) -> Arc<SocketAccess> {
        let access = SocketAccess::new(u, lazy_pool());
        install(&access, &[], elevated, None);
        let expected = if elevated {
            AdminSubscription::Allowed
        } else {
            AdminSubscription::Refused
        };
        assert_eq!(access.subscribe_admin().await, expected);
        access
    }

    /// Lets the cooldown after the last read an `admin_subscribe` asked for
    /// pass.
    async fn cooled_down() {
        tokio::time::advance(ADMIN_SUBSCRIBE_COOLDOWN + Duration::from_millis(1)).await;
    }

    /// An `admin_subscribe` arrives and needs a read: its request is
    /// recorded. Returns what an answer must be planned at to answer it.
    fn read_asked(access: &SocketAccess) -> u64 {
        match access.request_admin() {
            AdminRequest::Read(asked) => asked,
            other => panic!("no read asked for: {other:?}"),
        }
    }

    /// What a revalidation of the admin scope alone found.
    fn admin_found(elevated: bool) -> ReadResult {
        ReadResult {
            admin_ok: Some(elevated),
            ..all_found(&[])
        }
    }

    /// What the socket's last `admin_subscribe` was answered, if that was
    /// at the current user epoch.
    fn standing_reply(access: &SocketAccess) -> Option<AdminSubscription> {
        let epoch = access.user_epoch.load(Ordering::SeqCst);
        let inner = access.lock();
        inner
            .admin
            .as_ref()
            .and_then(|admin| admin.reply)
            .filter(|(at, _)| *at == epoch)
            .map(|(_, reply)| reply)
    }

    /// `Some(allowed)` if the admin scope is current.
    fn admin_decision(access: &SocketAccess) -> Option<bool> {
        let inner = access.lock();
        inner
            .admin
            .as_ref()
            .filter(|admin| access.admin_phase(admin) == Phase::Current)
            .map(|admin| admin.allowed)
    }

    /// One frame through the decision loop with scripted reads. Returns the
    /// way it left as text, or panics if it left in a way that must not
    /// exist. Then access comes back (invariant C): the epoch moves again
    /// (re-elevation, role restored) and a read at it says yes — whatever
    /// the first frame's fate, the next one must be delivered, without a
    /// client action.
    async fn one_decision(
        kind: Kind,
        bump_guild: bool,
        start: Start,
        steps: &[ScriptStep],
        then: bool,
        (u, g, c): (Uuid, Uuid, Uuid),
    ) -> &'static str {
        let access = SocketAccess::new(u, lazy_pool());
        let peer = OwnedVoicePeer {
            channel_id: c,
            session_id: Uuid::from_u128(7),
            admission: Some(VoiceAdmission {
                guild_id: Some(g),
                epochs: registry().snapshot(u, Some(g)),
            }),
        };
        // Current and allowed to begin with.
        match kind {
            Kind::GuildChannel => {
                let scope = ChannelScope::Guild(g);
                let recorded = access.commit_subscribe(c, scope, access.snapshot(scope), true);
                assert_eq!(recorded, SubscribeCommit::Recorded);
            }
            Kind::Direct => {
                let scope = ChannelScope::Direct;
                let recorded = access.commit_subscribe(c, scope, access.snapshot(scope), true);
                assert_eq!(recorded, SubscribeCommit::Recorded);
            }
            Kind::Feed => {
                let mut inner = access.lock();
                let epoch = registry().guild_epoch(g);
                let seen = epoch.load(Ordering::SeqCst);
                inner.guilds.insert(
                    g,
                    GuildScope {
                        epoch,
                        seen,
                        state: ScopeState::Checked,
                        member: Some(true),
                    },
                );
                inner.feed_guilds.insert(g);
            }
            Kind::Voice => access.track_voice(Some(&peer)),
            Kind::Admin => {
                install(&access, &[], true, None);
                assert_eq!(access.subscribe_admin().await, AdminSubscription::Allowed);
            }
        }
        let bump_guild = bump_guild.then_some(g);
        match start {
            Start::Current => {}
            Start::Bumped => match bump_guild {
                Some(g) => registry().bump_guild(g),
                None => registry().bump_user(u),
            },
            Start::Invalidated => access.force_full(),
        }
        install(&access, steps, then, bump_guild);

        let admitted = match kind {
            Kind::GuildChannel | Kind::Direct => access.admit(c).await,
            Kind::Feed => access.admit_guild(g).await,
            Kind::Voice => access.admit_voice(Some(&peer)).await,
            Kind::Admin => access.admit_admin().await,
        };

        let recorded = dropped(&access);
        let inner = access.lock();
        let (target, allowed, subscribed) = match kind {
            Kind::GuildChannel => (
                Target::Channel(ChannelScope::Guild(g)),
                inner.allowed.contains(&c),
                inner.requested.contains_key(&c),
            ),
            Kind::Direct => (
                Target::Channel(ChannelScope::Direct),
                inner.allowed.contains(&c),
                inner.requested.contains_key(&c),
            ),
            Kind::Feed => (
                Target::Channel(ChannelScope::Guild(g)),
                inner.guilds.get(&g).and_then(|s| s.member) == Some(true),
                true,
            ),
            Kind::Voice => (
                Target::Voice,
                inner.voice.as_ref().is_some_and(|v| v.allowed),
                inner.voice.is_some(),
            ),
            Kind::Admin => (
                Target::Admin,
                inner.admin.as_ref().is_some_and(|a| a.allowed),
                inner.admin.as_ref().is_some_and(|a| a.requested),
            ),
        };
        let current = subscribed && access.phase(&inner, target) == Phase::Current;
        drop(inner);
        // What the database said last, if that answer still stands (no epoch
        // moved while it was read).
        let last = access
            .script
            .lock()
            .unwrap()
            .as_ref()
            .and_then(|s| s.last_served);
        let standing = |answer: bool| matches!(last, Some(ScriptStep { bump: false, outcome: Some(a) }) if a == answer);
        let case = format!("{kind:?} bump_guild={bump_guild:?} {start:?} {steps:?} then={then}");
        let exit = 'exit: {
            if admitted {
                assert!(
                    current && allowed,
                    "admitted without current authorization: {case}"
                );
                assert!(
                    last.is_none() || standing(true),
                    "admitted on an answer that does not stand: {case}"
                );
                break 'exit "admitted";
            }
            assert!(
                !(current && allowed),
                "refused although allowed at the current epoch: {case}"
            );
            if recorded {
                break 'exit "undecided, recorded";
            }
            // Refused without a record: only a "no" that still stands may do
            // that — not a subscription that an outdated "no" took away.
            assert!(
                standing(false),
                "dropped, not denied by a standing answer, not recorded: {case}"
            );
            assert!(
                !subscribed || (current && !allowed),
                "denied, but the state does not say so: {case}"
            );
            "denied"
        };

        // Access comes back: past any backoff, the epoch moves and the read
        // at the new epoch says yes. Nothing the client asked for may have
        // been forgotten on the way (a "no" is not an unsubscribe).
        tokio::time::advance(RETRY_MAX + Duration::from_millis(1)).await;
        match bump_guild {
            Some(g) => registry().bump_guild(g),
            None => registry().bump_user(u),
        }
        install(&access, &[], true, bump_guild);
        let restored = match kind {
            Kind::GuildChannel | Kind::Direct => access.admit(c).await,
            Kind::Feed => access.admit_guild(g).await,
            Kind::Voice => access.admit_voice(Some(&peer)).await,
            Kind::Admin => access.admit_admin().await,
        };
        assert!(restored, "access came back, delivery did not: {case}");
        exit
    }

    /// One `admin_subscribe` through the same scripted reads, on a socket
    /// that never asked (`before` is `None`) or whose earlier request stands
    /// as `before` says. Returns its answer as text, or panics if the answer
    /// is not what a standing read said. Then access comes back, as in
    /// [`one_decision`]: whatever the answer was, the request is still
    /// there.
    async fn one_subscribe(
        before: Option<Start>,
        steps: &[ScriptStep],
        then: bool,
        u: Uuid,
    ) -> &'static str {
        let access = match before {
            None => SocketAccess::new(u, lazy_pool()),
            Some(start) => {
                let access = admin_socket(u, true).await;
                cooled_down().await;
                match start {
                    Start::Current => {}
                    Start::Bumped => registry().bump_user(u),
                    Start::Invalidated => access.force_full(),
                }
                access
            }
        };
        install(&access, steps, then, None);
        let answer = access.subscribe_admin().await;

        let decision = admin_decision(&access);
        let last = access
            .script
            .lock()
            .unwrap()
            .as_ref()
            .and_then(|s| s.last_served);
        let standing = |answer: bool| matches!(last, Some(ScriptStep { bump: false, outcome: Some(a) }) if a == answer);
        let case = format!("subscribe after {before:?} {steps:?} then={then}");
        assert!(access.admin_subscribed(), "the request is gone: {case}");
        let exit = match answer {
            AdminSubscription::Allowed => {
                assert!(
                    decision == Some(true) && standing(true),
                    "allowed, but no standing read said so: {case}"
                );
                "allowed"
            }
            AdminSubscription::Refused => {
                assert!(
                    decision == Some(false) && standing(false),
                    "refused, but no standing read said so: {case}"
                );
                "refused"
            }
            AdminSubscription::Undecided => {
                // Only when no read stands: it failed, or every one of them
                // was overtaken by an epoch move.
                assert!(
                    decision.is_none() && !standing(true) && !standing(false),
                    "undecided although a read stands: {case}"
                );
                assert!(access.is_stale(), "nothing left to decide it: {case}");
                "undecided"
            }
            AdminSubscription::Failed => panic!("one failure ended the socket: {case}"),
        };
        assert!(!access.take_resync(), "nothing was dropped: {case}");

        tokio::time::advance(RETRY_MAX + Duration::from_millis(1)).await;
        registry().bump_user(u);
        install(&access, &[], true, None);
        assert!(
            access.admit_admin().await,
            "access came back, delivery did not: {case}"
        );
        exit
    }

    /// Every way through the decision loop: all sequences of read outcomes
    /// (answers yes, answers no, fails), each with or without an epoch
    /// moving while the read is in flight, for every kind of frame, every
    /// starting state and both epochs. A frame leaves the loop admitted (on
    /// current authorization), denied (by a decision at the current epoch)
    /// or undecided with the drop recorded — never dropped without either.
    /// The same sequences behind an `admin_subscribe`: it answers what a
    /// standing read said or nothing, and the request survives.
    #[tokio::test(start_paused = true)]
    async fn every_exit_of_the_decision_loop_is_accounted_for() {
        const OUTCOMES: [Option<bool>; 3] = [Some(true), Some(false), None];
        let mut symbols = Vec::new();
        for outcome in OUTCOMES {
            for bump in [false, true] {
                symbols.push(ScriptStep { bump, outcome });
            }
        }
        let ids = (Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4());
        let kinds = [
            (Kind::GuildChannel, false),
            (Kind::GuildChannel, true),
            (Kind::Direct, false),
            (Kind::Feed, false),
            (Kind::Feed, true),
            (Kind::Voice, false),
            (Kind::Voice, true),
            (Kind::Admin, false),
        ];
        let starts = [Start::Current, Start::Bumped, Start::Invalidated];
        let mut exits = std::collections::BTreeMap::<&str, usize>::new();
        let mut answers = std::collections::BTreeMap::<&str, usize>::new();
        let n = symbols.len();
        for code in 0..n.pow(u32::try_from(DECISION_REVALIDATIONS).unwrap()) {
            let mut steps = Vec::with_capacity(DECISION_REVALIDATIONS);
            let mut rest = code;
            for _ in 0..DECISION_REVALIDATIONS {
                steps.push(symbols[rest % n]);
                rest /= n;
            }
            for then in [true, false] {
                for start in starts {
                    for (kind, bump_guild) in kinds {
                        let exit = one_decision(kind, bump_guild, start, &steps, then, ids).await;
                        *exits.entry(exit).or_default() += 1;
                    }
                }
                for before in std::iter::once(None).chain(starts.map(Some)) {
                    let answer = one_subscribe(before, &steps, then, ids.0).await;
                    *answers.entry(answer).or_default() += 1;
                }
            }
        }
        // All three ways out occur, and all three answers.
        assert_eq!(exits.len(), 3, "{exits:?}");
        assert_eq!(answers.len(), 3, "{answers:?}");
    }

    /// The case that was lost: epochs move under the first revalidations and
    /// the last one succeeds at the current epoch. Its answer counts.
    #[tokio::test(start_paused = true)]
    async fn the_answer_of_the_last_revalidation_counts() {
        let moving = ScriptStep {
            bump: true,
            outcome: Some(true),
        };
        let settled = ScriptStep {
            bump: false,
            outcome: Some(true),
        };
        let steps = [moving, moving, moving, settled];
        let ids = (Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4());
        for (kind, bump_guild) in [
            (Kind::GuildChannel, true),
            (Kind::Direct, false),
            (Kind::Feed, true),
            (Kind::Voice, false),
            (Kind::Admin, false),
        ] {
            let exit = one_decision(kind, bump_guild, Start::Bumped, &steps, false, ids).await;
            assert_eq!(exit, "admitted", "{kind:?}");
        }
        // One move more than the loop waits for: undecided, and recorded.
        let exit = one_decision(
            Kind::GuildChannel,
            true,
            Start::Bumped,
            &[moving; DECISION_REVALIDATIONS],
            true,
            ids,
        )
        .await;
        assert_eq!(exit, "undecided, recorded");
    }

    /// A re-check reads "not elevated"; the session is elevated and the
    /// user's epoch moves before that answer is applied. The outdated "no"
    /// must not end the subscription: the next read restores the feed.
    #[tokio::test(start_paused = true)]
    async fn an_outdated_admin_denial_does_not_end_the_subscription() {
        let u = Uuid::new_v4();
        let access = admin_socket(u, true).await;
        registry().bump_user(u);
        let no_while_elevated = ScriptStep {
            bump: true,
            outcome: Some(false),
        };
        install(&access, &[no_while_elevated], true, None);
        assert!(
            access.admit_admin().await,
            "the feed did not come back after the elevation"
        );
        assert!(access.admin_subscribed());

        // The same for the other things an answer can undo: an outdated
        // "channel gone" keeps the request, an outdated "no voice access"
        // asks for no removal.
        let (g, c) = (Uuid::new_v4(), Uuid::new_v4());
        let access = SocketAccess::new(u, lazy_pool());
        let scope = ChannelScope::Guild(g);
        assert_eq!(
            access.commit_subscribe(c, scope, access.snapshot(scope), true),
            SubscribeCommit::Recorded
        );
        let peer = OwnedVoicePeer {
            channel_id: c,
            session_id: Uuid::new_v4(),
            admission: Some(VoiceAdmission {
                guild_id: Some(g),
                epochs: registry().snapshot(u, Some(g)),
            }),
        };
        access.track_voice(Some(&peer));
        registry().bump_guild(g);
        install(&access, &[no_while_elevated], true, Some(g));
        assert!(access.refresh().await.is_ok());
        assert!(access.is_requested(c));
        assert!(access.take_voice_denied().is_none());
        assert!(access.admit(c).await);
        assert!(access.admit_voice(Some(&peer)).await);
    }

    /// The admin stream is a scope like the others. A read that says
    /// "elevated" while a de-elevation commits and bumps does not admit the
    /// frame (the epoch is compared after the read); a read that fails is
    /// not a "no": the subscription stays, the drop is recorded and the
    /// recovery asks for a resync; a read that says "not elevated" makes the
    /// stream not allowed but keeps the request.
    #[tokio::test(start_paused = true)]
    async fn the_admin_stream_is_a_scope() {
        let u = Uuid::new_v4();
        let yes_while_bumped = ScriptStep {
            bump: true,
            outcome: Some(true),
        };

        // Not subscribed: nothing, and nothing to revalidate.
        let access = SocketAccess::new(u, lazy_pool());
        assert!(!access.may_forward_admin());
        assert!(!access.admit_admin().await);
        assert!(!access.is_stale());
        // A subscribe whose reads are all overtaken by an epoch move is not
        // answered. The request is recorded all the same, and the next read
        // decides it.
        install(
            &access,
            &[yes_while_bumped; DECISION_REVALIDATIONS],
            true,
            None,
        );
        assert_eq!(access.subscribe_admin().await, AdminSubscription::Undecided);
        assert!(access.admin_subscribed() && !access.admin_allowed());
        assert!(access.is_stale() && access.may_forward_admin());
        assert!(access.admit_admin().await);

        // Subscribed and current: the "yes" is read again for the frame.
        let access = admin_socket(u, true).await;
        assert!(access.may_forward_admin());
        assert!(access.admit_admin().await);

        // The elevation ends while the re-check's read is in flight.
        registry().bump_user(u);
        install(&access, &[yes_while_bumped], false, None);
        assert!(access.may_forward_admin(), "stale: the writer decides");
        assert!(!access.admit_admin().await, "admitted on an obsolete epoch");
        assert!(access.admin_subscribed(), "a \"no\" is not an unsubscribe");
        assert!(!access.admin_allowed(), "a read said: not elevated");
        assert!(!access.may_forward_admin(), "current and not allowed");
        assert!(!dropped(&access));
        access.unsubscribe_admin();
        assert!(!access.admin_subscribed());

        // A failed read is not a denial.
        let access = admin_socket(u, true).await;
        registry().bump_user(u);
        install(&access, &[FAILS], true, None);
        assert!(!access.admit_admin().await);
        assert!(
            access.admin_subscribed(),
            "a failed read ended the subscription"
        );
        assert!(
            dropped(&access),
            "silent drop while the admin scope was failing"
        );
        assert!(access.may_forward_admin());
        assert!(!access.take_resync());
        tokio::time::advance(RETRY_BASE + Duration::from_millis(1)).await;
        assert!(access.refresh().await.is_ok());
        assert!(access.admit_admin().await);
        assert!(access.take_resync());

        // A subscribe that could not be checked is decided by the first
        // read that can be made.
        let access = SocketAccess::new(u, lazy_pool());
        install(&access, &[FAILS], true, None);
        assert_eq!(access.subscribe_admin().await, AdminSubscription::Undecided);
        assert!(access.admin_subscribed() && access.is_stale());
        tokio::time::advance(RETRY_BASE + Duration::from_millis(1)).await;
        assert!(access.admit_admin().await);
        assert!(!access.take_resync(), "nothing was dropped");
    }

    /// Subscribe, de-elevate, a revalidation completes ("not elevated" at
    /// the epoch that still holds), re-elevate, revalidate: the feed is
    /// delivered again without a new `admin_subscribe` (invariant C). A
    /// refused `admin_subscribe` stays requested the same way.
    #[tokio::test(start_paused = true)]
    async fn the_admin_feed_comes_back_after_a_completed_de_elevation() {
        let u = Uuid::new_v4();
        let access = admin_socket(u, true).await;
        assert!(access.admit_admin().await);

        // De-elevation commits and bumps; the revalidation completes.
        registry().bump_user(u);
        install(&access, &[], false, None);
        assert!(access.refresh().await.is_ok());
        assert!(!access.is_stale(), "the \"no\" is current");
        assert!(!access.may_forward_admin());
        assert!(!access.admit_admin().await);
        assert!(access.admin_subscribed());

        // Re-elevation commits and bumps; the revalidation says "elevated".
        registry().bump_user(u);
        assert!(access.is_stale(), "the request is revalidated");
        install(&access, &[], true, None);
        assert!(access.refresh().await.is_ok());
        assert!(access.may_forward_admin());
        assert!(access.admit_admin().await, "the feed did not come back");
        assert!(!access.take_resync(), "nothing was dropped undecided");

        // Refused at subscribe time, elevated afterwards.
        let access = admin_socket(u, false).await;
        assert!(access.admin_subscribed() && !access.admin_allowed());
        assert!(!access.admit_admin().await);
        registry().bump_user(u);
        install(&access, &[], true, None);
        assert!(access.admit_admin().await);
    }

    /// A "yes" for the admin stream is never reused: an elevation can end
    /// without an epoch moving (it expires, or another process removes it),
    /// so every admin frame of an allowed socket is decided by a read made
    /// after the frame was taken. A "no" stays cached per epoch: a socket
    /// that was refused costs no read per admin event.
    #[tokio::test(start_paused = true)]
    async fn a_positive_admin_decision_is_never_reused() {
        let u = Uuid::new_v4();
        let access = admin_socket(u, true).await;
        crate::ws::test_hooks::count_access_reads(u);

        // Elevated and unchanged: delivered, one read per frame.
        for n in 1..=3 {
            assert!(access.may_forward_admin());
            assert!(access.admit_admin().await);
            assert_eq!(crate::ws::test_hooks::access_reads(u).0, n);
        }

        // The elevation ended without any epoch moving: the next frame's
        // read says so, and the frame is not written.
        install(&access, &[], false, None);
        assert!(!access.admit_admin().await, "written on a cached yes");
        assert!(access.admin_subscribed() && !access.admin_allowed());
        assert_eq!(crate::ws::test_hooks::access_reads(u).0, 4);

        // From now on the "no" is current: no read per event.
        for _ in 0..3 {
            assert!(!access.may_forward_admin());
            assert!(!access.admit_admin().await);
        }
        assert_eq!(crate::ws::test_hooks::access_reads(u).0, 4);
        assert!(!dropped(&access) && !access.take_resync());

        // A subscriber that was never elevated: no read per event either.
        let u = Uuid::new_v4();
        let access = admin_socket(u, false).await;
        crate::ws::test_hooks::count_access_reads(u);
        for _ in 0..3 {
            assert!(!access.may_forward_admin());
            assert!(!access.admit_admin().await);
        }
        assert_eq!(crate::ws::test_hooks::access_reads(u), (0, 0));
    }

    /// What asking costs. Nothing while the socket's answer is a refusal at
    /// the current user epoch, however often the client asks, also after an
    /// unsubscribe and the periodic re-check; for a "yes", one read per
    /// cooldown, and in between the earlier answer stands (nothing is
    /// written on it: every frame is read on its own). A new user epoch
    /// makes the next request read again.
    #[tokio::test(start_paused = true)]
    async fn an_admin_subscribe_reads_only_when_it_must() {
        let reads = |u: Uuid| crate::ws::test_hooks::access_reads(u).0;
        let ask = |access: Arc<SocketAccess>| async move { access.subscribe_admin().await };

        // Refused already: no read, whatever the rows say by now (no epoch
        // moved, so nothing the server did changed them).
        let u = Uuid::new_v4();
        let access = admin_socket(u, false).await;
        crate::ws::test_hooks::count_access_reads(u);
        install(&access, &[], true, None);
        for _ in 0..3 {
            assert_eq!(ask(access.clone()).await, AdminSubscription::Refused);
        }
        access.unsubscribe_admin();
        assert!(!access.admin_subscribed());
        cooled_down().await;
        assert_eq!(ask(access.clone()).await, AdminSubscription::Refused);
        assert_eq!(reads(u), 0, "reads for a socket that is refused already");
        assert!(access.admin_subscribed() && !access.may_forward_admin());
        // The elevation bumps the epoch: the next request reads.
        registry().bump_user(u);
        assert_eq!(ask(access.clone()).await, AdminSubscription::Allowed);
        assert_eq!(reads(u), 1);

        // Allowed: within the cooldown the socket's earlier answer stands,
        // also across an unsubscribe, also when the rows no longer agree,
        // and also once a frame's own read found that (the frame is not
        // written; the answer is the socket's own).
        for _ in 0..3 {
            assert_eq!(ask(access.clone()).await, AdminSubscription::Allowed);
        }
        access.unsubscribe_admin();
        assert!(!access.admit_admin().await, "written without a request");
        install(&access, &[], false, None);
        assert_eq!(ask(access.clone()).await, AdminSubscription::Allowed);
        assert_eq!(reads(u), 1, "read again within the cooldown");
        // A frame's read, made here step by step: waiting for a permit of
        // the other revalidations could let the paused clock run past the
        // cooldown.
        assert!(access.admin_frame_taken());
        let Planned::Run(plan) = access.plan() else {
            panic!("nothing planned");
        };
        access.apply(&plan, admin_found(false));
        assert_eq!(admin_decision(&access), Some(false));
        assert_eq!(ask(access.clone()).await, AdminSubscription::Allowed);
        assert_eq!(reads(u), 1);
        // After the cooldown the next request reads, and reports it.
        cooled_down().await;
        assert_eq!(ask(access.clone()).await, AdminSubscription::Refused);
        assert_eq!(reads(u), 2);

        // After the cooldown a "yes" is read again, once: an elevation that
        // ended without an epoch moving is reported.
        let u = Uuid::new_v4();
        let access = admin_socket(u, true).await;
        crate::ws::test_hooks::count_access_reads(u);
        cooled_down().await;
        install(&access, &[], false, None);
        assert_eq!(ask(access.clone()).await, AdminSubscription::Refused);
        assert_eq!(reads(u), 1);

        // Within the cooldown and without a current decision: no read
        // here. The socket's own revalidation, due anyway, settles it.
        let u = Uuid::new_v4();
        let access = admin_socket(u, true).await;
        crate::ws::test_hooks::count_access_reads(u);
        registry().bump_user(u);
        assert_eq!(ask(access.clone()).await, AdminSubscription::Undecided);
        assert_eq!(reads(u), 0);
        assert!(access.is_stale());
        assert!(access.refresh().await.is_ok());
        assert_eq!(admin_decision(&access), Some(true));
        assert_eq!(reads(u), 1);

        // The periodic re-check invalidates the decision of a stream that
        // is not requested as well (and does not read it); the socket's
        // refusal still stands for its user epoch: the next request does
        // not read.
        let u = Uuid::new_v4();
        let access = admin_socket(u, false).await;
        crate::ws::test_hooks::count_access_reads(u);
        access.unsubscribe_admin();
        access.force_full();
        let Planned::Run(plan) = access.plan() else {
            panic!("nothing planned");
        };
        assert!(plan.admin.is_none(), "read for a stream nobody asked for");
        access.apply(&plan, all_found(&[]));
        cooled_down().await;
        assert_eq!(ask(access.clone()).await, AdminSubscription::Refused);
        assert_eq!(reads(u), 0);
    }

    /// The reads an `admin_subscribe` asks for run in a pool of their own:
    /// with every permit of the other revalidations taken it is read all
    /// the same, while another socket's frame waits for one of those.
    #[tokio::test]
    async fn an_admin_subscribe_never_takes_a_revalidation_permit() {
        let (u, other, g, c) = (
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
        );
        let access = SocketAccess::new(u, lazy_pool());
        install(&access, &[], true, None);
        let waiting = SocketAccess::new(other, lazy_pool());
        let scope = ChannelScope::Guild(g);
        assert_eq!(
            waiting.commit_subscribe(c, scope, waiting.snapshot(scope), true),
            SubscribeCommit::Recorded
        );
        registry().bump_guild(g);
        install(&waiting, &[], true, Some(g));

        let all = REVALIDATION_PERMITS
            .acquire_many(u32::try_from(REVALIDATION_CONCURRENCY).unwrap())
            .await
            .unwrap();
        let subscribe = access.subscribe_admin();
        let frame = waiting.admit(c);
        tokio::pin!(subscribe, frame);
        let subscribed = futures::poll!(&mut subscribe);
        let frame_waits = futures::poll!(&mut frame).is_pending();
        drop(all);
        assert_eq!(
            subscribed,
            std::task::Poll::Ready(AdminSubscription::Allowed),
            "it waits for a permit of the other revalidations"
        );
        assert!(frame_waits, "the permits were not all taken");
        assert!(frame.await);
    }

    /// An `admin_subscribe` waits only a bounded time for its pool. After
    /// that the request stands, is answered "not decided yet" and is
    /// settled by the socket's own revalidation.
    #[tokio::test(start_paused = true)]
    async fn an_admin_subscribe_waits_a_bounded_time_for_its_pool() {
        let u = Uuid::new_v4();
        let access = SocketAccess::new(u, lazy_pool());
        install(&access, &[], true, None);
        crate::ws::test_hooks::count_access_reads(u);
        // Another subscribe's read is under way.
        let busy = Arc::clone(&access.subscribe_permits)
            .acquire_owned()
            .await
            .unwrap();
        let asked = Instant::now();
        assert_eq!(access.subscribe_admin().await, AdminSubscription::Undecided);
        assert_eq!(asked.elapsed(), ADMIN_SUBSCRIBE_WAIT);
        assert_eq!(crate::ws::test_hooks::access_reads(u).0, 0);
        assert!(access.admin_subscribed() && access.is_stale());
        assert!(access.refresh().await.is_ok());
        assert_eq!(admin_decision(&access), Some(true));
        drop(busy);
    }

    /// An `admin_subscribe` waits for its own socket's revalidation first
    /// and for a permit of its pool second, under one deadline: while one
    /// socket's sender holds that socket's revalidation and waits for a
    /// permit of the other revalidations, that socket's subscribe holds no
    /// permit of the pool, and another socket's subscribe is read.
    #[tokio::test(start_paused = true)]
    async fn a_subscribe_waits_for_its_socket_before_it_takes_a_pool_permit() {
        let (u, other, g, c) = (
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
        );
        let first = SocketAccess::new(u, lazy_pool());
        let scope = ChannelScope::Guild(g);
        assert_eq!(
            first.commit_subscribe(c, scope, first.snapshot(scope), true),
            SubscribeCommit::Recorded
        );
        registry().bump_guild(g);
        install(&first, &[], true, Some(g));
        // Both sockets share one pool, as all of them do outside tests.
        let mut second = SocketAccess::new(other, lazy_pool());
        Arc::get_mut(&mut second)
            .expect("not shared yet")
            .subscribe_permits = Arc::clone(&first.subscribe_permits);
        install(&second, &[], true, None);

        let all = REVALIDATION_PERMITS
            .acquire_many(u32::try_from(REVALIDATION_CONCURRENCY).unwrap())
            .await
            .unwrap();
        // The first socket's sender takes its gate and waits for a permit.
        let frame = first.admit(c);
        let waiting = first.subscribe_admin();
        tokio::pin!(frame, waiting);
        assert!(futures::poll!(&mut frame).is_pending());
        assert!(futures::poll!(&mut waiting).is_pending());
        assert_eq!(
            first.subscribe_permits.available_permits(),
            ADMIN_SUBSCRIBE_CONCURRENCY,
            "a subscribe holds a permit of the pool while it waits for its socket"
        );
        assert_eq!(
            second.subscribe_admin().await,
            AdminSubscription::Allowed,
            "another socket's subscribe was not read"
        );
        drop(all);
        assert!(frame.await);
        // Answered by a read, or by its deadline.
        assert!(matches!(
            waiting.await,
            AdminSubscription::Allowed | AdminSubscription::Undecided
        ));
    }

    /// While every permit of the other revalidations is taken for longer
    /// than an `admin_subscribe` waits for its pool, a refused socket gets
    /// the same reply with and without an admin frame being decided: the
    /// subscribe waits for its socket's revalidation (the frame's) and then
    /// answers from its own read; only the time differs.
    #[tokio::test(start_paused = true)]
    async fn a_busy_socket_delays_the_answer_but_does_not_change_it() {
        let mut answers = Vec::new();
        for frame in [false, true] {
            let u = Uuid::new_v4();
            let access = admin_socket(u, false).await;
            cooled_down().await;
            // A change of the user's own that leaves them refused.
            registry().bump_user(u);
            let all = REVALIDATION_PERMITS
                .acquire_many(u32::try_from(REVALIDATION_CONCURRENCY).unwrap())
                .await
                .unwrap();
            let sender = async {
                if frame {
                    assert!(!access.admit_admin().await);
                }
            };
            let subscribe = access.subscribe_admin();
            tokio::pin!(sender, subscribe);
            let sent = futures::poll!(&mut sender).is_ready();
            let mut answer = None;
            if let std::task::Poll::Ready(early) = futures::poll!(&mut subscribe) {
                answer = Some(early);
            }
            tokio::time::advance(ADMIN_SUBSCRIBE_WAIT * 3).await;
            if answer.is_none() {
                if let std::task::Poll::Ready(late) = futures::poll!(&mut subscribe) {
                    answer = Some(late);
                }
            }
            drop(all);
            // Both go on (the gate is handed over in order).
            let rest = async {
                if !sent {
                    sender.await;
                }
            };
            let answer = match answer {
                Some(answer) => {
                    rest.await;
                    answer
                }
                None => tokio::join!(rest, subscribe).1,
            };
            answers.push(answer);
        }
        assert_eq!(answers, [AdminSubscription::Refused; 2]);
    }

    /// A subscribe that could not be decided in time says so: it answers
    /// "not decided yet" when the pool for its reads stays busy, and does
    /// not take the answer a frame's read settled meanwhile (frames decide
    /// on their own reads).
    #[tokio::test(start_paused = true)]
    async fn a_subscribe_that_ran_out_of_time_answers_undecided() {
        let u = Uuid::new_v4();
        let access = SocketAccess::new(u, lazy_pool());
        install(&access, &[], false, None);
        // Another subscribe's read holds the pool, and the socket's own
        // revalidation is under way.
        let busy = Arc::clone(&access.subscribe_permits)
            .acquire_owned()
            .await
            .unwrap();
        let gate = access.refresh_gate.lock().await;
        let subscribe = access.subscribe_admin();
        tokio::pin!(subscribe);
        assert!(futures::poll!(&mut subscribe).is_pending());
        // A frame is taken, and its own read, planned after the request,
        // settles "not elevated" before the subscribe has the gate.
        assert!(access.admin_frame_taken());
        let Planned::Run(plan) = access.plan() else {
            panic!("nothing planned");
        };
        access.apply(&plan, admin_found(false));
        assert_eq!(admin_decision(&access), Some(false));
        drop(gate);
        assert!(futures::poll!(&mut subscribe).is_pending());
        tokio::time::advance(ADMIN_SUBSCRIBE_WAIT).await;
        assert_eq!(
            futures::poll!(&mut subscribe),
            std::task::Poll::Ready(AdminSubscription::Undecided)
        );
        drop(busy);
    }

    /// The close after [`FAILURE_CLOSE_AFTER`] covers the reads an
    /// `admin_subscribe` asks for as well: a client that asks, is answered
    /// "not decided yet" on a failed read and withdraws the request, again
    /// and again, is told to end the socket once the admin stream has been
    /// failing that long, like the socket's own revalidation.
    #[tokio::test(start_paused = true)]
    async fn a_subscribe_whose_reads_keep_failing_ends_the_socket() {
        let u = Uuid::new_v4();
        let access = SocketAccess::new(u, lazy_pool());
        install(&access, &[FAILS; 16], true, None);
        let began = Instant::now();
        loop {
            let answer = access.subscribe_admin().await;
            if began.elapsed() >= FAILURE_CLOSE_AFTER {
                assert_eq!(answer, AdminSubscription::Failed, "the socket stays");
                break;
            }
            assert_eq!(answer, AdminSubscription::Undecided);
            access.unsubscribe_admin();
            assert!(access.failing_for().is_none());
            tokio::time::advance(ADMIN_SUBSCRIBE_COOLDOWN).await;
        }
    }

    /// A read that began before the admin frame was taken does not decide
    /// it: the frame waits for a read of its own.
    #[tokio::test(start_paused = true)]
    async fn an_admin_frame_is_decided_by_a_read_after_it_was_taken() {
        let u = Uuid::new_v4();
        let access = admin_socket(u, true).await;
        // A revalidation planned (and read: "elevated") before the frame...
        access.force_full();
        let gate = access.refresh_gate.lock().await;
        let Planned::Run(early) = access.plan() else {
            panic!("nothing planned");
        };
        // ...the frame is taken and waits for a revalidation (held up
        // here), then the early answer ("elevated") arrives.
        install(&access, &[], false, None);
        let frame = access.admit_admin();
        tokio::pin!(frame);
        assert!(
            futures::poll!(&mut frame).is_pending(),
            "decided without waiting for a read"
        );
        access.apply(&early, admin_found(true));
        assert_eq!(
            admin_decision(&access),
            None,
            "an answer older than the frame was applied"
        );
        drop(gate);
        assert!(!frame.await, "decided by a read that began before it");
    }

    /// An `admin_subscribe` whose read began before an admin frame was
    /// taken cannot decide that frame — also when the client unsubscribed
    /// and subscribed again while the frame was queued. Its "elevated"
    /// changes nothing; the frame waits for a read of its own ("not
    /// elevated" now), and that read answers the subscribe as well.
    #[tokio::test(start_paused = true)]
    async fn a_subscribe_read_older_than_an_admin_frame_does_not_decide_it() {
        for resubscribed in [false, true] {
            let u = Uuid::new_v4();
            let access = admin_socket(u, true).await;
            cooled_down().await;
            crate::ws::test_hooks::count_access_reads(u);
            if resubscribed {
                access.unsubscribe_admin();
            }
            // The subscribe's revalidation is planned and reads
            // "elevated"...
            let asked = read_asked(&access);
            let gate = access.refresh_gate.lock().await;
            let Planned::Run(early) = access.plan() else {
                panic!("nothing planned");
            };
            // ...the elevation ends without an epoch moving, and a frame
            // is taken while that read is in flight...
            install(&access, &[], false, None);
            let frame = access.admit_admin();
            tokio::pin!(frame);
            assert!(
                futures::poll!(&mut frame).is_pending(),
                "decided without waiting for a read"
            );
            // ...then the old "elevated" arrives.
            access.apply(&early, admin_found(true));
            assert_eq!(
                admin_decision(&access),
                None,
                "resubscribed={resubscribed}: an answer older than the frame was applied"
            );
            assert_eq!(access.admin_answer(asked).map(|(_, allowed)| allowed), None);
            drop(gate);
            assert!(!frame.await, "written on a read older than the frame");
            assert_eq!(admin_decision(&access), Some(false));
            // The frame's read began after the request was recorded: it is
            // the subscribe's answer too.
            assert_eq!(
                access.admin_answer(asked).map(|(_, allowed)| allowed),
                Some(false)
            );
            assert_eq!(crate::ws::test_hooks::access_reads(u).0, 1);
        }
    }

    /// Whether an admin frame is refused outright or needs a read is settled
    /// in one step, and a refused frame has no second step in which an
    /// answer could still arrive: it is refused at once, also while every
    /// revalidation is held up.
    #[tokio::test(start_paused = true)]
    async fn a_refused_admin_frame_is_refused_in_one_step() {
        let u = Uuid::new_v4();
        let access = admin_socket(u, true).await;
        cooled_down().await;
        crate::ws::test_hooks::count_access_reads(u);
        // Frame 1 is taken; an `admin_subscribe` arrives while it waits for
        // its read; the elevation ends without an epoch moving.
        assert!(access.admin_frame_taken());
        let asked = read_asked(&access);
        install(&access, &[], false, None);
        // One read, planned after both, answers both.
        assert!(!access.decide_admin().await);
        assert_eq!(
            access.admin_answer(asked).map(|(_, allowed)| allowed),
            Some(false)
        );
        assert_eq!(crate::ws::test_hooks::access_reads(u).0, 1);

        // Frame 2 meets a current "no".
        let gate = access.refresh_gate.lock().await;
        assert!(!access.admin_frame_taken());
        let second = access.admit_admin();
        tokio::pin!(second);
        assert_eq!(
            futures::poll!(&mut second),
            std::task::Poll::Ready(false),
            "a refused frame waits for something"
        );
        // Frame 3 is taken while the stream is not requested (the client
        // unsubscribed; its next subscribe is not recorded yet).
        access.unsubscribe_admin();
        assert!(!access.admin_frame_taken());
        let third = access.admit_admin();
        tokio::pin!(third);
        assert_eq!(
            futures::poll!(&mut third),
            std::task::Poll::Ready(false),
            "a frame nobody asked for waits for something"
        );
        drop(gate);
        assert_eq!(crate::ws::test_hooks::access_reads(u).0, 1);

        // The session is elevated again (an epoch bump): the subscribe
        // that follows is decided by its own read, and only frames taken
        // from then on are written.
        cooled_down().await;
        registry().bump_user(u);
        install(&access, &[], true, None);
        assert_eq!(access.subscribe_admin().await, AdminSubscription::Allowed);
        assert!(access.admit_admin().await);
    }

    /// Answers are applied in the order they were read; should an older one
    /// arrive after a newer one all the same, it changes nothing.
    #[tokio::test(start_paused = true)]
    async fn an_older_admin_answer_never_replaces_a_newer_one() {
        for newer in [false, true] {
            let u = Uuid::new_v4();
            let access = admin_socket(u, !newer).await;
            cooled_down().await;
            // A revalidation is planned and reads the old state...
            registry().bump_user(u);
            let Planned::Run(early) = access.plan() else {
                panic!("nothing planned");
            };
            // ...the elevation changes; an `admin_subscribe` reads and
            // records the new state...
            install(&access, &[], newer, None);
            let expected = if newer {
                AdminSubscription::Allowed
            } else {
                AdminSubscription::Refused
            };
            assert_eq!(access.subscribe_admin().await, expected);
            // ...then the early answer arrives.
            access.apply(&early, admin_found(!newer));
            assert_eq!(
                admin_decision(&access),
                Some(newer),
                "an older answer replaced a newer one"
            );
            assert_eq!(access.may_forward_admin(), newer);
        }
    }

    /// Only what concerns the admin stream makes a read of it not count: a
    /// DM created while an `admin_subscribe` is being read does not, and an
    /// admin frame does not make a channel's read count less.
    #[tokio::test(start_paused = true)]
    async fn unrelated_changes_do_not_spoil_an_admin_read() {
        let u = Uuid::new_v4();
        let access = SocketAccess::new(u, lazy_pool());
        // The subscribe's read is in flight...
        let asked = read_asked(&access);
        let Planned::Run(plan) = access.plan() else {
            panic!("nothing planned");
        };
        // ...when a new DM is recorded, still to be checked...
        assert!(access.insert_direct(Uuid::new_v4()));
        // ...and the answer counts for the admin stream.
        access.apply(&plan, admin_found(true));
        assert_eq!(
            access.admin_answer(asked).map(|(_, allowed)| allowed),
            Some(true),
            "a new DM made the admin read not count"
        );
        assert_eq!(admin_decision(&access), Some(true));
        assert!(access.is_stale(), "the DM is still to be checked");

        // Through the entry point: a DM per read, and still answered by the
        // first one.
        let access = SocketAccess::new(u, lazy_pool());
        install(&access, &[], true, None);
        let gate = access.refresh_gate.lock().await;
        let subscribe = access.subscribe_admin();
        tokio::pin!(subscribe);
        assert!(futures::poll!(&mut subscribe).is_pending());
        assert!(access.insert_direct(Uuid::new_v4()));
        drop(gate);
        assert_eq!(subscribe.await, AdminSubscription::Allowed);

        // The other way round: a channel's read is in flight when an admin
        // frame is taken.
        let (g, c) = (Uuid::new_v4(), Uuid::new_v4());
        let access = admin_socket(u, true).await;
        let scope = ChannelScope::Guild(g);
        assert_eq!(
            access.commit_subscribe(c, scope, access.snapshot(scope), true),
            SubscribeCommit::Recorded
        );
        registry().bump_guild(g);
        let Planned::Run(plan) = access.plan() else {
            panic!("nothing planned");
        };
        assert!(access.admin_frame_taken());
        access.apply(&plan, all_found(&[c]));
        assert_eq!(access.channel_phase(c), Some(Phase::Current));
        assert!(access.decide_admin().await);
    }

    /// Admin frames taken while an `admin_subscribe` waits for its answer
    /// do not leave it unanswered: the first frame's own read began after
    /// the request and answers it, whatever is taken afterwards.
    #[tokio::test(start_paused = true)]
    async fn admin_frames_do_not_starve_a_subscribe() {
        let u = Uuid::new_v4();
        let access = admin_socket(u, true).await;
        cooled_down().await;
        // The subscribe's read is in flight...
        let asked = read_asked(&access);
        let gate = access.refresh_gate.lock().await;
        let Planned::Run(first) = access.plan() else {
            panic!("nothing planned");
        };
        // ...a frame is taken: that read no longer counts...
        let frame = access.admit_admin();
        tokio::pin!(frame);
        assert!(futures::poll!(&mut frame).is_pending());
        access.apply(&first, admin_found(true));
        assert_eq!(access.admin_answer(asked).map(|(_, allowed)| allowed), None);
        drop(gate);
        // ...the frame's read does, for both.
        assert!(frame.await);
        assert_eq!(
            access.admin_answer(asked).map(|(_, allowed)| allowed),
            Some(true)
        );
        // The next frame is taken before the subscribe has looked.
        assert!(access.admin_frame_taken());
        assert_eq!(
            access.admin_answer(asked).map(|(_, allowed)| allowed),
            Some(true),
            "the next frame took the subscribe's answer away"
        );
        assert!(access.decide_admin().await);
    }

    /// An `admin_subscribe` settles a failing admin scope through the same
    /// revalidation as everything else: not while the socket is in backoff
    /// (the request is recorded, the drop kept), and afterwards with a
    /// resync only if the stream is allowed.
    #[tokio::test(start_paused = true)]
    async fn a_subscribe_recovers_a_failing_admin_scope_like_any_revalidation() {
        for elevated in [false, true] {
            let u = Uuid::new_v4();
            let access = admin_socket(u, elevated).await;
            cooled_down().await;
            registry().bump_user(u);
            install(&access, &[FAILS], elevated, None);
            assert!(!access.admit_admin().await);
            assert!(dropped(&access));

            crate::ws::test_hooks::count_access_reads(u);
            assert_eq!(access.subscribe_admin().await, AdminSubscription::Undecided);
            assert_eq!(
                crate::ws::test_hooks::access_reads(u).0,
                0,
                "read during the backoff"
            );
            assert!(dropped(&access), "the drop was forgotten");
            assert!(access.admin_subscribed());

            tokio::time::advance(RETRY_BASE + Duration::from_millis(1)).await;
            let expected = if elevated {
                AdminSubscription::Allowed
            } else {
                AdminSubscription::Refused
            };
            assert_eq!(access.subscribe_admin().await, expected);
            assert!(!dropped(&access));
            assert_eq!(access.take_resync(), elevated, "elevated={elevated}");
            assert!(access.retry_at().is_none());

            // An unsubscribe takes back what the stream was owed: the same
            // failure, then `admin_unsubscribe`, then the recovery.
            let u = Uuid::new_v4();
            let access = admin_socket(u, elevated).await;
            cooled_down().await;
            registry().bump_user(u);
            install(&access, &[FAILS], elevated, None);
            assert!(!access.admit_admin().await);
            access.unsubscribe_admin();
            assert!(!dropped(&access));
            tokio::time::advance(RETRY_BASE + Duration::from_millis(1)).await;
            assert_eq!(access.subscribe_admin().await, expected);
            assert!(!access.take_resync(), "a resync for a stream given up");
        }
    }

    /// A socket that is refused admin events cannot tell when they flow: an
    /// admin event dropped while its revalidation was failing causes no
    /// resync when the recovery confirms "not allowed".
    #[tokio::test(start_paused = true)]
    async fn a_refused_admin_subscriber_is_not_resynced_by_admin_events() {
        let u = Uuid::new_v4();
        let access = admin_socket(u, false).await;
        registry().bump_user(u);
        install(&access, &[FAILS], false, None);
        assert!(access.may_forward_admin(), "stale: the writer decides");
        assert!(!access.admit_admin().await);
        assert!(dropped(&access), "the undecided drop is recorded");

        tokio::time::advance(RETRY_MAX + Duration::from_millis(1)).await;
        assert!(access.refresh().await.is_ok());
        assert!(!access.admin_allowed());
        assert!(!access.take_resync(), "nothing owed was missed");

        // An allowed stream that recovers as allowed still resynchronises.
        let access = admin_socket(u, true).await;
        registry().bump_user(u);
        install(&access, &[FAILS], true, None);
        assert!(!access.admit_admin().await);
        tokio::time::advance(RETRY_MAX + Duration::from_millis(1)).await;
        assert!(access.admit_admin().await);
        assert!(access.take_resync());
    }

    /// The answer to a repeated `admin_subscribe` is the socket's own: it
    /// depends on the user epoch, this socket's earlier answers and a read
    /// the subscribe asks for, never on whether an admin frame was decided
    /// in between. For a refused user whose epoch moved (a role of their
    /// own) and for an admin whose elevation ran out, within the cooldown.
    #[tokio::test(start_paused = true)]
    async fn the_answer_to_admin_subscribe_is_the_sockets_own() {
        for elevated in [false, true] {
            let mut answers = Vec::new();
            for frame in [false, true] {
                let u = Uuid::new_v4();
                let access = admin_socket(u, elevated).await;
                install(&access, &[], false, None);
                if !elevated {
                    // A change of the user's own that leaves them refused.
                    registry().bump_user(u);
                }
                if frame {
                    // An admin frame taken and decided by its own read,
                    // step by step (waiting for a permit of the other
                    // revalidations could let the paused clock run past the
                    // cooldown).
                    assert!(access.admin_frame_taken());
                    let Planned::Run(plan) = access.plan() else {
                        panic!("nothing planned");
                    };
                    access.apply(&plan, admin_found(false));
                    assert_eq!(admin_decision(&access), Some(false));
                }
                answers.push(access.subscribe_admin().await);
            }
            assert_eq!(
                answers[0], answers[1],
                "elevated={elevated}: an admin frame changed the answer"
            );
        }
    }

    fn voice_peer(u: Uuid, g: Uuid) -> OwnedVoicePeer {
        OwnedVoicePeer {
            channel_id: Uuid::new_v4(),
            session_id: Uuid::new_v4(),
            admission: Some(VoiceAdmission {
                guild_id: Some(g),
                epochs: registry().snapshot(u, Some(g)),
            }),
        }
    }

    /// A failed check sets one backoff for the socket, and it outlasts the
    /// scopes: nothing is read before the retry time, whatever goes away or
    /// comes back meanwhile. A socket with nothing left to check has no
    /// retry time to wake for all the same, and a failure that nothing
    /// renewed for a long time no longer counts.
    #[tokio::test(start_paused = true)]
    async fn the_backoff_outlasts_its_scope_and_an_idle_socket_does_not_wait() {
        let idle = |access: &SocketAccess| {
            access.retry_at().is_none() && access.failing_for().is_none() && !access.is_stale()
        };
        let reads = |u: Uuid| crate::ws::test_hooks::access_reads(u).0;
        let soon = RETRY_BASE + Duration::from_millis(1);

        // The admin stream is failing, and the client unsubscribes.
        let (u, g, c) = (Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4());
        let access = admin_socket(u, true).await;
        let scope = ChannelScope::Guild(g);
        assert_eq!(
            access.commit_subscribe(c, scope, access.snapshot(scope), true),
            SubscribeCommit::Recorded
        );
        install(&access, &[FAILS], true, None);
        assert!(!access.admit_admin().await);
        let retry = access.retry_at();
        assert!(retry.is_some());
        access.unsubscribe_admin();
        assert!(idle(&access), "a socket with nothing to check waits");
        // What the socket loop does on its next wake is no failed attempt.
        assert!(access.refresh().await.is_ok());
        // The backoff is still there: another scope that needs a read is
        // not read before the retry time.
        registry().bump_guild(g);
        crate::ws::test_hooks::count_access_reads(u);
        assert!(!access.admit(c).await);
        assert_eq!(reads(u), 0, "a read during the backoff");
        assert_eq!(access.retry_at(), retry, "the retry time moved");
        tokio::time::advance(soon).await;
        assert!(access.admit(c).await);
        assert_eq!(reads(u), 1);
        assert!(idle(&access));

        // Asking and unsubscribing in turns while the database fails: one
        // attempt per backoff, and the failures add up.
        let u = Uuid::new_v4();
        let access = SocketAccess::new(u, lazy_pool());
        install(&access, &[FAILS; 8], true, None);
        crate::ws::test_hooks::count_access_reads(u);
        for _ in 0..4 {
            assert_eq!(access.subscribe_admin().await, AdminSubscription::Undecided);
            access.unsubscribe_admin();
            assert!(idle(&access));
        }
        assert_eq!(reads(u), 1, "attempts during the backoff");
        tokio::time::advance(FAILURE_CLOSE_AFTER / 2).await;
        assert_eq!(access.subscribe_admin().await, AdminSubscription::Undecided);
        assert_eq!(reads(u), 2);
        assert_eq!(
            access.retry_at().map(|at| at - Instant::now()),
            Some(RETRY_BASE * 2),
            "the second failure did not double the delay"
        );
        assert!(
            access
                .failing_for()
                .is_some_and(|d| d >= FAILURE_CLOSE_AFTER / 2),
            "the second failure does not count from the first"
        );
        // Much later (nothing renewed the failure meanwhile) one read
        // fails: failing since now, with the first delay.
        access.unsubscribe_admin();
        tokio::time::advance(FAILURE_CLOSE_AFTER + RETRY_MAX).await;
        assert_eq!(access.subscribe_admin().await, AdminSubscription::Undecided);
        assert_eq!(reads(u), 3);
        assert_eq!(access.failing_for(), Some(Duration::ZERO));
        assert_eq!(
            access.retry_at().map(|at| at - Instant::now()),
            Some(RETRY_BASE)
        );

        // The voice room is failing, and the peer leaves, or is replaced by
        // a new one (a rejoin): nothing to wait for.
        for rejoined in [false, true] {
            let (u, g) = (Uuid::new_v4(), Uuid::new_v4());
            let access = SocketAccess::new(u, lazy_pool());
            let peer = voice_peer(u, g);
            assert!(access.admit_voice(Some(&peer)).await);
            registry().bump_guild(g);
            install(&access, &[FAILS], true, Some(g));
            assert!(!access.admit_voice(Some(&peer)).await);
            assert!(access.retry_at().is_some());
            if rejoined {
                assert!(access.admit_voice(Some(&voice_peer(u, g))).await);
            } else {
                access.track_voice(None);
            }
            assert!(idle(&access), "rejoined={rejoined}: an idle socket waits");
        }

        // The scope goes away while the read that then fails is in flight.
        let u = Uuid::new_v4();
        let access = admin_socket(u, true).await;
        assert!(access.admin_frame_taken());
        let Planned::Run(plan) = access.plan() else {
            panic!("nothing planned");
        };
        access.unsubscribe_admin();
        access.fail(&plan);
        assert!(idle(&access), "waiting for a scope that is gone");

        // An unsubscribed channel leaves its server's scope to the next
        // revalidation: the socket still waits for that retry.
        let (u, g, c) = (Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4());
        let access = SocketAccess::new(u, lazy_pool());
        let scope = ChannelScope::Guild(g);
        assert_eq!(
            access.commit_subscribe(c, scope, access.snapshot(scope), true),
            SubscribeCommit::Recorded
        );
        registry().bump_guild(g);
        install(&access, &[FAILS], true, Some(g));
        assert!(!access.admit(c).await);
        access.unsubscribe(c);
        assert!(access.retry_at().is_some() && access.is_stale());
        tokio::time::advance(soon).await;
        assert!(access.refresh().await.is_ok());
        assert!(idle(&access));

        // A revalidation that finds nothing to do is not held up by a
        // backoff, whether its retry time has passed or not.
        let access = SocketAccess::new(Uuid::new_v4(), lazy_pool());
        for retry_in in [Duration::ZERO, RETRY_MAX] {
            let now = Instant::now();
            access.lock().failure = Some(Failure {
                since: now,
                attempts: 1,
                retry_at: now + retry_in,
            });
            assert!(access.refresh().await.is_ok());
            assert!(idle(&access));
        }
    }

    /// What happens around the two tasks of a socket in one schedule of
    /// [`explore`].
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum World {
        /// The elevation ends without an epoch moving (it ran out, or
        /// another process removed it).
        Expire,
        /// It ends through the server: committed, and the epoch bumped.
        DeElevate,
        /// The session is elevated: committed, and the epoch bumped.
        Elevate,
        /// The user's epoch moves for a change that leaves the elevation as
        /// it is (a role of the user's own).
        Bump,
        /// The database stops answering.
        Down,
        /// It answers again, and the backoff has passed.
        Up,
    }

    /// What the socket's own task does in one schedule of [`explore`].
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Doing {
        /// An `admin_subscribe` after the cooldown of the socket's first.
        Subscribe,
        /// One within that cooldown.
        SubscribeSoon,
        /// `admin_unsubscribe`, then `admin_subscribe` (after the cooldown).
        Resubscribe,
        /// The periodic re-check of everything.
        Backstop,
        /// A DM the user was just added to is recorded (by the forwarder),
        /// then the socket revalidates.
        NewDm,
    }

    /// The error codes an `admin_subscribe` is answered with.
    const NOT_ELEVATED: &str = "ADMIN_NOT_ELEVATED";
    const UNAVAILABLE: &str = "SERVICE_UNAVAILABLE";
    /// Not a code: the socket ended.
    const ENDED: &str = "ended";

    /// What the client gets for an `admin_subscribe` answered `answer`
    /// (`None`: no reply, admin events follow).
    const fn reply_of(answer: AdminSubscription) -> Option<&'static str> {
        match answer {
            AdminSubscription::Allowed => None,
            AdminSubscription::Refused => Some(NOT_ELEVATED),
            AdminSubscription::Undecided => Some(UNAVAILABLE),
            AdminSubscription::Failed => Some(ENDED),
        }
    }

    /// The code of what the handler's `admin_subscribe` arm gave.
    fn reply_code(
        reply: Result<Option<crate::ws::events::ServerEvent>, AccessCheckFailed>,
    ) -> Option<&'static str> {
        use crate::ws::events::ServerEvent;
        match reply {
            Ok(None) => None,
            Ok(Some(ServerEvent::Error { code, .. })) if code == NOT_ELEVATED => Some(NOT_ELEVATED),
            Ok(Some(ServerEvent::Error { code, .. })) if code == UNAVAILABLE => Some(UNAVAILABLE),
            Ok(Some(other)) => panic!("an unexpected reply: {other:?}"),
            Err(AccessCheckFailed) => Some(ENDED),
        }
    }

    /// A scripted read says something about the admin stream only if it
    /// covered it: a read for a new DM alone does not justify an admin
    /// decision.
    #[tokio::test(start_paused = true)]
    async fn a_read_says_nothing_about_the_admin_stream_unless_it_covered_it() {
        let u = Uuid::new_v4();
        let access = admin_socket(u, true).await;
        let said = |access: &SocketAccess| {
            let script = access.script.lock().unwrap();
            let script = script.as_ref().unwrap();
            (script.last_served.is_some(), script.last_admin)
        };
        assert!(access.insert_direct(Uuid::new_v4()));
        assert!(access.refresh().await.is_ok());
        assert_eq!(said(&access), (true, None), "a DM read spoke for admin");
        assert!(access.admin_frame_taken());
        assert!(access.decide_admin().await);
        assert_eq!(said(&access), (true, Some(Some(true))));
    }

    /// Whether the admin stream is allowed is assigned in one place in
    /// `ws/`, the revalidation's `apply`: every assignment of a scope's
    /// `allowed` or `decided` in the module is listed here.
    #[test]
    fn the_admin_decision_has_one_writer() {
        let assignment = regex::Regex::new(r"\b(\w+)\.(allowed|decided)\s*=[^=]").unwrap();
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/ws");
        let mut found = Vec::new();
        let mut dirs = vec![dir.clone()];
        while let Some(next) = dirs.pop() {
            for entry in std::fs::read_dir(&next).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    dirs.push(path);
                } else if path.extension().is_some_and(|e| e == "rs") {
                    let text = std::fs::read_to_string(&path).unwrap();
                    let file = path
                        .strip_prefix(&dir)
                        .unwrap()
                        .to_string_lossy()
                        .into_owned();
                    for hit in assignment.captures_iter(&text) {
                        found.push(format!("{file}: {}.{}", &hit[1], &hit[2]));
                    }
                }
            }
        }
        found.sort();
        assert_eq!(
            found,
            [
                "access.rs: admin.allowed",
                "access.rs: admin.decided",
                "access.rs: voice.allowed",
            ],
            "a new writer of a scope decision"
        );
    }

    /// What one schedule did, in ticks (one per step).
    #[derive(Default)]
    struct Trace {
        /// Per read that covered the admin stream: when it began, and what
        /// it said (`None`: it failed).
        reads: Vec<(u64, Option<bool>)>,
        /// Per admin frame: when it was taken, when it was decided, and
        /// whether it was written.
        frames: Vec<(u64, u64, bool)>,
        answers: Vec<Asked>,
        /// When the client unsubscribed, and when it asked again.
        withdrawn: Option<(u64, Option<u64>)>,
        /// When the user's epoch moved, the elevation ended, it began.
        bumped: Option<u64>,
        ended: Option<u64>,
        began: Option<u64>,
        /// The database failed at some point.
        failed: bool,
        /// The socket task's `admin_subscribe`: how many world events had
        /// happened when it arrived, and when it was answered.
        world_around: Option<(usize, usize)>,
    }

    /// One `admin_subscribe` of a schedule.
    #[derive(Debug, Clone, Copy)]
    struct Asked {
        /// When it arrived.
        at: u64,
        /// When it was answered.
        answered: u64,
        /// The code the client got (`None`: no reply).
        reply: Option<&'static str>,
        /// What has to answer it at once, without a read, if anything: the
        /// socket's own earlier answer at the current user epoch (a refusal;
        /// within the cooldown also a "yes"), or within the cooldown without
        /// one "not decided yet".
        at_once: Option<AdminSubscription>,
    }

    impl Trace {
        /// Whether a read said `said` that began no earlier than `since`
        /// and no later than `until`.
        fn read_between(&self, since: u64, until: u64, said: bool) -> bool {
            self.reads
                .iter()
                .any(|(began, answer)| (since..=until).contains(began) && *answer == Some(said))
        }

        /// What must hold for the frames and answers of every schedule, on
        /// a socket whose first `admin_subscribe` was answered `elevated`
        /// and whose own task was `doing` (`Err`: what does not hold).
        ///
        /// A decision is justified only by a read that began after what
        /// asked for it and no later than the decision: a later read, for
        /// something else, does not make an earlier decision right.
        fn check(&self, elevated: bool, doing: Doing) -> Result<(), &'static str> {
            for &(taken, decided, written) in &self.frames {
                if written {
                    // Only for a stream that was requested when the frame
                    // was taken.
                    if self.withdrawn.is_some_and(|(from, again)| {
                        taken > from && again.is_none_or(|again| taken < again)
                    }) {
                        return Err("a frame was written for a stream nobody asked for");
                    }
                    // On a "yes" whose read began after the frame was
                    // taken, and after the last epoch move its decision
                    // saw.
                    let moved = self.bumped.filter(|bumped| *bumped < decided);
                    let since = moved.map_or(taken, |bumped| bumped.max(taken));
                    if !self.read_between(since, decided, true) {
                        return Err("a frame was written without a read of its own");
                    }
                    continue;
                }
                // Nothing that was owed is lost while the database
                // answers: not while the elevation lasts, and not once it
                // began.
                let owed = if elevated {
                    self.ended.is_none_or(|ended| ended > decided)
                } else {
                    self.began.is_some_and(|began| began < taken)
                };
                if owed && !self.failed && doing != Doing::Resubscribe {
                    return Err("a frame the socket was owed was not written");
                }
            }
            for asked in &self.answers {
                // The read behind the answer began after the subscribe
                // arrived; behind a decision in hand, at the user epoch
                // that held then.
                let epoch = self.bumped.filter(|bumped| *bumped < asked.at);
                let (since, until) = match asked.at_once {
                    Some(answer) => {
                        if (asked.reply, asked.answered) != (reply_of(answer), asked.at) {
                            return Err(
                                "a subscribe that a decision in hand answers waited or read",
                            );
                        }
                        (epoch.unwrap_or(0), asked.at)
                    }
                    None => (asked.at, asked.answered),
                };
                let justified = match asked.reply {
                    None => self.read_between(since, until, true),
                    Some(NOT_ELEVATED) => self.read_between(since, until, false),
                    Some(UNAVAILABLE) => asked.at_once.is_some() || self.failed,
                    Some(_) => self.failed,
                };
                if !justified {
                    return Err("a subscribe was answered without a read to say so");
                }
            }
            Ok(())
        }
    }

    /// The explorer's own judgement, on made-up traces: a read counts for a
    /// decision only if it began within it.
    #[test]
    fn the_explorer_wants_a_read_within_each_decision() {
        let yes = Some(true);
        // A frame taken at 3 is written at 5 on the read from 2; the next
        // frame's read at 8 does not make that right.
        let late = Trace {
            reads: vec![(2, yes), (8, yes)],
            frames: vec![(3, 5, true), (7, 9, true)],
            ..Trace::default()
        };
        assert_eq!(
            late.check(true, Doing::Backstop),
            Err("a frame was written without a read of its own")
        );
        // With a read of its own at 4 it is.
        let own = Trace {
            reads: vec![(2, yes), (4, yes), (8, yes)],
            ..late
        };
        assert_eq!(own.check(true, Doing::Backstop), Ok(()));

        // A frame taken at 5 while the client had unsubscribed (4 to 7) is
        // written on a read of its own all the same.
        let unasked = Trace {
            reads: vec![(5, yes)],
            frames: vec![(5, 6, true)],
            withdrawn: Some((4, Some(7))),
            ..Trace::default()
        };
        assert_eq!(
            unasked.check(true, Doing::Resubscribe),
            Err("a frame was written for a stream nobody asked for")
        );

        // "Allowed" at 3, before the elevation at 5 and the first read that
        // says so at 7.
        let asked = Asked {
            at: 2,
            answered: 3,
            reply: None,
            at_once: None,
        };
        let early = Trace {
            reads: vec![(2, Some(false)), (7, yes)],
            answers: vec![asked],
            bumped: Some(5),
            began: Some(5),
            ..Trace::default()
        };
        assert_eq!(
            early.check(false, Doing::Subscribe),
            Err("a subscribe was answered without a read to say so")
        );
        // Answered at 8, after that read, it is.
        let waited = Trace {
            answers: vec![Asked {
                answered: 8,
                ..asked
            }],
            ..early
        };
        assert_eq!(waited.check(false, Doing::Subscribe), Ok(()));

        // A decision in hand answers at once and without a read: a "no"
        // from 1 at 4, but not across the epoch that moved at 3.
        let cached = Asked {
            at: 4,
            answered: 4,
            reply: Some(NOT_ELEVATED),
            at_once: Some(AdminSubscription::Refused),
        };
        let in_hand = Trace {
            reads: vec![(1, Some(false))],
            answers: vec![cached],
            ..Trace::default()
        };
        assert_eq!(in_hand.check(false, Doing::Subscribe), Ok(()));
        let outdated = Trace {
            reads: vec![(1, Some(false))],
            answers: vec![cached],
            bumped: Some(3),
            ..Trace::default()
        };
        assert_eq!(
            outdated.check(false, Doing::Subscribe),
            Err("a subscribe was answered without a read to say so")
        );
        // It may not read for it either (answered only at 6).
        let slow = Trace {
            reads: vec![(1, Some(false)), (5, Some(false))],
            answers: vec![Asked {
                answered: 6,
                ..cached
            }],
            ..Trace::default()
        };
        assert_eq!(
            slow.check(false, Doing::Subscribe),
            Err("a subscribe that a decision in hand answers waited or read")
        );
    }

    /// What the schedules of one [`explore`] family have in common.
    #[derive(Debug, Clone, Copy)]
    struct Family {
        user: Uuid,
        /// What the socket's first `admin_subscribe` was answered.
        elevated: bool,
        /// Admin frames the sender task writes.
        frames: usize,
        /// What the socket task does meanwhile.
        doing: Doing,
        /// What happens around them, in this order.
        world: &'static [World],
    }

    /// One schedule of three things going on at once on a socket: the sender
    /// task writes its admin frames, the socket task does its part, and the
    /// world changes.
    ///
    /// Each task runs up to its next seam, read or wait for the gate, then
    /// the schedule picks who goes on: `choices[k]` among the steps
    /// possible at the k-th decision (the first one beyond its end). A read
    /// has its answer when it begins and returns when its task is picked
    /// again. Returns how many steps were possible at each decision, after
    /// checking what must hold in every schedule, and the socket task's
    /// `admin_subscribe`, if any: the world events before it arrived and
    /// before it was answered, and the reply code.
    ///
    /// The socket task's `admin_subscribe` goes through the handler's arm
    /// (`handlers::admin_subscribe_reply`), and only reads that covered the
    /// admin stream count as reads behind an admin decision.
    async fn explore(family: Family, choices: &[usize]) -> Explored {
        #[derive(Clone, Copy, PartialEq, Eq)]
        enum Task {
            Ready,
            /// Its read has its answer and has not returned (it holds the
            /// gate).
            Reading,
            /// It waits for the gate.
            Waiting,
            /// It waited for the gate and is the next to have it (the gate
            /// is handed over in order).
            Next,
            Done,
        }
        let Family {
            user: u,
            elevated,
            frames,
            doing,
            world,
        } = family;
        let case = || format!("{family:?} {choices:?}");
        let later = RETRY_MAX + Duration::from_millis(1);

        let access = admin_socket(u, elevated).await;
        let soon = doing == Doing::SubscribeSoon;
        if !soon {
            cooled_down().await;
        }
        let seams = Arc::new(Seams {
            parked: 0.into(),
            yielded: false.into(),
            release: tokio::sync::Semaphore::new(0),
        });
        let script = |change: &mut dyn FnMut(&mut Script)| {
            change(access.script.lock().unwrap().as_mut().unwrap());
        };
        script(&mut |script| script.seams = Some(seams.clone()));
        let tick = std::cell::Cell::new(1_u64);
        // World events that have happened.
        let happened = std::cell::Cell::new(0_usize);
        let trace = std::cell::RefCell::new(Trace {
            // The read behind the socket's first answer.
            reads: vec![(0, Some(elevated))],
            ..Trace::default()
        });

        let sender = async {
            for n in 0..frames {
                if n > 0 {
                    access.seam().await;
                }
                let taken = tick.get();
                let written = access.admit_admin().await;
                trace.borrow_mut().frames.push((taken, tick.get(), written));
            }
        };
        let socket = async {
            if matches!(doing, Doing::Backstop | Doing::NewDm) {
                if doing == Doing::Backstop {
                    access.force_full();
                } else {
                    assert!(access.insert_direct(Uuid::new_v4()));
                }
                let _ = access.refresh().await;
                return;
            }
            if doing == Doing::Resubscribe {
                access.unsubscribe_admin();
                trace.borrow_mut().withdrawn = Some((tick.get(), None));
                access.seam().await;
            }
            let at = tick.get();
            let world_at = happened.get();
            if let Some((_, again)) = trace.borrow_mut().withdrawn.as_mut() {
                *again = Some(at);
            }
            let at_once = match standing_reply(&access) {
                Some(AdminSubscription::Refused) => Some(AdminSubscription::Refused),
                Some(AdminSubscription::Allowed) if soon => Some(AdminSubscription::Allowed),
                _ if soon => Some(AdminSubscription::Undecided),
                _ => None,
            };
            let reply = reply_code(crate::ws::handlers::admin_subscribe_reply(&access).await);
            let mut trace = trace.borrow_mut();
            trace.answers.push(Asked {
                at,
                answered: tick.get(),
                reply,
                at_once,
            });
            trace.world_around = Some((world_at, happened.get()));
        };
        let mut tasks: [std::pin::Pin<Box<dyn std::future::Future<Output = ()> + '_>>; 2] =
            [Box::pin(sender), Box::pin(socket)];
        let mut state = [Task::Ready; 2];
        // Per task: whether the read it is in failed.
        let mut failing = [false; 2];
        let mut widths = Vec::new();
        // A read failed and the backoff has not passed since (time only
        // moves with `Up`).
        let mut backing_off = false;
        while state != [Task::Done; 2] {
            // Who can go on: a task that is not waiting for the gate, and
            // the world (time passes with `Up`: not while a read is in
            // flight, it would time out).
            let reading = state.contains(&Task::Reading);
            let mut steps: Vec<Option<usize>> = (0..2)
                .filter(|i| matches!(state[*i], Task::Ready | Task::Reading | Task::Next))
                .map(Some)
                .collect();
            if happened.get() < world.len() && !(reading && world[happened.get()] == World::Up) {
                steps.push(None);
            }
            assert!(!steps.is_empty(), "nothing can go on: {}", case());
            let pick = choices.get(widths.len()).copied().unwrap_or(0);
            widths.push(steps.len());
            assert!(widths.len() < 100, "no end: {}", case());
            tick.set(tick.get() + 1);
            let now = tick.get();
            match steps[pick] {
                None => {
                    let event = world[happened.get()];
                    happened.set(happened.get() + 1);
                    script(&mut |script| match event {
                        World::Expire | World::DeElevate => script.then = false,
                        World::Elevate => script.then = true,
                        World::Bump => {}
                        World::Down => script.steps = vec![FAILS; 64].into(),
                        World::Up => script.steps.clear(),
                    });
                    if matches!(event, World::DeElevate | World::Elevate | World::Bump) {
                        registry().bump_user(u);
                        trace.borrow_mut().bumped = Some(now);
                    }
                    if event == World::Up {
                        backing_off = false;
                    }
                    match event {
                        World::Expire | World::DeElevate => trace.borrow_mut().ended = Some(now),
                        World::Elevate => trace.borrow_mut().began = Some(now),
                        World::Bump => {}
                        World::Down => trace.borrow_mut().failed = true,
                        World::Up => tokio::time::advance(later).await,
                    }
                }
                Some(i) => {
                    let was_reading = state[i] == Task::Reading;
                    if was_reading {
                        seams.release.add_permits(1);
                    }
                    let gate_taken = matches!(state[1 - i], Task::Reading | Task::Next);
                    state[i] = loop {
                        seams.yielded.store(false, Ordering::SeqCst);
                        if futures::poll!(tasks[i].as_mut()).is_ready() {
                            break Task::Done;
                        }
                        if seams.yielded.load(Ordering::SeqCst) {
                            break Task::Ready;
                        }
                        if gate_taken {
                            break Task::Waiting;
                        }
                        if seams.parked.load(Ordering::SeqCst) == 1 {
                            let mut served = (None, None);
                            script(&mut |script| served = (script.last_served, script.last_admin));
                            failing[i] = served.0.expect("a read was served").outcome.is_none();
                            // Only a read that covered the admin stream
                            // can be behind an admin decision.
                            if let Some(said) = served.1 {
                                trace.borrow_mut().reads.push((now, said));
                            }
                            break Task::Reading;
                        }
                        // It waits for one of the process-wide permits,
                        // which other tests hold for a moment.
                        std::thread::yield_now();
                    };
                    if state[i] != Task::Reading && state[1 - i] == Task::Waiting {
                        state[1 - i] = Task::Next;
                    }
                    // Its read has returned; a new one may have begun.
                    let began_now = state[i] == Task::Reading;
                    if was_reading && failing[i] {
                        backing_off = true;
                    }
                    assert!(
                        !(began_now && backing_off),
                        "a read during the backoff: {}",
                        case()
                    );
                }
            }
            assert!(
                access.retry_at().is_none() || access.is_stale(),
                "a retry time with nothing to retry: {}",
                case()
            );
        }
        drop(tasks);

        let trace = trace.into_inner();
        if let Err(wrong) = trace.check(elevated, doing) {
            panic!("{wrong}: {}", case());
        }
        let resync = access.take_resync();
        if !trace.failed {
            assert!(
                !dropped(&access) && !resync,
                "a frame was dropped undecided: {}",
                case()
            );
        }
        if !elevated && trace.began.is_none() {
            assert!(!resync, "a refused socket was told to resync: {}", case());
        }

        // Whatever happened, the request is still there: the next
        // elevation brings the stream back without the client asking.
        script(&mut |script| {
            script.seams = None;
            script.steps.clear();
            script.then = true;
        });
        tokio::time::advance(later).await;
        registry().bump_user(u);
        assert!(
            access.admit_admin().await,
            "access came back, delivery did not: {}",
            case()
        );
        Explored {
            widths,
            subscribe: trace.world_around.map(|(arrived, answered)| {
                let reply = trace.answers.last().expect("answered").reply;
                (arrived, answered, reply)
            }),
        }
    }

    /// What [`explore`] returns for one schedule.
    struct Explored {
        widths: Vec<usize>,
        subscribe: Option<(usize, usize, Option<&'static str>)>,
    }

    /// Every schedule of one family: how many, and what each one's
    /// `admin_subscribe` saw and got ([`Explored::subscribe`]).
    async fn explore_all(
        family: Family,
    ) -> (usize, Vec<Option<(usize, usize, Option<&'static str>)>>) {
        let mut choices: Vec<usize> = Vec::new();
        let mut subscribes = Vec::new();
        loop {
            let Explored { widths, subscribe } = explore(family, &choices).await;
            subscribes.push(subscribe);
            // The next schedule: at the last decision that had another
            // step, take it.
            choices.resize(widths.len(), 0);
            while let Some(last) = choices.pop() {
                if last + 1 < widths[choices.len()] {
                    choices.push(last + 1);
                    break;
                }
            }
            if choices.is_empty() {
                return (subscribes.len(), subscribes);
            }
        }
    }

    /// Every interleaving of the sender task (admin frames, each one taken,
    /// pinned and decided) with the socket task (an `admin_subscribe`
    /// through the handler's arm, an unsubscribe and subscribe, or the
    /// periodic re-check), while at any point between two steps the
    /// elevation ends without an epoch moving, ends or begins with a bump,
    /// the epoch moves without a change, or the database fails and
    /// recovers.
    ///
    /// In all of them: a frame is written only on a read that began after
    /// it was taken (and after the last bump), a subscribe is answered only
    /// by a read that began after it arrived or by the socket's own earlier
    /// answer, nothing owed is lost, a refused socket is never told to
    /// resynchronise, the backoff never outlives the scopes that need it,
    /// and the stream comes back with the next elevation.
    ///
    /// And pairwise: the same schedule of the socket task and the world with
    /// and without admin frames gives the same reply code, for a refused
    /// socket whose epoch moves and for an admin whose elevation runs out
    /// (compared where no world event happened while the subscribe was
    /// under way).
    #[tokio::test(start_paused = true)]
    async fn every_interleaving_of_admin_frames_and_the_socket_task() {
        use Doing::{Backstop, NewDm, Resubscribe, Subscribe, SubscribeSoon};
        use World::{Bump, DeElevate, Down, Elevate, Expire, Up};
        let user = Uuid::new_v4();
        let family = |elevated, frames, doing, world| Family {
            user,
            elevated,
            frames,
            doing,
            world,
        };
        let families = [
            family(true, 2, Subscribe, &[Expire]),
            family(true, 2, SubscribeSoon, &[Expire]),
            family(true, 2, Resubscribe, &[Expire]),
            family(true, 2, Backstop, &[Expire]),
            family(true, 2, NewDm, &[Expire]),
            family(true, 2, Subscribe, &[DeElevate]),
            family(true, 2, Resubscribe, &[DeElevate]),
            family(true, 2, Backstop, &[DeElevate]),
            family(true, 2, SubscribeSoon, &[DeElevate]),
            family(false, 2, Subscribe, &[Elevate]),
            family(false, 2, SubscribeSoon, &[Elevate]),
            family(false, 2, Resubscribe, &[Elevate]),
            family(true, 1, Subscribe, &[Down, Up]),
            family(true, 1, Resubscribe, &[Down, Up]),
            family(true, 1, Backstop, &[Down, Up]),
            family(true, 1, NewDm, &[Down, Up]),
            family(false, 2, Resubscribe, &[Down, Up]),
            family(false, 2, Backstop, &[Down, Up]),
            // Compared with and without frames below.
            family(true, 0, Subscribe, &[Expire]),
            family(true, 0, SubscribeSoon, &[Expire]),
            family(true, 0, Resubscribe, &[Expire]),
            family(false, 2, Subscribe, &[Bump]),
            family(false, 2, SubscribeSoon, &[Bump]),
            family(false, 2, Resubscribe, &[Bump]),
            family(false, 0, Subscribe, &[Bump]),
            family(false, 0, SubscribeSoon, &[Bump]),
            family(false, 0, Resubscribe, &[Bump]),
        ];
        let mut schedules = 0_usize;
        let mut explored = Vec::new();
        for family in families {
            let (count, subscribes) = explore_all(family).await;
            schedules += count;
            explored.push((family, subscribes));
        }
        let mut pairs = 0;
        for (alone, quiet) in explored.iter().filter(|(f, _)| f.frames == 0) {
            let (_, busy) = explored
                .iter()
                .find(|(f, _)| {
                    f.frames > 0
                        && (f.elevated, f.doing, f.world)
                            == (alone.elevated, alone.doing, alone.world)
                })
                .expect("a family with frames to compare with");
            // Without frames: the reply per number of world events before
            // a subscribe that no world event overtook.
            let mut without = HashMap::new();
            for &(arrived, answered, reply) in quiet.iter().flatten() {
                if arrived == answered {
                    let other = without.insert(arrived, reply);
                    assert!(
                        other.is_none_or(|other| other == reply),
                        "without frames the reply differs between schedules: {alone:?}"
                    );
                }
            }
            for &(arrived, answered, reply) in busy.iter().flatten() {
                if arrived == answered {
                    let expected = without
                        .get(&arrived)
                        .unwrap_or_else(|| panic!("nothing to compare with: {alone:?}"));
                    assert_eq!(
                        reply, *expected,
                        "admin frames changed the reply: {alone:?}, world events before: {arrived}"
                    );
                    pairs += 1;
                }
            }
        }
        assert!(pairs > 0, "nothing was compared");
        assert!(schedules > 100_000, "only {schedules} schedules explored");
    }

    /// Every SQL statement that writes a table feeding channel access, per
    /// source file, with what keeps open connections in line. A new writer
    /// changes a count and fails [`every_access_writer_is_audited`]: make sure
    /// it calls `access_changed` after its commit (or say why it need not),
    /// then update this list.
    const AUDITED_WRITERS: &[(&str, usize, &str)] = &[
        ("admin/queries.rs", 10, "elevation delete: de-elevate handler calls access_changed; guild suspension: not an access input; guild delete: access_changed; ban (two inserts): bump_after(UserBanned) at the transaction's commit; unban: a grant for an account whose sessions ended with the ban (no socket); account delete: bump_after(AccountDeleted) at the commit"),
        ("auth/queries.rs", 1, "first-admin bootstrap: the role for the account being registered (no socket yet); admin events also need an elevation, which bumps"),
        ("auth/registration.rs", 1, "sweep of accounts whose registration bound nothing within 30 minutes: they never had a session, so no socket"),
        ("auth/revocation.rs", 1, "elevation ends with the session: the socket closes (1008)"),
        ("chat/messages.rs", 7, "test fixtures"),
        ("chat/queries.rs", 9, "channel create, DM rename/icon: no access change; DM and group-DM create (after the block check): a grant into a channel that did not exist, so no socket can hold it as requested, and every participant's sockets record it undecided on dm_created and confirm by a read (participants are only ever added at creation); DM leave: access_changed; overrides: access_changed"),
        ("cli/recovery.rs", 2, "CLI admin demote runs in another process: every admin frame re-reads the elevation"),
        ("db/queries.rs", 3, "channel create/update: no access change; channel delete: access_changed"),
        ("db/tests.rs", 13, "test fixtures"),
        ("discovery/queries.rs", 1, "discovery join: access_changed (grant)"),
        ("governance/queries.rs", 1, "scheduled account deletion: bump_after_if(AccountDeleted) in governance/deletion.rs"),
        ("guild/core.rs", 1, "guild settings (name, icon, ...): not an access input"),
        ("guild/screening.rs", 3, "screening disabled (promotes pending) and accepted: access_changed (grant); enabling never demotes"),
        ("guild/settings.rs", 1, "guild settings: not an access input"),
        ("guild/queries/core.rs", 11, "bot install: a grant for a bot (no chat socket; bot visibility is read per event at dispatch); guild create (no sockets yet), banner, position, prompt: not access inputs; guild delete, member delete (kick/leave): access_changed; bot uninstall removes the bot's roles in its transaction: bump_after_if(MemberRolesChanged) in guild/bots.rs (bots hold no chat socket; their visibility is read from the database for every event)"),
        ("guild/queries/invites.rs", 1, "invite join: access_changed (grant); admin-created account has no sockets"),
        ("guild/queries/reaction_roles.rs", 2, "reaction role add/remove/swap: access_changed"),
        ("guild/queries/roles.rs", 5, "role create (no members): none; role update with permissions, role delete, assign, remove: access_changed; assigning to an installed bot is the same handler and write"),
        ("permissions/queries.rs", 12, "create_elevated_session: the elevate handler commits through bump_after (AdminAccessChanged); the other helpers have no production caller (admin grant/revoke, role CRUD, member roles); role creation is grant only"),
        ("system_account.rs", 2, "Kaiku DM creation for a new user: grant only"),
        ("voice/ws_handler_test.rs", 9, "test fixtures"),
        ("ws/bot_gateway.rs", 8, "test fixtures"),
    ];

    #[test]
    fn every_access_writer_is_audited() {
        let writes = regex::Regex::new(
            r"(?is)\b(insert\s+into|update|delete\s+from)\s+(guild_members|guild_member_roles|guild_roles|channel_overrides|channels|dm_participants|system_admins|elevated_sessions|guilds|global_bans|guild_bot_installations)\b|\bdelete\s+from\s+users\b",
        )
        .unwrap();
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut found: std::collections::BTreeMap<String, usize> =
            std::collections::BTreeMap::new();
        let mut dirs = vec![root.clone()];
        while let Some(dir) = dirs.pop() {
            for entry in std::fs::read_dir(&dir).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    dirs.push(path);
                } else if path.extension().is_some_and(|e| e == "rs") {
                    let text = std::fs::read_to_string(&path).unwrap();
                    let n = writes.find_iter(&text).count();
                    if n > 0 {
                        let rel = path
                            .strip_prefix(&root)
                            .unwrap()
                            .to_string_lossy()
                            .replace('\\', "/");
                        found.insert(rel, n);
                    }
                }
            }
        }
        let audited: std::collections::BTreeMap<String, usize> = AUDITED_WRITERS
            .iter()
            .map(|(file, n, _)| ((*file).to_string(), *n))
            .collect();
        assert_eq!(
            found, audited,
            "writers of access tables changed: audit them (access_changed after commit?) and update AUDITED_WRITERS"
        );
    }
}
