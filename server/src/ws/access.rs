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
//! Every change calls [`access_changed`] after its transaction committed. It
//! bumps the epoch synchronously, before its first `.await`, so the bump is
//! in place before the triggering request answers and before anything that
//! request publishes afterwards.
//!
//! Each socket ([`SocketAccess`]) remembers, per server and for its user, the
//! epoch at which its allowed set was last computed. Every channel-scoped
//! event is admitted when its frame is **written** to the socket (the sender
//! task), not when it is queued: if the relevant epoch moved, the socket
//! revalidates first and only then decides. In steady state that is two
//! atomic loads and a set lookup per event, no database work. After a bump
//! nothing of that scope is written on stale authorization: neither events
//! published after the commit nor events that were already queued.
//!
//! `subscribe` uses the same epochs: it reads them, checks access and adds the
//! channel only if they are unchanged, otherwise it checks again.
//!
//! # Revalidation cost
//!
//! A revalidation is scoped (a guild bump re-checks only that server's
//! channels) and batched: one [`filter_viewable_channels_in_guilds`] call for
//! all server channels (at most 4 SQL statements) plus, for a user bump, one
//! query for the requested DMs. Bumps arriving while a revalidation runs
//! coalesce into one follow-up. Idle sockets are woken by the bump and
//! revalidate after [`WAKE_DEBOUNCE`], so voice and the admin flag do not
//! wait for the next event. A periodic backstop (about every minute, with
//! jitter) re-runs the full revalidation.
//!
//! # Failure
//!
//! If a revalidation fails (database error or timeout), the stale scopes'
//! channels leave the allowed set (fail closed) and the revalidation is
//! retried with backoff; the socket stays open. Only after failing for
//! [`FAILURE_CLOSE_AFTER`] does the socket close with 1011.
//!
//! # Several nodes
//!
//! [`access_changed`] also publishes an [`AccessNotice`] on
//! [`NOTICE_CHANNEL`] (detached, short timeout). Nothing subscribes to it
//! today; a multi-node setup would run one subscriber per process that bumps
//! the local registry for notices whose `origin` is another process.
//!
//! [`filter_viewable_channels_in_guilds`]: crate::permissions::filter_viewable_channels_in_guilds

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

impl AccessRegistry {
    /// The epoch cell of `guild_id` (created at 0).
    pub fn guild_epoch(&self, guild_id: Uuid) -> Arc<AtomicU64> {
        self.guilds.entry(guild_id).or_default().clone()
    }

    /// The epoch cell of `user_id` (created at 0).
    pub fn user_epoch(&self, user_id: Uuid) -> Arc<AtomicU64> {
        self.users.entry(user_id).or_default().clone()
    }

    /// Current epochs of `user_id` and (if given) `guild_id`.
    pub fn snapshot(&self, user_id: Uuid, guild_id: Option<Uuid>) -> EpochSnapshot {
        EpochSnapshot {
            user: self.user_epoch(user_id).load(Ordering::SeqCst),
            guild: guild_id.map(|g| self.guild_epoch(g).load(Ordering::SeqCst)),
        }
    }

    /// Advance `guild_id`'s epoch and wake the sockets.
    pub fn bump_guild(&self, guild_id: Uuid) {
        self.guild_epoch(guild_id).fetch_add(1, Ordering::SeqCst);
        self.wake.send_modify(|n| *n = n.wrapping_add(1));
    }

