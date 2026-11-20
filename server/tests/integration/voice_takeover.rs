//! After the same user joined a room from a second socket, the first socket
//! holds no seat any more: its voice events act on nothing.

use std::time::Duration;

use futures::SinkExt;
use sqlx::PgPool;
use tokio_tungstenite::tungstenite::Message;
use uuid::Uuid;
use vc_server::permissions::GuildPermissions;

use super::helpers::{
    add_guild_member, create_guild_with_default_role, create_test_user,
    create_voice_channel, expect_no_ws_event, generate_access_token, next_ws_event,
    spawn_test_server, ws_connect, TestApp, TestServer, WsClient,
};

const EVENT_TIMEOUT: Duration = Duration::from_secs(15);
const QUIET: Duration = Duration::from_millis(700);

async fn send(ws: &mut WsClient, value: serde_json::Value) {
    ws.send(Message::Text(value.to_string().into()))
        .await
        .expect("send ws frame");
}

async fn connect_and_join(server: &TestServer, token: &str, channel: Uuid) -> WsClient {
    let mut ws = ws_connect(server, token).await;
    next_ws_event(&mut ws, "ready", EVENT_TIMEOUT).await;
    send(
        &mut ws,
        serde_json::json!({ "type": "voice_join", "channel_id": channel }),
    )
    .await;
    next_ws_event(&mut ws, "voice_room_state", EVENT_TIMEOUT).await;
    ws
}

/// A valid publisher offer (audio), as a client would send.
async fn audio_offer() -> String {
    use webrtc::api::media_engine::MediaEngine;
    use webrtc::api::APIBuilder;
    use webrtc::peer_connection::configuration::RTCConfiguration;
    use webrtc::rtp_transceiver::rtp_codec::RTPCodecType;

    super::helpers::voice_media::install_crypto_provider();
    let mut media = MediaEngine::default();
    media.register_default_codecs().expect("codecs");
    let api = APIBuilder::new().with_media_engine(media).build();
    let pc = api
        .new_peer_connection(RTCConfiguration::default())
        .await
        .expect("pc");
    pc.add_transceiver_from_kind(RTPCodecType::Audio, None)
        .await
        .expect("transceiver");
    let offer = pc.create_offer(None).await.expect("offer");
    let sdp = offer.sdp.clone();
    let _ = pc.close().await;
    sdp
}

struct Takeover {
    app: TestApp,
    _server: TestServer,
    voice: Uuid,
    user: Uuid,
    old: WsClient,
    new: WsClient,
    other: WsClient,
}

/// `user` joins on `old`, `other` joins, then `user` joins again on `new`.
async fn takeover(pool: PgPool) -> Takeover {
    let app = TestApp::with_pool_and_screen_share_limiter(pool.clone()).await;
    let server = spawn_test_server(app.router.clone()).await;
    let (owner, _) = create_test_user(&pool).await;
    let (user, _) = create_test_user(&pool).await;
    let guild =
        create_guild_with_default_role(&pool, owner, GuildPermissions::EVERYONE_DEFAULT).await;
    add_guild_member(&pool, guild, user).await;
    let voice = create_voice_channel(&pool, guild, "voice").await;
    let token = generate_access_token(&app.config, user);

    let mut old = connect_and_join(&server, &token, voice).await;
    let mut other = connect_and_join(&server, &generate_access_token(&app.config, owner), voice).await;
    let new = connect_and_join(&server, &token, voice).await;
    let removed = next_ws_event(&mut old, "voice_removed", EVENT_TIMEOUT).await;
    assert_eq!(removed["reason"], "session_replaced");
    // The others saw the join of the new session; nothing else is pending.
    next_ws_event(&mut other, "voice_user_joined", EVENT_TIMEOUT).await;
    Takeover {
        app,
        _server: server,
        voice,
        user,
        old,
        new,
        other,
    }
}

impl Takeover {
    async fn new_session_still_seated(&self) {
        let room = self.app.state.sfu.get_room(self.voice).await.expect("room");
        assert!(room.get_peer(self.user).await.is_some(), "user no longer in the room");
        assert_eq!(room.participant_count().await, 2);
    }
}

