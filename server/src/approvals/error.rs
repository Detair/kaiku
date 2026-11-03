//! Approval errors with the C26 codes.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use chrono::{DateTime, Utc};

use crate::auth::AuthError;

/// Why an approval call failed (C26 error table).
#[derive(Debug, thiserror::Error)]
pub enum ApprovalError {
    #[error("Approval not found")]
    NotFound,
    #[error("This request was already answered")]
    State,
    #[error("This request has expired")]
    Expired,
    #[error("The signature is not valid")]
    SignatureInvalid,
    #[error("Approvals are turned off for this account")]
    NotAllowed,
    #[error("Use a key phone for this")]
    KeyDeviceRequired,
    #[error("This phone is not known")]
    KeyDeviceUnknown,
    #[error("This phone is not ready yet")]
    KeyDevicePending { ready_at: Option<DateTime<Utc>> },
    #[error("This phone was removed")]
    KeyDeviceRevoked,
    #[error("This setup link is not valid")]
    EnrollmentTokenInvalid,
    #[error("This setup link was already used")]
    EnrollmentAlreadyClaimed,
    #[error("Wrong code")]
    EnrollCodeWrong { tries_left: i16 },
    #[error("Too many wrong codes; start again")]
    EnrollmentFailed,
    #[error("This setup is no longer open")]
    EnrollmentState,
    #[error("Too many open setups")]
    EnrollmentLimit,
    #[error("This phone cannot be used as a key")]
    DeviceUnsuitable { reason: &'static str },
    #[error("This account has been banned")]
    AccountBanned,
    /// The scanned sign-in code is unknown, used or older than 60 s (C27).
    #[error("This code has just changed; scan the new one")]
    LoginCodeStale,
    /// Another phone holds the attempt's claim.
    #[error("Another phone is already confirming this sign-in")]
    LoginAttemptClaimed,
    /// A password/OIDC attempt of another account (C26 claim).
    #[error("This sign-in belongs to another account")]
    LoginAttemptOtherAccount,
    /// The signed `response` is not the scanned token (B.2; nothing changes).
    #[error("This answer does not match the scanned code")]
    ResponseMismatch,
    #[error("{0}")]
    Validation(String),
    #[error("Service temporarily unavailable")]
    Unavailable,
    #[error(transparent)]
    Auth(#[from] AuthError),
    #[error("Database error")]
    Database(#[from] sqlx::Error),
}

impl From<crate::approvals::transition::TransitionError> for ApprovalError {
    fn from(e: crate::approvals::transition::TransitionError) -> Self {
        match e {
            crate::approvals::transition::TransitionError::Precondition(_) => Self::State,
            crate::approvals::transition::TransitionError::Contended => Self::Unavailable,
            crate::approvals::transition::TransitionError::Db(e) => Self::Database(e),
        }
    }
}

impl IntoResponse for ApprovalError {
    fn into_response(self) -> Response {
        let this = match self {
            Self::Auth(e) => return e.into_response(),
            other => other,
        };
        let (status, code) = match &this {
            Self::Auth(_) => unreachable!("handled above"),
            Self::NotFound => (StatusCode::NOT_FOUND, "APPROVAL_NOT_FOUND"),
            Self::State => (StatusCode::CONFLICT, "APPROVAL_STATE"),
            Self::Expired => (StatusCode::GONE, "APPROVAL_EXPIRED"),
            Self::SignatureInvalid => (StatusCode::BAD_REQUEST, "SIGNATURE_INVALID"),
            Self::NotAllowed => (StatusCode::FORBIDDEN, "APPROVAL_NOT_ALLOWED"),
            Self::KeyDeviceRequired => (StatusCode::FORBIDDEN, "KEY_DEVICE_REQUIRED"),
            Self::KeyDeviceUnknown => (StatusCode::NOT_FOUND, "KEY_DEVICE_UNKNOWN"),
            Self::KeyDevicePending { .. } => (StatusCode::CONFLICT, "KEY_DEVICE_PENDING"),
            Self::KeyDeviceRevoked => (StatusCode::FORBIDDEN, "KEY_DEVICE_REVOKED"),
            Self::EnrollmentTokenInvalid => (StatusCode::NOT_FOUND, "ENROLLMENT_TOKEN_INVALID"),
            Self::EnrollmentAlreadyClaimed => (StatusCode::CONFLICT, "ENROLLMENT_ALREADY_CLAIMED"),
            Self::EnrollCodeWrong { .. } => (StatusCode::BAD_REQUEST, "ENROLL_CODE_WRONG"),
            Self::EnrollmentFailed => (StatusCode::GONE, "ENROLLMENT_FAILED"),
            Self::EnrollmentState => (StatusCode::CONFLICT, "ENROLLMENT_STATE"),
            Self::EnrollmentLimit => (StatusCode::CONFLICT, "ENROLLMENT_LIMIT"),
            Self::DeviceUnsuitable { .. } => (StatusCode::BAD_REQUEST, "DEVICE_UNSUITABLE"),
            Self::AccountBanned => (StatusCode::FORBIDDEN, "ACCOUNT_BANNED"),
            Self::LoginCodeStale => (StatusCode::NOT_FOUND, "LOGIN_CODE_STALE"),
            Self::LoginAttemptClaimed => (StatusCode::CONFLICT, "LOGIN_ATTEMPT_CLAIMED"),
            Self::LoginAttemptOtherAccount => (StatusCode::CONFLICT, "LOGIN_ATTEMPT_OTHER_ACCOUNT"),
            Self::ResponseMismatch => (StatusCode::BAD_REQUEST, "APPROVAL_RESPONSE_MISMATCH"),
            Self::Validation(_) => (StatusCode::BAD_REQUEST, "VALIDATION_ERROR"),
            Self::Unavailable => (StatusCode::SERVICE_UNAVAILABLE, "SERVICE_UNAVAILABLE"),
            Self::Database(e) => {
                tracing::error!(error = %e, "approvals database error");
                (StatusCode::INTERNAL_SERVER_ERROR, "INTERNAL_ERROR")
            }
        };
        let message = if matches!(this, Self::Database(_)) {
            "Internal server error".to_string()
        } else {
            this.to_string()
        };
        let mut body = serde_json::json!({ "error": code, "message": message });
        match &this {
            Self::KeyDevicePending { ready_at } => body["ready_at"] = serde_json::json!(ready_at),
            Self::EnrollCodeWrong { tries_left } => {
                body["tries_left"] = serde_json::json!(tries_left);
            }
            Self::DeviceUnsuitable { reason } => body["reason"] = serde_json::json!(reason),
            _ => {}
        }
        (status, Json(body)).into_response()
    }
}
