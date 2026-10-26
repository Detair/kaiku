//! Call wakes (contract C20 addendum "call wake"): wake the callees' phones
//! when a DM call starts ringing and again, once, when it ends.
//!
//! - Payload: `{"t":"call","channel_id","call_id","caller_id","call_state":"ringing"|"ended"}`,
//!   plus `reason` (`no_answer` | `all_declined` | `cancelled` | `last_left`) for `ended`. FCM
//!   data-only, every value a string; `UnifiedPush` gets the same JSON. No display name, no
//!   content, no count.
//! - Delivery: high priority for both states; `collapse_key` = `call-{channel_id}` (separate from
//!   message wakes, which collapse on the bare channel id); TTL 45 s (`ringing`) / 3600 s
//!   (`ended`).
//! - Recipients: the call's targets (DM participants except the caller) minus users blocked in
//!   either direction (DB-authoritative, as for DM wakes) and minus users over the per-user cap.
//!   The set that got `ringing` is stored in Valkey under `call_push_rung:{call_id}` (7 h); `ended`
//!   goes only to that set.
//! - Exactly once per `call_id`: `ended` swaps the stored set for a tombstone atomically (`SET ...
//!   GET`), and `ringing` stores its set with `NX`, so an `ended` that wins a race against a slow
//!   `ringing` task suppresses that `ringing` instead of leaving a phone ringing.
//! - Both run in spawned tasks (best effort) and bypass coalescing, but count toward the per-user
//!   cap ([`super::PushService::admit_call`]).

use std::sync::Arc;

use fred::prelude::*;
use fred::types::{Expiration, SetOptions};
use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;

use super::{PushPayload, PushService};
use crate::chat::queries::blocked_among;
use crate::voice::call::EndReason;
use crate::voice::call_service::end_reason_name;

/// FCM TTL of a `ringing` wake (a ring is useless once the call timed out).
pub const RINGING_TTL: &str = "45s";
/// FCM TTL of an `ended` wake.
pub const ENDED_TTL: &str = "3600s";
/// Lifetime of the stored `ringing` recipient set. Covers the active-call
/// backstop (6 h) so `last_left` still reaches the rung users.
const RUNG_SET_TTL_SECS: i64 = 7 * 60 * 60;
/// Lifetime of the tombstone written when the call ended.
const TOMBSTONE_TTL_SECS: i64 = 60 * 60;
const TOMBSTONE: &str = "ended";

/// A content-free call wake.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallWake {
    pub channel_id: Uuid,
    pub call_id: Uuid,
    pub caller_id: Uuid,
    /// `None` while ringing, the end reason once ended.
    pub ended: Option<EndReason>,
}

impl CallWake {
    #[must_use]
    pub const fn ringing(channel_id: Uuid, call_id: Uuid, caller_id: Uuid) -> Self {
        Self {
            channel_id,
            call_id,
            caller_id,
            ended: None,
        }
    }

    #[must_use]
    pub const fn ended(
        channel_id: Uuid,
        call_id: Uuid,
        caller_id: Uuid,
        reason: EndReason,
    ) -> Self {
        Self {
            channel_id,
            call_id,
            caller_id,
            ended: Some(reason),
        }
    }

    fn json(&self) -> serde_json::Value {
        let mut v = json!({
            "t": "call",
            "channel_id": self.channel_id.to_string(),
            "call_id": self.call_id.to_string(),
            "caller_id": self.caller_id.to_string(),
            "call_state": if self.ended.is_some() { "ended" } else { "ringing" },
        });
        if let Some(reason) = self.ended {
            v["reason"] = json!(end_reason_name(reason));
        }
        v
    }
}

impl PushPayload for CallWake {
    fn fcm_data(&self) -> serde_json::Value {
        self.json()
    }

    fn unifiedpush_body(&self) -> Vec<u8> {
        serde_json::to_vec(&self.json()).unwrap_or_default()
    }

    fn high_priority(&self) -> bool {
        true
    }

    fn collapse_key(&self) -> String {
        format!("call-{}", self.channel_id)
    }

    fn ttl(&self) -> &'static str {
        if self.ended.is_some() {
            ENDED_TTL
        } else {
            RINGING_TTL
        }
    }
}

fn rung_key(call_id: Uuid) -> String {
    format!("call_push_rung:{call_id}")
}

/// Everything a call wake needs; held by the call service.
#[derive(Debug, Clone)]
pub struct CallPusher {
    pub db: PgPool,
    pub redis: Client,
    pub push: Arc<PushService>,
}

impl CallPusher {
    /// The call `call_id` started ringing `targets`: wake them in a spawned
    /// task. Returns at once.
    pub fn spawn_ringing(&self, channel_id: Uuid, call_id: Uuid, caller: Uuid, targets: Vec<Uuid>) {
        let this = self.clone();
        tokio::spawn(async move {
            if let Err(e) = this.ringing(channel_id, call_id, caller, targets).await {
                tracing::warn!(error = %e, %channel_id, "push: call ringing wake failed");
            }
        });
    }

    /// The call `call_id` ended: wake the users that got `ringing`, once, in a
    /// spawned task. Returns at once.
    pub fn spawn_ended(&self, channel_id: Uuid, call_id: Uuid, caller: Uuid, reason: EndReason) {
        let this = self.clone();
        tokio::spawn(async move {
            if let Err(e) = this.ended(channel_id, call_id, caller, reason).await {
                tracing::warn!(error = %e, %channel_id, "push: call ended wake failed");
            }
        });
    }

    async fn ringing(
        &self,
        channel_id: Uuid,
        call_id: Uuid,
        caller: Uuid,
        targets: Vec<Uuid>,
    ) -> anyhow::Result<()> {
        let targets: Vec<Uuid> = targets.into_iter().filter(|&id| id != caller).collect();
        // A failed block lookup wakes nobody (same rule as DM wakes).
        let blocked = blocked_among(&self.db, caller, &targets).await?;
        // Admit before storing, so a wake dropped by the per-user cap leaves
        // that user out of the set and no `ended` follows for them.
        let recipients: Vec<Uuid> = targets
            .into_iter()
            .filter(|id| !blocked.contains(id) && self.push.admit_call(*id))
            .collect();
        let stored: Option<String> = self
            .redis
            .set(
                rung_key(call_id),
                serde_json::to_string(&recipients)?,
                Some(Expiration::EX(RUNG_SET_TTL_SECS)),
                Some(SetOptions::NX),
                false,
            )
            .await?;
        if stored.is_none() {
            // The call already ended (tombstone): do not ring.
            return Ok(());
        }
        let wake = CallWake::ringing(channel_id, call_id, caller);
        for user in recipients {
            self.push.send_call(&self.db, user, wake.clone());
        }
        Ok(())
    }

    async fn ended(
        &self,
        channel_id: Uuid,
        call_id: Uuid,
        caller: Uuid,
        reason: EndReason,
    ) -> anyhow::Result<()> {
        let previous: Option<String> = self
            .redis
            .set(
                rung_key(call_id),
                TOMBSTONE,
                Some(Expiration::EX(TOMBSTONE_TTL_SECS)),
                None,
                true,
            )
            .await?;
        // Nothing rang yet (the ringing task will see the tombstone), or
        // `ended` already went out for this call.
        let Some(previous) = previous.filter(|p| p != TOMBSTONE) else {
            return Ok(());
        };
        let recipients: Vec<Uuid> = serde_json::from_str(&previous)?;
        let wake = CallWake::ended(channel_id, call_id, caller, reason);
        for user in recipients {
            self.push.notify_call(&self.db, user, wake.clone());
        }
        Ok(())
    }
}
