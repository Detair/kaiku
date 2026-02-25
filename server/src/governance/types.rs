//! Data Governance Request/Response Types

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Response for a data export job.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct ExportJobResponse {
    /// Job ID.
    pub id: Uuid,
    /// Job status: pending, processing, completed, failed, expired.
    pub status: String,
    /// Export archive size in bytes (when completed).
    pub file_size_bytes: Option<i64>,
    /// When the export download expires (when completed).
    pub expires_at: Option<DateTime<Utc>>,
    /// Error message (when failed).
    pub error_message: Option<String>,
    /// When the job was created.
    pub created_at: DateTime<Utc>,
    /// When the job completed (when completed).
    pub completed_at: Option<DateTime<Utc>>,
}

/// Request to delete an account.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct DeleteAccountRequest {
    /// Current password for verification (required for local auth users).
    pub password: Option<String>,
    /// Confirmation string — must be "DELETE" to proceed.
    pub confirm: String,
}

/// Response after requesting account deletion.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct DeleteAccountResponse {
    /// When the account will be permanently deleted.
    pub deletion_scheduled_at: DateTime<Utc>,
    /// Human-readable message.
    pub message: String,
}

/// Response after cancelling account deletion.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct CancelDeletionResponse {
    /// Human-readable message.
    pub message: String,
}
