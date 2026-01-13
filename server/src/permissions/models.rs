//! Database models for permission system.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

use super::guild::GuildPermissions;

/// System admin record.
#[derive(Debug, Clone, FromRow, Serialize)]
pub struct SystemAdmin {
    pub user_id: Uuid,
    pub granted_by: Option<Uuid>,
    pub granted_at: DateTime<Utc>,
}

/// Elevated session for sudo-style admin access.
#[derive(Debug, Clone, FromRow, Serialize)]
pub struct ElevatedSession {
    pub id: Uuid,
    pub user_id: Uuid,
    pub session_id: Uuid,
    pub ip_address: String,
    pub elevated_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub reason: Option<String>,
}

/// Guild role with permissions.
#[derive(Debug, Clone, FromRow, Serialize)]
pub struct GuildRole {
    pub id: Uuid,
    pub guild_id: Uuid,
    pub name: String,
    pub color: Option<String>,
    #[sqlx(try_from = "i64")]
    pub permissions: GuildPermissions,
    pub position: i32,
    pub is_default: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Guild member role assignment.
#[derive(Debug, Clone, FromRow, Serialize)]
pub struct GuildMemberRole {
    pub guild_id: Uuid,
    pub user_id: Uuid,
    pub role_id: Uuid,
    pub assigned_by: Option<Uuid>,
    pub assigned_at: DateTime<Utc>,
}

/// Channel permission override.
#[derive(Debug, Clone, FromRow, Serialize)]
pub struct ChannelOverride {
    pub id: Uuid,
    pub channel_id: Uuid,
    pub role_id: Uuid,
    #[sqlx(try_from = "i64")]
    pub allow_permissions: GuildPermissions,
    #[sqlx(try_from = "i64")]
    pub deny_permissions: GuildPermissions,
}

/// System audit log entry.
#[derive(Debug, Clone, FromRow, Serialize)]
pub struct AuditLogEntry {
    pub id: Uuid,
    pub actor_id: Uuid,
    pub action: String,
    pub target_type: Option<String>,
    pub target_id: Option<Uuid>,
    pub details: Option<serde_json::Value>,
    pub ip_address: Option<String>,
    pub created_at: DateTime<Utc>,
}

/// System announcement.
#[derive(Debug, Clone, FromRow, Serialize)]
pub struct SystemAnnouncement {
    pub id: Uuid,
    pub author_id: Uuid,
    pub title: String,
    pub content: String,
    pub severity: String,
    pub active: bool,
    pub starts_at: DateTime<Utc>,
    pub ends_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Pending approval for dual-approval actions.
#[derive(Debug, Clone, FromRow, Serialize)]
pub struct PendingApproval {
    pub id: Uuid,
    pub action_type: String,
    pub target_type: String,
    pub target_id: Uuid,
    pub requested_by: Uuid,
    pub approved_by: Option<Uuid>,
    pub status: String,
    pub execute_after: Option<DateTime<Utc>>,
    pub expires_at: DateTime<Utc>,
    pub executed_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Break-glass emergency request.
#[derive(Debug, Clone, FromRow, Serialize)]
pub struct BreakGlassRequest {
    pub id: Uuid,
    pub admin_id: Uuid,
    pub action_type: String,
    pub target_type: String,
    pub target_id: Uuid,
    pub justification: String,
    pub incident_ticket: Option<String>,
    pub status: String,
    pub execute_at: DateTime<Utc>,
    pub blocked_by: Option<Uuid>,
    pub block_reason: Option<String>,
    pub executed_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Global user ban.
#[derive(Debug, Clone, FromRow, Serialize)]
pub struct GlobalBan {
    pub user_id: Uuid,
    pub banned_by: Uuid,
    pub reason: String,
    pub expires_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

// Implement TryFrom for GuildPermissions to work with sqlx
impl TryFrom<i64> for GuildPermissions {
    type Error = std::convert::Infallible;

    fn try_from(value: i64) -> Result<Self, Self::Error> {
        Ok(GuildPermissions::from_db(value))
    }
}

/// Request types for API
#[derive(Debug, Deserialize)]
pub struct CreateGuildRoleRequest {
    pub name: String,
    pub color: Option<String>,
    pub permissions: Option<u64>,
    pub position: Option<i32>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateGuildRoleRequest {
    pub name: Option<String>,
    pub color: Option<String>,
    pub permissions: Option<u64>,
    pub position: Option<i32>,
}

#[derive(Debug, Deserialize)]
pub struct SetChannelOverrideRequest {
    pub allow: Option<u64>,
    pub deny: Option<u64>,
}

#[derive(Debug, Deserialize)]
pub struct ElevateSessionRequest {
    pub mfa_code: String,
    pub reason: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct BreakGlassRequestBody {
    pub action_type: String,
    pub target_type: String,
    pub target_id: Uuid,
    pub justification: String,
    pub incident_ticket: Option<String>,
}