    /// Advance `user_id`'s epoch and wake the sockets.
    pub fn bump_user(&self, user_id: Uuid) {
        self.user_epoch(user_id).fetch_add(1, Ordering::SeqCst);
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

    /// Bump this change's epoch in this process (synchronous).
    pub fn bump(self) {
        match self.scope() {
            Some((NoticeScope::Guild, id)) => registry().bump_guild(id),
            Some((NoticeScope::User, id)) => registry().bump_user(id),
            None => {}
        }
    }
}

/// Apply `change` to open connections. Call it after the change committed.
///
/// The epoch is bumped before the first `.await`. The cross-node notice is
/// published in a detached task, so a Valkey outage never delays the
/// request. Voice participants who lost access are removed before this
/// returns (rooms are in this process). It never fails the request.
#[tracing::instrument(skip(state))]
pub async fn access_changed(state: &AppState, change: AccessChange) {
    change.bump();
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

struct GuildSeen {
    epoch: Arc<AtomicU64>,
    seen: u64,
}

struct Failure {
    since: Instant,
    attempts: u32,
    retry_at: Instant,
}

struct Inner {
    requested: HashMap<Uuid, ChannelScope>,
    allowed: HashSet<Uuid>,
    seen_user: u64,
    guilds: HashMap<Uuid, GuildSeen>,
    failure: Option<Failure>,
    force_full: bool,
}

/// One socket's requested and allowed channels and the epochs they were
/// computed at (see the module docs).
pub struct SocketAccess {
    user_id: Uuid,
    db: PgPool,
    user_epoch: Arc<AtomicU64>,
    inner: Mutex<Inner>,
    /// Serialises revalidations (the only lock held across `.await`).
    refresh_gate: tokio::sync::Mutex<()>,
    /// Wakes the socket loop when a revalidation failed (to schedule its
    /// retry).
    attention: Notify,
    /// The user epoch at which the admin event flag was last checked.
    admin_seen: AtomicU64,
}

struct Plan {
    full: bool,
    forced: bool,
    user_now: u64,
    guild_targets: Vec<(Uuid, u64)>,
    guild_channels: Vec<(Uuid, Uuid)>,
    direct: Vec<Uuid>,
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
                seen_user: now,
                guilds: HashMap::new(),
                failure: None,
                force_full: false,
            }),
            refresh_gate: tokio::sync::Mutex::new(()),
            attention: Notify::new(),
            admin_seen: AtomicU64::new(now),
        })
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn fresh(&self, inner: &Inner, scope: ChannelScope) -> bool {
        if inner.seen_user != self.user_epoch.load(Ordering::SeqCst) {
            return false;
        }
        match scope {
            ChannelScope::Direct => true,
            ChannelScope::Guild(g) => inner
                .guilds
                .get(&g)
                .is_some_and(|gs| gs.seen == gs.epoch.load(Ordering::SeqCst)),
        }
    }

    /// Whether the client asked for `channel_id` (and has not unsubscribed).
    pub fn is_requested(&self, channel_id: Uuid) -> bool {
        self.lock().requested.contains_key(&channel_id)
    }

    /// Whether `channel_id` is currently in the allowed set (as last
    /// computed; [`Self::admit`] is the delivery decision).
    pub fn is_allowed(&self, channel_id: Uuid) -> bool {
        self.lock().allowed.contains(&channel_id)
    }

    /// Cheap pre-filter for the forwarder: `false` if the event can be
    /// dropped now (not requested, or not allowed on current epochs); `true`
    /// if it must be queued for the write-time decision.
    pub fn may_forward(&self, channel_id: Uuid) -> bool {
        let inner = self.lock();
        let Some(&scope) = inner.requested.get(&channel_id) else {
            return false;
        };
        !self.fresh(&inner, scope) || inner.allowed.contains(&channel_id)
    }

    /// Delivery decision for an event of `channel_id`, taken when its frame
    /// is written: revalidates first if a relevant epoch moved. Fails closed.
    pub async fn admit(&self, channel_id: Uuid) -> bool {
        for _ in 0..4 {
            {
                let inner = self.lock();
                let Some(&scope) = inner.requested.get(&channel_id) else {
                    return false;
                };
                if self.fresh(&inner, scope) {
                    return inner.allowed.contains(&channel_id);
                }
                if inner
                    .failure
                    .as_ref()
                    .is_some_and(|f| Instant::now() < f.retry_at)
                {
                    return false;
                }
            }
            if self.refresh().await.is_err() {
                return false;
            }
        }
        // Epochs kept moving under four revalidations in a row.
        false
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

    /// Record a `subscribe` decision taken at `snap`. Returns `false` (and
    /// changes nothing) if an epoch moved since: the caller checks again.
    /// A granted channel becomes requested and allowed; a refused one leaves
    /// allowed (it stays requested if it was, so it comes back with access).
    pub fn commit_subscribe(
        &self,
        channel_id: Uuid,
        scope: ChannelScope,
        snap: EpochSnapshot,
        granted: bool,
    ) -> bool {
        let mut inner = self.lock();
        if self.snapshot(scope) != snap {
            return false;
        }
        if granted {
            if let (ChannelScope::Guild(g), Some(seen)) = (scope, snap.guild) {
                inner.guilds.entry(g).or_insert_with(|| GuildSeen {
                    epoch: registry().guild_epoch(g),
                    seen,
                });
            }
            inner.requested.insert(channel_id, scope);
            inner.allowed.insert(channel_id);
        } else {
            inner.allowed.remove(&channel_id);
        }
        true
    }

    /// The client unsubscribed: drop `channel_id` from both sets.
    pub fn unsubscribe(&self, channel_id: Uuid) {
        let mut inner = self.lock();
        inner.requested.remove(&channel_id);
        inner.allowed.remove(&channel_id);
    }

    /// A DM the user was just added to (`dm_created`): requested and allowed.
    pub fn insert_direct(&self, channel_id: Uuid) {
        let mut inner = self.lock();
        inner.requested.insert(channel_id, ChannelScope::Direct);
        inner.allowed.insert(channel_id);
    }

    /// Whether any epoch this socket depends on moved since its last
    /// revalidation (or a full one was requested).
    pub fn is_stale(&self) -> bool {
        let inner = self.lock();
        inner.force_full
            || inner.seen_user != self.user_epoch.load(Ordering::SeqCst)
            || inner
                .guilds
                .values()
                .any(|gs| gs.seen != gs.epoch.load(Ordering::SeqCst))
    }

    /// Make the next [`Self::refresh`] re-check everything (backstop).
    pub fn force_full(&self) {
        self.lock().force_full = true;
    }

    /// When the next retry of a failed revalidation is due.
    pub fn retry_at(&self) -> Option<Instant> {
        self.lock().failure.as_ref().map(|f| f.retry_at)
    }

    /// How long revalidations have been failing, if they are.
    pub fn failing_for(&self) -> Option<Duration> {
        self.lock().failure.as_ref().map(|f| f.since.elapsed())
    }

    /// Resolves when a revalidation failed since the last call.
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

    /// Recompute the allowed set for every stale scope (all scopes after a
    /// user bump or [`Self::force_full`]) with batched reads. Concurrent
    /// calls are serialised; bumps during a run make the next run.
    pub async fn refresh(&self) -> Result<(), AccessCheckFailed> {
        let _gate = self.refresh_gate.lock().await;
        let Some(plan) = self.plan()? else {
            return Ok(());
        };

        crate::ws::test_hooks::count_access_read(
            self.user_id,
            usize::from(!plan.guild_channels.is_empty()) + usize::from(!plan.direct.is_empty()),
        );
        let read = tokio::time::timeout(REFRESH_TIMEOUT, self.read(&plan)).await;
        match read {
            Ok(Ok((viewable, direct_ok))) => {
                self.apply(&plan, &viewable, &direct_ok);
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

    fn plan(&self) -> Result<Option<Plan>, AccessCheckFailed> {
        let mut inner = self.lock();
        if inner
            .failure
            .as_ref()
            .is_some_and(|f| Instant::now() < f.retry_at)
        {
            return Err(AccessCheckFailed);
        }
        let user_now = self.user_epoch.load(Ordering::SeqCst);
        let forced = std::mem::take(&mut inner.force_full);
        let full = forced || inner.seen_user != user_now;
        let guild_targets: Vec<(Uuid, u64)> = inner
            .guilds
            .iter()
            .filter_map(|(g, gs)| {
                let now = gs.epoch.load(Ordering::SeqCst);
                (full || gs.seen != now).then_some((*g, now))
            })
            .collect();
        if !full && guild_targets.is_empty() {
            return Ok(None);
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
        Ok(Some(Plan {
            full,
            forced,
            user_now,
            guild_targets,
            guild_channels,
            direct,
        }))
    }

    async fn read(&self, plan: &Plan) -> Result<(HashSet<Uuid>, HashSet<Uuid>), sqlx::Error> {
        if crate::ws::test_hooks::access_read_fails(self.user_id) {
            return Err(sqlx::Error::PoolTimedOut);
        }
        let viewable = if plan.guild_channels.is_empty() {
            HashSet::new()
        } else {
            crate::permissions::filter_viewable_channels_in_guilds(
                &self.db,
                self.user_id,
                &plan.guild_channels,
            )
            .await?
        };
        let direct_ok: HashSet<Uuid> = if plan.direct.is_empty() {
            HashSet::new()
        } else {
            sqlx::query_scalar::<_, Uuid>(
                "SELECT channel_id FROM dm_participants WHERE user_id = $1 AND channel_id = ANY($2)",
            )
            .bind(self.user_id)
            .bind(&plan.direct)
            .fetch_all(&self.db)
            .await?
            .into_iter()
            .collect()
        };
        Ok((viewable, direct_ok))
    }

    fn apply(&self, plan: &Plan, viewable: &HashSet<Uuid>, direct_ok: &HashSet<Uuid>) {
        let mut guard = self.lock();
        let inner = &mut *guard;
        let mut lost = Vec::new();
        let mut regained = Vec::new();
        for &(g, channel) in &plan.guild_channels {
            // Only channels still requested in the same scope (an
            // unsubscribe or a fresh subscribe during the read wins).
            if inner.requested.get(&channel) == Some(&ChannelScope::Guild(g)) {
                let ok = viewable.contains(&channel);
                set_allowed(&mut inner.allowed, channel, ok, &mut lost, &mut regained);
            }
        }
        for &channel in &plan.direct {
            if inner.requested.get(&channel) == Some(&ChannelScope::Direct) {
                let ok = direct_ok.contains(&channel);
                set_allowed(&mut inner.allowed, channel, ok, &mut lost, &mut regained);
            }
        }
        if plan.full {
            inner.seen_user = inner.seen_user.max(plan.user_now);
        }
        for &(g, now) in &plan.guild_targets {
            if let Some(gs) = inner.guilds.get_mut(&g) {
                gs.seen = gs.seen.max(now);
            }
        }
        // Servers without requested channels need no epoch.
        let requested = &inner.requested;
        inner.guilds.retain(|g, _| {
            requested
                .values()
                .any(|scope| *scope == ChannelScope::Guild(*g))
        });
        inner.failure = None;
        drop(guard);
        if !lost.is_empty() || !regained.is_empty() {
            debug!(
                user_id = %self.user_id,
                lost = ?lost,
                regained = ?regained,
                "Channel access revalidated"
            );
        }
    }

    fn fail(&self, plan: &Plan) {
        let mut inner = self.lock();
        for (_, channel) in &plan.guild_channels {
            inner.allowed.remove(channel);
        }
        for channel in &plan.direct {
            inner.allowed.remove(channel);
        }
        if plan.forced {
            inner.force_full = true;
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
        AccessChange::GuildRolesChanged { guild_id: g }.bump();
        let after = registry().snapshot(u, Some(g));
        assert_eq!(after.user, before.user);
        assert_eq!(after.guild, before.guild.map(|e| e + 1));
    }

    #[tokio::test]
    async fn steady_state_needs_no_revalidation() {
        let (u, g, c) = (Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4());
        let access = SocketAccess::new(u, lazy_pool());
        let scope = ChannelScope::Guild(g);
        assert!(access.commit_subscribe(c, scope, access.snapshot(scope), true));
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
        assert!(!access.commit_subscribe(c, scope, snap, true));
        assert!(!access.is_requested(c));
        registry().bump_user(u);
        assert!(!access.commit_subscribe(c, scope, access.snapshot(ChannelScope::Direct), true));
    }

    #[tokio::test]
    async fn a_bump_makes_the_socket_stale_and_unsubscribe_clears_both_sets() {
        let (u, g, c) = (Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4());
        let access = SocketAccess::new(u, lazy_pool());
        let scope = ChannelScope::Guild(g);
        assert!(access.commit_subscribe(c, scope, access.snapshot(scope), true));
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
