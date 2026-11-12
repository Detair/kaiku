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
//!   reaction roles), group-DM membership, admin elevation. Affects all of that user's channels and
//!   the admin event subscription.
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
//! | `Guild` (a server's own feed) | user scope + that server's scope | [`SocketAccess::admit_guild`]: member |
//! | `Voice` (everything of a voice room) | the voice scope: user epoch + the room's server epoch as of the join check | [`SocketAccess::admit_voice`]: the socket owns the peer and voice access holds |
//! | `Admin` | user epoch | the admin flag, re-checked when the epoch moved |
//!
//! A scope is **current** (computed at the epoch that still holds), **stale**
//! (the epoch moved, or it was invalidated) or **failing** (see below). A
//! frame of a current scope is decided from memory: atomic loads and a
//! lookup. A frame of a stale scope waits for a revalidation. So after a bump
//! nothing of that scope is written on old authorization: neither events
//! published after the commit nor events that were already queued.
//!
//! `subscribe` uses the same epochs: it reads them, checks access and records
//! the result only if they are unchanged, otherwise it checks again.
//!
//! # Revalidation cost
//!
//! One revalidation ([`SocketAccess::refresh`]) covers every scope of the
//! socket that is not current, with batched reads: the account's ban state,
//! one [`viewable_and_existing_channels`] call for the stale servers'
//! channels, the user's memberships, after a user bump one query for the
//! requested DMs, and the voice check if the voice scope is stale. At most
//! [`REVALIDATION_CONCURRENCY`] run at once process-wide. Bumps arriving
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
//! for [`FAILURE_CLOSE_AFTER`] does the socket close with 1011 regardless.
//! The forwarder never drops for a stale or failing scope; the writer does,
//! in one place ([`SocketAccess`]'s decision loop).
//!
//! # Bounds
//!
//! A socket keeps at most [`MAX_REQUESTED_CHANNELS`] requested channels, one
//! scope per server with a requested channel or a feed event, and one voice
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

