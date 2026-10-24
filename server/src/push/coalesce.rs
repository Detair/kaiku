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
//!   [`READ_GATE`] and was not cleared since ([`Coalescer::take_read_gate`]).
//!
//! State lives in a `std::sync::Mutex` that is never held across an `.await`; all methods are
//! synchronous and take `now` explicitly, so they are pure `Instant` arithmetic.
//!
//! Memory: every 256th `admit` sweeps entries that hold no live state: last activity older
//! than [`IDLE_TTL`] (so no flush is due) and no unexpired read gate (gates live up to
//! [`READ_GATE`]). If the map still exceeds [`MAX_ENTRIES`], gated entries idle for
//! [`IDLE_TTL`] are dropped too (their read signal is lost). Per-user rate entries are dropped
//! once their 60 s history is empty.
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
/// Soft bound on (user, channel) entries; above it read gates are dropped early.
pub const MAX_ENTRIES: usize = 65_536;

const RATE_PERIOD: Duration = Duration::from_mins(1);
const SWEEP_EVERY: u64 = 256;

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

#[derive(Debug, Default)]
struct State {
    entries: HashMap<(Uuid, Uuid), Entry>,
    /// Push instants per user within the last [`RATE_PERIOD`] (at most
    /// [`USER_MAX_PER_MINUTE`] each).
    rate: HashMap<Uuid, VecDeque<Instant>>,
    admits: u64,
}

impl State {
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

    fn sweep(&mut self, now: Instant) {
        // A flush timer fires within WINDOW of the last activity, so an entry
        // idle for IDLE_TTL has no live timer; a late flush on a dropped entry
        // finds nothing and returns None.
        let idle = |e: &Entry| now.saturating_duration_since(e.last_activity) >= IDLE_TTL;
        self.entries.retain(|_, e| {
            let gate_live = e
                .woke_at
                .is_some_and(|t| now.saturating_duration_since(t) < READ_GATE);
            !idle(e) || gate_live
        });
        if self.entries.len() > MAX_ENTRIES {
            self.entries.retain(|_, e| !idle(e));
        }
        self.rate.retain(|_, sent| {
            sent.back()
                .is_some_and(|&t| now.saturating_duration_since(t) < RATE_PERIOD)
        });
    }
}

/// Push coalescer and read gate. See the module docs.
#[derive(Default)]
pub struct Coalescer {
    state: Mutex<State>,
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

        let entry = st.entries.entry(key).or_insert_with(|| Entry::new(now));
        entry.last_activity = now;
        if let Some(window_until) = entry.window_until {
            // Inside the window, or the window ended but its flush has not
            // run yet (late timer): join the pending trailing push.
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

        if !st.try_consume(user, now) {
            return Admit::Dropped;
        }
        if let Some(entry) = st.entries.get_mut(&key) {
            entry.window_until = Some(now + WINDOW);
            entry.woke_at = Some(now);
        }
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

    /// Whether a `read` signal should go out for (`user`, `channel`). True at most once per
    /// wake, and only within [`READ_GATE`] of it. Also discards a pending trailing push: the
    /// user has read those messages.
    pub fn take_read_gate(&self, user: Uuid, channel: Uuid, now: Instant) -> bool {
        let mut st = self.lock();
        let Some(entry) = st.entries.get_mut(&(user, channel)) else {
            return false;
        };
        entry.pending = 0;
        // Without an outstanding timer, the next message pushes immediately.
        // With one, new messages still fold into it (it fires within WINDOW).
        if !entry.scheduled {
            entry.window_until = None;
        }
        entry
            .woke_at
            .take()
            .is_some_and(|t| now.saturating_duration_since(t) < READ_GATE)
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
        // An empty flush yields nothing.
        assert_eq!(c.flush(u, ch, t0 + WINDOW + WINDOW), Some(1));
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
    fn read_discards_pending_trailing_push() {
        let (c, u, ch, t0) = ids();
        c.admit(u, ch, t0);
        c.admit(u, ch, t0 + ms(1));
        assert!(c.take_read_gate(u, ch, t0 + ms(2)));
        assert_eq!(c.flush(u, ch, t0 + WINDOW), None);
        // A new message after the read is collected for the next push again.
        assert!(matches!(
            c.admit(u, ch, t0 + WINDOW + ms(1)),
            Admit::SendNow
        ));
        assert!(c.take_read_gate(u, ch, t0 + WINDOW + ms(2)));
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
