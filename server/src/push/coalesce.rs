//! Per-(recipient, channel) push coalescing and the read gate (contract C20).
//!
//! - The first message for a (user, channel) pushes immediately (`SendNow`) and opens a [`WINDOW`].
//!   Further messages inside the window fold into one trailing push at the end of the window:
//!   exactly one `admit` per window returns `Deferred { schedule: true }`, and the caller then
//!   sleeps until `flush_at` and calls [`Coalescer::flush`]. A non-empty flush re-arms the window,
//!   so a steady stream yields at most one push per window.
//! - Every push (leading or trailing) counts against [`USER_MAX_PER_MINUTE`] per user (sliding 60 s
//!   window). Over the cap a leading message is `Dropped` and a trailing flush yields `None`.
//! - A `read` signal is warranted only when a wake went out for that (user, channel) within
//!   [`READ_GATE`] and was not cleared since ([`Coalescer::take_read_gate`]). Consuming the gate
//!   changes nothing else: the window and a pending trailing push stay, so a message arriving
//!   around a read is still pushed and alternating message/read does not produce repeated immediate
//!   wakes.
//! - A flush timer that has not run a full [`WINDOW`] after its deadline is treated as lost, so the
//!   channel is not blocked forever.
//!
//! State lives in a `std::sync::Mutex` that is never held across an `.await`; all methods are
//! synchronous and take `now` explicitly, so they are pure `Instant` arithmetic.
//!
//! Memory:
//! - Every 256th `admit` sweeps entries idle for [`IDLE_TTL`] that hold no unexpired read gate
//!   (gates live up to [`READ_GATE`]). Per-user rate history is dropped once older than a minute.
//! - A dropped admission (per-user cap) allocates nothing: the cap is checked before an entry is
//!   created.
//! - [`MAX_ENTRIES`] is a hard cap. At the cap, a new entry first evicts every idle entry, **read
//!   gates included**: those channels then get no `read` signal (the phone keeps the notification
//!   until it is opened or replaced). If nothing is idle, the message is pushed immediately without
//!   coalescing and without creating state (so it arms no read gate); eviction is retried at most
//!   once per second.
//!
//! **Single replica:** the state is per process. Several server replicas would each coalesce
//! and rate-limit on their own; that needs shared state first.

use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use uuid::Uuid;

/// Coalescing window per (user, channel).
pub const WINDOW: Duration = Duration::from_secs(10);
/// Maximum pushes (leading + trailing) per user per minute.
pub const USER_MAX_PER_MINUTE: usize = 30;
/// A `read` signal follows only a wake sent within this period.
pub const READ_GATE: Duration = Duration::from_hours(24);
/// Entries without live state are dropped after this idle period.
pub const IDLE_TTL: Duration = Duration::from_hours(1);
/// Hard cap on (user, channel) entries. See the module docs.
pub const MAX_ENTRIES: usize = 65_536;

const RATE_PERIOD: Duration = Duration::from_mins(1);
const SWEEP_EVERY: u64 = 256;
/// While the map is full of active entries, retry eviction at most this often.
const EVICT_RETRY: Duration = Duration::from_secs(1);

/// Outcome of [`Coalescer::admit`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Admit {
    /// Push now with `count = 1`.
    SendNow,
    /// Folded into the trailing push at `flush_at`. `schedule` is true exactly once per window:
    /// that caller sleeps until `flush_at` and then calls [`Coalescer::flush`].
    Deferred { flush_at: Instant, schedule: bool },
    /// Over the per-user cap; nothing is sent.
    Dropped,
}

#[derive(Debug)]
struct Entry {
    /// End of the current coalescing window.
    window_until: Option<Instant>,
    /// Messages folded into the next trailing push.
    pending: u32,
    /// A flush timer is outstanding for this entry.
    scheduled: bool,
    /// When the last wake went out (read gate armed), if not cleared since.
    woke_at: Option<Instant>,
    last_activity: Instant,
}

impl Entry {
    const fn new(now: Instant) -> Self {
        Self {
            window_until: None,
            pending: 0,
            scheduled: false,
            woke_at: None,
            last_activity: now,
        }
    }
}

