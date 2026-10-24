//! Mobile push notifications: a provider-agnostic, **content-free** wake-signal
//! dispatcher.
//!
//! A push never carries message text — only a type + routing (`channel_id`, count)
//! — so the transport never sees content. The app wakes and syncs over its
//! normal authenticated (and E2EE-decrypting) channel.
//!
//! Transports (`send_one` dispatches on the subscription's `provider`):
//! - `unifiedpush`: a user-supplied distributor URL. It goes through the shared SSRF guard before
//!   every send, identical to webhooks.
//! - `fcm`: Firebase Cloud Messaging HTTP v1 ([`fcm`]), enabled only when a service account is
//!   configured ([`service::PushService`]). Its hosts are fixed, so the SSRF guard does not apply.
//!
//! Wire format: contract C20 (`dm`, `mention`, `read`; `call` reserved).

pub mod fcm;
pub mod service;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
pub use service::PushService;
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
    /// The channel was read on another device: clear its notification.
    pub const fn read(channel_id: Uuid) -> Self {
        Self {
            t: "read",
            channel_id,
            count: 0,
        }
    }
    pub const fn call(channel_id: Uuid) -> Self {
        Self {
            t: "call",
            channel_id,
            count: 1,
        }
    }

    /// Whether the transport should deliver this signal with high priority
    /// (wake the device immediately). Only `read` is normal priority.
    #[must_use]
    pub fn is_high_priority(&self) -> bool {
        self.t != "read"
    }
}

/// A stored push subscription (one row per device).
///
/// `Debug` is hand-written: `endpoint` (an FCM registration token or a
/// `UnifiedPush` URL) and the keys are secrets and never printed.
#[derive(Clone, FromRow, Serialize, utoipa::ToSchema)]
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
    /// Stable per-installation id (not a secret); `None` for older clients.
    pub device_id: Option<Uuid>,
}

impl std::fmt::Debug for PushSubscription {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PushSubscription")
            .field("id", &self.id)
            .field("user_id", &self.user_id)
            .field("provider", &self.provider)
            .field("device_label", &self.device_label)
            .field("device_id", &self.device_id)
            .field("created_at", &self.created_at)
            .field("last_used_at", &self.last_used_at)
            .finish_non_exhaustive()
    }
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
/// push latency never blocks message handling.
pub fn spawn_dispatch(state: AppState, user_id: Uuid, signal: WakeSignal) {
    tokio::spawn(async move {
        if let Err(e) = deliver(&state, user_id, &signal).await {
            tracing::warn!(error = %e, user_id = %user_id, "push dispatch failed");
        }
    });
}

