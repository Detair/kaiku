//! Voice with real media: simulated participants negotiate with the SFU and
//! send RTP (see `helpers::voice_media`).

use std::time::Duration;

use sqlx::PgPool;
use uuid::Uuid;
use vc_server::permissions::GuildPermissions;

use super::helpers::voice_media::{media_app, mic_track, SimOptions, SimParticipant};
use super::helpers::{
    add_guild_member, create_guild_with_default_role, create_test_user, create_voice_channel,
    generate_access_token, spawn_test_server, TestApp, TestServer,
};

const WAIT: Duration = Duration::from_secs(15);

struct Fixture {
    app: TestApp,
    server: TestServer,
    guild: Uuid,
    voice: Uuid,
}

async fn fixture(pool: PgPool) -> Fixture {
    let app = media_app(pool.clone()).await;
    let server = spawn_test_server(app.router.clone()).await;
    let (owner, _) = create_test_user(&pool).await;
    let guild =
        create_guild_with_default_role(&pool, owner, GuildPermissions::EVERYONE_DEFAULT).await;
    let voice = create_voice_channel(&pool, guild, "voice").await;
    Fixture {
        app,
        server,
        guild,
        voice,
    }
}

impl Fixture {
    /// A new member of the server (no role).
    async fn member(&self) -> (Uuid, String) {
        let (user, _) = create_test_user(&self.app.pool).await;
        add_guild_member(&self.app.pool, self.guild, user).await;
        (user, generate_access_token(&self.app.config, user))
    }

    async fn participant(&self, options: SimOptions) -> SimParticipant {
        let (user, token) = self.member().await;
        SimParticipant::connect(&self.server, &token, user, self.voice, options).await
    }

    async fn participant_for(&self, user: Uuid, options: SimOptions) -> SimParticipant {
        let token = generate_access_token(&self.app.config, user);
        SimParticipant::connect(&self.server, &token, user, self.voice, options).await
    }
}

fn share_start(channel: Uuid, stream_id: Uuid) -> serde_json::Value {
    serde_json::json!({
        "type": "voice_screen_share_start",
        "channel_id": channel,
        "stream_id": stream_id,
        "quality": "medium",
        "has_audio": false,
        "source_label": "Display 1",
    })
}

/// Two participants who hear each other.
async fn two_in_call(f: &Fixture) -> (SimParticipant, SimParticipant) {
    let mut a = f.participant(SimOptions::default()).await;
    let mut b = f.participant(SimOptions::default()).await;
    a.join_and_publish_mic().await;
    b.join_and_publish_mic().await;
    assert!(
        b.wait_for_packets(&mic_track(a.user_id), 5, WAIT).await,
        "B does not hear A: {:?}",
        b.seen_tracks()
    );
    assert!(
        a.wait_for_packets(&mic_track(b.user_id), 5, WAIT).await,
        "A does not hear B: {:?}",
        a.seen_tracks()
    );
    (a, b)
}

fn video_tracks_of(p: &SimParticipant, source: Uuid) -> Vec<String> {
    p.seen_tracks()
        .into_iter()
        .filter(|t| t.starts_with(&source.to_string()) && !t.ends_with(":microphone"))
        .collect()
}

/// C30: a video track that no start announced is dropped; no viewer gets it
/// and the sender gets `VIDEO_SOURCE_UNANNOUNCED`.
#[sqlx::test]
async fn video_without_start_reaches_no_viewer(pool: PgPool) {
    let f = fixture(pool).await;
    let (mut a, b) = two_in_call(&f).await;

    a.add_track(true, "screen").await;
    a.publish().await;

    let err = a
        .try_next_matching(Duration::from_secs(8), |ev| ev["type"] == "voice_error")
        .await
        .expect("sender was not told that its track was dropped");
    assert_eq!(err["code"], "VIDEO_SOURCE_UNANNOUNCED", "{err}");
    tokio::time::sleep(Duration::from_secs(1)).await;
    assert!(
        video_tracks_of(&b, a.user_id).is_empty(),
        "viewer got an unannounced track: {:?}",
        b.seen_tracks()
    );
}

