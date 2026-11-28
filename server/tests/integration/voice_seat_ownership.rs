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

    /// The room's share counter key (none without a room).
    async fn counter_key(&self) -> Option<String> {
        let room = self.app.state.sfu.get_room(self.voice).await?;
        Some(format!("screenshare:limit:{}", room.share_counter))
    }

    async fn slots(&self) -> i64 {
        let Some(key) = self.counter_key().await else {
            return 0;
        };
        let n: Option<i64> = self.app.state.redis.get(&key).await.expect("get counter");
        n.unwrap_or(0)
    }

    async fn release_slots(&self) {
        if let Some(key) = self.counter_key().await {
            let _: i64 = self.app.state.redis.del(&key).await.expect("del counter");
        }
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

/// Join: a seat removed between the joiner's room state and the join
/// announcement (a revocation or the reaper) is not announced: the others
/// are not told about a participant who is not there.
#[sqlx::test]
async fn a_seat_removed_before_its_announcement_is_not_announced(pool: PgPool) {
    let mut r = room(pool).await;
    let pause = pause_voice_step(r.user, "join_announce");
    let mut ws = ws_connect(&r.server, &r.token).await;
    next_ws_event(&mut ws, "ready", WAIT).await;
    send(
        &mut ws,
        serde_json::json!({ "type": "voice_join", "channel_id": r.voice }),
    )
    .await;
    Room::reached(&pause).await;

    let room = r.app.state.sfu.get_room(r.voice).await.expect("room");
    let session = room.get_peer(r.user).await.expect("seat").session_id;
    vc_server::voice::leave::leave_voice_internal(
        &r.app.state.sfu,
        &r.app.pool,
        r.user,
        r.voice,
        r.app.state.screen_share_limiter.as_ref(),
        Some(session),
    )
    .await
    .expect("seat removed");
    pause.release.notify_one();

    expect_no_ws_event(&mut r.viewer, "voice_user_joined", QUIET).await;
}

/// Expiry: the ten-second timer of a start whose track never came, past its
/// checks when the same seat stops and starts the webcam again, ends only
/// the start it was armed for: the new webcam stays.
#[sqlx::test]
async fn expiry_ends_only_the_start_it_was_armed_for(pool: PgPool) {
    let mut r = room(pool).await;
    let mut seat = joined(&r.server, &r.token, r.voice).await;
    next_ws_event(&mut r.viewer, "voice_user_joined", WAIT).await;
    let pause = pause_voice_step(r.user, "end_webcam");
    send(&mut seat, webcam_start(r.voice)).await;
    next_ws_event(&mut seat, "webcam_started", WAIT).await;
    next_ws_event(&mut r.viewer, "webcam_started", WAIT).await;

    Room::reached(&pause).await; // the expiry, about 10 s later
    send(
        &mut seat,
        serde_json::json!({ "type": "voice_webcam_stop", "channel_id": r.voice }),
    )
    .await;
    next_ws_event(&mut r.viewer, "webcam_stopped", WAIT).await;
    send(&mut seat, webcam_start(r.voice)).await;
    next_ws_event(&mut seat, "webcam_started", WAIT).await;
    next_ws_event(&mut r.viewer, "webcam_started", WAIT).await;
    pause.release.notify_one();

    r.assert_new_webcam_kept().await;
}

/// Removing a room waits for no store: with a store that never answers,
/// the last participant's leave removes the room without holding the room
/// list for the store; looking up rooms stays quick throughout, and a join
/// in another channel is not held up.
#[sqlx::test]
async fn removing_a_room_waits_for_no_store(pool: PgPool) {
    use fred::interfaces::ClientLike;
    const QUICK: Duration = Duration::from_millis(500);
    let r = room(pool).await;
    // A store that accepts connections and never answers.
    let silent = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let port = silent.local_addr().expect("addr").port();
    let held = tokio::spawn(async move {
        let mut open = Vec::new();
        while let Ok((socket, _)) = silent.accept().await {
            open.push(socket);
        }
    });
    let config = fred::types::config::Config::from_url(&format!("redis://127.0.0.1:{port}"))
        .expect("config");
    let store = fred::clients::Client::new(config, None, None, None);
    let _connecting = store.connect();
    let limiter = vc_server::voice::ScreenShareLimiter::new(store);

    let sfu = r.app.state.sfu.clone();
    let room = sfu.get_room(r.voice).await.expect("room");
    let viewer = room
        .peers
        .read()
        .await
        .values()
        .next()
        .cloned()
        .expect("seat");
    drop(room);
    // The leave stops right before it removes the room.
    let pause = pause_voice_step(viewer.user_id, "room_cleanup");
    let leaving = {
        let (sfu, pool, voice) = (sfu.clone(), r.app.pool.clone(), r.voice);
        tokio::spawn(async move {
            vc_server::voice::leave::leave_voice_internal(
                &sfu,
                &pool,
                viewer.user_id,
                voice,
                Some(&limiter),
                Some(viewer.session_id),
            )
            .await
        })
    };
    Room::reached(&pause).await;
    pause.release.notify_one();

    // Until the room is gone, every look-up of the room list is quick.
    let deadline = tokio::time::Instant::now() + WAIT;
    loop {
        assert!(
            tokio::time::Instant::now() < deadline,
            "the room was not removed"
        );
        let listed = tokio::time::timeout(QUICK, sfu.get_room(r.voice))
            .await
            .expect("the room list was held while removing the room");
        if listed.is_none() {
            break;
        }
        tokio::task::yield_now().await;
    }
    tokio::time::timeout(QUICK, sfu.get_or_create_room(Uuid::new_v4()))
        .await
        .expect("a join elsewhere waited for the store");
    leaving.await.expect("join").expect("leave");
    held.abort();
}

/// A room created for a channel after its old room was removed counts its
/// shares from zero, whatever the old room's count was (a release that
/// failed) and whatever the old room's late calls do.
#[sqlx::test]
async fn a_new_room_counts_its_shares_from_zero(pool: PgPool) {
    let r = room(pool).await;
    let limiter = r.app.state.screen_share_limiter.clone().expect("limiter");
    let sfu = r.app.state.sfu.clone();
    let old = sfu.get_room(r.voice).await.expect("room");
    // The old room's count is left too high (a release that failed).
    limiter.start(&old.share_counter, 6).await.expect("slot");
    limiter.start(&old.share_counter, 6).await.expect("slot");

    // The room goes (its last participant left) and a new one comes.
    old.peers.write().await.clear();
    sfu.cleanup_room_if_empty(r.voice).await;
    let new = sfu.get_or_create_room(r.voice).await;
    assert!(!std::sync::Arc::ptr_eq(&old, &new), "a new room");

    limiter
        .start(&new.share_counter, 1)
        .await
        .expect("the new room starts from zero");
    // A late release of the old room does not touch the new room's count.
    limiter.stop_n(&old.share_counter, 2).await;
    limiter.stop(&old.share_counter).await;
    assert!(
        limiter.start(&new.share_counter, 1).await.is_err(),
        "the new room's share was released by the old room's late calls"
    );
}

/// Takeover with a share: the others get the share's stop once, and the
/// replaced seat's slot is freed exactly once (beside the new join). The
/// viewer's own share stays live as a sentinel: a second release would
/// take its slot too.
#[sqlx::test]
async fn a_takeover_frees_the_replaced_share_once(pool: PgPool) {
    let mut r = room(pool).await;
    let mut old = joined(&r.server, &r.token, r.voice).await;
    next_ws_event(&mut r.viewer, "voice_user_joined", WAIT).await;
    send(&mut r.viewer, share_start(r.voice, Uuid::new_v4())).await;
    next_ws_event(&mut r.viewer, "screen_share_started", WAIT).await;
    next_ws_event(&mut old, "screen_share_started", WAIT).await;
    send(&mut old, share_start(r.voice, Uuid::new_v4())).await;
    next_ws_event(&mut old, "screen_share_started", WAIT).await;
    next_ws_event(&mut r.viewer, "screen_share_started", WAIT).await;
    assert_eq!(r.slots().await, 2);

    let _new = joined(&r.server, &r.token, r.voice).await;
    let stopped = next_ws_event(&mut r.viewer, "screen_share_stopped", WAIT).await;
    assert_eq!(stopped["reason"], "replaced", "{stopped}");
    expect_no_ws_event(&mut r.viewer, "screen_share_stopped", QUIET).await;
    let deadline = tokio::time::Instant::now() + WAIT;
    while r.slots().await != 1 {
        assert!(
            tokio::time::Instant::now() < deadline,
            "the slots are {} instead of the sentinel's one",
            r.slots().await
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    tokio::time::sleep(QUIET).await;
    assert_eq!(r.slots().await, 1, "a slot was released twice");
}

/// A socket that holds seats in two rooms leaves both when it closes: no
/// seat stays behind that the others see and that takes a place.
#[sqlx::test]
async fn a_closing_socket_leaves_every_seat_it_holds(pool: PgPool) {
    let mut r = room(pool).await;
    let guild: Uuid = sqlx::query_scalar("SELECT guild_id FROM channels WHERE id = $1")
        .bind(r.voice)
        .fetch_one(&r.app.pool)
        .await
        .expect("guild of the channel");
    let second = create_voice_channel(&r.app.pool, guild, "voice 2").await;
    let mut ws = joined(&r.server, &r.token, r.voice).await;
    next_ws_event(&mut r.viewer, "voice_user_joined", WAIT).await;
    send(
        &mut ws,
        serde_json::json!({ "type": "voice_join", "channel_id": second }),
    )
    .await;
    next_ws_event(&mut ws, "voice_room_state", WAIT).await;
    let sfu = r.app.state.sfu.clone();
    for channel in [r.voice, second] {
        let room = sfu.get_room(channel).await.expect("room");
        assert!(room.get_peer(r.user).await.is_some(), "seat in {channel}");
    }

    ws.close(None).await.expect("close the socket");
    drop(ws);
    let deadline = tokio::time::Instant::now() + WAIT;
    for channel in [r.voice, second] {
        loop {
            let seated = match sfu.get_room(channel).await {
                Some(room) => room.get_peer(r.user).await.is_some(),
                None => false,
            };
            if !seated {
                break;
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "the closed socket's seat in {channel} stayed"
            );
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }
}

/// The two connections of a replaced seat are closed independently: a
/// publisher close that does not finish leaves the subscriber connection
/// to be closed all the same.
#[sqlx::test]
async fn a_replaced_seats_connections_close_independently(pool: PgPool) {
    use webrtc::peer_connection::peer_connection_state::RTCPeerConnectionState;
    let mut r = room(pool).await;
    let _old = joined(&r.server, &r.token, r.voice).await;
    next_ws_event(&mut r.viewer, "voice_user_joined", WAIT).await;
    let room = r.app.state.sfu.get_room(r.voice).await.expect("room");
    let old_seat = room.get_peer(r.user).await.expect("old seat");
    drop(room);

    let pause = pause_voice_step(r.user, "close_publisher");
    let _new = r.take_over().await;
    Room::reached(&pause).await;

    let deadline = tokio::time::Instant::now() + WAIT;
    while old_seat.subscriber_pc.connection_state() != RTCPeerConnectionState::Closed {
        assert!(
            tokio::time::Instant::now() < deadline,
            "the subscriber connection waited for the publisher close"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_ne!(
        old_seat.publisher_pc.connection_state(),
        RTCPeerConnectionState::Closed,
        "the publisher close was held"
    );
    pause.release.notify_one();
    while old_seat.publisher_pc.connection_state() != RTCPeerConnectionState::Closed {
        assert!(
            tokio::time::Instant::now() < deadline,
            "the publisher connection was not closed"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

/// A release that never reached the room's counter (as when the store
/// fails at a stop) does not keep the room from sharing: a start that would
/// be refused for the limit finds that the room holds fewer shares,
/// corrects the counter and is accepted. A room really at its limit is
/// still refused.
#[sqlx::test]
async fn a_lost_release_does_not_block_sharing(pool: PgPool) {
    let mut r = room(pool).await;
    sqlx::query("UPDATE channels SET max_screen_shares = 1 WHERE id = $1")
        .bind(r.voice)
        .execute(&r.app.pool)
        .await
        .expect("limit");
    let mut old = joined(&r.server, &r.token, r.voice).await;
    next_ws_event(&mut r.viewer, "voice_user_joined", WAIT).await;
    send(&mut old, share_start(r.voice, Uuid::new_v4())).await;
    next_ws_event(&mut old, "screen_share_started", WAIT).await;
    next_ws_event(&mut r.viewer, "screen_share_started", WAIT).await;
    assert_eq!(r.slots().await, 1);

    // The user leaves; the release goes to a counter that is not the
    // room's, so the room's counter keeps the slot.
    let mut lost = vc_server::voice::ScreenShareLimiter::with_key_prefix(
        r.app.state.redis.clone(),
        format!("lost-release:{}:", Uuid::new_v4()),
    );
    lost.init().await.expect("script");
    let sfu = r.app.state.sfu.clone();
    let seat = sfu
        .get_room(r.voice)
        .await
        .expect("room")
        .get_peer(r.user)
        .await
        .expect("seat");
    vc_server::voice::leave::leave_voice_internal(
        &sfu,
        &r.app.pool,
        r.user,
        r.voice,
        Some(&lost),
        Some(seat.session_id),
    )
    .await
    .expect("leave");
    next_ws_event(&mut r.viewer, "screen_share_stopped", WAIT).await;
    assert_eq!(r.slots().await, 1, "the lost release changed the counter");

    send(&mut r.viewer, share_start(r.voice, Uuid::new_v4())).await;
    next_ws_event(&mut r.viewer, "screen_share_started", WAIT).await;
    assert_eq!(r.slots().await, 1);

    // Now the room really is at its limit.
    send(&mut r.viewer, share_start(r.voice, Uuid::new_v4())).await;
    let refused = next_ws_event(&mut r.viewer, "voice_error", WAIT).await;
    assert_eq!(refused["code"], "SCREEN_SHARE_LIMIT", "{refused}");
    assert_eq!(r.slots().await, 1);
}