/// Deliver a wake signal to every subscription of `user_id`. Subscriptions that
/// report `Gone` are pruned; successful ones get `last_used_at` updated.
///
/// # Errors
/// Loading the user's subscriptions failed.
pub async fn deliver(
    state: &AppState,
    user_id: Uuid,
    signal: &WakeSignal,
) -> Result<(), sqlx::Error> {
    let subs: Vec<PushSubscription> = sqlx::query_as(
        "SELECT id, user_id, provider, endpoint, public_key, auth_key, device_label,
                created_at, last_used_at, device_id
         FROM push_subscriptions WHERE user_id = $1",
    )
    .bind(user_id)
    .fetch_all(&state.db)
    .await?;

    for sub in subs {
        let delivery = send_one(state, &sub, signal).await;
        crate::observability::metrics::record_push_delivery(
            provider_label(&sub.provider),
            match delivery {
                Delivery::Sent => "sent",
                Delivery::Gone => "gone",
                Delivery::Failed => "failed",
            },
        );
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

/// Bounded metric label for a provider string read from the database.
fn provider_label(provider: &str) -> &'static str {
    match provider {
        "unifiedpush" => "unifiedpush",
        "fcm" => "fcm",
        _ => "other",
    }
}

/// Send to one subscription, dispatching on its provider. New providers (APNs)
/// add a match arm here.
async fn send_one(state: &AppState, sub: &PushSubscription, signal: &WakeSignal) -> Delivery {
    // A user-supplied endpoint (UnifiedPush) must be validated (https only,
    // SSRF guard, pinned client) before sending. FCM talks to fixed hosts.
    match sub.provider.as_str() {
        "unifiedpush" => send_unifiedpush(&sub.endpoint, signal).await,
        "fcm" => {
            if let Some(client) = state.push.fcm() {
                client.send(&sub.endpoint, signal).await
            } else {
                // Keep the row: FCM may be configured again later.
                tracing::warn!("push: fcm subscription but FCM is not configured; skipping");
                Delivery::Failed
            }
        }
        other => {
            tracing::warn!(provider = %other, "unknown push provider; skipping");
            Delivery::Failed
        }
    }
}

fn endpoint_is_https(endpoint: &str) -> bool {
    reqwest::Url::parse(endpoint).is_ok_and(|u| u.scheme() == "https")
}

/// `UnifiedPush`: POST the content-free JSON to the distributor endpoint. The
/// endpoint is user-supplied, so it goes through the SSRF guard (private-IP /
/// DNS-rebind protection) before the request — identical to webhook delivery.
async fn send_unifiedpush(endpoint: &str, signal: &WakeSignal) -> Delivery {
    // Legacy rows may hold plain-http endpoints; never deliver to them and let
    // the caller prune the subscription.
    if !endpoint_is_https(endpoint) {
        tracing::warn!("push endpoint is not https; dropping subscription");
        return Delivery::Gone;
    }
    // SSRF guard: reject internal/private targets and connect only to the
    // address that passed validation; redirects are never followed.
    let verified = match crate::webhooks::ssrf::verify_resolved_ip(endpoint).await {
        Ok(v) => v,
        Err(crate::webhooks::ssrf::SsrfError::Blocked(_)) => {
            // Malformed or private/reserved target: it will never become valid,
            // so let the caller prune the subscription.
            tracing::warn!("push endpoint blocked by SSRF guard; dropping subscription");
            return Delivery::Gone;
        }
        Err(crate::webhooks::ssrf::SsrfError::Resolution(_)) => {
            tracing::warn!("push endpoint could not be resolved; will retry later");
            return Delivery::Failed;
        }
    };
    let Ok(client) =
        crate::webhooks::ssrf::pinned_client(&verified, std::time::Duration::from_secs(10))
    else {
        return Delivery::Failed;
    };

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
            } else if status.is_redirection() {
                tracing::warn!("push endpoint answered with a redirect; not followed");
                Delivery::Failed
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
    #[error("Push provider unavailable")]
    ProviderUnavailable,
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
            Self::ProviderUnavailable => (
                StatusCode::BAD_REQUEST,
                "PUSH_PROVIDER_UNAVAILABLE",
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
    #[validate(length(min = 1, max = 4096))]
    pub endpoint: String,
    pub public_key: Option<String>,
    pub auth_key: Option<String>,
    #[validate(length(max = 64))]
    pub device_label: Option<String>,
    /// Stable per-installation id (C19). Optional for older clients.
    pub device_id: Option<Uuid>,
}

/// Maximum push subscriptions kept per user; least recently used are pruned.
pub const MAX_SUBSCRIPTIONS_PER_USER: i64 = 10;

/// FCM registration token shape (C19).
fn is_valid_fcm_token(token: &str) -> bool {
    (20..=4096).contains(&token.len())
        && token
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b':' | b'-'))
}

/// Public FCM settings the Android app compares with its baked-in project.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct FcmPublicConfig {
    pub project_id: String,
}

/// Response of `GET /api/me/push-subscriptions/config` (C18).
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct PushConfigResponse {
    pub providers: Vec<&'static str>,
    pub fcm: Option<FcmPublicConfig>,
}

/// `GET /api/me/push-subscriptions/config` — which transports this server offers.
pub async fn get_config(
    State(state): State<AppState>,
    _auth: AuthUser,
) -> Json<PushConfigResponse> {
    match state.push.fcm() {
        Some(fcm) => Json(PushConfigResponse {
            providers: vec!["fcm", "unifiedpush"],
            fcm: Some(FcmPublicConfig {
                project_id: fcm.project_id().to_string(),
            }),
        }),
        None => Json(PushConfigResponse {
            providers: vec!["unifiedpush"],
            fcm: None,
        }),
    }
}

