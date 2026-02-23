//! Webhook API Handlers
//!
//! CRUD endpoints for webhook management. Owner-only enforcement.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use tracing::{info, instrument, warn};
use uuid::Uuid;

use super::types::{
    CreateWebhookRequest, DeliveryLogEntry, TestDeliveryResult, UpdateWebhookRequest,
    WebhookCreatedResponse, WebhookError, WebhookResponse,
};
use super::{queries, signing};
use crate::api::AppState;
use crate::auth::mfa_crypto::{decrypt_mfa_secret, encrypt_mfa_secret};
use crate::auth::AuthUser;

/// Verify application ownership and return application ID.
async fn verify_ownership(
    pool: &sqlx::PgPool,
    app_id: Uuid,
    user_id: Uuid,
) -> Result<(), WebhookError> {
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

/// Get the MFA encryption key bytes from config, or return an error if not configured.
fn get_encryption_key(state: &AppState) -> Result<Vec<u8>, WebhookError> {
    let key_hex = state.config.mfa_encryption_key.as_ref().ok_or_else(|| {
        WebhookError::Validation(
            "Server encryption key not configured (MFA_ENCRYPTION_KEY)".to_string(),
        )
    })?;
    let key = hex::decode(key_hex)
        .map_err(|_| WebhookError::Validation("Server encryption key misconfigured".to_string()))?;
    if key.len() != 32 {
        tracing::error!(
            actual_len = key.len(),
            "MFA_ENCRYPTION_KEY has wrong length (expected 32 bytes)"
        );
        return Err(WebhookError::Validation(
            "Server encryption key misconfigured".to_string(),
        ));
    }
    Ok(key)
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
    let parsed = reqwest::Url::parse(url)
        .map_err(|_| WebhookError::Validation("Invalid URL format".to_string()))?;

    let host = parsed
        .host_str()
        .ok_or_else(|| WebhookError::Validation("URL must contain a host".to_string()))?;

    if super::ssrf::is_blocked_host(host) {
        return Err(WebhookError::Validation(
            "URL must not point to a private or reserved address".to_string(),
        ));
    }

    Ok(())
}

/// POST /`api/applications/{app_id}/webhooks`
#[utoipa::path(
    post,
    path = "/api/applications/{app_id}/webhooks",
    tag = "webhooks",
    params(("app_id" = Uuid, Path, description = "Application ID")),
    request_body = CreateWebhookRequest,
    responses(
        (status = 201, description = "Webhook created", body = WebhookCreatedResponse),
    ),
    security(("bearer_auth" = [])),
)]
#[instrument(skip(state, claims))]
pub async fn create_webhook(
    State(state): State<AppState>,
    Path(app_id): Path<Uuid>,
    claims: AuthUser,
    Json(req): Json<CreateWebhookRequest>,
) -> Result<(StatusCode, Json<WebhookCreatedResponse>), (StatusCode, String)> {
    verify_ownership(&state.db, app_id, claims.id).await?;

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
    let count = queries::count_webhooks(&state.db, app_id)
        .await
        .map_err(WebhookError::Database)?;
    if count >= 5 {
        return Err(WebhookError::MaxWebhooksReached.into());
    }

    let secret = signing::generate_signing_secret();

    // Encrypt signing secret before storing in database
    let encryption_key = get_encryption_key(&state)?;
    let encrypted_secret = encrypt_mfa_secret(&secret, &encryption_key)
        .map_err(|e| WebhookError::Validation(format!("Failed to encrypt signing secret: {e}")))?;

    let webhook_id = queries::create_webhook(
        &state.db,
        app_id,
        &req.url,
        &encrypted_secret,
        &req.subscribed_events,
        req.description.as_deref(),
    )
    .await
    .map_err(WebhookError::Database)?;

    info!(webhook_id = %webhook_id, app_id = %app_id, "Webhook created");

    // Return plaintext secret to user (shown only once at creation)
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

/// GET /`api/applications/{app_id}/webhooks`
#[utoipa::path(
    get,
    path = "/api/applications/{app_id}/webhooks",
    tag = "webhooks",
    params(("app_id" = Uuid, Path, description = "Application ID")),
    responses(
        (status = 200, description = "List of webhooks", body = Vec<WebhookResponse>),
    ),
    security(("bearer_auth" = [])),
)]
#[instrument(skip(state, claims))]
pub async fn list_webhooks(
    State(state): State<AppState>,
    Path(app_id): Path<Uuid>,
    claims: AuthUser,
) -> Result<Json<Vec<WebhookResponse>>, (StatusCode, String)> {
    verify_ownership(&state.db, app_id, claims.id).await?;

    let webhooks = queries::list_webhooks(&state.db, app_id)
        .await
        .map_err(WebhookError::Database)?;

    Ok(Json(webhooks))
}

/// GET /`api/applications/{app_id}/webhooks/{wh_id}`
#[utoipa::path(
    get,
    path = "/api/applications/{app_id}/webhooks/{wh_id}",
    tag = "webhooks",
    params(
        ("app_id" = Uuid, Path, description = "Application ID"),
        ("wh_id" = Uuid, Path, description = "Webhook ID"),
    ),
    responses(
        (status = 200, description = "Webhook details", body = WebhookResponse),
    ),
    security(("bearer_auth" = [])),
)]
#[instrument(skip(state, claims))]
pub async fn get_webhook(
    State(state): State<AppState>,
    Path((app_id, wh_id)): Path<(Uuid, Uuid)>,
    claims: AuthUser,
) -> Result<Json<WebhookResponse>, (StatusCode, String)> {
    verify_ownership(&state.db, app_id, claims.id).await?;

    let webhook = queries::get_webhook(&state.db, wh_id, app_id)
        .await
        .map_err(WebhookError::Database)?
        .ok_or(WebhookError::NotFound)?;

    Ok(Json(webhook))
}