#[sqlx::test]
async fn old_socket_leave_does_not_remove_the_new_session(pool: PgPool) {
    let mut t = takeover(pool).await;
    send(
        &mut t.old,
        serde_json::json!({ "type": "voice_leave", "channel_id": t.voice }),
    )
    .await;
    expect_no_ws_event(&mut t.other, "voice_user_left", QUIET).await;
    t.new_session_still_seated().await;
    expect_no_ws_event(&mut t.new, "voice_removed", Duration::from_millis(100)).await;
}

#[sqlx::test]
async fn old_socket_signaling_is_not_applied_to_the_new_session(pool: PgPool) {
    let mut t = takeover(pool).await;
    let sdp = audio_offer().await;

    send(
        &mut t.old,
        serde_json::json!({ "type": "voice_publisher_offer", "channel_id": t.voice, "sdp": sdp }),
    )
    .await;
    let err = next_ws_event(&mut t.old, "voice_error", EVENT_TIMEOUT).await;
    assert_eq!(err["code"], "PARTICIPANT_NOT_FOUND", "{err}");
    expect_no_ws_event(&mut t.new, "voice_publisher_answer", QUIET).await;

    send(
        &mut t.old,
        serde_json::json!({ "type": "voice_subscriber_answer", "channel_id": t.voice, "sdp": "v=0\r\n" }),
    )
    .await;
    let err = next_ws_event(&mut t.old, "voice_error", EVENT_TIMEOUT).await;
    assert_eq!(err["code"], "PARTICIPANT_NOT_FOUND", "{err}");

    send(
        &mut t.old,
        serde_json::json!({
            "type": "voice_ice_candidate",
            "channel_id": t.voice,
            "candidate": "{\"candidate\":\"candidate:1 1 udp 1 192.0.2.1 9 typ host\",\"sdpMid\":\"0\",\"sdpMLineIndex\":0}",
            "pc_type": "publisher",
        }),
    )
    .await;
    let err = next_ws_event(&mut t.old, "voice_error", EVENT_TIMEOUT).await;
    assert_eq!(err["code"], "PARTICIPANT_NOT_FOUND", "{err}");
    t.new_session_still_seated().await;
}

#[sqlx::test]
async fn old_socket_cannot_share_mute_or_start_webcam(pool: PgPool) {
    let mut t = takeover(pool).await;
    send(
        &mut t.old,
        serde_json::json!({
            "type": "voice_screen_share_start",
            "channel_id": t.voice,
            "stream_id": Uuid::new_v4(),
            "quality": "medium",
            "has_audio": false,
            "source_label": "Display 1",
        }),
    )
    .await;
    let err = next_ws_event(&mut t.old, "voice_error", EVENT_TIMEOUT).await;
    assert_eq!(err["code"], "PARTICIPANT_NOT_FOUND", "{err}");

    send(
        &mut t.old,
        serde_json::json!({ "type": "voice_webcam_start", "channel_id": t.voice, "quality": "medium" }),
    )
    .await;
    let err = next_ws_event(&mut t.old, "voice_error", EVENT_TIMEOUT).await;
    assert_eq!(err["code"], "PARTICIPANT_NOT_FOUND", "{err}");

    send(
        &mut t.old,
        serde_json::json!({ "type": "voice_mute", "channel_id": t.voice }),
    )
    .await;
    expect_no_ws_event(&mut t.other, "screen_share_started", QUIET).await;
    expect_no_ws_event(&mut t.other, "webcam_started", Duration::from_millis(100)).await;
    expect_no_ws_event(&mut t.other, "voice_user_muted", Duration::from_millis(100)).await;
    let room = t.app.state.sfu.get_room(t.voice).await.expect("room");
    assert!(room.get_screen_shares().await.is_empty());
    assert!(room.get_webcams().await.is_empty());
    let peer = room.get_peer(t.user).await.expect("peer");
    assert!(!peer.is_self_muted().await);
}

#[sqlx::test]
async fn old_socket_disconnect_removes_nothing(pool: PgPool) {
    let mut t = takeover(pool).await;
    let Takeover { old, .. } = &mut t;
    let _ = old.close(None).await;
    expect_no_ws_event(&mut t.other, "voice_user_left", QUIET).await;
    t.new_session_still_seated().await;
}