/// Most channels one socket can have requested at a time.
///
/// A `subscribe` beyond it is refused (`TOO_MANY_SUBSCRIPTIONS`); the client
/// unsubscribes from something first. Bounds the per-socket state and the
/// size of one revalidation.
pub const MAX_REQUESTED_CHANNELS: usize = 2_000;

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
    pub fn forget_guild(&self, guild_id: Uuid) {
        self.guilds.remove(&guild_id);
    }

    /// Drop the cell of a deleted account (its sockets are closed).
    pub fn forget_user(&self, user_id: Uuid) {
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
    /// The user's admin elevation ended (admin event subscription).
    AdminAccessChanged { user_id: Uuid },
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
            | Self::AdminAccessChanged { user_id } => Some((NoticeScope::User, user_id)),
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
        if let Self::GuildDeleted { guild_id } = self {
            registry().forget_guild(guild_id);
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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
    /// Backoff after failed revalidations (the database is shared, so one
    /// for the socket).
    failure: Option<Failure>,
    /// A scope recovered after dropping events: the socket must make the
    /// client resynchronise (close 1011, see `ws::handlers`).
    resync: bool,
    /// Counts explicit invalidations; a revalidation planned before one
    /// must not mark its scopes checked.
    invalidations: u64,
}

/// What an event's delivery depends on.
#[derive(Clone, Copy)]
enum Target {
    /// A channel's events; a server's own feed depends on the same scopes
    /// as its channels (`ChannelScope::Guild`).
    Channel(ChannelScope),
    Voice,
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
    /// Wakes the socket loop: a revalidation failed (retry), a scope needs
    /// one, the client must resynchronise, or the voice peer lost access.
    attention: Notify,
    /// The user epoch at which the admin event flag was last checked.
    admin_seen: AtomicU64,
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
    /// The socket already has [`MAX_REQUESTED_CHANNELS`] channels.
    Full,
}

impl SocketAccess {
    /// A socket of `user_id` with nothing requested.
    pub fn new(user_id: Uuid, db: PgPool) -> Arc<Self> {
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
                failure: None,
                resync: false,
                invalidations: 0,
            }),
            refresh_gate: tokio::sync::Mutex::new(()),
            attention: Notify::new(),
            admin_seen: AtomicU64::new(now),
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
        }
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

    /// The decision loop shared by every kind of event.
    ///
    /// `look` names what the event depends on and, if that is current, the
    /// answer: `None` = refused outright (not requested, no peer),
    /// `Some((target, Some(true)))` = allowed when `target` is current,
    /// `Some((target, None))` = not allowed as computed.
    async fn decide(
        &self,
        outbound: bool,
        mut look: impl FnMut(&mut Inner) -> Option<(Target, Option<bool>)>,
    ) -> bool {
        let mut last = None;
        for _ in 0..4 {
            {
                let mut inner = self.lock();
                let Some((target, answer)) = look(&mut inner) else {
                    return false;
                };
                last = Some(target);
                match self.phase(&inner, target) {
                    Phase::Current => return answer.unwrap_or(false),
                    // No attempt can be made now: dropped without a decision.
                    Phase::Stale | Phase::Failing if Self::in_backoff(&inner) => {
                        if outbound {
                            self.record_drop(&mut inner, target);
                        }
                        return false;
                    }
                    Phase::Stale | Phase::Failing => {}
                }
            }
            if self.refresh().await.is_err() {
                break;
            }
        }
        // The revalidation failed, or epochs kept moving under four of them.
        if let (true, Some(target)) = (outbound, last) {
            let mut inner = self.lock();
            if self.phase(&inner, target) != Phase::Current {
                self.record_drop(&mut inner, target);
            }
        }
        false
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

    /// Whether `channel_id` may be added to the requested set (it is in it
    /// already, or the set is below [`MAX_REQUESTED_CHANNELS`]).
    fn has_room(inner: &Inner, channel_id: Uuid) -> bool {
        inner.requested.len() < MAX_REQUESTED_CHANNELS || inner.requested.contains_key(&channel_id)
    }

    /// Whether a `subscribe` of `channel_id` can be recorded at all (asked
    /// before any database work).
    pub fn has_room_for(&self, channel_id: Uuid) -> bool {
        Self::has_room(&self.lock(), channel_id)
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
        if !Self::has_room(&inner, channel_id) {
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
        inner.requested.insert(channel_id, scope);
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
        if !Self::has_room(&inner, channel_id) {
            return false;
        }
        inner.requested.insert(channel_id, scope);
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
        inner.requested.remove(&channel_id);
        inner.allowed.remove(&channel_id);
    }

    /// A DM the user was just added to (`dm_created`): requested, and
    /// allowed only after a revalidation confirms participation (the user
    /// may have left again before this socket processed the event).
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
        self.user_phase(&inner) != Phase::Current
            || inner.guilds.values().any(|gs| gs.phase() != Phase::Current)
            || inner.voice.as_ref().is_some_and(|v| {
                self.voice_phase(v) != Phase::Current || (!v.allowed && !v.denial_taken)
            })
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
    }

    /// When the next retry of a failed revalidation is due.
    pub fn retry_at(&self) -> Option<Instant> {
        self.lock().failure.as_ref().map(|f| f.retry_at)
    }

    /// How long revalidations have been failing, if they are.
    pub fn failing_for(&self) -> Option<Duration> {
        self.lock().failure.as_ref().map(|f| f.since.elapsed())
    }

    /// Resolves when the socket loop should look at this socket's access
    /// (see the `attention` field).
    pub async fn attention(&self) {
        self.attention.notified().await;
    }

    /// Whether the admin event flag was checked at the current user epoch.
    pub fn admin_fresh(&self) -> bool {
        self.admin_seen.load(Ordering::SeqCst) == self.user_epoch.load(Ordering::SeqCst)
    }

    /// The current user epoch (read before an admin check).
    pub fn user_epoch_now(&self) -> u64 {
        self.user_epoch.load(Ordering::SeqCst)
    }

    /// Record that the admin flag was checked at user epoch `epoch`.
    pub fn set_admin_seen(&self, epoch: u64) {
        self.admin_seen.store(epoch, Ordering::SeqCst);
    }

    /// Revalidate every scope that is not current — all of them after a user
    /// bump or [`Self::force_full`] — with batched reads. Concurrent calls
    /// are serialised; bumps during a run make the next run.
    ///
    /// Nothing is consumed before the read succeeded: dropping this future
    /// leaves every scope as it was.
    pub async fn refresh(&self) -> Result<(), AccessCheckFailed> {
        let _gate = self.refresh_gate.lock().await;
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
        let _permit = REVALIDATION_PERMITS.acquire().await;
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
        let inner = self.lock();
        if Self::in_backoff(&inner) {
            return Planned::Backoff;
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
        if !full && guild_targets.is_empty() && voice.is_none() {
            return Planned::Nothing;
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
        })
    }

    async fn read(&self, plan: &Plan) -> Result<ReadResult, String> {
        if crate::ws::test_hooks::access_read_fails(self.user_id) {
            return Err(sqlx::Error::PoolTimedOut.to_string());
        }
        // A banned account keeps its rows (membership, roles, DMs) but may
        // see nothing: its sessions end with the ban, and until each socket
        // has closed, nothing more is written to it.
        let banned: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM global_bans gb
                            WHERE gb.user_id = $1
                              AND (gb.expires_at IS NULL OR gb.expires_at > NOW()))",
        )
        .bind(self.user_id)
        .fetch_one(&self.db)
        .await
        .map_err(|e| e.to_string())?;
        if banned {
            return Ok(ReadResult {
                viewable: HashSet::new(),
                // Nothing is known to be deleted: the requests stay.
                existing: plan.guild_channels.iter().map(|(_, c)| *c).collect(),
                direct_ok: HashSet::new(),
                direct_existing: plan.direct.iter().copied().collect(),
                member_of: Some(HashSet::new()),
                voice_ok: plan.voice.as_ref().map(|_| false),
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
        Ok(ReadResult {
            viewable,
            existing,
            direct_ok,
            direct_existing,
            member_of,
            voice_ok,
        })
    }

    fn apply(&self, plan: &Plan, found: ReadResult) {
        let mut guard = self.lock();
        let inner = &mut *guard;
        let mut lost = Vec::new();
        let mut regained = Vec::new();
        // allowed = requested ∩ existing ∩ permitted; a deleted channel can
        // never come back, so it also leaves requested.
        for &(g, channel) in &plan.guild_channels {
            // Only channels still requested in the same scope (an
            // unsubscribe or a fresh subscribe during the read wins).
            if inner.requested.get(&channel) == Some(&ChannelScope::Guild(g)) {
                if !found.existing.contains(&channel) {
                    inner.requested.remove(&channel);
                }
                let ok = found.viewable.contains(&channel);
                set_allowed(&mut inner.allowed, channel, ok, &mut lost, &mut regained);
            }
        }
        for &channel in &plan.direct {
            if inner.requested.get(&channel) == Some(&ChannelScope::Direct) {
                if !found.direct_existing.contains(&channel) {
                    inner.requested.remove(&channel);
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
                inner.user.seen = inner.user.seen.max(plan.user_now);
            }
        }
        for &(g, now) in &plan.guild_targets {
            if let Some(gs) = inner.guilds.get_mut(&g) {
                resync |= gs.state.settle(fresh);
                if fresh {
                    gs.seen = gs.seen.max(now);
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
                resync |= voice.state.settle(fresh);
                if fresh {
                    voice.seen_user = voice.seen_user.max(planned.user_now);
                    voice.seen_guild = voice.seen_guild.max(planned.guild_now);
                }
                voice.allowed = ok;
                voice_denied = !ok && !voice.denial_taken;
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
        inner.failure = None;
        if resync {
            inner.resync = true;
        }
        drop(guard);
        if resync || voice_denied || !fresh {
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
        registry().forget_user(u);
        assert!(!registry().users.contains_key(&u));
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

    /// Invariant A: a write whose outcome is not known (the request was
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

    /// Invariant F: the requested set is bounded.
    #[tokio::test]
    async fn requested_channels_are_capped() {
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
        assert!(!access.has_room_for(extra));
        assert_eq!(
            access.commit_subscribe(extra, scope, access.snapshot(scope), true),
            SubscribeCommit::Full
        );
        assert!(!access.record_unchecked(extra, scope));
        assert!(!access.insert_direct(extra));
        assert_eq!(access.requested_len(), MAX_REQUESTED_CHANNELS);
        assert!(!access.is_requested(extra));
        // A channel that is requested already can still be decided again...
        assert_eq!(
            access.commit_subscribe(first, scope, access.snapshot(scope), true),
            SubscribeCommit::Recorded
        );
        // ...and unsubscribing makes room.
        access.unsubscribe(first);
        assert!(access.has_room_for(extra));
    }

    fn all_found(channels: &[Uuid]) -> ReadResult {
        ReadResult {
            viewable: channels.iter().copied().collect(),
            existing: channels.iter().copied().collect(),
            direct_ok: HashSet::new(),
            direct_existing: HashSet::new(),
            member_of: Some(HashSet::new()),
            voice_ok: None,
        }
    }

    fn dropped(access: &SocketAccess) -> bool {
        let inner = access.lock();
        let is = |s: ScopeState| s == ScopeState::Failing { dropped: true };
        is(inner.user.state)
            || inner.guilds.values().any(|gs| is(gs.state))
            || inner.voice.as_ref().is_some_and(|v| is(v.state))
    }

    /// Invariant D: a forced (backstop) revalidation that fails **without**
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

    /// Invariant B (voice): room content needs an owned peer **and** current
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

    /// Every SQL statement that writes a table feeding channel access, per
    /// source file, with what keeps open connections in line. A new writer
    /// changes a count and fails [`every_access_writer_is_audited`]: make sure
    /// it calls `access_changed` after its commit (or say why it need not),
    /// then update this list.
    const AUDITED_WRITERS: &[(&str, usize, &str)] = &[
        ("admin/queries.rs", 6, "elevation delete: de-elevate handler calls access_changed; guild suspension: not an access input; guild delete: access_changed"),
        ("auth/queries.rs", 1, "first-admin bootstrap: grant, admin flag re-checked on subscribe"),
        ("auth/revocation.rs", 1, "elevation ends with the session: the socket closes (1008)"),
        ("chat/messages.rs", 6, "test fixtures"),
        ("chat/queries.rs", 9, "channel create, DM create (new participants auto-subscribe), DM rename/icon: no access change; DM leave: access_changed; overrides: access_changed"),
        ("cli/recovery.rs", 2, "CLI admin demote runs in another process: socket backstop (<= 60 s)"),
        ("db/queries.rs", 3, "channel create/update: no access change; channel delete: access_changed"),
        ("db/tests.rs", 13, "test fixtures"),
        ("discovery/queries.rs", 1, "discovery join: access_changed (grant)"),
        ("guild/core.rs", 1, "guild settings (name, icon, ...): not an access input"),
        ("guild/screening.rs", 3, "screening disabled (promotes pending) and accepted: access_changed (grant); enabling never demotes"),
        ("guild/settings.rs", 1, "guild settings: not an access input"),
        ("guild/queries/core.rs", 8, "guild create (no sockets yet), banner, position, prompt: not access inputs; guild delete, member delete (kick/leave): access_changed"),
        ("guild/queries/invites.rs", 1, "invite join: access_changed (grant); admin-created account has no sockets"),
        ("guild/queries/reaction_roles.rs", 2, "reaction role add/remove/swap: access_changed"),
        ("guild/queries/roles.rs", 5, "role create (no members): none; role update with permissions, role delete, assign, remove: access_changed"),
        ("permissions/queries.rs", 12, "helpers without a production caller (admin grant/revoke, elevation, role CRUD, member roles); role creation is grant only"),
        ("system_account.rs", 2, "Kaiku DM creation for a new user: grant only"),
        ("voice/ws_handler_test.rs", 9, "test fixtures"),
        ("ws/bot_gateway.rs", 2, "test fixtures"),
    ];

    #[test]
    fn every_access_writer_is_audited() {
        let writes = regex::Regex::new(
            r"(?is)\b(insert\s+into|update|delete\s+from)\s+(guild_members|guild_member_roles|guild_roles|channel_overrides|channels|dm_participants|system_admins|elevated_sessions|guilds)\b",
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
