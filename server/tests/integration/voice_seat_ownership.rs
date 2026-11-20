//! What a voice seat puts into the room belongs to that seat: when the same
//! user takes a new seat while an old seat's leave, start, stop or expiry is
//! half done, the old seat's late step changes nothing of the new seat's and
//! tells the others nothing about it.
//!
//! Each case pauses the old seat at the step the test names (a pause point
//! the test controls), lets the user take the new seat, then lets the old
//! step finish and checks the new seat's state and what the others saw.

use std::time::Duration;

use fred::interfaces::KeysInterface;
use futures::SinkExt;
use sqlx::PgPool;
use tokio_tungstenite::tungstenite::Message;
use uuid::Uuid;
use vc_server::permissions::GuildPermissions;
use vc_server::ws::test_hooks::{pause_voice_step, Pause};

use super::helpers::{
    add_guild_member, create_guild_with_default_role, create_test_user, create_voice_channel,
    expect_no_ws_event, generate_access_token, next_ws_event, spawn_test_server, ws_connect,
    TestApp, TestServer, WsClient,
};

const WAIT: Duration = Duration::from_secs(15);
const QUIET: Duration = Duration::from_millis(800);

async fn send(ws: &mut WsClient, value: serde_json::Value) {
    ws.send(Message::Text(value.to_string().into()))
        .await
        .expect("send ws frame");
}

struct Room {
    app: TestApp,
    server: TestServer,
    voice: Uuid,
    user: Uuid,
    token: String,
    /// Another participant: what the others are told.
    viewer: WsClient,
}

async fn room(pool: PgPool) -> Room {
    let app = TestApp::with_pool_and_screen_share_limiter(pool.clone()).await;
    let server = spawn_test_server(app.router.clone()).await;
    let (owner, _) = create_test_user(&pool).await;
    let (user, _) = create_test_user(&pool).await;
    let guild =
        create_guild_with_default_role(&pool, owner, GuildPermissions::EVERYONE_DEFAULT).await;
    add_guild_member(&pool, guild, user).await;
    let voice = create_voice_channel(&pool, guild, "voice").await;
    let viewer = joined(&server, &generate_access_token(&app.config, owner), voice).await;
    let token = generate_access_token(&app.config, user);
    Room {
        app,
        server,
        voice,
        user,
        token,
        viewer,
    }
}

async fn joined(server: &TestServer, token: &str, voice: Uuid) -> WsClient {
    let mut ws = ws_connect(server, token).await;
    next_ws_event(&mut ws, "ready", WAIT).await;
    send(
        &mut ws,
        serde_json::json!({ "type": "voice_join", "channel_id": voice }),
    )
    .await;
    next_ws_event(&mut ws, "voice_room_state", WAIT).await;
    ws
}

fn share_start(voice: Uuid, stream: Uuid) -> serde_json::Value {
    serde_json::json!({
        "type": "voice_screen_share_start",
        "channel_id": voice,
        "stream_id": stream,
        "quality": "medium",
        "has_audio": false,
        "source_label": "Display 1",
    })
}

fn webcam_start(voice: Uuid) -> serde_json::Value {
    serde_json::json!({ "type": "voice_webcam_start", "channel_id": voice, "quality": "medium" })
}

impl Room {
    /// The user takes a new seat from another socket; returns that socket.
    async fn take_over(&mut self) -> WsClient {
        let new = joined(&self.server, &self.token, self.voice).await;
        next_ws_event(&mut self.viewer, "voice_user_joined", WAIT).await;
        new
    }

    async fn slots(&self) -> i64 {
        let key = format!("screenshare:limit:{}", self.voice);
        let n: Option<i64> = self.app.state.redis.get(&key).await.expect("get counter");
        n.unwrap_or(0)
    }

    async fn release_slots(&self) {
        let key = format!("screenshare:limit:{}", self.voice);
        let _: i64 = self.app.state.redis.del(&key).await.expect("del counter");
    }

    async fn reached(pause: &Pause) {
        tokio::time::timeout(WAIT, pause.reached.notified())
            .await
            .expect("the old seat reached the pause point");
    }

