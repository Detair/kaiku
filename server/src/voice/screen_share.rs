//! Screen sharing data types and state.

use std::sync::Arc;

use fred::clients::Client;
use fred::interfaces::LuaInterface;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
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
    /// Unique identifier for this screen share stream
    pub stream_id: Uuid,
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
    /// When the screen share session started
    pub started_at: chrono::DateTime<chrono::Utc>,
}

impl ScreenShareInfo {
    /// Create a new [`ScreenShareInfo`].
    pub fn new(
        stream_id: Uuid,
        user_id: Uuid,
        username: String,
        source_label: String,
        has_audio: bool,
        quality: Quality,
    ) -> Self {
        Self {
            stream_id,
            user_id,
            username,
            source_label,
            has_audio,
            quality,
            started_at: chrono::Utc::now(),
        }
    }
}

/// Request to start a screen share.
#[derive(Clone, Debug, Deserialize, utoipa::ToSchema)]
pub struct ScreenShareStartRequest {
    /// Client-generated stream identifier for this screen share
    pub stream_id: Uuid,
    /// Requested quality tier
    pub quality: Quality,
    /// Include system audio
    pub has_audio: bool,
    /// Source label for display
    pub source_label: String,
}

/// Request to stop a specific screen share stream.
#[derive(Clone, Debug, Deserialize, utoipa::ToSchema)]
pub struct ScreenShareStopRequest {
    /// Stream identifier of the screen share to stop
    pub stream_id: Uuid,
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
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
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

impl ScreenShareError {
    /// Machine-readable error code (the serialized, upper-case form).
    pub const fn code(&self) -> &'static str {
        match self {
            Self::NoPermission => "NO_PERMISSION",
            Self::LimitReached => "LIMIT_REACHED",
            Self::NotInChannel => "NOT_IN_CHANNEL",
            Self::QualityNotAllowed => "QUALITY_NOT_ALLOWED",
            Self::RenegotiationFailed => "RENEGOTIATION_FAILED",
            Self::InternalError => "INTERNAL_ERROR",
            Self::InvalidSourceLabel => "INVALID_SOURCE_LABEL",
            Self::AlreadySharing => "ALREADY_SHARING",
        }
    }
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

/// Embedded Lua script for atomic screen share limit management.
const SCREEN_SHARE_LIMIT_SCRIPT: &str = include_str!("screen_share_limit.lua");

/// Atomic screen share limit manager backed by a Redis Lua script.
///
/// Provides `check()`, `start()`, and `stop()` operations that are each
/// executed as a single atomic Redis command (via EVALSHA).
#[derive(Clone)]
pub struct ScreenShareLimiter {
    redis: Client,
    script_sha: Arc<RwLock<String>>,
}

impl ScreenShareLimiter {
    /// Create a new limiter. Call [`init`] before use to load the script.
    pub fn new(redis: Client) -> Self {
        Self {
            redis,
            script_sha: Arc::new(RwLock::new(String::new())),
        }
    }

    /// Load the Lua script into Redis. Must be called at startup.
    pub async fn init(&mut self) -> Result<(), fred::error::Error> {
        let sha: String = self.redis.script_load(SCREEN_SHARE_LIMIT_SCRIPT).await?;
        tracing::info!(sha = %sha, "Screen share limit script loaded");
        *self.script_sha.write().await = sha;
        Ok(())
    }

    /// Reload script on NOSCRIPT error.
    async fn reload_script(&self) -> Result<String, fred::error::Error> {
        let sha: String = self.redis.script_load(SCREEN_SHARE_LIMIT_SCRIPT).await?;
        *self.script_sha.write().await = sha.clone();
        Ok(sha)
    }

    /// Run the Lua script with retry on NOSCRIPT.
    async fn run_script(
        &self,
        channel_id: Uuid,
        max_shares: u32,
        op: &str,
    ) -> Result<(bool, i64), ScreenShareError> {
        self.run_script_with_extra(channel_id, max_shares, op, &[])
            .await
    }