#[derive(Debug)]
struct State {
    entries: HashMap<(Uuid, Uuid), Entry>,
    /// Push instants per user within the last [`RATE_PERIOD`] (at most
    /// [`USER_MAX_PER_MINUTE`] each).
    rate: HashMap<Uuid, VecDeque<Instant>>,
    admits: u64,
    /// Hard cap on `entries` ([`MAX_ENTRIES`]; smaller in tests).
    max_entries: usize,
    /// Earliest next eviction attempt while the map is full, so a full map of
    /// active entries is not rescanned on every admission.
    next_evict: Option<Instant>,
}

impl State {
    fn new(max_entries: usize) -> Self {
        Self {
            entries: HashMap::new(),
            rate: HashMap::new(),
            admits: 0,
            max_entries,
            next_evict: None,
        }
    }

    /// Record one push for `user` if the cap allows it.
    fn try_consume(&mut self, user: Uuid, now: Instant) -> bool {
        let sent = self.rate.entry(user).or_default();
        while sent
            .front()
            .is_some_and(|&t| now.saturating_duration_since(t) >= RATE_PERIOD)
        {
            sent.pop_front();
        }
        if sent.len() >= USER_MAX_PER_MINUTE {
            return false;
        }
        sent.push_back(now);
        true
    }

    /// Periodic sweep: drop idle entries without a live read gate.
    fn sweep(&mut self, now: Instant) {
        self.entries.retain(|_, e| {
            let gate_live = e
                .woke_at
                .is_some_and(|t| now.saturating_duration_since(t) < READ_GATE);
            !is_idle(e, now) || gate_live
        });
        self.rate.retain(|_, sent| {
            sent.back()
                .is_some_and(|&t| now.saturating_duration_since(t) < RATE_PERIOD)
        });
    }

    /// Make room for one new entry: at the hard cap, evict every idle entry,
    /// read gates included. Returns whether there is room.
    fn make_room(&mut self, now: Instant) -> bool {
        if self.entries.len() < self.max_entries {
            return true;
        }
        if self.next_evict.is_some_and(|t| now < t) {
            return false;
        }
        self.entries.retain(|_, e| !is_idle(e, now));
        let room = self.entries.len() < self.max_entries;
        self.next_evict = (!room).then(|| now + EVICT_RETRY);
        room
    }
}

/// Idle for [`IDLE_TTL`]. A flush timer fires within [`WINDOW`] of the last
/// activity, so an idle entry has no live timer; a late flush on an evicted
/// entry finds nothing and returns `None`.
fn is_idle(e: &Entry, now: Instant) -> bool {
    now.saturating_duration_since(e.last_activity) >= IDLE_TTL
}

/// Push coalescer and read gate. See the module docs.
pub struct Coalescer {
    state: Mutex<State>,
}

impl Default for Coalescer {
    fn default() -> Self {
        Self {
            state: Mutex::new(State::new(MAX_ENTRIES)),
        }
    }
}

/// Sizes only: the map can be large and holds per-user activity.
impl std::fmt::Debug for Coalescer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // try_lock: formatting must never block or self-deadlock.
        let mut d = f.debug_struct("Coalescer");
        match self.state.try_lock() {
            Ok(st) => d
                .field("entries", &st.entries.len())
                .field("rate_users", &st.rate.len()),
            Err(_) => d.field("state", &"<busy>"),
        };
        d.finish()
    }
}

