//! Screen sharing data types and state.

use fred::prelude::*;
use serde::{Deserialize, Serialize};
use tracing::{error, warn};
use uuid::Uuid;

use super::Quality;

/// Maximum length for `source_label` field
const MAX_SOURCE_LABEL_LENGTH: usize = 255;

/// Validate a source label string.
/// Returns an error if the label is too long or contains invalid characters.
pub fn validate_source_label(label: &str) -> Result<(), ScreenShareError> {
    if label.len() > MAX_SOURCE_LABEL_LENGTH {
        return Err(ScreenShareError::InvalidSourceLabel);
    }

    // Allow alphanumeric, whitespace, and common punctuation
    for ch in label.chars() {
        if !ch.is_alphanumeric() && !ch.is_whitespace() && !"()-_.,:;'\"!?#@&+".contains(ch) {
            return Err(ScreenShareError::InvalidSourceLabel);
        }
    }

    Ok(())
}

/// Information about an active screen share session.
#[derive(Clone, Debug, Serialize, Deserialize, utoipa::ToSchema)]
pub struct ScreenShareInfo {
    /// User who is sharing
    pub user_id: Uuid,
    /// Username for display
    pub username: String,
    /// Label of shared source (e.g., "Display 1", "Firefox")
    pub source_label: String,
    /// Whether screen audio is included
    pub has_audio: bool,
    /// Current quality tier
    pub quality: Quality,
}

impl ScreenShareInfo {
    /// Create a new [`ScreenShareInfo`].
    #[must_use]
    pub const fn new(
        user_id: Uuid,
        username: String,
        source_label: String,
        has_audio: bool,
        quality: Quality,
    ) -> Self {
        Self {
            user_id,
            username,
            source_label,
            has_audio,
            quality,
        }
    }
}

/// Request to start a screen share.
#[derive(Clone, Debug, Deserialize, utoipa::ToSchema)]
pub struct ScreenShareStartRequest {
    /// Requested quality tier
    pub quality: Quality,
    /// Include system audio
    pub has_audio: bool,
    /// Source label for display
    pub source_label: String,
}

/// Response to screen share check/start request.
#[derive(Clone, Debug, Serialize, utoipa::ToSchema)]
pub struct ScreenShareCheckResponse {
    /// Whether screen sharing is allowed
    pub allowed: bool,
    /// Quality tier granted (may be lower than requested)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub granted_quality: Option<Quality>,
    /// Error message if not allowed
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ScreenShareError>,
}

/// Screen share error reasons.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ScreenShareError {
    /// User doesn't have `SCREEN_SHARE` permission
    NoPermission,
    /// Channel screen share limit reached
    LimitReached,
    /// User not in the voice channel
    NotInChannel,
    /// Premium quality requested but user lacks `PREMIUM_VIDEO`
    QualityNotAllowed,
    /// WebRTC renegotiation failed
    RenegotiationFailed,
    /// Internal server error (e.g., Redis failure)
    InternalError,
    /// Source label is invalid (too long or contains invalid characters)
    InvalidSourceLabel,
    /// User already has an active screen share
    AlreadySharing,
}

impl ScreenShareCheckResponse {
    /// Create an allowed response.
    pub const fn allowed(quality: Quality) -> Self {
        Self {
            allowed: true,
            granted_quality: Some(quality),
            error: None,
        }
    }

    /// Create a denied response.
    pub const fn denied(error: ScreenShareError) -> Self {
        Self {
            allowed: false,
            granted_quality: None,
            error: Some(error),
        }
    }
}

/// Check if screen share limit is reached for a channel (without incrementing).
pub async fn check_limit(
    redis: &Client,
    channel_id: Uuid,
    max_shares: u32,
) -> Result<(), ScreenShareError> {
    let key = format!("screenshare:limit:{channel_id}");

    // Get current count
    match redis.get::<Option<u32>, _>(&key).await {
        Ok(Some(count)) => {
            if count >= max_shares {
                return Err(ScreenShareError::LimitReached);
            }
        }
        Ok(None) => {
            // No active shares, so limit not reached (unless max_shares is 0)
            if max_shares == 0 {
                return Err(ScreenShareError::LimitReached);
            }
        }
        Err(e) => {
            error!(channel_id = %channel_id, error = %e, "Redis GET failed in check_limit");
            return Err(ScreenShareError::InternalError);
        }
    }

    Ok(())
}