/// A refused start (override denies `SCREEN_SHARE`) followed by the track,
/// as an rc.5 client does it: the track is dropped too.
#[sqlx::test]
async fn refused_start_then_track_reaches_no_viewer(pool: PgPool) {
    let f = fixture(pool.clone()).await;
    let everyone: Uuid =
        sqlx::query_scalar("SELECT id FROM guild_roles WHERE guild_id = $1 AND is_default")
            .bind(f.guild)
            .fetch_one(&pool)
            .await
            .unwrap();
    sqlx::query(
        "INSERT INTO channel_overrides (id, channel_id, role_id, allow_permissions, deny_permissions)
         VALUES ($1, $2, $3, 0, $4)",
    )
    .bind(Uuid::now_v7())
    .bind(f.voice)
    .bind(everyone)
    .bind(GuildPermissions::SCREEN_SHARE.to_db())
    .execute(&pool)
    .await
    .unwrap();
    let (mut a, mut b) = two_in_call(&f).await;

    a.send(share_start(f.voice, Uuid::new_v4()));
    let refused = a.next_event("voice_error", WAIT).await;
    assert_eq!(refused["code"], "SCREEN_SHARE_FORBIDDEN", "{refused}");
    a.add_track(true, "screen").await;
    a.publish().await;
    let dropped = a.next_event("voice_error", Duration::from_secs(8)).await;
    assert_eq!(dropped["code"], "VIDEO_SOURCE_UNANNOUNCED", "{dropped}");

    tokio::time::sleep(Duration::from_secs(1)).await;
    assert!(
        video_tracks_of(&b, a.user_id).is_empty(),
        "{:?}",
        b.seen_tracks()
    );
    assert!(
        b.try_next_event("screen_share_started", Duration::from_millis(200))
            .await
            .is_none(),
        "others were told about a refused share"
    );
}

/// C30 as clients from now on do it: start, wait for the acknowledgement,
/// then add the track. The viewer gets the track under the stream id.
#[sqlx::test]
async fn acknowledged_start_then_track_is_forwarded(pool: PgPool) {
    let f = fixture(pool).await;
    let (mut a, b) = two_in_call(&f).await;

    let stream = Uuid::new_v4();
    a.send(share_start(f.voice, stream));
    let ack = a.next_event("screen_share_started", WAIT).await;
    assert_eq!(ack["stream_id"], stream.to_string());
    a.add_track(true, "screen").await;
    a.publish().await;

    let id = format!("{}:screen_video:{stream}", a.user_id);
    assert!(
        b.wait_for_packets(&id, 5, WAIT).await,
        "viewer did not get the share: {:?}",
        b.seen_tracks()
    );
}

/// The rc.5 order: the track goes out first and the start follows. The track
/// waits for its announcement and is then forwarded under the stream id.
#[sqlx::test]
async fn track_before_its_start_is_held_until_announced(pool: PgPool) {
    let f = fixture(pool).await;
    let (mut a, b) = two_in_call(&f).await;

    a.add_track(true, "screen").await;
    a.publish().await;
    tokio::time::sleep(Duration::from_millis(500)).await;
    let stream = Uuid::new_v4();
    a.send(share_start(f.voice, stream));
    a.next_event("screen_share_started", WAIT).await;

    let id = format!("{}:screen_video:{stream}", a.user_id);
    assert!(
        b.wait_for_packets(&id, 5, WAIT).await,
        "viewer did not get the share: {:?}",
        b.seen_tracks()
    );
    assert!(
        a.try_next_event("voice_error", Duration::from_millis(500))
            .await
            .is_none(),
        "announced track was reported as dropped"
    );
}

/// Webcam follows the same rule: announced by `voice_webcam_start`.
#[sqlx::test]
async fn webcam_start_then_track_is_forwarded(pool: PgPool) {
    let f = fixture(pool).await;
    let (mut a, b) = two_in_call(&f).await;

    a.send(serde_json::json!({
        "type": "voice_webcam_start",
        "channel_id": f.voice,
        "quality": "medium",
    }));
    a.next_event("webcam_started", WAIT).await;
    a.add_track(true, "cam").await;
    a.publish().await;

    let id = format!("{}:webcam", a.user_id);
    assert!(
        b.wait_for_packets(&id, 5, WAIT).await,
        "viewer did not get the webcam: {:?}",
        b.seen_tracks()
    );
}

