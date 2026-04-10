//! Webhook Types
//!
//! Data structures for webhooks, delivery logs, and dead letters.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::events::BotEventType;

/// Webhook configuration for an application (includes signing secret for delivery).
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Webhook {
    pub id: Uuid,
    pub application_id: Uuid,
    pub url: String,
    pub signing_secret: String,
    pub subscribed_events: Vec<BotEventType>,
    pub active: bool,
    pub description: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Webhook response returned on creation (includes signing secret once).
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct WebhookCreatedResponse {
    pub id: Uuid,
    pub application_id: Uuid,
    pub url: String,
    pub signing_secret: String,
    pub subscribed_events: Vec<BotEventType>,
    pub active: bool,
    pub description: Option<String>,
    pub created_at: DateTime<Utc>,
}

/// Webhook response (no signing secret).
#[derive(Debug, Serialize, sqlx::FromRow, utoipa::ToSchema)]
pub struct WebhookResponse {
    pub id: Uuid,
    pub application_id: Uuid,
    pub url: String,
    pub subscribed_events: Vec<BotEventType>,
    pub active: bool,
    pub description: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Request to create a webhook.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct CreateWebhookRequest {
    pub url: String,
    pub subscribed_events: Vec<BotEventType>,
    pub description: Option<String>,
}

/// Request to update a webhook.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct UpdateWebhookRequest {
    pub url: Option<String>,
    pub subscribed_events: Option<Vec<BotEventType>>,
    pub active: Option<bool>,
    pub description: Option<String>,
}

/// Delivery log entry.
#[derive(Debug, Serialize, sqlx::FromRow, utoipa::ToSchema)]
pub struct DeliveryLogEntry {
    pub id: Uuid,
    pub webhook_id: Uuid,
    pub event_type: BotEventType,
    pub event_id: Uuid,
    pub response_status: Option<i16>,
    pub success: bool,
    pub attempt: i32,
    pub error_message: Option<String>,
    pub latency_ms: Option<i32>,
    pub created_at: DateTime<Utc>,
}

/// Test delivery result.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct TestDeliveryResult {
    pub success: bool,
    pub response_status: Option<u16>,
    pub latency_ms: u64,
    pub error_message: Option<String>,
}

/// Item queued for webhook delivery via Redis.
///
/// Note: `signing_secret` is intentionally excluded — it is looked up from the
/// database at delivery time to avoid exposing secrets in Redis.
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct WebhookDeliveryItem {
    pub webhook_id: Uuid,
    pub url: String,
    pub event_type: BotEventType,
    pub event_id: Uuid,
    #[schema(value_type = Object)]
    pub payload: serde_json::Value,
    pub attempt: u32,
    pub event_time: DateTime<Utc>,
}