/// Try to start screen sharing using atomic Redis WATCH/MULTI/EXEC.
/// Uses optimistic locking to prevent race conditions.
pub async fn try_start_screen_share(
    redis: &Client,
    channel_id: Uuid,
    max_shares: u32,
) -> Result<(), ScreenShareError> {
    let key = format!("screenshare:limit:{channel_id}");

    // Get current count first
    let current: i64 = match redis.get(&key).await {
        Ok(val) => val,
        Err(e) => {
            error!(
                %channel_id,
                error = %e,
                "Redis GET failed for screen share count, denying request"
            );
            return Err(ScreenShareError::InternalError);
        }
    };

    // Check if we would exceed limit
    if current >= i64::from(max_shares) {
        return Err(ScreenShareError::LimitReached);
    }

    // Increment atomically
    let new_count: i64 = redis.incr(&key).await.map_err(|e| {
        error!(
            channel_id = %channel_id,
            error = %e,
            "Redis INCR failed in try_start_screen_share"
        );
        ScreenShareError::InternalError
    })?;

    // Double-check after increment (handles race condition)
    if new_count > i64::from(max_shares) {
        // We exceeded the limit, decrement back
        if let Err(e) = redis.decr::<i64, _>(&key).await {
            warn!(
                channel_id = %channel_id,
                error = %e,
                "Redis DECR failed after limit exceeded - counter may be desynchronized"
            );
        }
        return Err(ScreenShareError::LimitReached);
    }

    // Set expiration (5 minutes to allow recovery from crashes)
    if let Err(e) = redis.expire::<(), _>(&key, 300, None).await {
        warn!(
            channel_id = %channel_id,
            error = %e,
            "Redis EXPIRE failed - stale keys may accumulate"
        );
    }

    Ok(())
}

