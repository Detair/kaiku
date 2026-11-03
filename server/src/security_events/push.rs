//! `t=security` pushes (contract C28).
//!
//! Always HIGH priority, collapse key `security-<notice_id>`, TTL one day;
//! sent to every push subscription of the recipient, outside the C20
//! coalescer and the notification settings. No content beyond these fields:
//! the app renders the text itself, and no notice carries an action that
//! approves anything.

use uuid::Uuid;

use super::types::{EventType, Severity};
use crate::push::PushPayload;

/// One security push.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SecurityPush {
    pub notice_id: Uuid,
    pub event_type: EventType,
    pub severity: Severity,
    pub count: u32,
    pub subject_id: Uuid,
    /// Only when an admin or the recovery contact acted.
    pub actor_id: Option<Uuid>,
}

impl SecurityPush {
    fn body(&self, count: serde_json::Value) -> serde_json::Value {
        let mut v = serde_json::json!({
            "t": "security",
            "notice_id": self.notice_id.to_string(),
            "event_type": self.event_type.as_str(),
            "severity": self.severity.as_str(),
            "count": count,
            "subject_id": self.subject_id.to_string(),
        });
        if let Some(actor) = self.actor_id {
            v["actor_id"] = serde_json::Value::String(actor.to_string());
        }
        v
    }
}

impl PushPayload for SecurityPush {
    /// FCM `data`: every value a string.
    fn fcm_data(&self) -> serde_json::Value {
        self.body(serde_json::Value::String(self.count.to_string()))
    }

    /// `UnifiedPush`: the same JSON with `count` as a number.
    fn unifiedpush_body(&self) -> Vec<u8> {
        serde_json::to_vec(&self.body(serde_json::Value::from(self.count))).unwrap_or_default()
    }

    fn high_priority(&self) -> bool {
        true
    }

    fn collapse_key(&self) -> String {
        format!("security-{}", self.notice_id)
    }

    fn ttl(&self) -> &'static str {
        "86400s"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unifiedpush_count_is_a_number_and_fcm_a_string() {
        let p = SecurityPush {
            notice_id: Uuid::nil(),
            event_type: EventType::SignIn,
            severity: Severity::Normal,
            count: 3,
            subject_id: Uuid::nil(),
            actor_id: None,
        };
        let up: serde_json::Value = serde_json::from_slice(&p.unifiedpush_body()).unwrap();
        assert_eq!(up["count"], 3);
        assert_eq!(p.fcm_data()["count"], "3");
        assert!(p.fcm_data().get("actor_id").is_none());
        assert_eq!(p.collapse_key(), format!("security-{}", Uuid::nil()));
        assert_eq!(p.ttl(), "86400s");
        assert!(p.high_priority());
    }
}
