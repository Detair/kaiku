//! Mobile push notifications: a provider-agnostic, **content-free** wake-signal
//! dispatcher.
//!
//! A push never carries message text — only a type + routing (`channel_id`, count)
//! — so the transport (a self-hosted `UnifiedPush` distributor, later FCM) never
//! sees content. The app wakes and syncs over its normal authenticated (and
//! E2EE-decrypting) channel. User-supplied endpoints go through the shared SSRF
//! guard before every send, identical to webhooks.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;
use validator::Validate;

use crate::api::AppState;
use crate::auth::AuthUser;

/// A content-free wake signal. `t` is the notification kind; no sender/text.
#[derive(Debug, Clone, Serialize)]
pub struct WakeSignal {
    pub t: &'static str,
    pub channel_id: Uuid,
    pub count: u32,
}

impl WakeSignal {
    pub const fn dm(channel_id: Uuid, count: u32) -> Self {
        Self {
            t: "dm",
            channel_id,
            count,
        }
    }
    pub const fn mention(channel_id: Uuid, count: u32) -> Self {
        Self {
            t: "mention",
            channel_id,
            count,
        }
    }
    pub const fn call(channel_id: Uuid) -> Self {
        Self {
            t: "call",
            channel_id,
            count: 1,
        }
    }
}

/// A stored push subscription (one row per device).
#[derive(Debug, Clone, FromRow, Serialize, utoipa::ToSchema)]
pub struct PushSubscription {
    pub id: Uuid,
    #[serde(skip)]
    pub user_id: Uuid,
    pub provider: String,
    pub endpoint: String,
    #[serde(skip)]
    pub public_key: Option<String>,
    #[serde(skip)]
    pub auth_key: Option<String>,
    pub device_label: Option<String>,
    pub created_at: DateTime<Utc>,
    pub last_used_at: Option<DateTime<Utc>>,
}

/// Outcome of a single send.
#[derive(Debug, PartialEq, Eq)]
pub enum Delivery {
    Sent,
    /// The endpoint is dead (404/410) — prune the subscription.
    Gone,
    /// Transient/other failure — keep the subscription, try again next time.
    Failed,
}

// ============================================================================
// Dispatch
// ============================================================================

/// Deliver a wake signal to all of a user's devices. Spawned off the hot path so
/// push latency never blocks message handling. Prunes subscriptions that report
/// `Gone`.
pub fn spawn_dispatch(state: AppState, user_id: Uuid, signal: WakeSignal) {
    tokio::spawn(async move {
        if let Err(e) = dispatch(&state, user_id, &signal).await {
            tracing::warn!(error = %e, user_id = %user_id, "push dispatch failed");
        }
    });
}

async fn dispatch(state: &AppState, user_id: Uuid, signal: &WakeSignal) -> Result<(), sqlx::Error> {
    let subs: Vec<PushSubscription> = sqlx::query_as(
        "SELECT id, user_id, provider, endpoint, public_key, auth_key, device_label,
                created_at, last_used_at
         FROM push_subscriptions WHERE user_id = $1",
    )
    .bind(user_id)
    .fetch_all(&state.db)
    .await?;

    for sub in subs {
        let delivery = send_one(&state.http_client, &sub, signal).await;
        match delivery {
            Delivery::Gone => {
                let _ = sqlx::query("DELETE FROM push_subscriptions WHERE id = $1")
                    .bind(sub.id)
                    .execute(&state.db)
                    .await;
            }
            Delivery::Sent => {
                let _ =
                    sqlx::query("UPDATE push_subscriptions SET last_used_at = NOW() WHERE id = $1")
                        .bind(sub.id)
                        .execute(&state.db)
                        .await;
            }
            Delivery::Failed => {}
        }
    }
    Ok(())
}

/// Send to one subscription, dispatching on its provider. New providers (FCM,
/// APNs) add a match arm here.
async fn send_one(
    client: &reqwest::Client,
    sub: &PushSubscription,
    signal: &WakeSignal,
) -> Delivery {
    match sub.provider.as_str() {
        "unifiedpush" => send_unifiedpush(client, &sub.endpoint, signal).await,
        other => {
            tracing::warn!(provider = %other, "unknown push provider; skipping");
            Delivery::Failed
        }
    }
}

/// `UnifiedPush`: POST the content-free JSON to the distributor endpoint. The
/// endpoint is user-supplied, so it goes through the SSRF guard (private-IP /
/// DNS-rebind protection) before the request — identical to webhook delivery.
async fn send_unifiedpush(
    client: &reqwest::Client,
    endpoint: &str,
    signal: &WakeSignal,
) -> Delivery {
    // SSRF guard: reject internal/private targets.
    if crate::webhooks::ssrf::verify_resolved_ip(endpoint)
        .await
        .is_err()
    {
        tracing::warn!("push endpoint blocked by SSRF guard");
        return Delivery::Failed;
    }

    let body = match serde_json::to_vec(signal) {
        Ok(b) => b,
        Err(_) => return Delivery::Failed,
    };
    match client
        .post(endpoint)
        .header("content-type", "application/json")
        .body(body)
        .send()
        .await
    {
        Ok(resp) => {
            let status = resp.status();
            if status == reqwest::StatusCode::NOT_FOUND || status == reqwest::StatusCode::GONE {
                Delivery::Gone
            } else if status.is_success() {
                Delivery::Sent
            } else {
                Delivery::Failed
            }
        }
        Err(_) => Delivery::Failed,
    }
}

