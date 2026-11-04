//! Wire types of the account requests (C27 "Account requests").

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// `AccountRequest.staged.authenticator` / `new_authenticator`.
#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub struct AuthenticatorLabel {
    /// `key_device` or `passkey`.
    #[serde(rename = "type")]
    pub kind: String,
    pub label: String,
}

/// `AccountRequest.staged`.
#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub struct StagedView {
    pub authenticator: Option<AuthenticatorLabel>,
    pub password: bool,
    pub totp: bool,
    /// The staged contact's username.
    pub recovery_contact: Option<String>,
}

/// `AccountRequest` (the request client's view, C27).
#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub struct AccountRequest {
    pub id: Uuid,
    /// `self` or `admin`.
    pub kind: String,
    /// `recovery`, `bootstrap`, `admin_setup` or `admin_recovery`.
    pub purpose: String,
    pub state: String,
    pub created_at: DateTime<Utc>,
    pub ready_at: DateTime<Utc>,
    /// The Finish deadline (24 h after `ready`).
    pub expires_at: Option<DateTime<Utc>>,
    /// `recovery_code`, `password`, `totp`, `oidc`.
    pub factors: Vec<String>,
    pub lost_key_devices: i64,
    pub staged: StagedView,
}

/// `AccountRequestSummary.subject`.
#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub struct RequestSubject {
    pub user_id: Uuid,
    pub username: String,
    pub display_name: String,
}

/// `AccountRequestSummary.started_by` (the admin).
#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub struct StartedBy {
    pub user_id: Uuid,
    pub username: String,
}

/// One device named lost.
#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub struct LostDevice {
    pub id: Uuid,
    pub label: String,
}

/// `AccountRequestSummary.cancel`: how the reader may cancel.
#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub struct CancelVia {
    /// `step_up` or `part2`.
    pub via: String,
}

/// `AccountRequestSummary` (owner, contact, admins; C27).
#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub struct AccountRequestSummary {
    pub id: Uuid,
    pub subject: RequestSubject,
    pub kind: String,
    pub purpose: String,
    pub state: String,
    pub created_at: DateTime<Utc>,
    pub ready_at: DateTime<Utc>,
    pub started_by: Option<StartedBy>,
    pub lost_devices: Vec<LostDevice>,
    pub new_authenticator: Option<AuthenticatorLabel>,
    pub cancel: Option<CancelVia>,
}

/// `{requests: [AccountRequestSummary]}`.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct SummaryList {
    pub requests: Vec<AccountRequestSummary>,
}

/// A first or second factor of a start (C27).
#[derive(Deserialize, utoipa::ToSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum StartFactor {
    Password {
        password: String,
    },
    Totp {
        code: String,
    },
    Oidc {
        oidc_code: String,
        #[serde(default)]
        verifier: Option<String>,
    },
}

/// `POST /auth/account-requests` body (recovery or bootstrap).
#[derive(Deserialize, utoipa::ToSchema)]
pub struct StartRequest {
    pub username: String,
    /// `recovery` or `bootstrap`.
    pub purpose: String,
    /// Recovery: the recovery code.
    #[serde(default)]
    pub recovery_code: Option<String>,
    /// Recovery: the second factor.
    #[serde(default)]
    pub second: Option<StartFactor>,
    /// Recovery: `none` or `all_key_devices`.
    #[serde(default)]
    pub lost: Option<String>,
    /// Bootstrap: the password or a fresh OIDC sign-in.
    #[serde(default)]
    pub first: Option<StartFactor>,
    /// Bootstrap: a TOTP code.
    #[serde(default)]
    pub totp_code: Option<String>,
}

impl std::fmt::Debug for StartRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StartRequest")
            .field("purpose", &self.purpose)
            .finish_non_exhaustive()
    }
}

/// `POST /auth/account-requests` answer.
#[derive(Serialize, utoipa::ToSchema)]
pub struct StartResponse {
    pub request: AccountRequest,
    /// Native clients only (browsers get the cookie).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_secret: Option<String>,
}

impl std::fmt::Debug for StartResponse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StartResponse")
            .field("request", &self.request.id)
            .finish_non_exhaustive()
    }
}

/// Part 2 of the owner rule (B.2): a fresh TOTP code or a recovery code.
#[derive(Deserialize, utoipa::ToSchema)]
pub struct Part2Body {
    pub factor: Part2FactorBody,
}

/// `{type: "totp"|"recovery_code", code}`.
#[derive(Deserialize, utoipa::ToSchema)]
pub struct Part2FactorBody {
    #[serde(rename = "type")]
    pub kind: String,
    pub code: String,
}

impl Part2Body {
    /// The owner-rule factor (`type` must be `totp` or `recovery_code`).
    pub fn into_factor(self) -> crate::auth::AuthResult<crate::auth::owner_rule::Part2Factor> {
        use crate::auth::owner_rule::Part2Factor;
        match self.factor.kind.as_str() {
            "totp" => Ok(Part2Factor::Totp(self.factor.code)),
            "recovery_code" => Ok(Part2Factor::RecoveryCode(self.factor.code)),
            _ => Err(crate::auth::AuthError::Validation(
                "part2.factor.type".into(),
            )),
        }
    }
}

impl std::fmt::Debug for Part2Body {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Part2Body { .. }")
    }
}

/// `POST /api/account-requests/{id}/cancel` body.
#[derive(Debug, Default, Deserialize, utoipa::ToSchema)]
pub struct CancelRequest {
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(default)]
    pub part2: Option<Part2Body>,
}
