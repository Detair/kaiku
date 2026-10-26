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
pub(crate) mod presence_audience;

pub use events::{
    broadcast_admin_event, broadcast_guild_patch, broadcast_member_patch, broadcast_to_channel,
    broadcast_to_guild, broadcast_to_user, broadcast_to_users, broadcast_user_patch, channels,
    ActivityState, ClientEvent, ClientMessageState, CustomStatusState, OutboundMsg, ServerEvent,
    VoiceParticipant,
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

#[doc(hidden)]
pub mod test_hooks {
    //! Pause points for integration tests that need a deterministic
    //! interleaving. Without a registered pause each hook is one uncontended
    //! lock and an empty map lookup.

    use std::collections::HashMap;
    use std::sync::{Arc, Mutex, OnceLock};

    use tokio::sync::Notify;
    use uuid::Uuid;

    /// A pause registered for one user's next socket.
    #[derive(Clone, Default)]
    pub struct Pause {
        /// Notified when the socket reaches the pause point.
        pub reached: Arc<Notify>,
        /// Notify to let the socket continue.
        pub release: Arc<Notify>,
    }

    fn pauses() -> &'static Mutex<HashMap<Uuid, Pause>> {
        static PAUSES: OnceLock<Mutex<HashMap<Uuid, Pause>>> = OnceLock::new();
        PAUSES.get_or_init(Mutex::default)
    }

    /// Pause the next socket of `user_id` after it has read its presence
    /// audience (peers, servers, blocks) and before it subscribes.
    pub fn pause_after_audience_read(user_id: Uuid) -> Pause {
        let pause = Pause::default();
        pauses()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(user_id, pause.clone());
        pause
    }

    pub(crate) async fn after_audience_read(user_id: Uuid) {
        wait(pauses(), user_id).await;
    }

    fn reload_pauses() -> &'static Mutex<HashMap<Uuid, Pause>> {
        static PAUSES: OnceLock<Mutex<HashMap<Uuid, Pause>>> = OnceLock::new();
        PAUSES.get_or_init(Mutex::default)
    }

    /// Pause the next audience reload of a live socket of `user_id` after it
    /// has read the audience and before it applies it (subscribes).
    pub fn pause_in_audience_reload(user_id: Uuid) -> Pause {
        let pause = Pause::default();
        reload_pauses()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(user_id, pause.clone());
        pause
    }

    pub(crate) async fn in_audience_reload(user_id: Uuid) {
        wait(reload_pauses(), user_id).await;
    }

    fn barrier_pauses() -> &'static Mutex<HashMap<Uuid, Pause>> {
        static PAUSES: OnceLock<Mutex<HashMap<Uuid, Pause>>> = OnceLock::new();
        PAUSES.get_or_init(Mutex::default)
    }

    /// Pause the next audience reload round of `user_id` that installed new
    /// subscriptions, right after their barrier (new peers subscribed, not
    /// yet validated).
    pub fn pause_after_audience_barrier(user_id: Uuid) -> Pause {
        let pause = Pause::default();
        barrier_pauses()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(user_id, pause.clone());
        pause
    }

    pub(crate) async fn after_audience_barrier(user_id: Uuid) {
        wait(barrier_pauses(), user_id).await;
    }

    fn failing_reads() -> &'static Mutex<std::collections::HashSet<Uuid>> {
        static FAILING: OnceLock<Mutex<std::collections::HashSet<Uuid>>> = OnceLock::new();
        FAILING.get_or_init(Mutex::default)
    }

    /// Make the next presence-audience read of `user_id` fail like a database
    /// error.
    pub fn fail_next_audience_read(user_id: Uuid) {
        failing_reads()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(user_id);
    }

    pub(crate) fn audience_read_fails(user_id: Uuid) -> bool {
        failing_reads()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(&user_id)
    }

    fn snapshot_pauses() -> &'static Mutex<HashMap<Uuid, Pause>> {
        static PAUSES: OnceLock<Mutex<HashMap<Uuid, Pause>>> = OnceLock::new();
        PAUSES.get_or_init(Mutex::default)
    }

    /// Pause the next presence snapshot (connect, new peers, backstop) of a
    /// socket of `user_id` after its fence, before it queues any entry.
    pub fn pause_in_snapshot(user_id: Uuid) -> Pause {
        let pause = Pause::default();
        snapshot_pauses()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(user_id, pause.clone());
        pause
    }

    pub(crate) async fn in_snapshot(user_id: Uuid) {
        wait(snapshot_pauses(), user_id).await;
    }

    fn backstop_triggers() -> &'static Mutex<HashMap<Uuid, Arc<Notify>>> {
        static TRIGGERS: OnceLock<Mutex<HashMap<Uuid, Arc<Notify>>>> = OnceLock::new();
        TRIGGERS.get_or_init(Mutex::default)
    }

    /// Let tests run the presence backstop of `user_id`'s next socket on
    /// demand: register before the socket connects, then notify.
    pub fn run_backstop(user_id: Uuid) -> Arc<Notify> {
        let trigger = Arc::new(Notify::new());
        backstop_triggers()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(user_id, trigger.clone());
        trigger
    }

    /// The backstop trigger registered for `user_id`, if any (taken once).
    pub(crate) fn backstop_trigger(user_id: Uuid) -> Option<Arc<Notify>> {
        backstop_triggers()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(&user_id)
    }

    fn revocation_pauses() -> &'static Mutex<HashMap<Uuid, Pause>> {
        static PAUSES: OnceLock<Mutex<HashMap<Uuid, Pause>>> = OnceLock::new();
        PAUSES.get_or_init(Mutex::default)
    }

    /// Pause the next block or server leave made by `user_id` after its
    /// database change, before its completion notice.
    pub fn pause_after_revocation_commit(user_id: Uuid) -> Pause {
        let pause = Pause::default();
        revocation_pauses()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(user_id, pause.clone());
        pause
    }

    pub async fn after_revocation_commit(user_id: Uuid) {
        wait(revocation_pauses(), user_id).await;
    }

    fn before_commit_pauses() -> &'static Mutex<HashMap<Uuid, Pause>> {
        static PAUSES: OnceLock<Mutex<HashMap<Uuid, Pause>>> = OnceLock::new();
        PAUSES.get_or_init(Mutex::default)
    }

    /// Pause the next block or server leave made by `user_id` after its
    /// pending revocation was opened (and pre-notice sent), before its
    /// database change.
    pub fn pause_before_revocation_commit(user_id: Uuid) -> Pause {
        let pause = Pause::default();
        before_commit_pauses()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(user_id, pause.clone());
        pause
    }

    pub async fn before_revocation_commit(user_id: Uuid) {
        wait(before_commit_pauses(), user_id).await;
    }

    async fn wait(map: &Mutex<HashMap<Uuid, Pause>>, user_id: Uuid) {
        let pause = map
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(&user_id);
        if let Some(pause) = pause {
            pause.reached.notify_one();
            pause.release.notified().await;
        }
    }
}
