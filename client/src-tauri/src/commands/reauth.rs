//! Cancellable attempts for desktop flows that wait on the system browser.
//!
//! The C24 OIDC re-authentication commands (`oidc_reauth`,
//! `oidc_reauth_cancel`) that lived here are gone: C27 removes
//! `/auth/reauth/oidc/authorize/{provider_id}` (404) and replaces
//! re-authentication with step-up grants (passkey or phone, K18). What stays
//! is the attempt registry: a cancel that may arrive before, during or after
//! the flow, checked before every side effect. The desktop passkey commands
//! (`commands/passkey.rs`) reuse `Attempt`, `registry` and `run_cancellable`.

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use tokio::sync::Notify;

/// One browser-flow attempt: a persistent cancelled flag (checked before
/// every side effect) plus a wake-up for the waiting flow.
#[derive(Default)]
pub struct Attempt {
    cancelled: AtomicBool,
    wake: Notify,
}

impl Attempt {
    fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
        self.wake.notify_one();
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }

    /// `Err` once cancelled; call right before each side effect.
    pub fn check(&self) -> Result<(), String> {
        if self.is_cancelled() {
            Err(CANCELLED.to_string())
        } else {
            Ok(())
        }
    }
}

pub const CANCELLED: &str = "REAUTH_CANCELLED";
/// How many unmatched / finished ids are remembered (bounded on purpose).
const REMEMBERED_IDS: usize = 32;

/// Running attempts by id. A cancel for an unknown id is remembered only if it
/// might still register (IPC gap) and only in a small bounded list; a cancel
/// for a finished id is ignored.
#[derive(Default)]
pub struct Registry {
    running: HashMap<String, Arc<Attempt>>,
    early_cancels: VecDeque<String>,
    finished: VecDeque<String>,
}

fn remember(list: &mut VecDeque<String>, id: &str) {
    if list.len() >= REMEMBERED_IDS {
        list.pop_front();
    }
    list.push_back(id.to_string());
}

impl Registry {
    pub fn register(&mut self, id: &str) -> Result<Arc<Attempt>, String> {
        if self.running.contains_key(id) {
            return Err("REAUTH_FAILED".to_string());
        }
        let attempt = Arc::new(Attempt::default());
        if let Some(pos) = self.early_cancels.iter().position(|e| e == id) {
            self.early_cancels.remove(pos);
            attempt.cancel();
        }
        self.running.insert(id.to_string(), Arc::clone(&attempt));
        Ok(attempt)
    }

    pub fn cancel(&mut self, id: &str) {
        if let Some(attempt) = self.running.get(id) {
            attempt.cancel();
        } else if !self.finished.iter().any(|f| f == id) {
            remember(&mut self.early_cancels, id);
        }
    }

    pub fn finish(&mut self, id: &str) {
        if self.running.remove(id).is_some() {
            remember(&mut self.finished, id);
        }
    }
}

pub fn registry() -> std::sync::MutexGuard<'static, Registry> {
    static REGISTRY: OnceLock<Mutex<Registry>> = OnceLock::new();
    REGISTRY
        .get_or_init(Mutex::default)
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Run `flow` unless the attempt is cancelled. Cancellation is checked first
/// and polled first, so a ready flow cannot win against a pending cancel; a
/// cancelled flow is dropped where it stands (no browser, no listener). The
/// flag is checked once more on the way out, so a cancel that lands while the
/// flow itself is being polled still wins: a cancelled attempt never succeeds.
pub async fn run_cancellable<T>(
    attempt: &Attempt,
    flow: impl std::future::Future<Output = Result<T, String>>,
) -> Result<T, String> {
    attempt.check()?;
    let result = tokio::select! {
        biased;
        () = attempt.wake.notified() => Err(CANCELLED.to_string()),
        result = flow => result,
    };
    attempt.check()?;
    result
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[tokio::test]
    async fn pre_cancelled_ready_flow_never_runs() {
        let mut reg = Registry::default();
        reg.cancel("a1"); // arrives before the attempt registers
        let attempt = reg.register("a1").unwrap();
        let ran = AtomicBool::new(false);
        let result = run_cancellable(&attempt, async {
            ran.store(true, Ordering::SeqCst);
            Ok::<_, String>(1)
        })
        .await;
        assert_eq!(result, Err(CANCELLED.to_string()));
        assert!(!ran.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn cancel_during_flow_drops_it() {
        let mut reg = Registry::default();
        let attempt = reg.register("a2").unwrap();
        let waker = Arc::clone(&attempt);
        let canceller = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(30)).await;
            waker.cancel();
        });
        let result: Result<(), String> = run_cancellable(&attempt, std::future::pending()).await;
        assert_eq!(result, Err(CANCELLED.to_string()));
        canceller.await.unwrap();
    }

    #[test]
    fn side_effect_check_fails_after_cancel() {
        let mut reg = Registry::default();
        let attempt = reg.register("a3").unwrap();
        assert!(attempt.check().is_ok());
        reg.cancel("a3");
        assert_eq!(attempt.check(), Err(CANCELLED.to_string()));
    }

    #[test]
    fn late_cancel_after_finish_leaves_no_entry() {
        let mut reg = Registry::default();
        reg.register("a4").unwrap();
        reg.finish("a4");
        reg.cancel("a4");
        assert!(reg.running.is_empty());
        assert!(reg.early_cancels.is_empty());
    }

    #[test]
    fn unknown_cancels_are_bounded() {
        let mut reg = Registry::default();
        for i in 0..1000 {
            reg.cancel(&format!("unknown-{i}"));
        }
        assert_eq!(reg.early_cancels.len(), REMEMBERED_IDS);
        for i in 0..1000 {
            let id = format!("done-{i}");
            reg.register(&id).unwrap();
            reg.finish(&id);
        }
        assert_eq!(reg.finished.len(), REMEMBERED_IDS);
        assert!(reg.running.is_empty());
    }

    #[test]
    fn duplicate_attempt_id_is_rejected() {
        let mut reg = Registry::default();
        let first = reg.register("dup").unwrap();
        assert!(reg.register("dup").is_err());
        // The first attempt keeps its own state and can still be cancelled.
        reg.cancel("dup");
        assert!(first.is_cancelled());
        reg.finish("dup");
        assert!(reg.register("dup").is_ok());
    }

    #[tokio::test]
    async fn cancel_during_the_flow_poll_never_yields_its_result() {
        // The cancel lands while the flow itself is being polled and the flow
        // completes in that same poll: the result must still be discarded.
        let attempt = Attempt::default();
        let result = run_cancellable(&attempt, async {
            attempt.cancel();
            Ok::<_, String>(7)
        })
        .await;
        assert_eq!(result, Err(CANCELLED.to_string()));
    }

    #[tokio::test]
    async fn finished_flow_wins_without_cancel() {
        let attempt = Attempt::default();
        let result = run_cancellable(&attempt, async { Ok::<_, String>(7) }).await;
        assert_eq!(result, Ok(7));
    }
}
