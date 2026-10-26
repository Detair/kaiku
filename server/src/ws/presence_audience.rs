//! Presence audience of one socket: whose presence it may receive.
//!
//! Everything a socket knows about its presence audience lives in one
//! [`Audience`] behind one lock: the validated peers it forwards, the
//! channels it follows, the users blocked either way, suspensions and the
//! versions it already sent. **Every** presence event (live, snapshot,
//! backstop, final `offline`) is authorized and queued while holding that
//! lock, so a block or an invalidation that the socket has processed can
//! never be overtaken by an event queued with an older view.
//!
//! - **Invalidation.** An audience notice names the counterpart whose relation changed (friendship,
//!   block, server membership). The forwarder suspends that user synchronously, before it handles
//!   the next message, and queues a re-check; a notice without a user (own membership, server
//!   deleted) suspends everyone until a full reload. Each invalidation bumps an epoch.
//! - **Revalidation.** A reload or re-check remembers the epoch it started at. A result for a user
//!   invalidated after that start is discarded and read again; only fresh results authorize (and
//!   lift suspensions). Peers newly subscribed stay unauthorized until a read after their
//!   subscription barrier confirms them. A failed read keeps suspensions.
//! - **Snapshots** (new peers, connect, backstop) read the published state (`presence:state`), then
//!   fence the forwarder (every notice published before the fence is applied), then queue each
//!   entry under the lock.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

use fred::prelude::*;
use tokio::sync::{mpsc, oneshot, Mutex, RwLock};
use tracing::{error, warn};
use uuid::Uuid;

use super::events::{channels, OutboundMsg, ServerEvent};
use super::handlers::{send_outbound, OutboundSendError, PubsubExit};
use crate::api::AppState;
use crate::db;
use crate::presence::connections::{self, Published};
use crate::social::block_cache;

/// Rounds of "read, subscribe what is new, read again" per reload or
/// re-check before giving up and closing the socket (1011).
pub const MAX_AUDIENCE_ROUNDS: usize = 4;

/// Above this many coalesced re-check targets, a full reload replaces the
/// per-user queries.
pub const RECHECK_TARGETS_MAX: usize = 16;

/// How often a socket re-sends what advanced since it last sent it
/// ([`send_states`] for all peers), plus up to ten seconds of jitter.
/// Together with the server's periodic reconcile
/// ([`connections::reconcile_connected`]) any stale status or rich presence
/// heals within about a minute.
pub const PRESENCE_BACKSTOP_INTERVAL: Duration = Duration::from_mins(1);

/// How long a socket waits for its own fence.
const FENCE_TIMEOUT: Duration = Duration::from_secs(5);

/// Rich-presence kinds, each with its own version.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RichKind {
    Activity,
    Custom,
}

impl RichKind {
    fn parse(kind: &str) -> Option<Self> {
        match kind {
            "activity" => Some(Self::Activity),
            "custom" => Some(Self::Custom),
            _ => None,
        }
    }
}

/// Presence authorization and delivery state of one socket (see the module
/// docs). Only ever used under its lock.
#[derive(Debug, Default)]
pub struct Audience {
    /// Validated peers: presence of these users is forwarded.
    pub peers: HashSet<Uuid>,
    /// Users whose `presence:{id}` channel is followed (validated or pending).
    pub subscribed: HashSet<Uuid>,
    /// Users blocked in either direction.
    pub blocked: HashSet<Uuid>,
    /// Users invalidated by a notice and not revalidated since.
    pub suspended: HashSet<Uuid>,
    /// Everyone is suspended until a full reload (notice without a user).
    pub suspend_all: bool,
    /// Bumped by every invalidation.
    pub epoch: u64,
    /// Epoch of each user's last invalidation.
    pub invalidated: HashMap<Uuid, u64>,
    /// Epoch of the last invalidation of everyone.
    pub all_invalidated: u64,
    /// Last status version sent per peer.
    pub versions: HashMap<Uuid, u64>,
    /// Last rich-presence version sent per peer and kind.
    pub rich: HashMap<(Uuid, RichKind), u64>,
}