// ============================================================================
// Subscription CRUD (user-scoped)
// ============================================================================

#[derive(Debug, thiserror::Error)]
pub enum PushError {
    #[error("Subscription not found")]
    NotFound,
    #[error("Validation failed: {0}")]
    Validation(String),
    #[error("Database error")]
    Database(#[from] sqlx::Error),
}

impl IntoResponse for PushError {
    fn into_response(self) -> Response {
        if let Self::Database(e) = &self {
            tracing::error!(error = %e, "Push subscription database operation failed");
        }
        let (status, code, msg) = match &self {
            Self::NotFound => (
                StatusCode::NOT_FOUND,
                "SUBSCRIPTION_NOT_FOUND",
                self.to_string(),
            ),
            Self::Validation(_) => (
                StatusCode::BAD_REQUEST,
                "VALIDATION_ERROR",
                self.to_string(),
            ),
            Self::Database(_) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "INTERNAL_ERROR",
                "Database error".to_string(),
            ),
        };
        (
            status,
            Json(serde_json::json!({ "error": code, "message": msg })),
        )
            .into_response()
    }
}

#[derive(Debug, Deserialize, Validate, utoipa::ToSchema)]
pub struct CreateSubscriptionRequest {
    #[validate(length(min = 1, max = 32))]
    pub provider: String,
    #[validate(length(min = 1, max = 2048))]
    pub endpoint: String,
    pub public_key: Option<String>,
    pub auth_key: Option<String>,
    #[validate(length(max = 64))]
    pub device_label: Option<String>,
}

/// `POST /api/me/push-subscriptions` — register (upsert) a device.
pub async fn create_subscription(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(body): Json<CreateSubscriptionRequest>,
) -> Result<(StatusCode, Json<PushSubscription>), PushError> {
    body.validate()
        .map_err(|e| PushError::Validation(crate::validation::format_validation_errors(&e)))?;

    let sub: PushSubscription = sqlx::query_as(
        "INSERT INTO push_subscriptions (user_id, provider, endpoint, public_key, auth_key, device_label)
         VALUES ($1, $2, $3, $4, $5, $6)
         ON CONFLICT (user_id, endpoint) DO UPDATE SET
             provider = EXCLUDED.provider, public_key = EXCLUDED.public_key,
             auth_key = EXCLUDED.auth_key, device_label = EXCLUDED.device_label
         RETURNING id, user_id, provider, endpoint, public_key, auth_key, device_label,
                   created_at, last_used_at",
    )
    .bind(auth.id)
    .bind(&body.provider)
    .bind(&body.endpoint)
    .bind(&body.public_key)
    .bind(&body.auth_key)
    .bind(&body.device_label)
    .fetch_one(&state.db)
    .await?;

    Ok((StatusCode::CREATED, Json(sub)))
}

/// `GET /api/me/push-subscriptions` — list the caller's devices.
pub async fn list_subscriptions(
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<Json<Vec<PushSubscription>>, PushError> {
    let subs: Vec<PushSubscription> = sqlx::query_as(
        "SELECT id, user_id, provider, endpoint, public_key, auth_key, device_label,
                created_at, last_used_at
         FROM push_subscriptions WHERE user_id = $1 ORDER BY created_at DESC",
    )
    .bind(auth.id)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(subs))
}

/// `DELETE /api/me/push-subscriptions/{id}` — unregister (own device only).
pub async fn delete_subscription(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(sub_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, PushError> {
    let affected = sqlx::query("DELETE FROM push_subscriptions WHERE id = $1 AND user_id = $2")
        .bind(sub_id)
        .bind(auth.id)
        .execute(&state.db)
        .await?
        .rows_affected();
    if affected == 0 {
        return Err(PushError::NotFound);
    }
    Ok(Json(serde_json::json!({ "deleted": true, "id": sub_id })))
}

/// Router mounted at `/api/me/push-subscriptions`.
pub fn router() -> axum::Router<AppState> {
    use axum::routing::{delete, get};
    axum::Router::new()
        .route("/", get(list_subscriptions).post(create_subscription))
        .route("/{id}", delete(delete_subscription))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wake_signal_is_content_free() {
        let sig = WakeSignal::dm(Uuid::nil(), 3);
        let v = serde_json::to_value(&sig).unwrap();
        assert_eq!(v["t"], "dm");
        assert_eq!(v["count"], 3);
        assert!(v.get("channel_id").is_some());
        let obj = v.as_object().unwrap();
        assert_eq!(
            obj.len(),
            3,
            "wake signal must carry ONLY t/channel_id/count"
        );
        assert!(obj.get("content").is_none());
        assert!(obj.get("body").is_none());
        assert!(obj.get("sender").is_none());
    }

    #[test]
    fn wake_signal_kinds() {
        assert_eq!(WakeSignal::mention(Uuid::nil(), 2).t, "mention");
        assert_eq!(WakeSignal::call(Uuid::nil()).t, "call");
    }
}
