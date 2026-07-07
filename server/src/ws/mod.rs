//! WebSocket Handler
//!
//! Real-time communication for chat and voice signaling.
//!
//! ## Authentication
//!
//! WebSocket authentication uses the `Sec-WebSocket-Protocol` header instead of
//! query parameters to avoid token exposure in logs, browser history, and referrer
//! headers.
//!
//! Client should connect with:
//! ```text
//! Sec-WebSocket-Protocol: access_token.<jwt_token>
//! ```
//!
//! Server responds with:
//! ```text
//! Sec-WebSocket-Protocol: access_token
//! ```
//!
//! ## Module layout
//!
//! - [`events`] — `ClientEvent` and `ServerEvent` types, broadcast helpers, Redis pub/sub channel
//!   names. Callers from other modules depend on these for publishing events
//!   (`broadcast_to_channel`, `broadcast_to_user`, ...).
//! - [`handlers`] — HTTP upgrade entry point, per-connection socket loop, `handle_client_message`
//!   dispatch, and the `handle_pubsub` forwarder.
//! - [`bot_events`] / [`bot_gateway`] — Separate WebSocket gateway for bots.

pub mod bot_events;
pub mod bot_gateway;
pub mod events;
pub mod handlers;

pub use events::{
    broadcast_admin_event, broadcast_guild_patch, broadcast_member_patch, broadcast_to_channel,
    broadcast_to_guild, broadcast_to_user, broadcast_user_patch, channels, ActivityState,
    ClientEvent,
    ClientMessageState, CustomStatusState, OutboundMsg, ServerEvent, VoiceParticipant,
};
pub use handlers::{handle_client_message, handler, spawn_custom_status_sweep};

#[cfg(test)]
mod tests {
    use uuid::Uuid;
    use vc_common::protocol::PcType;

    use super::{ClientEvent, ServerEvent};

    #[test]
    fn test_publisher_offer_serialization() {
        let event = ClientEvent::VoicePublisherOffer {
            channel_id: Uuid::nil(),
            sdp: "v=0\r\n".to_string(),
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("voice_publisher_offer"));
        let parsed: ClientEvent = serde_json::from_str(&json).unwrap();
        match parsed {
            ClientEvent::VoicePublisherOffer { sdp, .. } => assert_eq!(sdp, "v=0\r\n"),
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn test_subscriber_answer_serialization() {
        let event = ClientEvent::VoiceSubscriberAnswer {
            channel_id: Uuid::nil(),
            sdp: "v=0\r\n".to_string(),
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("voice_subscriber_answer"));
        let parsed: ClientEvent = serde_json::from_str(&json).unwrap();
        match parsed {
            ClientEvent::VoiceSubscriberAnswer { sdp, .. } => assert_eq!(sdp, "v=0\r\n"),
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn test_subscriber_offer_serialization() {
        let event = ServerEvent::VoiceSubscriberOffer {
            channel_id: Uuid::nil(),
            sdp: "v=0\r\n".to_string(),
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("voice_subscriber_offer"));
    }

    #[test]
    fn test_publisher_answer_serialization() {
        let event = ServerEvent::VoicePublisherAnswer {
            channel_id: Uuid::nil(),
            sdp: "v=0\r\n".to_string(),
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("voice_publisher_answer"));
    }

    #[test]
    fn test_ice_candidate_pc_type_default() {
        // Old clients send without pc_type — should default to "publisher"
        let json = r#"{"type":"voice_ice_candidate","channel_id":"00000000-0000-0000-0000-000000000000","candidate":"candidate:..."}"#;
        let parsed: ClientEvent = serde_json::from_str(json).unwrap();
        match parsed {
            ClientEvent::VoiceIceCandidate { pc_type, .. } => {
                assert_eq!(pc_type, PcType::Publisher);
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn test_ice_candidate_pc_type_explicit() {
        // New clients send with explicit pc_type
        let json = r#"{"type":"voice_ice_candidate","channel_id":"00000000-0000-0000-0000-000000000000","candidate":"candidate:...","pc_type":"subscriber"}"#;
        let parsed: ClientEvent = serde_json::from_str(json).unwrap();
        match parsed {
            ClientEvent::VoiceIceCandidate { pc_type, .. } => {
                assert_eq!(pc_type, PcType::Subscriber);
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn test_server_ice_candidate_includes_pc_type() {
        let event = ServerEvent::VoiceIceCandidate {
            channel_id: Uuid::nil(),
            candidate: "candidate:...".to_string(),
            pc_type: PcType::Subscriber,
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("\"pc_type\":\"subscriber\""));
    }
}