impl Audience {
    /// Audience as read at connect (validated before anything is sent).
    pub fn new(peers: &[Uuid], blocked: HashSet<Uuid>) -> Self {
        Self {
            peers: peers.iter().copied().collect(),
            subscribed: peers.iter().copied().collect(),
            blocked,
            ..Self::default()
        }
    }

    /// Whether presence of `user` may be sent now.
    pub fn allowed(&self, user: &Uuid) -> bool {
        !self.suspend_all
            && self.peers.contains(user)
            && !self.blocked.contains(user)
            && !self.suspended.contains(user)
    }

    /// A notice invalidated `user` (or everyone): stop forwarding at once.
    pub fn invalidate(&mut self, user: Option<Uuid>) {
        self.epoch += 1;
        if let Some(user) = user {
            self.suspended.insert(user);
            self.invalidated.insert(user, self.epoch);
        } else {
            self.suspend_all = true;
            self.all_invalidated = self.epoch;
        }
    }

    /// Whether a read started at epoch `start` is still valid for `user`.
    pub fn fresh_for(&self, user: &Uuid, start: u64) -> bool {
        self.all_invalidated <= start && self.invalidated.get(user).is_none_or(|e| *e <= start)
    }

    /// A full read started at `start` was applied: lift every suspension it
    /// covers (invalidations before it began).
    fn lift(&mut self, start: u64) {
        if self.all_invalidated <= start {
            self.suspend_all = false;
        }
        self.invalidated.retain(|_, epoch| *epoch > start);
        let invalidated = &self.invalidated;
        self.suspended.retain(|user| invalidated.contains_key(user));
    }

    /// Clear the suspension of `user` after a fresh re-check.
    fn clear_suspension(&mut self, user: &Uuid) {
        self.suspended.remove(user);
        self.invalidated.remove(user);
    }

    /// Record a status version for `user` if it is newer than what was sent.
    pub fn admit_status(&mut self, user: Uuid, version: u64) -> bool {
        if self
            .versions
            .get(&user)
            .is_some_and(|sent| *sent >= version)
        {
            return false;
        }
        self.versions.insert(user, version);
        true
    }

    /// Record a rich-presence version if it is newer than what was sent.
    pub fn admit_rich(&mut self, user: Uuid, kind: RichKind, version: u64) -> bool {
        if self
            .rich
            .get(&(user, kind))
            .is_some_and(|sent| *sent >= version)
        {
            return false;
        }
        self.rich.insert((user, kind), version);
        true
    }

    fn forget(&mut self, user: &Uuid) {
        self.versions.remove(user);
        self.rich.remove(&(*user, RichKind::Activity));
        self.rich.remove(&(*user, RichKind::Custom));
    }
}

/// Presence state shared between a socket's loop and its forwarder.
#[derive(Clone)]
pub struct PresenceState {
    /// The audience (one lock for authorization and delivery).
    pub audience: Arc<Mutex<Audience>>,
    /// Servers whose `guild:{id}` channel the socket follows.
    pub guilds: Arc<RwLock<HashSet<Uuid>>>,
    /// Fences this socket waits for.
    pub fences: Arc<std::sync::Mutex<HashMap<Uuid, oneshot::Sender<()>>>>,
}

impl PresenceState {
    pub fn new(audience: Audience, guilds: &[Uuid]) -> Self {
        Self {
            audience: Arc::new(Mutex::new(audience)),
            guilds: Arc::new(RwLock::new(guilds.iter().copied().collect())),
            fences: Arc::default(),
        }
    }
}

/// What the socket loop still has to recompute after audience notices.
#[derive(Debug, Default)]
pub struct AudiencePending {
    /// Reload everything.
    pub full: bool,
    /// Re-check these users.
    pub targets: HashSet<Uuid>,
}

impl AudiencePending {
    pub fn has_work(&self) -> bool {
        self.full || !self.targets.is_empty()
    }
}