    /// Run the Lua script with optional extra ARGV arguments and retry on NOSCRIPT.
    async fn run_script_with_extra(
        &self,
        channel_id: Uuid,
        max_shares: u32,
        op: &str,
        extra_args: &[String],
    ) -> Result<(bool, i64), ScreenShareError> {
        let key = format!("screenshare:limit:{channel_id}");
        let sha = self.script_sha.read().await.clone();
        let mut args: Vec<String> = vec![max_shares.to_string(), op.to_string()];
        args.extend_from_slice(extra_args);

        match self
            .redis
            .evalsha::<Vec<i64>, _, _, _>(&sha, vec![key.as_str()], args.clone())
            .await
        {
            Ok(result) => Self::parse_result(&result),
            Err(e) if e.to_string().contains("NOSCRIPT") => {
                warn!(
                    channel_id = %channel_id,
                    "NOSCRIPT error, reloading screen share limit script"
                );
                let new_sha = self.reload_script().await.map_err(|e| {
                    error!(error = %e, "Failed to reload screen share limit script");
                    ScreenShareError::InternalError
                })?;
                let result = self
                    .redis
                    .evalsha::<Vec<i64>, _, _, _>(&new_sha, vec![key.as_str()], args)
                    .await
                    .map_err(|e| {
                        error!(
                            channel_id = %channel_id,
                            error = %e,
                            "EVALSHA failed after reload"
                        );
                        ScreenShareError::InternalError
                    })?;
                Self::parse_result(&result)
            }
            Err(e) => {
                error!(
                    channel_id = %channel_id,
                    error = %e,
                    "Redis EVALSHA failed"
                );
                Err(ScreenShareError::InternalError)
            }
        }
    }

    fn parse_result(result: &[i64]) -> Result<(bool, i64), ScreenShareError> {
        if result.len() < 2 {
            error!("Unexpected Lua script result length: {}", result.len());
            return Err(ScreenShareError::InternalError);
        }
        Ok((result[0] == 1, result[1]))
    }

    /// Check if a screen share slot is available (does not reserve it).
    pub async fn check(&self, channel_id: Uuid, max_shares: u32) -> Result<(), ScreenShareError> {
        let (allowed, _) = self.run_script(channel_id, max_shares, "check").await?;
        if allowed {
            Ok(())
        } else {
            Err(ScreenShareError::LimitReached)
        }
    }

    /// Atomically reserve a screen share slot. Returns error if limit reached.
    pub async fn start(&self, channel_id: Uuid, max_shares: u32) -> Result<(), ScreenShareError> {
        let (allowed, _) = self.run_script(channel_id, max_shares, "start").await?;
        if allowed {
            Ok(())
        } else {
            Err(ScreenShareError::LimitReached)
        }
    }

    /// Release a screen share slot (decrements if count > 0).
    pub async fn stop(&self, channel_id: Uuid) {
        // max_shares arg is unused by "stop" op, pass 0
        if let Err(e) = self.run_script(channel_id, 0, "stop").await {
            warn!(
                channel_id = %channel_id,
                error = ?e,
                "Failed to decrement screen share counter"
            );
        }
    }

