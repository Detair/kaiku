//! Rich presence activity types.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use unicode_segmentation::UnicodeSegmentation;

/// Maximum length for activity name.
pub const MAX_ACTIVITY_NAME_LEN: usize = 128;

/// Maximum length for activity details.
pub const MAX_ACTIVITY_DETAILS_LEN: usize = 256;

/// Maximum length for custom status text.
pub const MAX_CUSTOM_STATUS_TEXT_LEN: usize = 128;

/// Maximum grapheme clusters for custom status emoji.
pub const MAX_CUSTOM_STATUS_EMOJI_GRAPHEMES: usize = 10;

/// Maximum combining marks per base character.
const MAX_COMBINING_MARKS_PER_BASE: usize = 3;

/// Type of activity the user is engaged in.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ActivityType {
    Game,
    Listening,
    Watching,
    Coding,
    Custom,
}

/// Returns true if the character is an unsafe Unicode format or override character.
fn is_unsafe_unicode(c: char) -> bool {
    (c.is_control() && c != ' ' && c != '\n')
        || matches!(c, '\u{200B}' | '\u{200C}' | '\u{200D}') // zero-width chars
        || matches!(c, '\u{202C}' | '\u{202D}' | '\u{202E}') // bidi overrides
}

/// Check if a Unicode character is a combining mark.
///
/// Covers the main combining mark blocks used in Zalgo text attacks.
const fn is_combining_mark(c: char) -> bool {
    let cp = c as u32;
    matches!(cp,
        0x0300..=0x036F   // Combining Diacritical Marks
        | 0x1AB0..=0x1AFF // Combining Diacritical Marks Extended
        | 0x1DC0..=0x1DFF // Combining Diacritical Marks Supplement
        | 0x20D0..=0x20FF // Combining Diacritical Marks for Symbols
        | 0xFE20..=0xFE2F // Combining Half Marks
    )
}

/// Validate text for unsafe Unicode characters and combining mark abuse.
///
/// Reusable across custom status, activity names, display names, etc.
pub fn validate_unicode_text(text: &str, max_chars: usize) -> Result<(), &'static str> {
    if text.chars().count() > max_chars {
        return Err("Text too long");
    }

    if text.chars().any(is_unsafe_unicode) {
        return Err("Text contains invalid characters");
    }

    // Check for Zalgo-style combining mark abuse
    let mut combining_count: usize = 0;
    for c in text.chars() {
        if is_combining_mark(c) {
            combining_count += 1;
            if combining_count > MAX_COMBINING_MARKS_PER_BASE {
                return Err("Too many combining marks on a single character");
            }
        } else {
            combining_count = 0;
        }
    }

    Ok(())
}

/// Custom status set by a user (text + optional emoji + optional expiry).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, utoipa::ToSchema)]
pub struct CustomStatus {
    /// Display text for the custom status.
    pub text: String,
    /// Optional emoji (max 10 grapheme clusters).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub emoji: Option<String>,
    /// When the custom status expires (UTC).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<DateTime<Utc>>,
}

impl CustomStatus {
    /// Validate custom status data. Returns an error message if invalid.
    pub fn validate(&self) -> Result<(), &'static str> {
        let trimmed = self.text.trim();
        if trimmed.is_empty() {
            return Err("Custom status text cannot be empty");
        }
        validate_unicode_text(trimmed, MAX_CUSTOM_STATUS_TEXT_LEN)?;

        if let Some(ref emoji) = self.emoji {
            if emoji.graphemes(true).count() > MAX_CUSTOM_STATUS_EMOJI_GRAPHEMES {
                return Err("Emoji field too long (max 10 emoji)");
            }
            validate_unicode_text(emoji, MAX_CUSTOM_STATUS_TEXT_LEN)?;
        }

        if let Some(expires_at) = self.expires_at {
            if expires_at <= Utc::now() {
                return Err("Expiry time must be in the future");
            }
        }

        Ok(())
    }
}