/// Forwarder: an audience notice arrived. Suspends the user it names (or
/// everyone) **before** the next message is handled, records the work and
/// wakes the socket loop.
pub async fn on_audience_notice(
    presence: &PresenceState,
    pending: &std::sync::Mutex<AudiencePending>,
    wake: &mpsc::Sender<()>,
    own_id: Uuid,
    changed: Option<Uuid>,
) {
    let target = changed.filter(|user| *user != own_id);
    presence.audience.lock().await.invalidate(target);
    {
        let mut pending = pending
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        match target {
            Some(user) => {
                pending.targets.insert(user);
            }
            None => pending.full = true,
        }
    }
    // A full queue already holds a wake-up.
    let _ = wake.try_send(());
}

/// Forwarder: this socket's fence arrived.
pub fn on_fence(presence: &PresenceState, id: Uuid) {
    let waiter = presence
        .fences
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .remove(&id);
    if let Some(waiter) = waiter {
        let _ = waiter.send(());
    }
}

/// Forwarder: a `presence:{uid}` message. Authorized, version-filtered and
/// queued under the audience lock.
pub async fn forward_presence(
    presence: &PresenceState,
    tx: &mpsc::Sender<OutboundMsg>,
    uid: Uuid,
    payload: &str,
) -> Result<(), OutboundSendError> {
    let mut audience = presence.audience.lock().await;
    if !audience.allowed(&uid) {
        return Ok(());
    }
    if let Ok(envelope) = serde_json::from_str::<connections::PresenceEnvelope>(payload) {
        if audience.admit_status(uid, envelope.v) {
            send_outbound(tx, OutboundMsg::Event(envelope.event)).await?;
        }
    } else if let Ok(envelope) = serde_json::from_str::<connections::RichEnvelope>(payload) {
        let Some(kind) = RichKind::parse(&envelope.kind) else {
            return Ok(());
        };
        if audience.admit_rich(uid, kind, envelope.rv) {
            send_outbound(tx, OutboundMsg::Event(envelope.event)).await?;
        }
    } else if let Ok(event) = serde_json::from_str::<ServerEvent>(payload) {
        send_outbound(tx, OutboundMsg::Event(event)).await?;
    }
    Ok(())
}

/// Forwarder: a block or unblock of the own user's sessions.
pub async fn set_blocked(presence: &PresenceState, user: Uuid, blocked: bool) {
    let mut audience = presence.audience.lock().await;
    if blocked {
        audience.blocked.insert(user);
    } else {
        audience.blocked.remove(&user);
    }
}

/// Whether a reload runs during socket setup (before `Ready`) or on a live
/// socket.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReloadMode {
    /// Socket setup: nothing sent yet; no final `offline`, no snapshot here.
    Initial,
    /// Live socket: dropped peers get a final `offline`, new peers a
    /// snapshot.
    Update,
}

/// What audience work operates on.
pub struct AudienceCtx<'a> {
    pub state: &'a AppState,
    pub user_id: Uuid,
    pub subscriber: &'a Client,
    pub presence: &'a PresenceState,
    /// Block set used to filter non-presence events (messages, typing).
    pub blocked_users: &'a RwLock<HashSet<Uuid>>,
    pub tx: &'a mpsc::Sender<OutboundMsg>,
    /// Work that failed before anything was installed goes back here.
    pub pending: &'a std::sync::Mutex<AudiencePending>,
}

impl AudienceCtx<'_> {
    fn requeue(&self, full: bool, targets: impl IntoIterator<Item = Uuid>) {
        let mut pending = self
            .pending
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        pending.full |= full;
        pending.targets.extend(targets);
    }

    async fn subscribe(&self, channels: Vec<String>) -> Result<(), PubsubExit> {
        if channels.is_empty() {
            return Ok(());
        }
        // The PING reply proves the subscriptions are live before anything
        // is read again.
        let subscribed = async {
            self.subscriber.subscribe(channels).await?;
            self.subscriber.ping::<fred::types::Value>(None).await
        };
        subscribed.await.map(|_| ()).map_err(|e| {
            error!("Failed to subscribe to presence channels: {}", e);
            PubsubExit::SubscribeFailed
        })
    }

    async fn unsubscribe(&self, channels: Vec<String>) {
        if channels.is_empty() {
            return;
        }
        if let Err(e) = self.subscriber.unsubscribe(channels).await {
            // Already unauthorized; this only saves traffic.
            warn!("Failed to unsubscribe presence channels: {}", e);
        }
    }
}