impl Coalescer {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        // A panic while holding the lock leaves consistent-enough state
        // (plain counters); keep serving instead of poisoning push forever.
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// A new message for `user` in `channel`.
    pub fn admit(&self, user: Uuid, channel: Uuid, now: Instant) -> Admit {
        let key = (user, channel);
        let mut st = self.lock();
        st.admits = st.admits.wrapping_add(1);
        if st.admits.is_multiple_of(SWEEP_EVERY) {
            st.sweep(now);
        }

        if let Some(entry) = st.entries.get_mut(&key) {
            entry.last_activity = now;
            if let Some(window_until) = entry.window_until {
                // A timer that has not flushed a full window after its
                // deadline is lost; forget it so the channel is not stuck.
                if entry.scheduled && now >= window_until + WINDOW {
                    entry.scheduled = false;
                    entry.pending = 0;
                }
                // Inside the window, or the window ended but its flush has
                // not run yet (late timer): join the pending trailing push.
                if now < window_until || entry.scheduled {
                    entry.pending = entry.pending.saturating_add(1);
                    let schedule = !entry.scheduled;
                    entry.scheduled = true;
                    return Admit::Deferred {
                        flush_at: window_until,
                        schedule,
                    };
                }
            }
        }

        // Leading push. The cap is checked before any entry is created, so a
        // dropped admission allocates nothing.
        if !st.try_consume(user, now) {
            return Admit::Dropped;
        }
        if let Some(entry) = st.entries.get_mut(&key) {
            entry.window_until = Some(now + WINDOW);
            entry.woke_at = Some(now);
            return Admit::SendNow;
        }
        if st.make_room(now) {
            let mut entry = Entry::new(now);
            entry.window_until = Some(now + WINDOW);
            entry.woke_at = Some(now);
            st.entries.insert(key, entry);
        }
        // Without room: push uncoalesced and keep no state (no read gate).
        Admit::SendNow
    }

    /// The trailing flush for (`user`, `channel`): the pending count, if any and within the
    /// per-user cap. A non-empty flush re-arms the window and the read gate.
    pub fn flush(&self, user: Uuid, channel: Uuid, now: Instant) -> Option<u32> {
        let key = (user, channel);
        let mut st = self.lock();
        let entry = st.entries.get_mut(&key)?;
        entry.scheduled = false;
        let pending = std::mem::take(&mut entry.pending);
        if pending == 0 || !st.try_consume(user, now) {
            return None;
        }
        let entry = st.entries.get_mut(&key)?;
        entry.window_until = Some(now + WINDOW);
        entry.woke_at = Some(now);
        entry.last_activity = now;
        Some(pending)
    }

    /// Whether a `read` signal should go out for (`user`, `channel`): true at most once per
    /// wake, and only within [`READ_GATE`] of it. Consuming the gate has no other effect: the
    /// window and any pending trailing push stay, so messages that arrive around the read are
    /// still pushed.
    pub fn take_read_gate(&self, user: Uuid, channel: Uuid, now: Instant) -> bool {
        let mut st = self.lock();
        st.entries
            .get_mut(&(user, channel))
            .and_then(|e| e.woke_at.take())
            .is_some_and(|t| now.saturating_duration_since(t) < READ_GATE)
    }

    #[cfg(test)]
    fn with_max_entries(max: usize) -> Self {
        Self {
            state: Mutex::new(State::new(max)),
        }
    }

    #[cfg(test)]
    fn entry_count(&self) -> usize {
        self.lock().entries.len()
    }