/// `POST /api/me/push-subscriptions` — register (upsert) a device.
pub async fn create_subscription(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(body): Json<CreateSubscriptionRequest>,
) -> Result<(StatusCode, Json<PushSubscription>), PushError> {
    body.validate()
        .map_err(|e| PushError::Validation(crate::validation::format_validation_errors(&e)))?;

    match body.provider.as_str() {
        "unifiedpush" => {
            let parsed = reqwest::Url::parse(&body.endpoint)
                .map_err(|_| PushError::Validation("endpoint must be a valid URL".into()))?;
            if parsed.scheme() != "https" {
                return Err(PushError::Validation("endpoint must use https".into()));
            }
            match parsed.host_str() {
                Some(h) if !crate::webhooks::ssrf::is_blocked_host(h) => {}
                _ => return Err(PushError::Validation("endpoint host is not allowed".into())),
            }
        }
        "fcm" => {
            if state.push.fcm().is_none() {
                return Err(PushError::ProviderUnavailable);
            }
            // The token is a secret: the message never echoes it.
            if !is_valid_fcm_token(&body.endpoint) {
                return Err(PushError::Validation(
                    "invalid FCM registration token".into(),
                ));
            }
        }
        _ => return Err(PushError::Validation("unsupported push provider".into())),
    }

    let mut tx = state.db.begin().await?;
    // Serialize registrations per user (cap race, same-endpoint races).
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1::text, 0))")
        .bind(auth.id)
        .execute(&mut *tx)
        .await?;
    // A handed-down phone: the same (provider, endpoint) under other users is
    // removed. Matching only on the endpoint proves possession; a device_id is
    // client-asserted and never triggers cross-user deletes.
    sqlx::query(
        "DELETE FROM push_subscriptions
         WHERE provider = $1 AND endpoint = $2 AND user_id <> $3",
    )
    .bind(&body.provider)
    .bind(&body.endpoint)
    .bind(auth.id)
    .execute(&mut *tx)
    .await?;
    let sub: PushSubscription = if let Some(device_id) = body.device_id {
        // A device holds exactly one subscription per user.
        sqlx::query(
            "DELETE FROM push_subscriptions
             WHERE user_id = $1 AND endpoint = $2 AND device_id IS DISTINCT FROM $3",
        )
        .bind(auth.id)
        .bind(&body.endpoint)
        .bind(device_id)
        .execute(&mut *tx)
        .await?;
        sqlx::query_as(
            "INSERT INTO push_subscriptions
                 (user_id, provider, endpoint, public_key, auth_key, device_label, device_id)
             VALUES ($1, $2, $3, $4, $5, $6, $7)
             ON CONFLICT (user_id, device_id) WHERE device_id IS NOT NULL DO UPDATE SET
                 provider = EXCLUDED.provider, endpoint = EXCLUDED.endpoint,
                 public_key = EXCLUDED.public_key, auth_key = EXCLUDED.auth_key,
                 device_label = EXCLUDED.device_label
             RETURNING id, user_id, provider, endpoint, public_key, auth_key, device_label,
                       created_at, last_used_at, device_id",
        )
        .bind(auth.id)
        .bind(&body.provider)
        .bind(&body.endpoint)
        .bind(&body.public_key)
        .bind(&body.auth_key)
        .bind(&body.device_label)
        .bind(device_id)
        .fetch_one(&mut *tx)
        .await?
    } else {
        sqlx::query_as(
            "INSERT INTO push_subscriptions (user_id, provider, endpoint, public_key, auth_key, device_label)
             VALUES ($1, $2, $3, $4, $5, $6)
             ON CONFLICT (user_id, endpoint) DO UPDATE SET
                 provider = EXCLUDED.provider, public_key = EXCLUDED.public_key,
                 auth_key = EXCLUDED.auth_key, device_label = EXCLUDED.device_label
             RETURNING id, user_id, provider, endpoint, public_key, auth_key, device_label,
                       created_at, last_used_at, device_id",
        )
        .bind(auth.id)
        .bind(&body.provider)
        .bind(&body.endpoint)
        .bind(&body.public_key)
        .bind(&body.auth_key)
        .bind(&body.device_label)
        .fetch_one(&mut *tx)
        .await?
    };

    // Keep the most recently used rows; the row just written is never pruned.
    sqlx::query(
        "DELETE FROM push_subscriptions WHERE id IN (
             SELECT id FROM push_subscriptions WHERE user_id = $1 AND id <> $2
             ORDER BY COALESCE(last_used_at, created_at) DESC, id
             OFFSET $3)",
    )
    .bind(auth.id)
    .bind(sub.id)
    .bind(MAX_SUBSCRIPTIONS_PER_USER - 1)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;

    Ok((StatusCode::CREATED, Json(sub)))
}