/// Drop `removed` from the audience (they had been subscribed). Must be
/// called with the audience lock held; on a live socket a peer this socket
/// had sent anything for gets a final `offline`.
async fn drop_peers(
    audience: &mut Audience,
    removed: &[Uuid],
    mode: ReloadMode,
    tx: &mpsc::Sender<OutboundMsg>,
) -> Result<(), PubsubExit> {
    for user in removed {
        // A final `offline` only replaces something this socket actually
        // sent for that user.
        let sent = audience.versions.contains_key(user)
            || audience.rich.contains_key(&(*user, RichKind::Activity))
            || audience.rich.contains_key(&(*user, RichKind::Custom));
        audience.peers.remove(user);
        audience.subscribed.remove(user);
        audience.forget(user);
        if sent && mode == ReloadMode::Update {
            let offline = ServerEvent::PresenceUpdate {
                user_id: *user,
                status: "offline".to_string(),
            };
            send_outbound(tx, OutboundMsg::Event(offline))
                .await
                .map_err(PubsubExit::from)?;
        }
    }
    Ok(())
}

/// Reload the whole audience (connect, own membership change, server
/// deleted, many targets) and apply it.
///
/// Each round reads peers, servers and block sets, drops what is gone and
/// subscribes what is new (behind a barrier, not yet authorized). A round
/// that subscribed anything is followed by another read. A read that
/// installs nothing new and is still fresh (no invalidation of a listed user
/// since it began) authorizes its peers and lifts the suspensions it covers;
/// newly authorized peers then get their snapshot (live socket).
///
/// A read error before anything was installed keeps the suspensions, queues
/// a retry (next heartbeat) and returns `Ok(false)`. An error after
/// subscriptions were installed, a failed subscription, or no fresh quiet
/// read within [`MAX_AUDIENCE_ROUNDS`] ends the socket with 1011.
pub async fn refresh_full(ctx: &AudienceCtx<'_>, mode: ReloadMode) -> Result<bool, PubsubExit> {
    let mut installed = false;
    for _ in 0..MAX_AUDIENCE_ROUNDS {
        let start = ctx.presence.audience.lock().await.epoch;
        let (wanted, guilds, blocked) = match load_audience(ctx.state, ctx.user_id).await {
            Ok(loaded) => loaded,
            Err(e) if installed => {
                warn!(
                    "Failed to revalidate presence audience for {} after subscribing; closing: {}",
                    ctx.user_id, e
                );
                return Err(PubsubExit::SubscribeFailed);
            }
            Err(e) => {
                warn!(
                    "Failed to refresh presence audience for {} (retried with the next heartbeat): {}",
                    ctx.user_id, e
                );
                ctx.requeue(true, []);
                return Ok(false);
            }
        };
        if mode == ReloadMode::Update {
            crate::ws::test_hooks::in_audience_reload(ctx.user_id).await;
        }

        (*ctx.blocked_users.write().await).clone_from(&blocked);
        let (removed, added) = {
            let mut audience = ctx.presence.audience.lock().await;
            audience.blocked = blocked;
            let removed: Vec<Uuid> = audience.subscribed.difference(&wanted).copied().collect();
            drop_peers(&mut audience, &removed, mode, ctx.tx).await?;
            let added: Vec<Uuid> = wanted.difference(&audience.subscribed).copied().collect();
            (removed, added)
        };
        let old_guilds = ctx.presence.guilds.read().await.clone();
        let removed_guilds: Vec<Uuid> = old_guilds.difference(&guilds).copied().collect();
        let added_guilds: Vec<Uuid> = guilds.difference(&old_guilds).copied().collect();
        ctx.unsubscribe(
            removed
                .iter()
                .map(|id| channels::user_presence(*id))
                .chain(removed_guilds.iter().map(|id| channels::guild_events(*id)))
                .collect(),
        )
        .await;
        *ctx.presence.guilds.write().await = guilds;

        if !added.is_empty() || !added_guilds.is_empty() {
            ctx.subscribe(
                added
                    .iter()
                    .map(|id| channels::user_presence(*id))
                    .chain(added_guilds.iter().map(|id| channels::guild_events(*id)))
                    .collect(),
            )
            .await?;
            ctx.presence
                .audience
                .lock()
                .await
                .subscribed
                .extend(added.iter().copied());
            installed = true;
            crate::ws::test_hooks::after_audience_barrier(ctx.user_id).await;
            continue;
        }

        // A quiet read: authorize, unless an invalidation overtook it.
        let newly = {
            let mut audience = ctx.presence.audience.lock().await;
            let fresh = wanted.iter().all(|user| audience.fresh_for(user, start))
                && audience.all_invalidated <= start;
            if fresh {
                let newly: Vec<Uuid> = wanted.difference(&audience.peers).copied().collect();
                audience.peers = wanted;
                audience.lift(start);
                Some(newly)
            } else {
                None
            }
        };
        let Some(newly) = newly else {
            continue;
        };
        if mode == ReloadMode::Update {
            send_states(ctx, &newly).await?;
        }
        return Ok(true);
    }
    warn!(
        "Presence audience of {} kept changing during revalidation; closing",
        ctx.user_id
    );
    Err(PubsubExit::SubscribeFailed)
}

