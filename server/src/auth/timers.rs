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
    let expired = match state.db.acquire().await {
        Ok(mut conn) => match crate::approvals::core::expire_due(&mut conn).await {
            // 3. Enrolments whose setup token or code is past due (SM2).
            Ok(_) => match crate::approvals::enrollments::expire_due(&mut conn).await {
                // 4. Login attempts: dead claims reopen, past due expire (SM1).
                Ok(_) => match crate::auth::login_attempts::core::expire_due(&mut conn).await {
                    // 5. Accounts whose registration ticket bound nothing (F8).
                    Ok(_) => crate::auth::registration::delete_unbound_due(&mut conn).await,
                    Err(e) => Err(e),
                },
                Err(e) => Err(e),
            },
            Err(e) => Err(e),
        },
        Err(e) => Err(e),
    };
    if let Err(e) = expired {
        tracing::warn!(error = %e, "sweep: challenge expiry failed");
        if first_err.is_none() {
            first_err = Some(e.into());
        }
    }

    // 6. Account requests: ready soon, ready, admin completion, expiry (SM3).
    match crate::auth::account_requests::core::timers(&crate::auth::ctx::AuthCtx::from(state)).await
    {
        Ok(events) => crate::security_events::spawn_delivery(state, events),
        Err(e) => {
            tracing::warn!(error = %e, "sweep: account request timers failed");
            first_err.get_or_insert_with(|| anyhow::anyhow!(e.to_string()));
        }
    }

    first_err.map_or(Ok(()), Err)
}
