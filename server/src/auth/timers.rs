//! The minute sweep of the login state machines (spec B.7 rule 3).
//!
//! One task runs [`run_once`] every 60 s. Its steps run in a fixed order;
//! later tasks append steps (expiring challenges, enrolments, …). A failing
//! step is logged and the loop continues; every step is idempotent, so the
//! next tick retries.

use std::time::Duration;

use crate::api::AppState;

/// Interval between two sweeps.
pub const SWEEP_INTERVAL: Duration = Duration::from_mins(1);

/// Start the sweep.
pub fn spawn(state: AppState) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(SWEEP_INTERVAL);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            interval.tick().await;
            if let Err(e) = run_once(&state).await {
                tracing::warn!(error = %e, "auth sweep step failed; retrying next minute");
            }
        }
    })
}

/// One sweep: every step in order. The first failing step's error is
/// returned after all steps ran.
#[tracing::instrument(skip(state))]
pub async fn run_once(state: &AppState) -> anyhow::Result<()> {
    let mut first_err: Option<anyhow::Error> = None;

    // 1. Deliver security notices left undelivered (DC11 outbox).
    match crate::security_events::notices::deliver_pending(state).await {
        Ok(n) if n > 0 => tracing::debug!(count = n, "sweep delivered pending notices"),
        Ok(_) => {}
        Err(e) => {
            tracing::warn!(error = %e, "sweep: notice delivery failed");
            first_err.get_or_insert(e);
        }
    }

    // 2. Expire challenges past their deadline (B.2).
    match state.db.acquire().await {
        Ok(mut conn) => {
            if let Err(e) = crate::approvals::core::expire_due(&mut conn).await {
                tracing::warn!(error = %e, "sweep: challenge expiry failed");
                first_err.get_or_insert_with(|| e.into());
            }
            // 3. Enrolments whose setup token or code is past due (SM2).
            if let Err(e) = crate::approvals::enrollments::expire_due(&mut conn).await {
                tracing::warn!(error = %e, "sweep: enrolment expiry failed");
                first_err.get_or_insert_with(|| e.into());
            }
        }
        Err(e) => {
            tracing::warn!(error = %e, "sweep: no connection for the expiry steps");
            first_err.get_or_insert_with(|| e.into());
        }
    }

    first_err.map_or(Ok(()), Err)
}
