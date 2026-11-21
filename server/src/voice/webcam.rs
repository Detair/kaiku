//! Webcam video data types and state.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::Quality;

/// Information about an active webcam session.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WebcamInfo {
    /// User who has their webcam on.
    pub user_id: Uuid,
    /// Username for display.
    pub username: String,
    /// Current quality tier.
    pub quality: Quality,
    /// The voice seat (peer session) the webcam belongs to. Server-side
    /// only: a seat changes or removes only its own webcam.
    #[serde(skip)]
    pub owner: Uuid,
}

impl WebcamInfo {
    /// Create a new [`WebcamInfo`].
    #[must_use]
    pub const fn new(user_id: Uuid, username: String, quality: Quality) -> Self {
        Self {
            user_id,
            username,
            quality,
            owner: Uuid::nil(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_webcam_info_creation() {
        let user_id = Uuid::new_v4();
        let info = WebcamInfo::new(user_id, "alice".to_string(), Quality::High);

        assert_eq!(info.user_id, user_id);
        assert_eq!(info.username, "alice");
        assert_eq!(info.quality, Quality::High);
    }

    #[test]
    fn test_webcam_info_serialization() {
        let user_id = Uuid::new_v4();
        let info = WebcamInfo::new(user_id, "bob".to_string(), Quality::Medium);

        let json = serde_json::to_string(&info).unwrap();
        assert!(json.contains("\"username\":\"bob\""));

        let deserialized: WebcamInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.user_id, user_id);
        assert_eq!(deserialized.username, "bob");
    }
}
