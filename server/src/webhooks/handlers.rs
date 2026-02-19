//! Webhook API Handlers
//!
//! CRUD endpoints for webhook management. Owner-only enforcement.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use sqlx::PgPool;
use tracing::{info, instrument};
use uuid::Uuid;

use crate::auth::AuthUser;

use super::queries;
use super::signing;
use super::types::*;

/// Verify application ownership and return application ID.
async fn verify_ownership(pool: &PgPool, app_id: Uuid, user_id: Uuid) -> Result<(), WebhookError> {
    let row: Option<(Uuid,)> =
        sqlx::query_as("SELECT owner_id FROM bot_applications WHERE id = $1")
            .bind(app_id)
            .fetch_optional(pool)
            .await
            .map_err(WebhookError::Database)?;

    let (owner_id,) = row.ok_or(WebhookError::ApplicationNotFound)?;
    if owner_id != user_id {
        return Err(WebhookError::Forbidden);
    }

    Ok(())
}

/// Validate a URL for webhook delivery (includes SSRF protection).
fn validate_url(url: &str) -> Result<(), WebhookError> {
    if url.len() < 10 || url.len() > 2048 {
        return Err(WebhookError::Validation(
            "URL must be between 10 and 2048 characters".to_string(),
        ));
    }
    if !url.starts_with("http://") && !url.starts_with("https://") {
        return Err(WebhookError::Validation(
            "URL must start with http:// or https://".to_string(),
        ));
    }

    // SSRF protection: block private/reserved hostnames
    let parsed = reqwest::Url::parse(url).map_err(|_| {
        WebhookError::Validation("Invalid URL format".to_string())
    })?;

    let host = parsed.host_str().ok_or_else(|| {
        WebhookError::Validation("URL must contain a host".to_string())
    })?;

    if super::ssrf::is_blocked_host(host) {
        return Err(WebhookError::Validation(
            "URL must not point to a private or reserved address".to_string(),
        ));
    }

    Ok(())
}

/// POST /api/applications/{app_id}/webhooks
#[instrument(skip(pool, claims))]
pub async fn create_webhook(
    State(pool): State<PgPool>,
    Path(app_id): Path<Uuid>,
    claims: AuthUser,
    Json(req): Json<CreateWebhookRequest>,
) -> Result<(StatusCode, Json<WebhookCreatedResponse>), (StatusCode, String)> {
    verify_ownership(&pool, app_id, claims.id).await?;

    // Validate
    validate_url(&req.url)?;

    if req.subscribed_events.is_empty() {
        return Err(WebhookError::Validation(
            "At least one subscribed event is required".to_string(),
        )
        .into());
    }

    if let Some(ref desc) = req.description {
        if desc.len() > 500 {
            return Err(WebhookError::Validation(
                "Description must be max 500 characters".to_string(),
            )
            .into());
        }
    }

    // Check limit
    let count = queries::count_webhooks(&pool, app_id)
        .await
        .map_err(WebhookError::Database)?;
    if count >= 5 {
        return Err(WebhookError::MaxWebhooksReached.into());
    }

    let secret = signing::generate_signing_secret();
    let webhook_id = queries::create_webhook(
        &pool,
        app_id,
        &req.url,
        &secret,
        &req.subscribed_events,
        req.description.as_deref(),
    )
    .await
    .map_err(WebhookError::Database)?;

    info!(webhook_id = %webhook_id, app_id = %app_id, "Webhook created");

    Ok((
        StatusCode::CREATED,
        Json(WebhookCreatedResponse {
            id: webhook_id,
            application_id: app_id,
            url: req.url,
            signing_secret: secret,
            subscribed_events: req.subscribed_events,
            active: true,
            description: req.description,
            created_at: chrono::Utc::now(),
        }),
    ))
}

/// GET /api/applications/{app_id}/webhooks
#[instrument(skip(pool, claims))]
pub async fn list_webhooks(
    State(pool): State<PgPool>,
    Path(app_id): Path<Uuid>,
    claims: AuthUser,
) -> Result<Json<Vec<WebhookResponse>>, (StatusCode, String)> {
    verify_ownership(&pool, app_id, claims.id).await?;

    let webhooks = queries::list_webhooks(&pool, app_id)
        .await
        .map_err(WebhookError::Database)?;

    Ok(Json(webhooks))
}

/// GET /api/applications/{app_id}/webhooks/{wh_id}
#[instrument(skip(pool, claims))]
pub async fn get_webhook(
    State(pool): State<PgPool>,
    Path((app_id, wh_id)): Path<(Uuid, Uuid)>,
    claims: AuthUser,
) -> Result<Json<WebhookResponse>, (StatusCode, String)> {
    verify_ownership(&pool, app_id, claims.id).await?;

    let webhook = queries::get_webhook(&pool, wh_id, app_id)
        .await
        .map_err(WebhookError::Database)?
        .ok_or(WebhookError::NotFound)?;

    Ok(Json(webhook))
}

