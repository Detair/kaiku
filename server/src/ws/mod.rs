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

pub mod access;
pub mod bot_events;
pub mod bot_gateway;
pub mod connection_limit;
pub mod events;
pub mod handlers;
pub(crate) mod presence_audience;

pub use events::{
    broadcast_admin_event, broadcast_guild_patch, broadcast_member_patch, broadcast_to_channel,
    broadcast_to_guild, broadcast_to_user, broadcast_to_users, broadcast_user_patch, channels,
    ActivityState, ClientEvent, ClientMessageState, CustomStatusState, OutboundMsg, ServerEvent,
    VoiceParticipant, VoiceRemovedReason,
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
    fn voice_removed_wire_format() {
        for (reason, wire) in [
            (super::VoiceRemovedReason::Kicked, "kicked"),
            (super::VoiceRemovedReason::Banned, "banned"),
            (super::VoiceRemovedReason::PermissionLost, "permission_lost"),
            (super::VoiceRemovedReason::ChannelDeleted, "channel_deleted"),
        ] {
            let event = ServerEvent::VoiceRemoved {
                channel_id: Uuid::nil(),
                reason,
            };
            let json: serde_json::Value = serde_json::to_value(&event).unwrap();
            assert_eq!(
                json,
                serde_json::json!({
                    "type": "voice_removed",
                    "channel_id": "00000000-0000-0000-0000-000000000000",
                    "reason": wire,
                })
            );
            assert_eq!(reason.as_str(), wire);
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
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex, OnceLock};

    use tokio::sync::Notify;
    use uuid::Uuid;

    /// Set once any hook is registered (tests only). Until then every hook
    /// is a single relaxed load: production never takes a hook lock.
    static ARMED: AtomicBool = AtomicBool::new(false);

    fn arm() {
        ARMED.store(true, Ordering::Relaxed);
    }

    #[inline]
    fn armed() -> bool {
        ARMED.load(Ordering::Relaxed)
    }

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
        arm();
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
        arm();
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
        arm();
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
        arm();
        failing_reads()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(user_id);
    }

    pub(crate) fn audience_read_fails(user_id: Uuid) -> bool {
        if !armed() {
            return false;
        }
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
        arm();
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
        arm();
        let trigger = Arc::new(Notify::new());
        backstop_triggers()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(user_id, trigger.clone());
        trigger
    }

    /// The backstop trigger registered for `user_id`, if any (taken once).
    pub(crate) fn backstop_trigger(user_id: Uuid) -> Option<Arc<Notify>> {
        if !armed() {
            return None;
        }
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
        arm();
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
        arm();
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

    fn channel_write_pauses() -> &'static Mutex<HashMap<Uuid, Pause>> {
        static PAUSES: OnceLock<Mutex<HashMap<Uuid, Pause>>> = OnceLock::new();
        PAUSES.get_or_init(Mutex::default)
    }

    /// Pause the next channel or voice-room event a socket of `user_id` is
    /// about to write, before its access decision (the event is already
    /// queued).
    pub fn pause_before_channel_write(user_id: Uuid) -> Pause {
        arm();
        let pause = Pause::default();
        channel_write_pauses()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(user_id, pause.clone());
        pause
    }

    pub(crate) async fn before_channel_write(user_id: Uuid) {
        wait(channel_write_pauses(), user_id).await;
    }

    fn subscribe_pauses() -> &'static Mutex<HashMap<Uuid, Pause>> {
        static PAUSES: OnceLock<Mutex<HashMap<Uuid, Pause>>> = OnceLock::new();
        PAUSES.get_or_init(Mutex::default)
    }

    /// Pause the next `subscribe` of `user_id` after its permission check,
    /// before its result is recorded.
    pub fn pause_subscribe_after_check(user_id: Uuid) -> Pause {
        arm();
        let pause = Pause::default();
        subscribe_pauses()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(user_id, pause.clone());
        pause
    }

    pub(crate) async fn subscribe_after_check(user_id: Uuid) {
        wait(subscribe_pauses(), user_id).await;
    }

    fn voice_join_pauses() -> &'static Mutex<HashMap<Uuid, Pause>> {
        static PAUSES: OnceLock<Mutex<HashMap<Uuid, Pause>>> = OnceLock::new();
        PAUSES.get_or_init(Mutex::default)
    }

    /// Pause the next voice join of `user_id` after its permission check,
    /// before its peer is created and added to the room.
    pub fn pause_voice_join_after_check(user_id: Uuid) -> Pause {
        arm();
        let pause = Pause::default();
        voice_join_pauses()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(user_id, pause.clone());
        pause
    }

    pub(crate) async fn voice_join_after_check(user_id: Uuid) {
        wait(voice_join_pauses(), user_id).await;
    }

    fn access_counters() -> &'static Mutex<HashMap<Uuid, (usize, usize)>> {
        static COUNTERS: OnceLock<Mutex<HashMap<Uuid, (usize, usize)>>> = OnceLock::new();
        COUNTERS.get_or_init(Mutex::default)
    }

    /// Count the access revalidation reads of `user_id`'s sockets from now
    /// on (see [`access_reads`]).
    pub fn count_access_reads(user_id: Uuid) {
        arm();
        access_counters()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(user_id, (0, 0));
    }

    /// `(revalidations, batched reads)` of `user_id`'s sockets since
    /// [`count_access_reads`].
    ///
    /// A batched read is one `filter_viewable_channels_in_guilds` call (at
    /// most 4 SQL statements) or one DM participation query.
    pub fn access_reads(user_id: Uuid) -> (usize, usize) {
        arm();
        access_counters()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(&user_id)
            .copied()
            .unwrap_or_default()
    }

    pub(crate) fn count_access_read(user_id: Uuid, reads: usize) {
        if !armed() {
            return;
        }
        if let Some(entry) = access_counters()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get_mut(&user_id)
        {
            entry.0 += 1;
            entry.1 += reads;
        }
    }

    fn failing_access_reads() -> &'static Mutex<HashMap<Uuid, usize>> {
        static FAILING: OnceLock<Mutex<HashMap<Uuid, usize>>> = OnceLock::new();
        FAILING.get_or_init(Mutex::default)
    }

    /// Make the next `n` access revalidations of `user_id`'s sockets fail
    /// like a database error.
    pub fn fail_access_reads(user_id: Uuid, n: usize) {
        arm();
        failing_access_reads()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(user_id, n);
    }

    pub(crate) fn access_read_fails(user_id: Uuid) -> bool {
        if !armed() {
            return false;
        }
        let mut map = failing_access_reads()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        match map.get_mut(&user_id) {
            Some(n) if *n > 0 => {
                *n -= 1;
                true
            }
            _ => false,
        }
    }

    fn wake_disabled() -> &'static Mutex<std::collections::HashSet<Uuid>> {
        static DISABLED: OnceLock<Mutex<std::collections::HashSet<Uuid>>> = OnceLock::new();
        DISABLED.get_or_init(Mutex::default)
    }

    /// Sockets of `user_id` opened from now on do not revalidate when woken
    /// by a bump (only at write time and on the backstop), so a test can
    /// show that delivery itself is fenced.
    pub fn disable_access_wake(user_id: Uuid) {
        arm();
        wake_disabled()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(user_id);
    }

    pub(crate) fn access_wake_disabled(user_id: Uuid) -> bool {
        if !armed() {
            return false;
        }
        wake_disabled()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .contains(&user_id)
    }

    fn bump_pauses() -> &'static Mutex<HashMap<Uuid, Pause>> {
        static PAUSES: OnceLock<Mutex<HashMap<Uuid, Pause>>> = OnceLock::new();
        PAUSES.get_or_init(Mutex::default)
    }

    /// Pause the next access trigger for `scope_id` (the user or server whose
    /// epoch it bumps) right after its commit and bump, before any other
    /// post-commit work of the request.
    pub fn pause_after_access_bump(scope_id: Uuid) -> Pause {
        arm();
        let pause = Pause::default();
        bump_pauses()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(scope_id, pause.clone());
        pause
    }

    pub(crate) async fn after_access_bump(change: crate::ws::access::AccessChange) {
        if !armed() {
            return;
        }
        if let Some((_, id)) = change.scope() {
            wait(bump_pauses(), id).await;
        }
    }

    fn feed_write_pauses() -> &'static Mutex<HashMap<Uuid, Pause>> {
        static PAUSES: OnceLock<Mutex<HashMap<Uuid, Pause>>> = OnceLock::new();
        PAUSES.get_or_init(Mutex::default)
    }

    /// Pause the next server-feed (`guild:{id}`) or admin event a socket of
    /// `user_id` is about to write, before its access decision (the event is
    /// already queued).
    pub fn pause_before_feed_write(user_id: Uuid) -> Pause {
        arm();
        let pause = Pause::default();
        feed_write_pauses()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(user_id, pause.clone());
        pause
    }

    pub(crate) async fn before_feed_write(user_id: Uuid) {
        wait(feed_write_pauses(), user_id).await;
    }

    fn upload_pauses() -> &'static Mutex<HashMap<Uuid, Pause>> {
        static PAUSES: OnceLock<Mutex<HashMap<Uuid, Pause>>> = OnceLock::new();
        PAUSES.get_or_init(Mutex::default)
    }

    /// Pause the next file upload of `user_id` after its objects are stored
    /// and before its database rows are written.
    pub fn pause_after_object_upload(user_id: Uuid) -> Pause {
        arm();
        let pause = Pause::default();
        upload_pauses()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(user_id, pause.clone());
        pause
    }

    pub(crate) async fn after_object_upload(user_id: Uuid) {
        wait(upload_pauses(), user_id).await;
    }

    fn voice_leave_pauses() -> &'static Mutex<HashMap<Uuid, Pause>> {
        static PAUSES: OnceLock<Mutex<HashMap<Uuid, Pause>>> = OnceLock::new();
        PAUSES.get_or_init(Mutex::default)
    }

    /// Pause the next voice departure of `user_id` (whoever asks for it: the
    /// user, a revocation, the closing socket, the shutdown) before the peer
    /// is removed.
    pub fn pause_voice_leave(user_id: Uuid) -> Pause {
        arm();
        let pause = Pause::default();
        voice_leave_pauses()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(user_id, pause.clone());
        pause
    }

    pub(crate) async fn before_voice_leave(user_id: Uuid) {
        wait(voice_leave_pauses(), user_id).await;
    }

    async fn wait(map: &Mutex<HashMap<Uuid, Pause>>, user_id: Uuid) {
        if !armed() {
            return;
        }
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
