//! Request and response types for the setup API.

use serde::{Deserialize, Serialize};
use validator::Validate;

/// Response for GET /api/setup/status.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct SetupStatusResponse {
    pub setup_complete: bool,
}

/// Response for GET /api/setup/config.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct SetupConfigResponse {
    pub server_name: String,
    pub registration_policy: String,
    pub terms_url: Option<String>,
    pub privacy_url: Option<String>,
}

/// Request body for POST /api/setup/complete.
#[derive(Debug, Deserialize, Validate, utoipa::ToSchema)]
pub struct CompleteSetupRequest {
    #[validate(length(min = 1, max = 64, message = "Server name must be 1-64 characters"))]
    pub server_name: String,
    #[validate(custom(function = "validate_registration_policy"))]
    pub registration_policy: String,
    #[validate(url(message = "Terms URL must be a valid URL"))]
    pub terms_url: Option<String>,
    #[validate(url(message = "Privacy URL must be a valid URL"))]
    pub privacy_url: Option<String>,
}

pub(super) fn validate_registration_policy(policy: &str) -> Result<(), validator::ValidationError> {
    if matches!(policy, "open" | "invite_only" | "closed") {
        Ok(())
    } else {
        Err(validator::ValidationError::new("invalid_policy"))
    }
}
