//! Push transports configured for this server instance, plus the in-memory
//! coalescer (single replica, see [`super::coalesce`]).

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
                    coalescer: Coalescer::new(),
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
    /// Admit one call wake for `user_id`: call wakes bypass the coalescing
    /// window but count toward the per-user cap. Returns whether it may go out.
    #[must_use]
    pub fn admit_call(&self, user_id: Uuid) -> bool {
        self.coalescer.admit_direct(user_id, Instant::now())
    }

    /// Send a call wake to every device of `user_id` in a spawned task, if the
    /// per-user cap allows it. Never blocks; errors are logged without secrets.
    pub fn notify_call(self: &Arc<Self>, db: &sqlx::PgPool, user_id: Uuid, wake: super::CallWake) {
        if self.admit_call(user_id) {
            self.send_call(db, user_id, wake);
        } else {
            tracing::debug!(user_id = %user_id, "push: per-user cap reached; call wake dropped");
        }
    }

    /// Send a call wake already admitted with [`Self::admit_call`], in a
    /// spawned task.
    pub fn send_call(self: &Arc<Self>, db: &sqlx::PgPool, user_id: Uuid, wake: super::CallWake) {
        let (push, db) = (Arc::clone(self), db.clone());
        tokio::spawn(async move {
            if let Err(e) = super::deliver_with(&db, &push, user_id, &wake).await {
                tracing::warn!(error = %e, user_id = %user_id, "push: call wake delivery failed");
            }
        });
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