/// Stop screen sharing, decrementing the limit counter.
/// Logs errors but continues, as cleanup must complete.
pub async fn stop_screen_share(redis: &Client, channel_id: Uuid) {
    let key = format!("screenshare:limit:{channel_id}");

    // Get current count to prevent going negative
    let current: i64 = match redis.get(&key).await {
        Ok(val) => val,
        Err(e) => {
            warn!(
                channel_id = %channel_id,
                error = %e,
                "Redis GET failed in stop_screen_share - skipping decrement"
            );
            return;
        }
    };

    // Only decrement if count is positive
    if current > 0 {
        if let Err(e) = redis.decr::<i64, _>(&key).await {
            warn!(
                channel_id = %channel_id,
                error = %e,
                "Redis DECR failed in stop_screen_share - counter may be desynchronized"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // =========================================================================
    // ScreenShareInfo Tests
    // =========================================================================

    #[test]
    fn test_screen_share_info_creation() {
        let user_id = Uuid::new_v4();
        let info = ScreenShareInfo::new(
            user_id,
            "alice".to_string(),
            "Display 1".to_string(),
            true,
            Quality::High,
        );

        assert_eq!(info.user_id, user_id);
        assert_eq!(info.username, "alice");
        assert_eq!(info.source_label, "Display 1");
        assert!(info.has_audio);
        assert_eq!(info.quality, Quality::High);
    }

    #[test]
    fn test_screen_share_info_without_audio() {
        let user_id = Uuid::new_v4();
        let info = ScreenShareInfo::new(
            user_id,
            "bob".to_string(),
            "Firefox".to_string(),
            false,
            Quality::Medium,
        );

        assert!(!info.has_audio);
        assert_eq!(info.quality, Quality::Medium);
    }

    #[test]
    fn test_screen_share_info_serialization() {
        let user_id = Uuid::new_v4();
        let info = ScreenShareInfo::new(
            user_id,
            "alice".to_string(),
            "Display 1".to_string(),
            true,
            Quality::High,
        );

        let json = serde_json::to_string(&info).unwrap();
        assert!(json.contains("\"username\":\"alice\""));
        assert!(json.contains("\"source_label\":\"Display 1\""));
        assert!(json.contains("\"has_audio\":true"));

        // Roundtrip
        let deserialized: ScreenShareInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.user_id, user_id);
        assert_eq!(deserialized.username, "alice");
    }

    // =========================================================================
    // ScreenShareCheckResponse Tests
    // =========================================================================

    #[test]
    fn test_check_response_allowed() {
        let resp = ScreenShareCheckResponse::allowed(Quality::High);
        assert!(resp.allowed);
        assert_eq!(resp.granted_quality, Some(Quality::High));
        assert!(resp.error.is_none());
    }

    #[test]
    fn test_check_response_denied() {
        let resp = ScreenShareCheckResponse::denied(ScreenShareError::LimitReached);
        assert!(!resp.allowed);
        assert!(resp.granted_quality.is_none());
        assert_eq!(resp.error, Some(ScreenShareError::LimitReached));
    }

    #[test]
    fn test_check_response_all_quality_tiers() {
        for quality in [
            Quality::Low,
            Quality::Medium,
            Quality::High,
            Quality::Premium,
        ] {
            let resp = ScreenShareCheckResponse::allowed(quality);
            assert!(resp.allowed);
            assert_eq!(resp.granted_quality, Some(quality));
        }
    }

    #[test]
    fn test_check_response_all_error_types() {
        let errors = [
            ScreenShareError::NoPermission,
            ScreenShareError::LimitReached,
            ScreenShareError::NotInChannel,
            ScreenShareError::QualityNotAllowed,
            ScreenShareError::RenegotiationFailed,
            ScreenShareError::InternalError,
            ScreenShareError::InvalidSourceLabel,
            ScreenShareError::AlreadySharing,
        ];

        for error in errors {
            let resp = ScreenShareCheckResponse::denied(error.clone());
            assert!(!resp.allowed);
            assert_eq!(resp.error, Some(error));
        }
    }

    #[test]
    fn test_check_response_serialization_allowed() {
        let resp = ScreenShareCheckResponse::allowed(Quality::High);
        let json = serde_json::to_string(&resp).unwrap();

        assert!(json.contains("\"allowed\":true"));
        assert!(json.contains("\"granted_quality\""));
        // error should be skipped when None
        assert!(!json.contains("\"error\""));
    }

    #[test]
    fn test_check_response_serialization_denied() {
        let resp = ScreenShareCheckResponse::denied(ScreenShareError::NoPermission);
        let json = serde_json::to_string(&resp).unwrap();

        assert!(json.contains("\"allowed\":false"));
        assert!(json.contains("\"error\":\"no_permission\""));
        // granted_quality should be skipped when None
        assert!(!json.contains("\"granted_quality\""));
    }

    // =========================================================================
    // validate_source_label Tests
    // =========================================================================

    #[test]
    fn test_validate_source_label_valid_simple() {
        assert!(validate_source_label("Display 1").is_ok());
        assert!(validate_source_label("Firefox").is_ok());
        assert!(validate_source_label("Google Chrome").is_ok());
    }

    #[test]
    fn test_validate_source_label_valid_with_punctuation() {
        assert!(validate_source_label("Display #1").is_ok());
        assert!(validate_source_label("My App (Window)").is_ok());
        assert!(validate_source_label("VS Code - project.rs").is_ok());
        assert!(validate_source_label("Terminal: bash").is_ok());
        assert!(validate_source_label("What's this?").is_ok());
        assert!(validate_source_label("File \"test.txt\"").is_ok());
    }

    #[test]
    fn test_validate_source_label_valid_unicode_alphanumeric() {
        // Unicode letters and numbers should be valid
        assert!(validate_source_label("日本語アプリ").is_ok());
        assert!(validate_source_label("Écran 2").is_ok());
        assert!(validate_source_label("Fenêtre principale").is_ok());
    }

    #[test]
    fn test_validate_source_label_empty() {
        // Empty string should be valid (no invalid chars)
        assert!(validate_source_label("").is_ok());
    }

    #[test]
    fn test_validate_source_label_too_long() {
        let long_label = "a".repeat(256);
        assert_eq!(
            validate_source_label(&long_label),
            Err(ScreenShareError::InvalidSourceLabel)
        );

        // Exactly 255 should be ok
        let max_label = "a".repeat(255);
        assert!(validate_source_label(&max_label).is_ok());
    }

    #[test]
    fn test_validate_source_label_invalid_chars() {
        // Control characters (null) should be invalid
        assert_eq!(
            validate_source_label("test\x00null"),
            Err(ScreenShareError::InvalidSourceLabel)
        );

        // Note: tab (\t) is whitespace in Rust and thus allowed by is_whitespace()
        // This is intentional - tabs are valid whitespace in source labels
        assert!(validate_source_label("test\ttab").is_ok());

        // Unusual symbols not in allowed list
        assert_eq!(
            validate_source_label("test<script>"),
            Err(ScreenShareError::InvalidSourceLabel)
        );
        assert_eq!(
            validate_source_label("test|pipe"),
            Err(ScreenShareError::InvalidSourceLabel)
        );
        assert_eq!(
            validate_source_label("test\\backslash"),
            Err(ScreenShareError::InvalidSourceLabel)
        );
    }

    // =========================================================================
    // ScreenShareError Tests
    // =========================================================================

    #[test]
    fn test_screen_share_error_serialization() {
        // All variants should serialize to snake_case
        let test_cases = [
            (ScreenShareError::NoPermission, "no_permission"),
            (ScreenShareError::LimitReached, "limit_reached"),
            (ScreenShareError::NotInChannel, "not_in_channel"),
            (ScreenShareError::QualityNotAllowed, "quality_not_allowed"),
            (
                ScreenShareError::RenegotiationFailed,
                "renegotiation_failed",
            ),
            (ScreenShareError::InternalError, "internal_error"),
            (ScreenShareError::InvalidSourceLabel, "invalid_source_label"),
            (ScreenShareError::AlreadySharing, "already_sharing"),
        ];

        for (error, expected) in test_cases {
            let json = serde_json::to_string(&error).unwrap();
            assert_eq!(json, format!("\"{expected}\""));
        }
    }

    #[test]
    fn test_screen_share_error_deserialization() {
        let error: ScreenShareError = serde_json::from_str("\"no_permission\"").unwrap();
        assert_eq!(error, ScreenShareError::NoPermission);

        let error: ScreenShareError = serde_json::from_str("\"limit_reached\"").unwrap();
        assert_eq!(error, ScreenShareError::LimitReached);
    }

    #[test]
    fn test_screen_share_error_equality() {
        assert_eq!(
            ScreenShareError::NoPermission,
            ScreenShareError::NoPermission
        );
        assert_ne!(
            ScreenShareError::NoPermission,
            ScreenShareError::LimitReached
        );
    }

    // =========================================================================
    // ScreenShareStartRequest Tests
    // =========================================================================

    #[test]
    fn test_start_request_deserialization() {
        let json = r#"{"quality":"high","has_audio":true,"source_label":"Display 1"}"#;
        let req: ScreenShareStartRequest = serde_json::from_str(json).unwrap();

        assert_eq!(req.quality, Quality::High);
        assert!(req.has_audio);
        assert_eq!(req.source_label, "Display 1");
    }

    #[test]
    fn test_start_request_deserialization_all_qualities() {
        let qualities = ["low", "medium", "high", "premium"];
        let expected = [
            Quality::Low,
            Quality::Medium,
            Quality::High,
            Quality::Premium,
        ];

        for (quality_str, expected_quality) in qualities.iter().zip(expected.iter()) {
            let json =
                format!(r#"{{"quality":"{quality_str}","has_audio":false,"source_label":"test"}}"#);
            let req: ScreenShareStartRequest = serde_json::from_str(&json).unwrap();
            assert_eq!(req.quality, *expected_quality);
        }
    }
}