/// Re-check only `targets` (a notice named them): one query each. A target
/// that is no longer a peer is dropped; one that is a peer is authorized
/// (and its suspension lifted) only by a fresh result, after its
/// subscription barrier if it was not subscribed yet. Results overtaken by a
/// newer invalidation are read again.
pub async fn recheck(ctx: &AudienceCtx<'_>, targets: HashSet<Uuid>) -> Result<(), PubsubExit> {
    if targets.len() > RECHECK_TARGETS_MAX {
        return refresh_full(ctx, ReloadMode::Update).await.map(|_| ());
    }
    let mut targets = targets;
    let mut newly = Vec::new();
    for _ in 0..MAX_AUDIENCE_ROUNDS {
        if targets.is_empty() {
            break;
        }
        let start = ctx.presence.audience.lock().await.epoch;
        let mut results = Vec::new();
        for target in &targets {
            match peer_relation(&ctx.state.db, ctx.user_id, *target).await {
                Ok(relation) => results.push((*target, relation)),
                Err(e) => {
                    warn!(
                        "Failed to re-check presence peer {} (stays suspended, retried with the next heartbeat): {}",
                        target, e
                    );
                    ctx.requeue(false, [*target]);
                }
            }
        }
        crate::ws::test_hooks::in_audience_reload(ctx.user_id).await;
        {
            let mut blocked_users = ctx.blocked_users.write().await;
            for (target, relation) in &results {
                if relation.blocked {
                    blocked_users.insert(*target);
                } else {
                    blocked_users.remove(target);
                }
            }
        }

        let mut retry = HashSet::new();
        let mut removed = Vec::new();
        let mut to_subscribe = Vec::new();
        {
            let mut audience = ctx.presence.audience.lock().await;
            for (target, relation) in results {
                if relation.blocked {
                    audience.blocked.insert(target);
                } else {
                    audience.blocked.remove(&target);
                }
                if !audience.fresh_for(&target, start) {
                    retry.insert(target);
                    continue;
                }
                let allowed = relation.related && !relation.blocked;
                if !allowed {
                    if audience.subscribed.contains(&target) {
                        drop_peers(&mut audience, &[target], ReloadMode::Update, ctx.tx).await?;
                        removed.push(target);
                    }
                    audience.clear_suspension(&target);
                } else if audience.subscribed.contains(&target) {
                    if audience.peers.insert(target) {
                        newly.push(target);
                    }
                    audience.clear_suspension(&target);
                } else {
                    let room = i64::try_from(audience.subscribed.len() + to_subscribe.len())
                        .is_ok_and(|n| n < connections::PRESENCE_PEER_LIMIT);
                    if room {
                        to_subscribe.push(target);
                    }
                }
            }
        }
        ctx.unsubscribe(
            removed
                .iter()
                .map(|id| channels::user_presence(*id))
                .collect(),
        )
        .await;
        if !to_subscribe.is_empty() {
            ctx.subscribe(
                to_subscribe
                    .iter()
                    .map(|id| channels::user_presence(*id))
                    .collect(),
            )
            .await?;
            ctx.presence
                .audience
                .lock()
                .await
                .subscribed
                .extend(to_subscribe.iter().copied());
            crate::ws::test_hooks::after_audience_barrier(ctx.user_id).await;
            // Validate again after the barrier.
            retry.extend(to_subscribe);
        }
        targets = retry;
    }
    if !targets.is_empty() {
        warn!(
            "Presence peers of {} kept changing during re-check; closing",
            ctx.user_id
        );
        return Err(PubsubExit::SubscribeFailed);
    }
    send_states(ctx, &newly).await
}

