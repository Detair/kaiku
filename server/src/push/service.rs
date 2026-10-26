//! Push transports configured for this server instance, plus the in-memory
//! coalescer (single replica, see [`super::coalesce`]).

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use uuid::Uuid;

use super::coalesce::{Admit, Coalescer};
use super::fcm::FcmClient;
use super::WakeSignal;
use crate::api::AppState;
use crate::config::Config;

/// Holds the configured push transports and the coalescer. `UnifiedPush`
/// needs no server-side state; FCM is present only when a service account is
/// configured.
#[derive(Debug, Default)]
pub struct PushService {
    fcm: Option<Arc<FcmClient>>,
    coalescer: Coalescer,
    /// Per-call locks ordering `ringing` before `ended` ([`super::call`]).
    call_locks: std::sync::Mutex<HashMap<Uuid, Arc<tokio::sync::Mutex<()>>>>,
}

impl PushService {
    /// Build from config. A key that cannot be used was already rejected by
    /// `Config::from_env`; if one slips through (hand-built config), FCM stays
    /// off and an error is logged rather than panicking.
    #[must_use]
    pub fn from_config(config: &Config) -> Self {
        let Some(key) = config.fcm_service_account.clone() else {
            tracing::info!("push: FCM disabled (no service account)");
            return Self::default();
        };
        let project_id = key.project_id.clone();
        match FcmClient::new(key, &config.fcm_api_base) {
            Ok(client) => {
                tracing::info!("push: FCM enabled for project {project_id}");
                Self {
                    fcm: Some(Arc::new(client)),
                    ..Self::default()
                }
            }
            Err(e) => {
                tracing::error!(error = %e, "push: FCM disabled (service account unusable)");
                Self::default()
            }
        }
    }

    /// The FCM client, when FCM is configured.
    #[must_use]
    pub fn fcm(&self) -> Option<&FcmClient> {
        self.fcm.as_deref()
    }

    /// Wake `user_id` for a new message, coalesced per (user, channel)
    /// (contract C20). Never blocks: delivery and the trailing-flush timer run
    /// in spawned tasks, and the coalescer lock is never held across `.await`.
    pub fn notify(&self, state: &AppState, user_id: Uuid, signal: WakeSignal) {
        match self
            .coalescer
            .admit(user_id, signal.channel_id, Instant::now())
        {
            Admit::SendNow => {
                spawn_deliver(state.clone(), user_id, WakeSignal { count: 1, ..signal });
            }
            Admit::Deferred {
                flush_at,
                schedule: true,
            } => {
                let state = state.clone();
                tokio::spawn(async move {
                    tokio::time::sleep_until(tokio::time::Instant::from_std(flush_at)).await;
                    if let Some(count) =
                        state
                            .push
                            .coalescer
                            .flush(user_id, signal.channel_id, Instant::now())
                    {
                        deliver_logged(&state, user_id, &WakeSignal { count, ..signal }).await;
                    }
                });
            }
            Admit::Deferred {
                schedule: false, ..
            } => {}
            Admit::Dropped => {
                tracing::debug!(user_id = %user_id, "push: per-user cap reached; wake dropped");
            }
        }
    }

    /// The user read `channel_id`: tell their phones to clear its
    /// notification, but only after a wake that was not cleared yet.
    pub fn notify_read(&self, state: &AppState, user_id: Uuid, channel_id: Uuid) {
        if self
            .coalescer
            .take_read_gate(user_id, channel_id, Instant::now())
        {
            spawn_deliver(state.clone(), user_id, WakeSignal::read(channel_id));
        }
    }
}

impl PushService {
    /// Admit one call `ringing` wake for `user_id` (own budget per callee;
    /// message wakes are not affected). Returns whether it may go out.
    #[must_use]
    pub fn admit_ringing(&self, user_id: Uuid) -> bool {
        self.admit_ringing_at(user_id, Instant::now())
    }

    pub(crate) fn admit_ringing_at(&self, user_id: Uuid, now: Instant) -> bool {
        self.coalescer.admit_ringing(user_id, now)
    }

    /// Give back a ringing slot admitted at `at` that was not used.
    pub(crate) fn refund_ringing(&self, user_id: Uuid, at: Instant) {
        self.coalescer.refund_ringing(user_id, at);
    }

    /// The per-call lock that orders a call's `ringing` before its `ended`
    /// (single replica). `create` inserts it if missing.
    pub(crate) fn call_lock(
        &self,
        call_id: Uuid,
        create: bool,
    ) -> Option<Arc<tokio::sync::Mutex<()>>> {
        let mut locks = self
            .call_locks
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if create {
            Some(Arc::clone(locks.entry(call_id).or_default()))
        } else {
            locks.get(&call_id).cloned()
        }
    }

    /// Forget the per-call lock (the ringing deliveries are done).
    pub(crate) fn drop_call_lock(&self, call_id: Uuid) {
        self.call_locks
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(&call_id);
    }
}

/// Deliver one call wake to every device of `user_id`; errors are logged
/// without secrets.
pub(crate) async fn deliver_call(
    db: &sqlx::PgPool,
    push: &PushService,
    user_id: Uuid,
    wake: &super::CallWake,
) {
    if let Err(e) = super::deliver_with(db, push, user_id, wake).await {
        tracing::warn!(error = %e, user_id = %user_id, "push: call wake delivery failed");
    }
}

fn spawn_deliver(state: AppState, user_id: Uuid, signal: WakeSignal) {
    tokio::spawn(async move { deliver_logged(&state, user_id, &signal).await });
}

async fn deliver_logged(state: &AppState, user_id: Uuid, signal: &WakeSignal) {
    if let Err(e) = super::deliver(state, user_id, signal).await {
        tracing::warn!(error = %e, user_id = %user_id, "push delivery failed");
    }
}