/// `GET /api/me/push-subscriptions` — list the caller's devices.
pub async fn list_subscriptions(
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<Json<Vec<PushSubscription>>, PushError> {
    let subs: Vec<PushSubscription> = sqlx::query_as(
        "SELECT id, user_id, provider, endpoint, public_key, auth_key, device_label,
                created_at, last_used_at, device_id
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

/// `DELETE /api/me/push-subscriptions/device/{device_id}` — unregister a device
/// (own rows only). Always 204, also when nothing matched.
pub async fn delete_device(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(device_id): Path<Uuid>,
) -> Result<StatusCode, PushError> {
    sqlx::query("DELETE FROM push_subscriptions WHERE device_id = $1 AND user_id = $2")
        .bind(device_id)
        .bind(auth.id)
        .execute(&state.db)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Router mounted at `/api/me/push-subscriptions`.
pub fn router() -> axum::Router<AppState> {
    use axum::routing::{delete, get};
    axum::Router::new()
        .route("/", get(list_subscriptions).post(create_subscription))
        .route("/config", get(get_config))
        .route("/device/{device_id}", delete(delete_device))
        .route("/{id}", delete(delete_subscription))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn non_https_endpoint_is_gone() {
        let sig = WakeSignal::dm(Uuid::nil(), 1);
        assert!(matches!(
            send_unifiedpush("http://push.example/x", &sig).await,
            Delivery::Gone
        ));
        assert!(matches!(
            send_unifiedpush("not a url", &sig).await,
            Delivery::Gone
        ));
    }

    #[tokio::test]
    async fn private_endpoint_is_gone() {
        let sig = WakeSignal::dm(Uuid::nil(), 1);
        for ep in [
            "https://127.0.0.1/x",
            "https://10.0.0.5/x",
            "https://[::1]/x",
            "https://[ff02::1]/x",
        ] {
            assert!(
                matches!(send_unifiedpush(ep, &sig).await, Delivery::Gone),
                "{ep} must be pruned"
            );
        }
    }

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
    fn subscription_debug_omits_endpoint_and_keys() {
        let sub = PushSubscription {
            id: Uuid::nil(),
            user_id: Uuid::nil(),
            provider: "fcm".into(),
            endpoint: "SENTINEL-FCM-TOKEN-1234".into(),
            public_key: Some("SENTINEL-PUBLIC-KEY".into()),
            auth_key: Some("SENTINEL-AUTH-KEY".into()),
            device_label: Some("Pixel".into()),
            created_at: Utc::now(),
            last_used_at: None,
            device_id: Some(Uuid::nil()),
        };
        let dbg = format!("{sub:?}");
        assert!(!dbg.contains("SENTINEL"), "{dbg}");
        assert!(dbg.contains("fcm") && dbg.contains("Pixel"));
    }

    #[test]
    fn wake_signal_kinds() {
        assert_eq!(WakeSignal::mention(Uuid::nil(), 2).t, "mention");
        let read = WakeSignal::read(Uuid::nil());
        assert_eq!((read.t, read.count), ("read", 0));
        assert!(!read.is_high_priority());
        assert!(WakeSignal::dm(Uuid::nil(), 1).is_high_priority());
        assert!(WakeSignal::mention(Uuid::nil(), 1).is_high_priority());
        assert_eq!(WakeSignal::call(Uuid::nil()).t, "call");
    }
}