/// PATCH /api/applications/{app_id}/webhooks/{wh_id}
#[instrument(skip(pool, claims))]
pub async fn update_webhook(
    State(pool): State<PgPool>,
    Path((app_id, wh_id)): Path<(Uuid, Uuid)>,
    claims: AuthUser,
    Json(req): Json<UpdateWebhookRequest>,
) -> Result<Json<WebhookResponse>, (StatusCode, String)> {
    verify_ownership(&pool, app_id, claims.id).await?;

    if let Some(ref url) = req.url {
        validate_url(url)?;
    }

    if let Some(ref events) = req.subscribed_events {
        if events.is_empty() {
            return Err(WebhookError::Validation(
                "At least one subscribed event is required".to_string(),
            )
            .into());
        }
    }

    if let Some(ref desc) = req.description {
        if desc.len() > 500 {
            return Err(WebhookError::Validation(
                "Description must be max 500 characters".to_string(),
            )
            .into());
        }
    }

    let description_option = if req.description.is_some() {
        Some(req.description.as_deref())
    } else {
        None
    };

    let updated = queries::update_webhook(
        &pool,
        wh_id,
        app_id,
        req.url.as_deref(),
        req.subscribed_events.as_deref(),
        req.active,
        description_option,
    )
    .await
    .map_err(WebhookError::Database)?;

    if !updated {
        return Err(WebhookError::NotFound.into());
    }

    let webhook = queries::get_webhook(&pool, wh_id, app_id)
        .await
        .map_err(WebhookError::Database)?
        .ok_or(WebhookError::NotFound)?;

    Ok(Json(webhook))
}

/// DELETE /api/applications/{app_id}/webhooks/{wh_id}
#[instrument(skip(pool, claims))]
pub async fn delete_webhook(
    State(pool): State<PgPool>,
    Path((app_id, wh_id)): Path<(Uuid, Uuid)>,
    claims: AuthUser,
) -> Result<StatusCode, (StatusCode, String)> {
    verify_ownership(&pool, app_id, claims.id).await?;

    let deleted = queries::delete_webhook(&pool, wh_id, app_id)
        .await
        .map_err(WebhookError::Database)?;

    if !deleted {
        return Err(WebhookError::NotFound.into());
    }

    Ok(StatusCode::NO_CONTENT)
}

/// POST /api/applications/{app_id}/webhooks/{wh_id}/test
#[instrument(skip(pool, claims))]
pub async fn test_webhook(
    State(pool): State<PgPool>,
    Path((app_id, wh_id)): Path<(Uuid, Uuid)>,
    claims: AuthUser,
) -> Result<Json<TestDeliveryResult>, (StatusCode, String)> {
    verify_ownership(&pool, app_id, claims.id).await?;

    let webhook = queries::get_webhook_full(&pool, wh_id)
        .await
        .map_err(WebhookError::Database)?
        .ok_or(WebhookError::NotFound)?;

    if webhook.application_id != app_id {
        return Err(WebhookError::NotFound.into());
    }

    // SSRF check at delivery time (DNS rebinding protection)
    if let Err(e) = super::ssrf::verify_resolved_ip(&webhook.url).await {
        return Ok(Json(TestDeliveryResult {
            success: false,
            response_status: None,
            latency_ms: 0,
            error_message: Some(format!("SSRF blocked: {e}")),
        }));
    }

    // Build test payload
    let event_id = Uuid::new_v4();
    let payload = serde_json::json!({
        "specversion": "1.0",
        "type": "webhook.test",
        "source": "canis",
        "id": event_id.to_string(),
        "time": chrono::Utc::now().to_rfc3339(),
        "data": { "test": true }
    });

    let payload_bytes = serde_json::to_vec(&payload).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to serialize test payload: {e}"),
        )
    })?;
    let signature = signing::sign_payload(&webhook.signing_secret, &payload_bytes);
    let timestamp = chrono::Utc::now().timestamp().to_string();

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to create HTTP client: {e}"),
            )
        })?;

    let start = std::time::Instant::now();
    let result = client
        .post(&webhook.url)
        .header("Content-Type", "application/json")
        .header("X-Webhook-Signature", format!("sha256={signature}"))
        .header("X-Webhook-Event", "webhook.test")
        .header("X-Webhook-ID", event_id.to_string())
        .header("X-Webhook-Timestamp", &timestamp)
        .body(payload_bytes)
        .send()
        .await;
    let latency = start.elapsed().as_millis() as u64;

    match result {
        Ok(resp) => {
            let status = resp.status().as_u16();
            let success = resp.status().is_success();
            Ok(Json(TestDeliveryResult {
                success,
                response_status: Some(status),
                latency_ms: latency,
                error_message: if !success {
                    Some(format!("HTTP {status}"))
                } else {
                    None
                },
            }))
        }
        Err(e) => Ok(Json(TestDeliveryResult {
            success: false,
            response_status: None,
            latency_ms: latency,
            error_message: Some(e.to_string()),
        })),
    }
}

/// GET /api/applications/{app_id}/webhooks/{wh_id}/deliveries
#[instrument(skip(pool, claims))]
pub async fn list_deliveries(
    State(pool): State<PgPool>,
    Path((app_id, wh_id)): Path<(Uuid, Uuid)>,
    claims: AuthUser,
) -> Result<Json<Vec<DeliveryLogEntry>>, (StatusCode, String)> {
    verify_ownership(&pool, app_id, claims.id).await?;

    // Verify webhook belongs to app
    let _ = queries::get_webhook(&pool, wh_id, app_id)
        .await
        .map_err(WebhookError::Database)?
        .ok_or(WebhookError::NotFound)?;

    let entries = queries::list_deliveries(&pool, wh_id, 50)
        .await
        .map_err(WebhookError::Database)?;

    Ok(Json(entries))
}