    #[cfg(test)]
    fn rate_user_count(&self) -> usize {
        self.lock().rate.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids() -> (Coalescer, Uuid, Uuid, Instant) {
        (
            Coalescer::new(),
            Uuid::now_v7(),
            Uuid::now_v7(),
            Instant::now(),
        )
    }

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    #[test]
    fn first_message_sends_immediately() {
        let (c, u, ch, t0) = ids();
        assert!(matches!(c.admit(u, ch, t0), Admit::SendNow));
    }

    #[test]
    fn burst_of_20_yields_one_leading_and_one_trailing() {
        let (c, u, ch, t0) = ids();
        let results: Vec<Admit> = (0..20).map(|i| c.admit(u, ch, t0 + ms(i * 250))).collect();

        assert_eq!(results[0], Admit::SendNow);
        let deferred: Vec<_> = results[1..]
            .iter()
            .map(|a| match *a {
                Admit::Deferred { flush_at, schedule } => (flush_at, schedule),
                other => panic!("expected Deferred, got {other:?}"),
            })
            .collect();
        assert_eq!(deferred.len(), 19);
        assert_eq!(deferred.iter().filter(|(_, s)| *s).count(), 1);
        assert!(
            deferred[0].1,
            "the first deferred message schedules the flush"
        );
        assert!(deferred.iter().all(|(f, _)| *f == t0 + WINDOW));

        assert_eq!(c.flush(u, ch, t0 + WINDOW), Some(19));
        // The flush re-armed the window.
        match c.admit(u, ch, t0 + ms(10_500)) {
            Admit::Deferred { flush_at, schedule } => {
                assert!(schedule);
                assert_eq!(flush_at, t0 + WINDOW + WINDOW);
            }
            other => panic!("expected Deferred, got {other:?}"),
        }
        assert_eq!(c.flush(u, ch, t0 + WINDOW + WINDOW), Some(1));
        // An empty flush yields nothing.
        assert_eq!(c.flush(u, ch, t0 + WINDOW + WINDOW), None);
    }

    #[test]
    fn message_after_window_sends_immediately() {
        let (c, u, ch, t0) = ids();
        assert_eq!(c.admit(u, ch, t0), Admit::SendNow);
        assert_eq!(c.admit(u, ch, t0 + WINDOW + ms(1)), Admit::SendNow);
    }

    #[test]
    fn late_timer_still_collects_messages() {
        // Window over, flush not yet run: the new message joins the pending flush.
        let (c, u, ch, t0) = ids();
        c.admit(u, ch, t0);
        assert!(matches!(
            c.admit(u, ch, t0 + ms(100)),
            Admit::Deferred { schedule: true, .. }
        ));
        assert!(matches!(
            c.admit(u, ch, t0 + WINDOW + ms(50)),
            Admit::Deferred {
                schedule: false,
                ..
            }
        ));
        assert_eq!(c.flush(u, ch, t0 + WINDOW + ms(60)), Some(2));
    }

    #[test]
    fn channels_and_users_are_independent() {
        let (c, u, ch, t0) = ids();
        let (u2, ch2) = (Uuid::now_v7(), Uuid::now_v7());
        assert_eq!(c.admit(u, ch, t0), Admit::SendNow);
        assert_eq!(c.admit(u, ch2, t0), Admit::SendNow);
        assert_eq!(c.admit(u2, ch, t0), Admit::SendNow);
        assert!(matches!(c.admit(u, ch, t0 + ms(1)), Admit::Deferred { .. }));
        assert_eq!(c.flush(u, ch2, t0 + WINDOW), None);
        assert_eq!(c.flush(u, ch, t0 + WINDOW), Some(1));
    }

    #[test]
    fn per_user_cap_drops_after_30_per_minute() {
        let (c, u, _, t0) = ids();
        let channels: Vec<Uuid> = (0..31).map(|_| Uuid::now_v7()).collect();
        let results: Vec<Admit> = channels.iter().map(|&ch| c.admit(u, ch, t0)).collect();
        assert_eq!(
            results.iter().filter(|a| **a == Admit::SendNow).count(),
            USER_MAX_PER_MINUTE
        );
        assert_eq!(results[30], Admit::Dropped);
        // Another user is unaffected.
        assert_eq!(c.admit(Uuid::now_v7(), channels[30], t0), Admit::SendNow);
        assert_eq!(
            c.admit(u, channels[30], t0 + Duration::from_secs(61)),
            Admit::SendNow
        );
    }

    #[test]
    fn trailing_flush_counts_against_cap() {
        let (c, u, _, t0) = ids();
        let channels: Vec<Uuid> = (0..30).map(|_| Uuid::now_v7()).collect();
        // One leading push on channel 0, one deferred message.
        assert_eq!(c.admit(u, channels[0], t0), Admit::SendNow);
        assert!(matches!(
            c.admit(u, channels[0], t0 + ms(1)),
            Admit::Deferred { schedule: true, .. }
        ));
        // 29 more leading pushes exhaust the cap.
        for &ch in &channels[1..] {
            assert_eq!(c.admit(u, ch, t0 + ms(2)), Admit::SendNow);
        }
        assert_eq!(c.flush(u, channels[0], t0 + WINDOW), None, "cap reached");
        // The lost trailing push must not leave its count behind.
        assert_eq!(
            c.lock().entries.get(&(u, channels[0])).map(|e| e.pending),
            Some(0),
            "pending cleared after cap-dropped flush"
        );

        // A trailing flush itself consumes budget.
        let (c, u, ch, t0) = ids();
        c.admit(u, ch, t0);
        c.admit(u, ch, t0 + ms(1));
        assert_eq!(c.flush(u, ch, t0 + WINDOW), Some(1));
        let others: Vec<Uuid> = (0..29).map(|_| Uuid::now_v7()).collect();
        let sent = others
            .iter()
            .filter(|&&o| c.admit(u, o, t0 + WINDOW) == Admit::SendNow)
            .count();
        assert_eq!(sent, USER_MAX_PER_MINUTE - 2);
    }

    #[test]
    fn read_gate_only_after_wake_and_only_once() {
        let (c, u, ch, t0) = ids();
        assert!(!c.take_read_gate(u, ch, t0), "no wake yet");
        c.admit(u, ch, t0);
        assert!(!c.take_read_gate(Uuid::now_v7(), ch, t0), "other user");
        assert!(!c.take_read_gate(u, Uuid::now_v7(), t0), "other channel");
        assert!(c.take_read_gate(u, ch, t0 + ms(1)));
        assert!(!c.take_read_gate(u, ch, t0 + ms(2)), "only once");

        // Expired after READ_GATE.
        let (c, u, ch, t0) = ids();
        c.admit(u, ch, t0);
        assert!(!c.take_read_gate(u, ch, t0 + READ_GATE));
    }

    #[test]
    fn read_keeps_pending_count_and_trailing_flush() {
        let (c, u, ch, t0) = ids();
        assert_eq!(c.admit(u, ch, t0), Admit::SendNow);
        assert!(matches!(
            c.admit(u, ch, t0 + ms(1)),
            Admit::Deferred { schedule: true, .. }
        ));
        assert!(c.take_read_gate(u, ch, t0 + ms(2)));
        // The window stays open and keeps collecting.
        assert!(matches!(
            c.admit(u, ch, t0 + ms(3)),
            Admit::Deferred {
                schedule: false,
                ..
            }
        ));
        assert_eq!(c.flush(u, ch, t0 + WINDOW), Some(2));
    }

    #[test]
    fn message_read_message_is_one_leading_and_one_trailing() {
        let (c, u, ch, t0) = ids();
        assert_eq!(c.admit(u, ch, t0), Admit::SendNow);
        assert!(c.take_read_gate(u, ch, t0 + ms(500)));
        match c.admit(u, ch, t0 + ms(1_000)) {
            Admit::Deferred { flush_at, schedule } => {
                assert!(schedule, "the second message schedules the trailing push");
                assert_eq!(flush_at, t0 + WINDOW);
            }
            other => panic!("expected Deferred, not a second leading push: {other:?}"),
        }
        assert_eq!(c.flush(u, ch, t0 + WINDOW), Some(1));
        // The trailing push re-armed the read gate.
        assert!(c.take_read_gate(u, ch, t0 + WINDOW + ms(1)));
    }

    #[test]
    fn lost_timer_does_not_block_channel() {
        let (c, u, ch, t0) = ids();
        c.admit(u, ch, t0);
        assert!(matches!(
            c.admit(u, ch, t0 + ms(1)),
            Admit::Deferred { schedule: true, .. }
        ));
        // The timer never flushes. Shortly after the window it still collects ...
        assert!(matches!(
            c.admit(u, ch, t0 + WINDOW + ms(1)),
            Admit::Deferred {
                schedule: false,
                ..
            }
        ));
        // ... but once a full extra window has passed, `scheduled` is stale.
        assert_eq!(c.admit(u, ch, t0 + WINDOW + WINDOW), Admit::SendNow);
    }

    #[test]
    fn dropped_admission_allocates_nothing() {
        let (c, u, _, t0) = ids();
        for _ in 0..USER_MAX_PER_MINUTE {
            assert_eq!(c.admit(u, Uuid::now_v7(), t0), Admit::SendNow);
        }
        assert_eq!(c.entry_count(), USER_MAX_PER_MINUTE);
        for _ in 0..10 {
            assert_eq!(c.admit(u, Uuid::now_v7(), t0), Admit::Dropped);
        }
        assert_eq!(c.entry_count(), USER_MAX_PER_MINUTE);
    }

    #[test]
    fn entry_cap_is_hard_and_evicts_idle_gated_entries() {
        let c = Coalescer::with_max_entries(4);
        let t0 = Instant::now();
        let gated: Vec<(Uuid, Uuid)> = (0..4).map(|_| (Uuid::now_v7(), Uuid::now_v7())).collect();
        for &(u, ch) in &gated {
            assert_eq!(c.admit(u, ch, t0), Admit::SendNow);
        }
        assert_eq!(c.entry_count(), 4);

        // Full and nothing idle: push without coalescing, no new state.
        let (u, ch) = (Uuid::now_v7(), Uuid::now_v7());
        assert_eq!(c.admit(u, ch, t0 + ms(1)), Admit::SendNow);
        assert_eq!(c.admit(u, ch, t0 + ms(2)), Admit::SendNow);
        assert_eq!(c.entry_count(), 4);
        assert!(
            !c.take_read_gate(u, ch, t0 + ms(3)),
            "no state, no read gate"
        );

        // Once the gated entries are idle, they are evicted despite their gates.
        let later = t0 + IDLE_TTL + ms(1);
        assert_eq!(c.admit(u, ch, later), Admit::SendNow);
        assert_eq!(c.entry_count(), 1);
        assert!(
            !c.take_read_gate(gated[0].0, gated[0].1, later),
            "gate evicted"
        );
        assert!(matches!(
            c.admit(u, ch, later + ms(1)),
            Admit::Deferred { schedule: true, .. }
        ));
    }

    #[test]
    fn idle_entries_are_collected() {
        let (c, _, _, t0) = ids();
        // 100 (user, channel) entries woken and read: no live state left.
        for _ in 0..100 {
            let (u, ch) = (Uuid::now_v7(), Uuid::now_v7());
            assert_eq!(c.admit(u, ch, t0), Admit::SendNow);
            assert!(c.take_read_gate(u, ch, t0));
        }
        // One woken, unread entry: its read gate keeps it past the idle TTL.
        let (gu, gch) = (Uuid::now_v7(), Uuid::now_v7());
        c.admit(gu, gch, t0);
        assert_eq!(c.entry_count(), 101);

        // The sweep runs on every 256th admit.
        let later = t0 + IDLE_TTL + ms(1);
        let probe_user = Uuid::now_v7();
        let probe = Uuid::now_v7();
        for _ in 0..SWEEP_EVERY {
            c.admit(probe_user, probe, later);
        }
        assert_eq!(c.entry_count(), 2, "gated entry + probe");
        assert_eq!(c.rate_user_count(), 1, "only the probe user sent recently");
        assert!(c.take_read_gate(gu, gch, later), "gate survived the sweep");

        // Once idle with no live gate, everything old goes.
        let much_later = later + READ_GATE + ms(1);
        let probe2 = Uuid::now_v7();
        for _ in 0..SWEEP_EVERY {
            c.admit(probe_user, probe2, much_later);
        }
        assert_eq!(c.entry_count(), 1, "only the fresh probe entry remains");
    }

    #[test]
    fn read_entries_are_collected_after_idle_ttl() {
        let (c, _, _, t0) = ids();
        for _ in 0..50 {
            let (u, ch) = (Uuid::now_v7(), Uuid::now_v7());
            c.admit(u, ch, t0);
            assert!(c.take_read_gate(u, ch, t0));
        }
        assert_eq!(c.entry_count(), 50);
        let later = t0 + IDLE_TTL + ms(1);
        let (probe_user, probe) = (Uuid::now_v7(), Uuid::now_v7());
        for _ in 0..SWEEP_EVERY {
            c.admit(probe_user, probe, later);
        }
        assert_eq!(c.entry_count(), 1);
    }
}