/// Two members join within 200 ms while a third is in the room; the
/// listeners answer offers slowly, so offers would overlap. Everyone hears
/// everyone.
#[sqlx::test]
async fn two_joins_within_200ms_hear_each_other(pool: PgPool) {
    let f = fixture(pool).await;
    let slow = SimOptions {
        answer_delay: Duration::from_millis(300),
    };
    let mut c = f.participant(slow).await;
    c.join_and_publish_mic().await;

    let mut a = f.participant(slow).await;
    let mut b = f.participant(slow).await;
    a.send_join();
    tokio::time::sleep(Duration::from_millis(100)).await;
    b.send_join();
    a.next_event("voice_room_state", WAIT).await;
    b.next_event("voice_room_state", WAIT).await;
    a.add_track(false, "mic").await;
    b.add_track(false, "mic").await;
    tokio::join!(a.publish(), b.publish());

    for (listener, speaker) in [(&a, &b), (&b, &a), (&c, &a), (&c, &b), (&a, &c), (&b, &c)] {
        assert!(
            listener
                .wait_for_packets(&mic_track(speaker.user_id), 5, Duration::from_secs(20))
                .await,
            "{} does not hear {}: {:?}",
            listener.user_id,
            speaker.user_id,
            listener.seen_tracks()
        );
    }
    // Each listener gets every other participant's microphone exactly once
    // (no duplicate sender from overlapping subscribe paths).
    for p in [&a, &b, &c] {
        let (active, _) = subscriber_counts(&f.app, f.voice, p.user_id).await;
        assert_eq!(active, 2, "{}: senders with a track", p.user_id);
    }
    // Every subscriber answer fit the offer it answered: no signaling error.
    for p in [&mut a, &mut b, &mut c] {
        let mut offers = 0;
        while let Ok(ev) = p.events.try_recv() {
            assert_ne!(ev["type"], "voice_error", "{ev}");
            assert_ne!(ev["type"], "sim_offer_rejected", "{ev}");
            if ev["type"] == "voice_subscriber_offer" {
                offers += 1;
            }
        }
        eprintln!("{}: {offers} subscriber offers", p.user_id);
    }
}

/// Senders at `user`'s subscriber connection that carry a track, and all
/// transceivers there.
async fn subscriber_counts(app: &TestApp, channel: Uuid, user: Uuid) -> (usize, usize) {
    let room = app.state.sfu.get_room(channel).await.expect("room");
    let peer = room.get_peer(user).await.expect("peer");
    let mut with_track = 0;
    for sender in peer.subscriber_pc.get_senders().await {
        if sender.track().await.is_some() {
            with_track += 1;
        }
    }
    (
        with_track,
        peer.subscriber_pc.get_transceivers().await.len(),
    )
}