    /// The new seat holds a webcam; the others were not told it stopped.
    async fn assert_new_webcam_kept(&mut self) {
        expect_no_ws_event(&mut self.viewer, "webcam_stopped", QUIET).await;
        let room = self.app.state.sfu.get_room(self.voice).await.expect("room");
        assert_eq!(
            room.get_webcams().await.len(),
            1,
            "the new seat's webcam is gone"
        );
    }
}

/// Leave: the user is seated again before the old seat's leave tells the
/// others: nobody is told the user left.
#[sqlx::test]
async fn leave_finishing_after_a_rejoin_tells_nobody(pool: PgPool) {
    let mut r = room(pool).await;
    let mut old = joined(&r.server, &r.token, r.voice).await;
    next_ws_event(&mut r.viewer, "voice_user_joined", WAIT).await;

    let pause = pause_voice_step(r.user, "leave");
    send(
        &mut old,
        serde_json::json!({ "type": "voice_leave", "channel_id": r.voice }),
    )
    .await;
    Room::reached(&pause).await;
    let _new = r.take_over().await;
    pause.release.notify_one();

    expect_no_ws_event(&mut r.viewer, "voice_user_left", QUIET).await;
    let room = r.app.state.sfu.get_room(r.voice).await.expect("room");
    assert!(room.get_peer(r.user).await.is_some(), "the user is gone");
}

/// Webcam start: the old seat's start, checked before the take-over, adds
/// nothing and does not remove the new seat's webcam.
#[sqlx::test]
async fn webcam_start_of_a_replaced_seat_changes_nothing(pool: PgPool) {
    let mut r = room(pool).await;
    let mut old = joined(&r.server, &r.token, r.voice).await;
    next_ws_event(&mut r.viewer, "voice_user_joined", WAIT).await;

    let pause = pause_voice_step(r.user, "webcam_start");
    send(&mut old, webcam_start(r.voice)).await;
    Room::reached(&pause).await;
    let mut new = r.take_over().await;
    send(&mut new, webcam_start(r.voice)).await;
    next_ws_event(&mut new, "webcam_started", WAIT).await;
    next_ws_event(&mut r.viewer, "webcam_started", WAIT).await;
    pause.release.notify_one();

    let err = next_ws_event(&mut old, "voice_error", WAIT).await;
    assert_eq!(err["code"], "PARTICIPANT_NOT_FOUND", "{err}");
    expect_no_ws_event(&mut r.viewer, "webcam_started", Duration::from_millis(100)).await;
    r.assert_new_webcam_kept().await;
}

/// Share start: a start whose seat was replaced after it registered the
/// share is not announced (the replacement already announced its end).
#[sqlx::test]
async fn share_start_of_a_replaced_seat_is_not_announced(pool: PgPool) {
    let mut r = room(pool).await;
    let mut old = joined(&r.server, &r.token, r.voice).await;
    next_ws_event(&mut r.viewer, "voice_user_joined", WAIT).await;

    let pause = pause_voice_step(r.user, "share_start");
    send(&mut old, share_start(r.voice, Uuid::new_v4())).await;
    Room::reached(&pause).await;
    // The take-over announced the share's end (before the user's join).
    let _new = r.take_over().await;
    pause.release.notify_one();

    expect_no_ws_event(&mut r.viewer, "screen_share_started", QUIET).await;
    let room = r.app.state.sfu.get_room(r.voice).await.expect("room");
    assert!(room.get_screen_shares().await.is_empty(), "ghost share");
    assert_eq!(r.slots().await, 0, "a slot is still held");
}

/// Webcam stop (or its expiry): an old seat's stop that was checked before
/// the take-over does not end the new seat's webcam.
#[sqlx::test]
async fn webcam_stop_of_a_replaced_seat_keeps_the_new_webcam(pool: PgPool) {
    let mut r = room(pool).await;
    let mut old = joined(&r.server, &r.token, r.voice).await;
    next_ws_event(&mut r.viewer, "voice_user_joined", WAIT).await;
    send(&mut old, webcam_start(r.voice)).await;
    next_ws_event(&mut old, "webcam_started", WAIT).await;
    next_ws_event(&mut r.viewer, "webcam_started", WAIT).await;

    let pause = pause_voice_step(r.user, "end_webcam");
    send(
        &mut old,
        serde_json::json!({ "type": "voice_webcam_stop", "channel_id": r.voice }),
    )
    .await;
    Room::reached(&pause).await;
    let mut new = r.take_over().await;
    send(&mut new, webcam_start(r.voice)).await;
    next_ws_event(&mut new, "webcam_started", WAIT).await;
    next_ws_event(&mut r.viewer, "webcam_started", WAIT).await;
    pause.release.notify_one();

    r.assert_new_webcam_kept().await;
}