    /// Release N screen share slots at once (atomic batch decrement).
    ///
    /// Used when a user with multiple active screen shares leaves.
    /// Clamps to zero if `count` exceeds the current counter value.
    pub async fn stop_n(&self, channel_id: Uuid, count: usize) {
        if count == 0 {
            return;
        }
        // max_shares arg is unused by "stop_n" op, pass 0
        if let Err(e) = self
            .run_script_with_extra(channel_id, 0, "stop_n", &[count.to_string()])
            .await
        {
            warn!(
                channel_id = %channel_id,
                count = count,
                error = ?e,
                "Failed to batch-decrement screen share counter"
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
        let before = chrono::Utc::now();
        let stream_id = Uuid::new_v4();
        let user_id = Uuid::new_v4();
        let info = ScreenShareInfo::new(
            stream_id,
            user_id,
            "alice".to_string(),
            "Display 1".to_string(),
            true,
            Quality::High,
        );
        let after = chrono::Utc::now();

        assert_eq!(info.stream_id, stream_id);
        assert_eq!(info.user_id, user_id);
        assert_eq!(info.username, "alice");
        assert_eq!(info.source_label, "Display 1");
        assert!(info.has_audio);
        assert_eq!(info.quality, Quality::High);
        assert!(info.started_at >= before && info.started_at <= after);
    }

    #[test]
    fn test_screen_share_info_without_audio() {
        let before = chrono::Utc::now();
        let user_id = Uuid::new_v4();
        let info = ScreenShareInfo::new(
            Uuid::new_v4(),
            user_id,
            "bob".to_string(),
            "Firefox".to_string(),
            false,
            Quality::Medium,
        );
        let after = chrono::Utc::now();

        assert!(!info.has_audio);
        assert_eq!(info.quality, Quality::Medium);
        assert!(info.started_at >= before && info.started_at <= after);
    }

    #[test]
    fn test_screen_share_info_serialization() {
        let stream_id = Uuid::new_v4();
        let user_id = Uuid::new_v4();
        let info = ScreenShareInfo::new(
            stream_id,
            user_id,
            "alice".to_string(),
            "Display 1".to_string(),
            true,
            Quality::High,
        );

        let json = serde_json::to_string(&info).unwrap();
        assert!(json.contains("\"stream_id\""));
        assert!(json.contains("\"username\":\"alice\""));
        assert!(json.contains("\"source_label\":\"Display 1\""));
        assert!(json.contains("\"has_audio\":true"));
        assert!(json.contains("\"started_at\""));

        // Roundtrip
        let deserialized: ScreenShareInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.stream_id, stream_id);
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
        assert!(json.contains("\"error\":\"NO_PERMISSION\""));
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
        // All variants should serialize to SCREAMING_SNAKE_CASE
        let test_cases = [
            (ScreenShareError::NoPermission, "NO_PERMISSION"),
            (ScreenShareError::LimitReached, "LIMIT_REACHED"),
            (ScreenShareError::NotInChannel, "NOT_IN_CHANNEL"),
            (ScreenShareError::QualityNotAllowed, "QUALITY_NOT_ALLOWED"),
            (
                ScreenShareError::RenegotiationFailed,
                "RENEGOTIATION_FAILED",
            ),
            (ScreenShareError::InternalError, "INTERNAL_ERROR"),
            (ScreenShareError::InvalidSourceLabel, "INVALID_SOURCE_LABEL"),
            (ScreenShareError::AlreadySharing, "ALREADY_SHARING"),
        ];

        for (error, expected) in test_cases {
            let json = serde_json::to_string(&error).unwrap();
            assert_eq!(json, format!("\"{expected}\""));
        }
    }

    #[test]
    fn test_screen_share_error_deserialization() {
        let error: ScreenShareError = serde_json::from_str("\"NO_PERMISSION\"").unwrap();
        assert_eq!(error, ScreenShareError::NoPermission);

        let error: ScreenShareError = serde_json::from_str("\"LIMIT_REACHED\"").unwrap();
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
        let stream_id = Uuid::new_v4();
        let json = format!(
            r#"{{"stream_id":"{stream_id}","quality":"high","has_audio":true,"source_label":"Display 1"}}"#
        );
        let req: ScreenShareStartRequest = serde_json::from_str(&json).unwrap();

        assert_eq!(req.stream_id, stream_id);
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

        let stream_id = Uuid::new_v4();
        for (quality_str, expected_quality) in qualities.iter().zip(expected.iter()) {
            let json = format!(
                r#"{{"stream_id":"{stream_id}","quality":"{quality_str}","has_audio":false,"source_label":"test"}}"#
            );
            let req: ScreenShareStartRequest = serde_json::from_str(&json).unwrap();
            assert_eq!(req.quality, *expected_quality);
        }
    }

    // =========================================================================
    // ScreenShareStopRequest Tests
    // =========================================================================

    #[test]
    fn test_stop_request_deserialization() {
        let stream_id = Uuid::new_v4();
        let json = format!(r#"{{"stream_id":"{stream_id}"}}"#);
        let req: ScreenShareStopRequest = serde_json::from_str(&json).unwrap();

        assert_eq!(req.stream_id, stream_id);
    }
}