/// Send the published presence of `peers` (status, activity, custom
/// status), each part only if its version is newer than what this socket
/// sent. Used for snapshots of new peers, the connect snapshot and the
/// periodic backstop.
///
/// Reads the published state first, then fences the forwarder (every
/// invalidation published before the fence is applied), then queues each
/// peer's entries under the audience lock, checking authorization there. A
/// Valkey error skips the send (the backstop retries); a fence that does not
/// come back ends the socket with 1011.
pub async fn send_states(ctx: &AudienceCtx<'_>, peers: &[Uuid]) -> Result<(), PubsubExit> {
    if peers.is_empty() {
        return Ok(());
    }
    let published = match connections::published_states(&ctx.state.redis, peers).await {
        Ok(published) => published,
        Err(e) => {
            connections::log_unavailable(&e);
            return Ok(());
        }
    };
    fence(ctx).await?;
    crate::ws::test_hooks::in_snapshot(ctx.user_id).await;
    for peer in peers {
        let Some(current) = published.get(peer) else {
            continue;
        };
        let mut audience = ctx.presence.audience.lock().await;
        if !audience.allowed(peer) {
            continue;
        }
        for event in admitted_events(&mut audience, *peer, current) {
            send_outbound(ctx.tx, OutboundMsg::Event(event))
                .await
                .map_err(PubsubExit::from)?;
        }
    }
    Ok(())
}

/// The events of `current` this socket has not sent yet (newer versions),
/// recording them as sent: status, then activity and custom status unless
/// the peer shows as offline.
pub fn admitted_events(
    audience: &mut Audience,
    peer: Uuid,
    current: &Published,
) -> Vec<ServerEvent> {
    let mut events = Vec::new();
    if audience.admit_status(peer, current.version) {
        events.push(ServerEvent::PresenceUpdate {
            user_id: peer,
            status: current.status.clone(),
        });
    }
    if current.status == "offline" {
        return events;
    }
    let rich = [
        (
            RichKind::Activity,
            &current.activity,
            current.activity_version,
        ),
        (RichKind::Custom, &current.custom, current.custom_version),
    ];
    for (kind, json, version) in rich {
        let Some(event) = json
            .as_deref()
            .and_then(|json| serde_json::from_str::<ServerEvent>(json).ok())
        else {
            continue;
        };
        if audience.admit_rich(peer, kind, version) {
            events.push(event);
        }
    }
    events
}