/// Share stop: an old seat's stop of a stream id that the new seat now uses
/// ends nothing of the new seat's and frees no slot of it.
#[sqlx::test]
async fn share_stop_of_a_replaced_seat_keeps_the_new_share(pool: PgPool) {
    let mut r = room(pool).await;
    let mut old = joined(&r.server, &r.token, r.voice).await;
    next_ws_event(&mut r.viewer, "voice_user_joined", WAIT).await;
    let stream = Uuid::new_v4();
    send(&mut old, share_start(r.voice, stream)).await;
    next_ws_event(&mut old, "screen_share_started", WAIT).await;
    next_ws_event(&mut r.viewer, "screen_share_started", WAIT).await;

    let pause = pause_voice_step(r.user, "end_share");
    send(
        &mut old,
        serde_json::json!({
            "type": "voice_screen_share_stop",
            "channel_id": r.voice,
            "stream_id": stream,
        }),
    )
    .await;
    Room::reached(&pause).await;
    let mut new = r.take_over().await;
    send(&mut new, share_start(r.voice, stream)).await;
    next_ws_event(&mut new, "screen_share_started", WAIT).await;
    next_ws_event(&mut r.viewer, "screen_share_started", WAIT).await;
    pause.release.notify_one();

    expect_no_ws_event(&mut r.viewer, "screen_share_stopped", QUIET).await;
    let room = r.app.state.sfu.get_room(r.voice).await.expect("room");
    assert_eq!(
        room.get_screen_shares().await.len(),
        1,
        "the new share is gone"
    );
    assert_eq!(r.slots().await, 1, "the new share's slot was freed");
    r.release_slots().await;
}

/// Expiry: the old seat's 10 s expiry, past its checks when the take-over
/// happens, does not end the new seat's webcam.
#[sqlx::test]
async fn expiry_of_a_replaced_seat_keeps_the_new_webcam(pool: PgPool) {
    let mut r = room(pool).await;
    let mut old = joined(&r.server, &r.token, r.voice).await;
    next_ws_event(&mut r.viewer, "voice_user_joined", WAIT).await;
    let pause = pause_voice_step(r.user, "end_webcam");
    send(&mut old, webcam_start(r.voice)).await;
    next_ws_event(&mut old, "webcam_started", WAIT).await;
    next_ws_event(&mut r.viewer, "webcam_started", WAIT).await;

    Room::reached(&pause).await; // the expiry, about 10 s later
    let mut new = r.take_over().await;
    send(&mut new, webcam_start(r.voice)).await;
    next_ws_event(&mut new, "webcam_started", WAIT).await;
    next_ws_event(&mut r.viewer, "webcam_started", WAIT).await;
    pause.release.notify_one();

    r.assert_new_webcam_kept().await;
}

/// Leave, then a fresh join (the old seat is already gone, nothing is
/// replaced): the old seat's webcam stop, sent after the new seat started
/// its webcam, would hide the new webcam on the clients, so it is not sent.
#[sqlx::test]
async fn leave_stop_after_a_fresh_join_keeps_the_new_webcam(pool: PgPool) {
    let mut r = room(pool).await;
    let mut old = joined(&r.server, &r.token, r.voice).await;
    next_ws_event(&mut r.viewer, "voice_user_joined", WAIT).await;
    send(&mut old, webcam_start(r.voice)).await;
    next_ws_event(&mut old, "webcam_started", WAIT).await;
    next_ws_event(&mut r.viewer, "webcam_started", WAIT).await;

    let pause = pause_voice_step(r.user, "leave");
    send(
        &mut old,
        serde_json::json!({ "type": "voice_leave", "channel_id": r.voice }),
    )
    .await;
    Room::reached(&pause).await;
    let mut new = r.take_over().await;
    send(&mut new, webcam_start(r.voice)).await;
    next_ws_event(&mut new, "webcam_started", WAIT).await;
    next_ws_event(&mut r.viewer, "webcam_started", WAIT).await;
    pause.release.notify_one();

    r.assert_new_webcam_kept().await;
}