/// Rich presence activity data.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Activity {
    /// Type of activity.
    #[serde(rename = "type")]
    pub activity_type: ActivityType,
    /// Display name (e.g., "Minecraft", "VS Code").
    pub name: String,
    /// When the activity started.
    pub started_at: DateTime<Utc>,
    /// Optional details (e.g., "Creative Mode", "Editing main.rs").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<String>,
}

impl Activity {
    /// Validate activity data. Returns an error message if invalid.
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.name.is_empty() {
            return Err("Activity name cannot be empty");
        }
        validate_unicode_text(&self.name, MAX_ACTIVITY_NAME_LEN)?;
        if let Some(ref details) = self.details {
            validate_unicode_text(details, MAX_ACTIVITY_DETAILS_LEN)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_activity_serialization() {
        let activity = Activity {
            activity_type: ActivityType::Game,
            name: "Minecraft".to_string(),
            started_at: Utc::now(),
            details: None,
        };
        let json = serde_json::to_string(&activity).unwrap();
        assert!(json.contains("\"type\":\"game\""));
        assert!(json.contains("\"name\":\"Minecraft\""));
        assert!(!json.contains("\"details\"")); // Should be skipped when None
    }

    #[test]
    fn test_activity_with_details() {
        let activity = Activity {
            activity_type: ActivityType::Coding,
            name: "VS Code".to_string(),
            started_at: Utc::now(),
            details: Some("Editing main.rs".to_string()),
        };
        let json = serde_json::to_string(&activity).unwrap();
        assert!(json.contains("\"type\":\"coding\""));
        assert!(json.contains("\"details\":\"Editing main.rs\""));
    }

    #[test]
    fn test_activity_deserialization() {
        let json = r#"{"type":"game","name":"Valorant","started_at":"2026-01-20T12:00:00Z"}"#;
        let activity: Activity = serde_json::from_str(json).unwrap();
        assert_eq!(activity.activity_type, ActivityType::Game);
        assert_eq!(activity.name, "Valorant");
        assert!(activity.details.is_none());
    }

    #[test]
    fn test_activity_type_serialization() {
        assert_eq!(
            serde_json::to_string(&ActivityType::Game).unwrap(),
            "\"game\""
        );
        assert_eq!(
            serde_json::to_string(&ActivityType::Listening).unwrap(),
            "\"listening\""
        );
        assert_eq!(
            serde_json::to_string(&ActivityType::Watching).unwrap(),
            "\"watching\""
        );
        assert_eq!(
            serde_json::to_string(&ActivityType::Coding).unwrap(),
            "\"coding\""
        );
        assert_eq!(
            serde_json::to_string(&ActivityType::Custom).unwrap(),
            "\"custom\""
        );
    }

    #[test]
    fn test_activity_type_deserialization() {
        assert_eq!(
            serde_json::from_str::<ActivityType>("\"game\"").unwrap(),
            ActivityType::Game
        );
        assert_eq!(
            serde_json::from_str::<ActivityType>("\"listening\"").unwrap(),
            ActivityType::Listening
        );
    }

    #[test]
    fn test_activity_validation_valid() {
        let activity = Activity {
            activity_type: ActivityType::Game,
            name: "Minecraft".to_string(),
            started_at: Utc::now(),
            details: Some("Creative Mode".to_string()),
        };
        assert!(activity.validate().is_ok());
    }

    #[test]
    fn test_activity_validation_empty_name() {
        let activity = Activity {
            activity_type: ActivityType::Game,
            name: String::new(),
            started_at: Utc::now(),
            details: None,
        };
        assert!(activity.validate().is_err());
    }

    #[test]
    fn test_activity_validation_name_too_long() {
        let activity = Activity {
            activity_type: ActivityType::Game,
            name: "x".repeat(MAX_ACTIVITY_NAME_LEN + 1),
            started_at: Utc::now(),
            details: None,
        };
        assert!(activity.validate().is_err());
    }

    #[test]
    fn test_activity_validation_details_too_long() {
        let activity = Activity {
            activity_type: ActivityType::Game,
            name: "Test".to_string(),
            started_at: Utc::now(),
            details: Some("x".repeat(MAX_ACTIVITY_DETAILS_LEN + 1)),
        };
        assert!(activity.validate().is_err());
    }

    #[test]
    fn test_activity_validation_control_characters() {
        let activity = Activity {
            activity_type: ActivityType::Game,
            name: "Test\x00Game".to_string(),
            started_at: Utc::now(),
            details: None,
        };
        assert!(activity.validate().is_err());
    }

    #[test]
    fn test_activity_deserialization_invalid_type() {
        let json = r#"{"type":"invalid_type","name":"Test","started_at":"2026-01-20T12:00:00Z"}"#;
        let result: Result<Activity, _> = serde_json::from_str(json);
        assert!(result.is_err());
    }

    #[test]
    fn test_activity_deserialization_missing_name() {
        let json = r#"{"type":"game","started_at":"2026-01-20T12:00:00Z"}"#;
        let result: Result<Activity, _> = serde_json::from_str(json);
        assert!(result.is_err());
    }

    #[test]
    fn test_activity_deserialization_missing_started_at() {
        let json = r#"{"type":"game","name":"Test"}"#;
        let result: Result<Activity, _> = serde_json::from_str(json);
        assert!(result.is_err());
    }

    #[test]
    fn test_activity_deserialization_invalid_timestamp() {
        let json = r#"{"type":"game","name":"Test","started_at":"not-a-timestamp"}"#;
        let result: Result<Activity, _> = serde_json::from_str(json);
        assert!(result.is_err());
    }

    #[test]
    fn test_activity_deserialization_extra_fields_ignored() {
        let json = r#"{"type":"game","name":"Test","started_at":"2026-01-20T12:00:00Z","unknown_field":"value"}"#;
        let result: Result<Activity, _> = serde_json::from_str(json);
        // Extra fields should be silently ignored by serde
        assert!(result.is_ok());
    }

    #[test]
    fn test_activity_type_deserialization_case_sensitive() {
        // ActivityType should be lowercase per serde config
        assert!(serde_json::from_str::<ActivityType>("\"GAME\"").is_err());
        assert!(serde_json::from_str::<ActivityType>("\"Game\"").is_err());
        assert!(serde_json::from_str::<ActivityType>("\"game\"").is_ok());
    }

    #[test]
    fn test_activity_name_max_length_boundary() {
        let activity = Activity {
            activity_type: ActivityType::Game,
            name: "x".repeat(MAX_ACTIVITY_NAME_LEN),
            started_at: Utc::now(),
            details: None,
        };
        assert!(
            activity.validate().is_ok(),
            "Name at exactly MAX_ACTIVITY_NAME_LEN should pass"
        );
    }

    #[test]
    fn test_activity_details_max_length_boundary() {
        let activity = Activity {
            activity_type: ActivityType::Game,
            name: "Test".to_string(),
            started_at: Utc::now(),
            details: Some("d".repeat(MAX_ACTIVITY_DETAILS_LEN)),
        };
        assert!(
            activity.validate().is_ok(),
            "Details at exactly MAX_ACTIVITY_DETAILS_LEN should pass"
        );
    }

    #[test]
    fn test_activity_roundtrip_with_all_types() {
        let types = [
            ActivityType::Game,
            ActivityType::Listening,
            ActivityType::Watching,
            ActivityType::Coding,
            ActivityType::Custom,
        ];
        for activity_type in types {
            let activity = Activity {
                activity_type: activity_type.clone(),
                name: "RoundTrip".to_string(),
                started_at: Utc::now(),
                details: Some("testing".to_string()),
            };
            let json = serde_json::to_string(&activity).unwrap();
            let roundtripped: Activity = serde_json::from_str(&json).unwrap();
            assert_eq!(
                roundtripped, activity,
                "Round-trip failed for {activity_type:?}"
            );
        }
    }

    // --- validate_unicode_text tests ---

    #[test]
    fn test_validate_unicode_text_valid() {
        assert!(validate_unicode_text("Hello world", 128).is_ok());
        assert!(validate_unicode_text("Café ☕", 128).is_ok());
        assert!(validate_unicode_text("a\u{0301}", 128).is_ok());
    }

    #[test]
    fn test_validate_unicode_text_too_long() {
        let long = "a".repeat(129);
        assert!(validate_unicode_text(&long, 128).is_err());
    }

    #[test]
    fn test_validate_unicode_text_control_chars() {
        assert!(validate_unicode_text("hello\x00world", 128).is_err());
        assert!(validate_unicode_text("hello\x1Fworld", 128).is_err());
    }

    #[test]
    fn test_validate_unicode_text_allows_newlines() {
        assert!(validate_unicode_text("line1\nline2", 128).is_ok());
    }

    #[test]
    fn test_validate_unicode_text_format_chars() {
        assert!(validate_unicode_text("hello\u{200B}world", 128).is_err());
        assert!(validate_unicode_text("hello\u{200D}world", 128).is_err());
        assert!(validate_unicode_text("hello\u{200C}world", 128).is_err());
    }

    #[test]
    fn test_validate_unicode_text_bidi_overrides() {
        assert!(validate_unicode_text("hello\u{202E}world", 128).is_err());
        assert!(validate_unicode_text("hello\u{202D}world", 128).is_err());
        assert!(validate_unicode_text("hello\u{202C}world", 128).is_err());
    }

    #[test]
    fn test_validate_unicode_text_combining_mark_limit() {
        // 3 combining marks on one base: OK
        let ok = "a\u{0301}\u{0302}\u{0303}";
        assert!(validate_unicode_text(ok, 128).is_ok());

        // 4 combining marks on one base: rejected (Zalgo)
        let zalgo = "a\u{0301}\u{0302}\u{0303}\u{0304}";
        assert!(validate_unicode_text(zalgo, 128).is_err());
    }

    // --- CustomStatus tests ---

    #[test]
    fn test_custom_status_validate_valid() {
        let status = CustomStatus {
            text: "In a meeting".to_string(),
            emoji: Some("📅".to_string()),
            expires_at: Some(Utc::now() + chrono::Duration::hours(1)),
        };
        assert!(status.validate().is_ok());
    }

    #[test]
    fn test_custom_status_validate_no_emoji() {
        let status = CustomStatus {
            text: "Busy".to_string(),
            emoji: None,
            expires_at: None,
        };
        assert!(status.validate().is_ok());
    }

    #[test]
    fn test_custom_status_validate_empty_text() {
        let status = CustomStatus {
            text: "   ".to_string(),
            emoji: None,
            expires_at: None,
        };
        assert!(status.validate().is_err());
    }

    #[test]
    fn test_custom_status_validate_text_too_long() {
        let status = CustomStatus {
            text: "a".repeat(129),
            emoji: None,
            expires_at: None,
        };
        assert!(status.validate().is_err());
    }

    #[test]
    fn test_custom_status_validate_emoji_too_many_graphemes() {
        let status = CustomStatus {
            text: "hi".to_string(),
            emoji: Some("🎮🎵🎨🎭🎪🎫🎬🎤🎧🎼🎹".to_string()), // 11 emoji
            expires_at: None,
        };
        assert!(status.validate().is_err());
    }

    #[test]
    fn test_custom_status_validate_expires_at_in_past() {
        let status = CustomStatus {
            text: "hi".to_string(),
            emoji: None,
            expires_at: Some(Utc::now() - chrono::Duration::hours(1)),
        };
        assert!(status.validate().is_err());
    }

    #[test]
    fn test_custom_status_serialization() {
        let status = CustomStatus {
            text: "In queue".to_string(),
            emoji: Some("🎮".to_string()),
            expires_at: None,
        };
        let json = serde_json::to_string(&status).unwrap();
        assert!(json.contains("\"text\":\"In queue\""));
        assert!(json.contains("\"emoji\":\"🎮\""));
        assert!(!json.contains("\"expires_at\"")); // skipped when None
    }
}
