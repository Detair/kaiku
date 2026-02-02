//! Rich presence activity types.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Maximum length for activity name.
pub const MAX_ACTIVITY_NAME_LEN: usize = 128;

/// Maximum length for activity details.
pub const MAX_ACTIVITY_DETAILS_LEN: usize = 256;

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
        if self.name.len() > MAX_ACTIVITY_NAME_LEN {
            return Err("Activity name too long (max 128 characters)");
        }
        if let Some(ref details) = self.details {
            if details.len() > MAX_ACTIVITY_DETAILS_LEN {
                return Err("Activity details too long (max 256 characters)");
            }
        }
        // Check for control characters (potential injection)
        if self.name.chars().any(|c| c.is_control() && c != ' ') {
            return Err("Activity name contains invalid characters");
        }
        if let Some(ref details) = self.details {
            if details
                .chars()
                .any(|c| c.is_control() && c != ' ' && c != '\n')
            {
                return Err("Activity details contains invalid characters");
            }
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
            name: "".to_string(),
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
}