/// PATCH /`api/applications/{app_id}/webhooks/{wh_id}`
#[utoipa::path(
    patch,
    path = "/api/applications/{app_id}/webhooks/{wh_id}",
    tag = "webhooks",
    params(
        ("app_id" = Uuid, Path, description = "Application ID"),
        ("wh_id" = Uuid, Path, description = "Webhook ID"),
    ),
    request_body = UpdateWebhookRequest,
    responses(
        (status = 200, description = "Webhook updated", body = WebhookResponse),
    ),
    security(("bearer_auth" = [])),
)]
#[instrument(skip(state, claims))]
pub async fn update_webhook(
    State(state): State<AppState>,
    Path((app_id, wh_id)): Path<(Uuid, Uuid)>,
    claims: AuthUser,
    Json(req): Json<UpdateWebhookRequest>,
) -> Result<Json<WebhookResponse>, (StatusCode, String)> {
    verify_ownership(&state.db, app_id, claims.id).await?;

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
        &state.db,
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

    let webhook = queries::get_webhook(&state.db, wh_id, app_id)
        .await
        .map_err(WebhookError::Database)?
        .ok_or(WebhookError::NotFound)?;

    Ok(Json(webhook))
}

/// DELETE /`api/applications/{app_id}/webhooks/{wh_id}`
#[utoipa::path(
    delete,
    path = "/api/applications/{app_id}/webhooks/{wh_id}",
    tag = "webhooks",
    params(
        ("app_id" = Uuid, Path, description = "Application ID"),
        ("wh_id" = Uuid, Path, description = "Webhook ID"),
    ),
    responses(
        (status = 204, description = "Webhook deleted"),
    ),
    security(("bearer_auth" = [])),
)]
#[instrument(skip(state, claims))]
pub async fn delete_webhook(
    State(state): State<AppState>,
    Path((app_id, wh_id)): Path<(Uuid, Uuid)>,
    claims: AuthUser,
) -> Result<StatusCode, (StatusCode, String)> {
    verify_ownership(&state.db, app_id, claims.id).await?;

    let deleted = queries::delete_webhook(&state.db, wh_id, app_id)
        .await
        .map_err(WebhookError::Database)?;

    if !deleted {
        return Err(WebhookError::NotFound.into());
    }

    Ok(StatusCode::NO_CONTENT)
}

/// POST /`api/applications/{app_id}/webhooks/{wh_id}/test`
#[utoipa::path(
    post,
    path = "/api/applications/{app_id}/webhooks/{wh_id}/test",
    tag = "webhooks",
    params(
        ("app_id" = Uuid, Path, description = "Application ID"),
        ("wh_id" = Uuid, Path, description = "Webhook ID"),
    ),
    responses(
        (status = 200, description = "Test result", body = TestDeliveryResult),
    ),
    security(("bearer_auth" = [])),
)]
#[instrument(skip(state, claims))]
pub async fn test_webhook(
    State(state): State<AppState>,
    Path((app_id, wh_id)): Path<(Uuid, Uuid)>,
    claims: AuthUser,
) -> Result<Json<TestDeliveryResult>, (StatusCode, String)> {
    verify_ownership(&state.db, app_id, claims.id).await?;

    let webhook = queries::get_webhook_full(&state.db, wh_id)
        .await
        .map_err(WebhookError::Database)?
        .ok_or(WebhookError::NotFound)?;

    if webhook.application_id != app_id {
        return Err(WebhookError::NotFound.into());
    }

    // Decrypt signing secret (fall back to plaintext for pre-encryption legacy webhooks)
    let signing_secret = {
        let key = get_encryption_key(&state)?;
        decrypt_mfa_secret(&webhook.signing_secret, &key).unwrap_or_else(|_| {
            warn!(webhook_id = %wh_id, "Signing secret not encrypted, using plaintext (legacy webhook)");
            webhook.signing_secret.clone()
        })
    };

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
    let signature = signing::sign_payload(&signing_secret, &payload_bytes);
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
                error_message: if success {
                    None
                } else {
                    Some(format!("HTTP {status}"))
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

/// GET /`api/applications/{app_id}/webhooks/{wh_id}/deliveries`
#[utoipa::path(
    get,
    path = "/api/applications/{app_id}/webhooks/{wh_id}/deliveries",
    tag = "webhooks",
    params(
        ("app_id" = Uuid, Path, description = "Application ID"),
        ("wh_id" = Uuid, Path, description = "Webhook ID"),
    ),
    responses(
        (status = 200, description = "Delivery history", body = Vec<DeliveryLogEntry>),
    ),
    security(("bearer_auth" = [])),
)]
#[instrument(skip(state, claims))]
pub async fn list_deliveries(
    State(state): State<AppState>,
    Path((app_id, wh_id)): Path<(Uuid, Uuid)>,
    claims: AuthUser,
) -> Result<Json<Vec<DeliveryLogEntry>>, (StatusCode, String)> {
    verify_ownership(&state.db, app_id, claims.id).await?;

    // Verify webhook belongs to app
    let _ = queries::get_webhook(&state.db, wh_id, app_id)
        .await
        .map_err(WebhookError::Database)?
        .ok_or(WebhookError::NotFound)?;

    let entries = queries::list_deliveries(&state.db, wh_id, 50)
        .await
        .map_err(WebhookError::Database)?;

    Ok(Json(entries))
}
