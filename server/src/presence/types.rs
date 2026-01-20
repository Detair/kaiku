//! Rich presence activity types.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Type of activity the user is engaged in.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ActivityType {
    Game,
    Listening,
    Watching,
    Coding,
    Custom,
}

/// Rich presence activity data.
#[derive(Debug, Clone, Serialize, Deserialize)]
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
        assert!(json.contains("Minecraft"));
        assert!(json.contains("\"type\":\"game\""));
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
}