/// Wait until the forwarder has processed everything published to this
/// socket before now.
async fn fence(ctx: &AudienceCtx<'_>) -> Result<(), PubsubExit> {
    let id = Uuid::new_v4();
    let (done, wait) = oneshot::channel();
    ctx.presence
        .fences
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .insert(id, done);
    let forget = || {
        ctx.presence
            .fences
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(&id);
    };
    if let Err(e) = connections::publish_fence(&ctx.state.redis, ctx.user_id, id).await {
        forget();
        connections::log_unavailable(&e);
        return Err(PubsubExit::StreamEnded);
    }
    if tokio::time::timeout(FENCE_TIMEOUT, wait).await == Ok(Ok(())) {
        Ok(())
    } else {
        forget();
        warn!("Presence fence of {} did not come back", ctx.user_id);
        Err(PubsubExit::StreamEnded)
    }
}

/// Peers, servers and block set (both directions) of `user_id`.
async fn load_audience(
    state: &AppState,
    user_id: Uuid,
) -> Result<(HashSet<Uuid>, HashSet<Uuid>, HashSet<Uuid>), sqlx::Error> {
    if crate::ws::test_hooks::audience_read_fails(user_id) {
        return Err(sqlx::Error::Protocol(
            "test hook: audience read fails".into(),
        ));
    }
    let peers = get_presence_peers(&state.db, user_id).await?;
    let guilds = db::get_user_guild_ids(&state.db, user_id).await?;
    let blocked = block_cache::load_blocked_users(&state.db, &state.redis, user_id)
        .await
        .map_err(|e| sqlx::Error::Protocol(e.to_string()))?;
    let blocked_by = block_cache::load_blocked_by(&state.db, &state.redis, user_id)
        .await
        .map_err(|e| sqlx::Error::Protocol(e.to_string()))?;
    Ok((
        peers.into_iter().collect(),
        guilds.into_iter().collect(),
        blocked.union(&blocked_by).copied().collect(),
    ))
}

/// Presence peers of `user_id` in one query: accepted friends first, then
/// members of the user's servers, without duplicates, the user, or anyone
/// blocked in either direction, at most `PRESENCE_PEER_LIMIT`.
pub async fn get_presence_peers(
    db: &sqlx::PgPool,
    user_id: Uuid,
) -> Result<Vec<Uuid>, sqlx::Error> {
    let peers: Vec<(Uuid,)> = sqlx::query_as(
        r"
        SELECT peer_id FROM (
            SELECT CASE
                WHEN f.requester_id = $1 THEN f.addressee_id
                ELSE f.requester_id
            END AS peer_id, 0 AS rank
            FROM friendships f
            WHERE (f.requester_id = $1 OR f.addressee_id = $1)
              AND f.status = 'accepted'
            UNION ALL
            SELECT other.user_id AS peer_id, 1 AS rank
            FROM guild_members mine
            JOIN guild_members other ON other.guild_id = mine.guild_id
            WHERE mine.user_id = $1 AND other.user_id <> $1
        ) peers
        WHERE NOT EXISTS (
            SELECT 1 FROM friendships b
            WHERE b.status = 'blocked'
              AND ((b.requester_id = $1 AND b.addressee_id = peers.peer_id)
                OR (b.requester_id = peers.peer_id AND b.addressee_id = $1))
        )
        GROUP BY peer_id
        ORDER BY MIN(rank), peer_id
        LIMIT $2
        ",
    )
    .bind(user_id)
    .bind(connections::PRESENCE_PEER_LIMIT)
    .fetch_all(db)
    .await?;

    Ok(peers.into_iter().map(|(id,)| id).collect())
}

/// How `other` relates to a user.
#[derive(Debug, Clone, Copy)]
struct PeerRelation {
    /// Friends or members of a common server.
    related: bool,
    /// Blocked in either direction.
    blocked: bool,
}