/// Ten joins and leaves of a second member leave the permanent
/// member's subscriber connection as it was: after each leave no sender
/// carries a track of the departed member. A removed track's transceiver
/// stays (inactive) in the session: at most one more per rejoin.
#[sqlx::test]
async fn ten_joins_and_leaves_keep_senders_constant(pool: PgPool) {
    let f = fixture(pool).await;
    let mut stay = f.participant(SimOptions::default()).await;
    stay.join_and_publish_mic().await;
    let (visitor, _) = f.member().await;

    let mut transceivers_after_first = None;
    for round in 0..10 {
        let mut v = f.participant_for(visitor, SimOptions::default()).await;
        let before = stay.packets(&mic_track(visitor));
        v.join_and_publish_mic().await;
        assert!(
            stay.wait_for_packets(&mic_track(visitor), before + 5, WAIT)
                .await,
            "round {round}: visitor not heard"
        );
        let (active, _) = subscriber_counts(&f.app, f.voice, stay.user_id).await;
        assert_eq!(active, 1, "round {round}: visitor's track not added");

        v.send(serde_json::json!({ "type": "voice_leave", "channel_id": f.voice }));
        stay.next_event("voice_user_left", WAIT).await;
        // The removal is announced by a new subscriber offer.
        let deadline = tokio::time::Instant::now() + WAIT;
        let (active, transceivers) = loop {
            let counts = subscriber_counts(&f.app, f.voice, stay.user_id).await;
            if counts.0 == 0 || tokio::time::Instant::now() > deadline {
                break counts;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        };
        assert_eq!(active, 0, "round {round}: departed track still sent");
        // Each departure leaves exactly one inactive m-line, and the server
        // counts it.
        let room = f.app.state.sfu.get_room(f.voice).await.expect("room");
        let inactive = room
            .get_peer(stay.user_id)
            .await
            .expect("stay")
            .inactive_mlines();
        assert_eq!(inactive, round + 1, "round {round}: inactive m-lines");
        assert_eq!(transceivers, inactive, "round {round}: transceivers");
        let first = *transceivers_after_first.get_or_insert(transceivers);
        assert!(
            transceivers <= first + round,
            "round {round}: transceivers grew from {first} to {transceivers}"
        );
        v.close().await;
    }
}

/// C31 `session_replaced`: the same user joins from a second socket. Only the old socket
/// gets `voice_removed { session_replaced }`; the room holds one entry for
/// the user; the third participant keeps exactly one track of that user
/// and hears the new session.
#[sqlx::test]
async fn second_socket_join_replaces_the_session(pool: PgPool) {
    let f = fixture(pool).await;
    let mut stay = f.participant(SimOptions::default()).await;
    stay.join_and_publish_mic().await;
    let (user, _) = f.member().await;

    let mut first = f.participant_for(user, SimOptions::default()).await;
    first.join_and_publish_mic().await;
    assert!(stay.wait_for_packets(&mic_track(user), 5, WAIT).await);

    let mut second = f.participant_for(user, SimOptions::default()).await;
    second.join_and_publish_mic().await;

    let removed = first.next_event("voice_removed", WAIT).await;
    assert_eq!(removed["reason"], "session_replaced", "{removed}");
    assert_eq!(removed["channel_id"], f.voice.to_string());

    let room = f.app.state.sfu.get_room(f.voice).await.expect("room");
    assert_eq!(room.participant_count().await, 2);
    let info = room.get_participant_info().await;
    assert_eq!(info.iter().filter(|p| p.user_id == user).count(), 1);

    // The new session is heard, and only one track of the user is sent.
    let before = stay.packets(&mic_track(user));
    assert!(
        stay.wait_for_packets(&mic_track(user), before + 5, WAIT)
            .await,
        "new session not heard"
    );
    let deadline = tokio::time::Instant::now() + WAIT;
    let active = loop {
        let (active, _) = subscriber_counts(&f.app, f.voice, stay.user_id).await;
        if active == 1 || tokio::time::Instant::now() > deadline {
            break active;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    };
    assert_eq!(active, 1, "old session's track still sent to the others");
    assert!(
        second
            .try_next_event("voice_removed", Duration::from_millis(500))
            .await
            .is_none(),
        "the new session was told it was removed"
    );
}

/// Audio follows the same rule as video: a refused share's audio (rc.5
/// order: tracks first, start after) reaches nobody, there is never a
/// second microphone, and the sender gets `AUDIO_SOURCE_UNANNOUNCED`.
#[sqlx::test]
async fn refused_share_audio_reaches_no_viewer(pool: PgPool) {
    let f = fixture(pool.clone()).await;
    let everyone: Uuid =
        sqlx::query_scalar("SELECT id FROM guild_roles WHERE guild_id = $1 AND is_default")
            .bind(f.guild)
            .fetch_one(&pool)
            .await
            .unwrap();
    sqlx::query(
        "INSERT INTO channel_overrides (id, channel_id, role_id, allow_permissions, deny_permissions)
         VALUES ($1, $2, $3, 0, $4)",
    )
    .bind(Uuid::now_v7())
    .bind(f.voice)
    .bind(everyone)
    .bind(GuildPermissions::SCREEN_SHARE.to_db())
    .execute(&pool)
    .await
    .unwrap();
    let (mut a, b) = two_in_call(&f).await;

    a.add_track(true, "screen").await;
    a.add_track(false, "screen-audio").await;
    a.publish().await;
    let mut share = serde_json::json!(share_start(f.voice, Uuid::new_v4()));
    share["has_audio"] = serde_json::json!(true);
    a.send(share);
    let mut codes = Vec::new();
    while let Some(err) = a
        .try_next_matching(Duration::from_secs(8), |ev| ev["type"] == "voice_error")
        .await
    {
        codes.push(err["code"].as_str().unwrap_or_default().to_owned());
        if codes.len() == 3 {
            break;
        }
    }
    codes.sort();
    assert_eq!(
        codes,
        [
            "AUDIO_SOURCE_UNANNOUNCED",
            "SCREEN_SHARE_FORBIDDEN",
            "VIDEO_SOURCE_UNANNOUNCED"
        ],
        "answers to the refused share"
    );
    let (active, _) = subscriber_counts(&f.app, f.voice, b.user_id).await;
    assert_eq!(active, 1, "the viewer gets more than A's microphone");
}

/// An accepted share with audio: the viewer gets the screen audio under the
/// share's stream id, next to (not instead of) the microphone.
#[sqlx::test]
async fn accepted_share_audio_is_forwarded_as_screen_audio(pool: PgPool) {
    let f = fixture(pool).await;
    let (mut a, b) = two_in_call(&f).await;

    let stream = Uuid::new_v4();
    let mut share = share_start(f.voice, stream);
    share["has_audio"] = serde_json::json!(true);
    a.send(share);
    a.next_event("screen_share_started", WAIT).await;
    a.add_track(true, "screen").await;
    a.add_track(false, "screen-audio").await;
    a.publish().await;

    let audio = format!("{}:screen_audio:{stream}", a.user_id);
    assert!(
        b.wait_for_packets(&audio, 5, WAIT).await,
        "viewer did not get the screen audio: {:?}",
        b.seen_tracks()
    );
    let mic = mic_track(a.user_id);
    let before = b.packets(&mic);
    assert!(
        b.wait_for_packets(&mic, before + 5, WAIT).await,
        "mic stopped"
    );
}

/// A share that was stopped before its track came leaves no announcement:
/// the next video (a webcam) is forwarded as the webcam.
#[sqlx::test]
async fn stopped_share_does_not_bind_a_later_webcam(pool: PgPool) {
    let f = fixture(pool).await;
    let (mut a, b) = two_in_call(&f).await;

    let stream = Uuid::new_v4();
    a.send(share_start(f.voice, stream));
    a.next_event("screen_share_started", WAIT).await;
    a.send(serde_json::json!({
        "type": "voice_screen_share_stop",
        "channel_id": f.voice,
        "stream_id": stream,
    }));
    a.next_event("screen_share_stopped", WAIT).await;
    a.send(serde_json::json!({
        "type": "voice_webcam_start",
        "channel_id": f.voice,
        "quality": "medium",
    }));
    a.next_event("webcam_started", WAIT).await;
    a.add_track(true, "cam").await;
    a.publish().await;

    let cam = format!("{}:webcam", a.user_id);
    assert!(
        b.wait_for_packets(&cam, 5, WAIT).await,
        "webcam not forwarded as webcam: {:?}",
        b.seen_tracks()
    );
    assert!(
        !b.seen_tracks().iter().any(|t| t.contains(":screen_video:")),
        "{:?}",
        b.seen_tracks()
    );
}

/// Viewers' key-frame requests reach the publisher, coalesced to at most
/// one per second per track however many viewers ask.
#[sqlx::test]
async fn viewer_keyframe_requests_reach_the_publisher_rate_limited(pool: PgPool) {
    let f = fixture(pool).await;
    let (mut a, b) = two_in_call(&f).await;
    let mut c = f.participant(SimOptions::default()).await;
    c.join_and_publish_mic().await;

    let stream = Uuid::new_v4();
    a.send(share_start(f.voice, stream));
    a.next_event("screen_share_started", WAIT).await;
    a.add_track(true, "screen").await;
    a.publish().await;
    let id = format!("{}:screen_video:{stream}", a.user_id);
    assert!(
        b.wait_for_packets(&id, 5, WAIT).await,
        "{:?}",
        b.seen_tracks()
    );
    assert!(
        c.wait_for_packets(&id, 5, WAIT).await,
        "{:?}",
        c.seen_tracks()
    );

    // Both viewers ask ten times a second for 2.5 s.
    let before = a.keyframe_requests("screen");
    let started = tokio::time::Instant::now();
    while started.elapsed() < Duration::from_millis(2500) {
        b.request_keyframe(&id).await;
        c.request_keyframe(&id).await;
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    tokio::time::sleep(Duration::from_millis(300)).await;
    let got = a.keyframe_requests("screen") - before;
    // Forwarded at most once per second (3 in 2.5 s), plus at most one of
    // the server's own periodic requests.
    assert!(
        got >= 2,
        "viewer requests did not reach the publisher: {got}"
    );
    assert!(got <= 4, "requests were not coalesced: {got}");
}

/// A seat replaced while its first track is being forwarded leaves nothing
/// behind at the others: no ghost forwarded track or subscription that would
/// later block the user's new seat.
#[sqlx::test]
async fn takeover_during_track_forwarding_leaves_no_ghost(pool: PgPool) {
    let f = fixture(pool).await;
    let mut viewer = f.participant(SimOptions::default()).await;
    viewer.join_and_publish_mic().await;
    let (user, _) = f.member().await;

    let mut old = f.participant_for(user, SimOptions::default()).await;
    old.join().await;
    let pause = vc_server::ws::test_hooks::pause_track_before_forwarding(user);
    old.add_track(false, "mic").await;
    old.publish().await;
    tokio::time::timeout(WAIT, pause.reached.notified())
        .await
        .expect("the track reached the forwarding step");

    // The same user takes the seat over from another socket meanwhile.
    let new = f.participant_for(user, SimOptions::default()).await;
    new.send_join();
    tokio::time::sleep(Duration::from_millis(500)).await;
    pause.release.notify_one();
    let mut new = new;
    new.next_event("voice_room_state", WAIT).await;
    tokio::time::sleep(Duration::from_millis(500)).await;

    let room = f.app.state.sfu.get_room(f.voice).await.expect("room");
    let viewer_peer = room.get_peer(viewer.user_id).await.expect("viewer");
    assert_eq!(
        viewer_peer.outgoing_tracks_count().await,
        0,
        "the replaced seat's track is still forwarded to the viewer"
    );
    assert_eq!(
        room.track_router
            .subscription_count(user, vc_server::voice::TrackSource::Microphone),
        0,
        "a subscription of the replaced seat is left"
    );

    // The new seat's microphone reaches the viewer.
    new.add_track(false, "mic").await;
    new.publish().await;
    let before = viewer.packets(&mic_track(user));
    assert!(
        viewer
            .wait_for_packets(&mic_track(user), before + 5, WAIT)
            .await,
        "the viewer does not hear the new seat"
    );
    old.close().await;
}

/// A stop that races the share's first track wins: the track is not
/// forwarded for a share that has ended.
#[sqlx::test]
async fn stop_racing_the_first_track_forwards_nothing(pool: PgPool) {
    let f = fixture(pool).await;
    let (mut a, mut b) = two_in_call(&f).await;

    let stream = Uuid::new_v4();
    a.send(share_start(f.voice, stream));
    a.next_event("screen_share_started", WAIT).await;
    let pause = vc_server::ws::test_hooks::pause_track_before_forwarding(a.user_id);
    a.add_track(true, "screen").await;
    a.publish().await;
    tokio::time::timeout(WAIT, pause.reached.notified())
        .await
        .expect("the track reached the forwarding step");
    a.send(serde_json::json!({
        "type": "voice_screen_share_stop",
        "channel_id": f.voice,
        "stream_id": stream,
    }));
    tokio::time::sleep(Duration::from_millis(500)).await;
    pause.release.notify_one();
    b.next_event("screen_share_stopped", WAIT).await;
    tokio::time::sleep(Duration::from_millis(500)).await;

    let (active, _) = subscriber_counts(&f.app, f.voice, b.user_id).await;
    assert_eq!(
        active, 1,
        "the ended share is still forwarded to the viewer"
    );
    let room = f.app.state.sfu.get_room(f.voice).await.expect("room");
    let a_peer = room.get_peer(a.user_id).await.expect("a");
    assert!(
        a_peer.incoming_tracks.read().await.len() == 1,
        "the ended share's track is still bound"
    );
}

/// The expiry belongs to its own start: a webcam switched off and on again
/// is not ended by the first start's timer.
#[sqlx::test]
async fn expiry_acts_only_on_its_own_start(pool: PgPool) {
    let f = fixture(pool).await;
    let (mut a, mut b) = two_in_call(&f).await;
    let start = serde_json::json!({
        "type": "voice_webcam_start",
        "channel_id": f.voice,
        "quality": "medium",
    });
    a.send(start.clone());
    a.next_event("webcam_started", WAIT).await;
    a.send(serde_json::json!({ "type": "voice_webcam_stop", "channel_id": f.voice }));
    b.next_event("webcam_stopped", WAIT).await;
    tokio::time::sleep(Duration::from_secs(5)).await;
    a.send(start);
    a.next_event("webcam_started", WAIT).await;

    // The first start's timer fires about 5 s from now; the second's at 10 s.
    let early = b
        .try_next_event("webcam_stopped", Duration::from_secs(8))
        .await;
    assert!(
        early.is_none(),
        "the first start's timer ended the second webcam: {early:?}"
    );
    let late = b.next_event("webcam_stopped", Duration::from_secs(6)).await;
    assert_eq!(late["reason"], "no_media", "{late}");
}

/// A simulcast webcam (one track, three layers by rid) reaches the viewers:
/// the server's answer carries the header extensions a simulcast sender
/// needs to mark its layers.
#[sqlx::test]
#[ignore = "needs simulcast, which the server does not negotiate (no layer-id header extension)"]
async fn simulcast_webcam_reaches_viewers(pool: PgPool) {
    let f = fixture(pool).await;
    let (mut a, b) = two_in_call(&f).await;
    a.send(serde_json::json!({ "type": "voice_webcam_start", "channel_id": f.voice, "quality": "medium" }));
    a.next_event("webcam_started", WAIT).await;
    a.add_simulcast_video("cam", &["l", "m", "h"], &["l", "m", "h"])
        .await;
    a.publish().await;
    let id = format!("{}:webcam", a.user_id);
    assert!(
        b.wait_for_packets(&id, 5, WAIT).await,
        "viewer did not get the simulcast webcam: {:?}",
        b.seen_tracks()
    );
}

/// With simulcast, a viewer's key-frame request reaches the layer it
/// watches, also when another layer arrived first.
#[sqlx::test]
#[ignore = "needs simulcast, which the server does not negotiate (no layer-id header extension)"]
async fn keyframe_request_reaches_the_watched_layer(pool: PgPool) {
    let f = fixture(pool).await;
    let (mut a, mut b) = two_in_call(&f).await;
    a.send(serde_json::json!({ "type": "voice_webcam_start", "channel_id": f.voice, "quality": "medium" }));
    a.next_event("webcam_started", WAIT).await;
    // Only the low layer flows at first, so it arrives first; the others
    // follow.
    let (layers, _sender) = a.add_simulcast_video("cam", &["l", "m", "h"], &["l"]).await;
    a.publish().await;
    tokio::time::sleep(Duration::from_millis(1500)).await;
    a.start_writing(layers["m"].clone(), true);
    a.start_writing(layers["h"].clone(), true);
    let id = format!("{}:webcam", a.user_id);
    assert!(
        b.wait_for_packets(&id, 5, WAIT).await,
        "{:?}",
        b.seen_tracks()
    );
    // The viewer asks to watch the high layer.
    b.send(serde_json::json!({
        "type": "voice_set_layer_preference",
        "channel_id": f.voice,
        "target_user_id": a.user_id,
        "track_source": "webcam",
        "preferred_layer": "high",
    }));
    let _ = b
        .try_next_event("voice_layer_changed", Duration::from_millis(500))
        .await;
    let room = f.app.state.sfu.get_room(f.voice).await.expect("room");
    assert_eq!(
        room.track_router
            .active_layer(b.user_id, a.user_id, vc_server::voice::TrackSource::Webcam),
        Some(vc_server::voice::Layer::High),
        "precondition: the viewer watches the high layer"
    );

    let key = "cam@h";
    let before = a.keyframe_requests(key);
    let started = tokio::time::Instant::now();
    while started.elapsed() < Duration::from_millis(2500) {
        b.request_keyframe(&id).await;
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    tokio::time::sleep(Duration::from_millis(300)).await;
    let got = a.keyframe_requests(key) - before;
    let all = a.keyframe_requests.lock().unwrap().clone();
    assert!(
        got >= 2,
        "the high layer got {got} key-frame requests; all: {all:?}"
    );
}

/// A participant whose microphone sends nothing yet (muted, push-to-talk)
/// shares the screen with sound: the sound is the share's audio (not the
/// microphone) and ends with the share.
#[sqlx::test]
async fn share_audio_before_any_microphone_audio_is_screen_audio(pool: PgPool) {
    let f = fixture(pool).await;
    let mut b = f.participant(SimOptions::default()).await;
    b.join_and_publish_mic().await;
    let mut a = f.participant(SimOptions::default()).await;
    a.join().await;
    a.add_silent_audio("mic").await;
    a.publish().await;
    tokio::time::sleep(Duration::from_millis(500)).await;

    let stream = Uuid::new_v4();
    let mut start = share_start(f.voice, stream);
    start["has_audio"] = serde_json::json!(true);
    a.send(start);
    a.next_event("screen_share_started", WAIT).await;
    a.add_track(true, "screen").await;
    a.add_track(false, "screen-audio").await;
    a.publish().await;

    let audio = format!("{}:screen_audio:{stream}", a.user_id);
    assert!(
        b.wait_for_packets(&audio, 5, WAIT).await,
        "the share's sound did not arrive as screen audio: {:?}",
        b.seen_tracks()
    );
    assert!(
        !b.seen_tracks().iter().any(|t| t == &mic_track(a.user_id)),
        "the share's sound was taken as the microphone: {:?}",
        b.seen_tracks()
    );

    a.send(serde_json::json!({
        "type": "voice_screen_share_stop",
        "channel_id": f.voice,
        "stream_id": stream,
    }));
    b.next_event("screen_share_stopped", WAIT).await;
    let deadline = tokio::time::Instant::now() + WAIT;
    loop {
        let (active, _) = subscriber_counts(&f.app, f.voice, b.user_id).await;
        if active == 0 {
            break;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "the share's sound outlived the share ({active} senders)"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// A sender that offers a webcam with three encodings (layer ids) gets an
/// answer whose video section offers no layer-id, repaired-stream-id or
/// media-id header extension: the server cannot tell layers apart, so no
/// simulcast source is set up. (The answer still lists the offered layers
/// and `a=simulcast`; webrtc-rs answers them from the offer.)
#[sqlx::test]
async fn three_encoding_offer_is_answered_without_layer_ids(pool: PgPool) {
    let f = fixture(pool).await;
    let (mut a, _b) = two_in_call(&f).await;
    a.send(serde_json::json!({ "type": "voice_webcam_start", "channel_id": f.voice, "quality": "medium" }));
    a.next_event("webcam_started", WAIT).await;
    a.add_simulcast_video("cam", &["l", "m", "h"], &[]).await;
    a.publish().await;

    let deadline = tokio::time::Instant::now() + WAIT;
    let answer = loop {
        let sdp = a
            .publisher
            .remote_description()
            .await
            .map(|d| d.sdp)
            .unwrap_or_default();
        if sdp.contains("m=video") {
            break sdp;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "no answer with a video section"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    };
    let video = &answer[answer.find("m=video").expect("video section")..];
    let video = video[1..].find("\nm=").map_or(video, |end| &video[..=end]);
    for refused in [
        "urn:ietf:params:rtp-hdrext:sdes:rtp-stream-id",
        "urn:ietf:params:rtp-hdrext:sdes:repaired-rtp-stream-id",
        "urn:ietf:params:rtp-hdrext:sdes:mid",
    ] {
        assert!(
            !video.contains(refused),
            "the answer's video section has {refused}:\n{video}"
        );
    }
}