/// [`PeerRelation`] of `other` to `user_id`, one query.
async fn peer_relation(
    db: &sqlx::PgPool,
    user_id: Uuid,
    other: Uuid,
) -> Result<PeerRelation, sqlx::Error> {
    let (related, blocked): (bool, bool) = sqlx::query_as(
        r"
        SELECT
            EXISTS (
                SELECT 1 FROM friendships f
                WHERE f.status = 'accepted'
                  AND ((f.requester_id = $1 AND f.addressee_id = $2)
                    OR (f.requester_id = $2 AND f.addressee_id = $1))
            )
            OR EXISTS (
                SELECT 1 FROM guild_members mine
                JOIN guild_members other ON other.guild_id = mine.guild_id
                WHERE mine.user_id = $1 AND other.user_id = $2
            ),
            EXISTS (
                SELECT 1 FROM friendships b
                WHERE b.status = 'blocked'
                  AND ((b.requester_id = $1 AND b.addressee_id = $2)
                    OR (b.requester_id = $2 AND b.addressee_id = $1))
            )
        ",
    )
    .bind(user_id)
    .bind(other)
    .fetch_one(db)
    .await?;
    Ok(PeerRelation { related, blocked })
}

#[cfg(test)]
mod tests {
    use super::*;

    const PEER: Uuid = Uuid::from_u128(7);

    #[test]
    fn invalidation_suspends_until_a_fresh_read() {
        let mut audience = Audience::new(&[PEER], HashSet::new());
        assert!(audience.allowed(&PEER));

        let start = audience.epoch;
        audience.invalidate(Some(PEER));
        assert!(!audience.allowed(&PEER), "suspended at once");
        // A read that began before the invalidation is stale for PEER.
        assert!(!audience.fresh_for(&PEER, start));
        // A read that began after it is fresh and lifts the suspension.
        let later = audience.epoch;
        assert!(audience.fresh_for(&PEER, later));
        audience.lift(later);
        assert!(audience.allowed(&PEER));

        // Everyone: suspended until a full read started after it.
        let start = audience.epoch;
        audience.invalidate(None);
        assert!(!audience.allowed(&PEER));
        assert!(!audience.fresh_for(&PEER, start));
        audience.lift(start);
        assert!(!audience.allowed(&PEER), "a stale read lifts nothing");
        audience.lift(audience.epoch);
        assert!(audience.allowed(&PEER));
    }

    #[test]
    fn blocked_or_pending_users_are_not_allowed() {
        let mut audience = Audience::new(&[PEER], HashSet::new());
        audience.blocked.insert(PEER);
        assert!(!audience.allowed(&PEER));
        let pending = Uuid::from_u128(8);
        audience.subscribed.insert(pending);
        assert!(!audience.allowed(&pending), "subscribed but not validated");
    }

    #[test]
    fn backstop_sends_only_what_advanced() {
        let mut audience = Audience::new(&[PEER], HashSet::new());
        let activity = |name: &str| {
            serde_json::to_string(&ServerEvent::RichPresenceUpdate {
                user_id: PEER,
                activity: Some(crate::presence::Activity {
                    activity_type: crate::presence::ActivityType::Game,
                    name: name.to_string(),
                    started_at: chrono::Utc::now(),
                    details: None,
                }),
            })
            .unwrap()
        };
        let published = |status: &str, v, a: &str, av| Published {
            status: status.to_string(),
            version: v,
            activity: Some(activity(a)),
            activity_version: av,
            custom: None,
            custom_version: 0,
        };

        // Snapshot: everything is new.
        let first = admitted_events(&mut audience, PEER, &published("busy", 3, "Chess", 2));
        assert_eq!(first.len(), 2);
        // A live activity update (av 3) was forwarded meanwhile.
        assert!(audience.admit_rich(PEER, RichKind::Activity, 3));
        // An older snapshot/backstop read (status 3, av 2) sends nothing: it
        // must not overwrite the newer activity.
        assert!(admitted_events(&mut audience, PEER, &published("busy", 3, "Chess", 2)).is_empty());
        // The backstop sends only what advanced: activity 4, same status.
        let advanced = admitted_events(&mut audience, PEER, &published("busy", 3, "Go", 4));
        assert!(matches!(
            advanced.as_slice(),
            [ServerEvent::RichPresenceUpdate { .. }]
        ));
        // Status advanced to offline: status only, no rich presence.
        let offline = admitted_events(&mut audience, PEER, &published("offline", 4, "Go", 5));
        assert!(matches!(
            offline.as_slice(),
            [ServerEvent::PresenceUpdate { status, .. }] if status == "offline"
        ));
    }
}
