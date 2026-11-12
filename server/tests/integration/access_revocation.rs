//! Losing access takes effect on open connections: kick, leave, role and
//! override changes and channel deletion drop the affected WebSocket channel
//! subscriptions and remove the member from voice (`voice_removed`, C31).

use std::time::Duration;

use futures::SinkExt;
use serde_json::{json, Value};
use sqlx::PgPool;
use tokio_tungstenite::tungstenite::Message;
use uuid::Uuid;
use vc_server::config::Config;
use vc_server::permissions::GuildPermissions;
use vc_server::ws::ServerEvent;

use super::helpers::{
    add_guild_member, create_channel, create_elevated_session, create_guild_with_default_role,
    create_test_user, create_voice_channel, expect_no_ws_event, generate_access_token, make_admin,
    next_ws_event, spawn_test_server, ws_connect, TestApp, TestServer, WsClient,
};

const EVENT_TIMEOUT: Duration = Duration::from_secs(15);
/// How long a test waits to be sure an event does NOT arrive.
const QUIET: Duration = Duration::from_secs(2);

fn member_perms() -> GuildPermissions {
    GuildPermissions::VIEW_CHANNEL
        | GuildPermissions::SEND_MESSAGES
        | GuildPermissions::VOICE_CONNECT
}

async fn everyone_role(pool: &PgPool, guild: Uuid) -> Uuid {
    sqlx::query_scalar("SELECT id FROM guild_roles WHERE guild_id = $1 AND is_default")
        .bind(guild)
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn create_role(pool: &PgPool, guild: Uuid, name: &str) -> Uuid {
    let id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO guild_roles (id, guild_id, name, permissions, position) VALUES ($1, $2, $3, 0, 1)",
    )
    .bind(id)
    .bind(guild)
    .bind(name)
    .execute(pool)
    .await
    .unwrap();
    id
}

async fn assign_role(pool: &PgPool, guild: Uuid, user: Uuid, role: Uuid) {
    sqlx::query("INSERT INTO guild_member_roles (guild_id, user_id, role_id) VALUES ($1, $2, $3)")
        .bind(guild)
        .bind(user)
        .bind(role)
        .execute(pool)
        .await
        .unwrap();
}

async fn set_override(
    pool: &PgPool,
    channel: Uuid,
    role: Uuid,
    allow: GuildPermissions,
    deny: GuildPermissions,
) {
    sqlx::query(
        "INSERT INTO channel_overrides (id, channel_id, role_id, allow_permissions, deny_permissions)
         VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(Uuid::now_v7())
    .bind(channel)
    .bind(role)
    .bind(allow.to_db())
    .bind(deny.to_db())
    .execute(pool)
    .await
    .unwrap();
}

/// Owner, member and their server (`@everyone` may view, write and talk).
struct Fixture {
    app: TestApp,
    server: TestServer,
    owner: Uuid,
    member: Uuid,
    guild: Uuid,
    owner_token: String,
    member_token: String,
}

async fn fixture_with_config(pool: &PgPool, config: Config) -> Fixture {
    let app = TestApp::with_pool_and_config(pool.clone(), config).await;
    let server = spawn_test_server(app.router.clone()).await;
    let (owner, _) = create_test_user(pool).await;
    let (member, _) = create_test_user(pool).await;
    let guild = create_guild_with_default_role(pool, owner, member_perms()).await;
    add_guild_member(pool, guild, member).await;
    let owner_token = generate_access_token(&app.config, owner);
    let member_token = generate_access_token(&app.config, member);
    Fixture {
        app,
        server,
        owner,
        member,
        guild,
        owner_token,
        member_token,
    }
}

async fn fixture(pool: &PgPool) -> Fixture {
    fixture_with_config(pool, Config::default_for_test()).await
}

async fn send(ws: &mut WsClient, value: Value) {
    ws.send(Message::Text(value.to_string().into()))
        .await
        .expect("send ws frame");
}

async fn connect_ready(server: &TestServer, token: &str) -> WsClient {
    let mut ws = ws_connect(server, token).await;
    next_ws_event(&mut ws, "ready", EVENT_TIMEOUT).await;
    ws
}

async fn subscribe(ws: &mut WsClient, channel: Uuid) {
    send(ws, json!({ "type": "subscribe", "channel_id": channel })).await;
    let ack = next_ws_event(ws, "subscribed", EVENT_TIMEOUT).await;
    assert_eq!(ack["channel_id"], channel.to_string());
}

async fn post_message(fx: &Fixture, channel: Uuid, content: &str) {
    let resp = reqwest::Client::new()
        .post(format!("{}/api/messages/channel/{channel}", fx.server.url))
        .bearer_auth(&fx.owner_token)
        .json(&json!({ "content": content }))
        .send()
        .await
        .expect("POST message");
    assert!(
        resp.status().is_success(),
        "POST message: {}",
        resp.status()
    );
}

async fn owner_request(fx: &Fixture, method: reqwest::Method, path: &str, body: Option<Value>) {
    let mut req = reqwest::Client::new()
        .request(method.clone(), format!("{}{path}", fx.server.url))
        .bearer_auth(&fx.owner_token);
    if let Some(body) = body {
        req = req.json(&body);
    }
    let resp = req.send().await.expect("owner request");
    assert!(
        resp.status().is_success(),
        "{method} {path}: {}",
        resp.status()
    );
}

/// The member's socket gets the "before" message and not the "after" one.
async fn assert_message_cut_off(fx: &Fixture, ws: &mut WsClient, channel: Uuid) {
    post_message(fx, channel, "after").await;
    expect_no_ws_event(ws, "message_new", QUIET).await;
}

async fn assert_receives(fx: &Fixture, ws: &mut WsClient, channel: Uuid) {
    post_message(fx, channel, "before").await;
    let ev = next_ws_event(ws, "message_new", EVENT_TIMEOUT).await;
    assert_eq!(ev["message"]["content"], "before");
}

// ---------------------------------------------------------------------------
// A4: channel subscriptions
// ---------------------------------------------------------------------------

/// A private channel visible through a role: removing the role ends the
/// member's live feed of it at once.
#[sqlx::test]
async fn role_removal_stops_private_channel_messages(pool: PgPool) {
    let fx = fixture(&pool).await;
    let private = create_channel(&pool, fx.guild, "private").await;
    let everyone = everyone_role(&pool, fx.guild).await;
    let role = create_role(&pool, fx.guild, "insiders").await;
    set_override(
        &pool,
        private,
        everyone,
        GuildPermissions::empty(),
        GuildPermissions::VIEW_CHANNEL,
    )
    .await;
    set_override(
        &pool,
        private,
        role,
        GuildPermissions::VIEW_CHANNEL,
        GuildPermissions::empty(),
    )
    .await;
    assign_role(&pool, fx.guild, fx.member, role).await;

    let mut ws = connect_ready(&fx.server, &fx.member_token).await;
    subscribe(&mut ws, private).await;
    assert_receives(&fx, &mut ws, private).await;

    owner_request(
        &fx,
        reqwest::Method::DELETE,
        &format!(
            "/api/guilds/{}/members/{}/roles/{role}",
            fx.guild, fx.member
        ),
        None,
    )
    .await;
    assert_message_cut_off(&fx, &mut ws, private).await;
}

#[sqlx::test]
async fn kick_stops_channel_messages(pool: PgPool) {
    let fx = fixture(&pool).await;
    let channel = create_channel(&pool, fx.guild, "general").await;
    let mut ws = connect_ready(&fx.server, &fx.member_token).await;
    subscribe(&mut ws, channel).await;
    assert_receives(&fx, &mut ws, channel).await;

    owner_request(
        &fx,
        reqwest::Method::DELETE,
        &format!("/api/guilds/{}/members/{}", fx.guild, fx.member),
        None,
    )
    .await;
    assert_message_cut_off(&fx, &mut ws, channel).await;
}

#[sqlx::test]
async fn override_deny_stops_channel_messages(pool: PgPool) {
    let fx = fixture(&pool).await;
    let channel = create_channel(&pool, fx.guild, "general").await;
    let everyone = everyone_role(&pool, fx.guild).await;
    let mut ws = connect_ready(&fx.server, &fx.member_token).await;
    subscribe(&mut ws, channel).await;
    assert_receives(&fx, &mut ws, channel).await;

    owner_request(
        &fx,
        reqwest::Method::PUT,
        &format!("/api/channels/{channel}/overrides/{everyone}"),
        Some(json!({ "allow": 0, "deny": GuildPermissions::VIEW_CHANNEL.bits() })),
    )
    .await;
    assert_message_cut_off(&fx, &mut ws, channel).await;
}

/// A change that published no notice (here: written straight to the
/// database) is caught by the periodic re-check.
#[sqlx::test]
async fn backstop_drops_subscription_without_notice(pool: PgPool) {
    let mut config = Config::default_for_test();
    config.session_recheck_secs = 1;
    let fx = fixture_with_config(&pool, config).await;
    let channel = create_channel(&pool, fx.guild, "general").await;
    let mut ws = connect_ready(&fx.server, &fx.member_token).await;
    subscribe(&mut ws, channel).await;
    assert_receives(&fx, &mut ws, channel).await;

    sqlx::query("DELETE FROM guild_members WHERE guild_id = $1 AND user_id = $2")
        .bind(fx.guild)
        .bind(fx.member)
        .execute(&pool)
        .await
        .unwrap();
    // Two re-check periods.
    tokio::time::sleep(Duration::from_millis(2500)).await;
    assert_message_cut_off(&fx, &mut ws, channel).await;
}

/// The admin event subscription is re-checked on the same tick: a socket
/// that is no longer an elevated admin stops getting admin events.
#[sqlx::test]
async fn admin_subscription_dropped_on_backstop(pool: PgPool) {
    let mut config = Config::default_for_test();
    config.session_recheck_secs = 1;
    let app = TestApp::with_pool_and_config(pool.clone(), config).await;
    let server = spawn_test_server(app.router.clone()).await;
    let (admin, _) = create_test_user(&pool).await;
    make_admin(&pool, admin).await;
    let token = create_elevated_session(&pool, &app.config, admin).await;

    let mut ws = connect_ready(&server, &token).await;
    send(&mut ws, json!({ "type": "admin_subscribe" })).await;
    // The subscribe has no ack: the ping after it is answered once it ran.
    until_pong(&mut ws).await;
    assert!(admin_event_arrives(&app, &mut ws, admin).await);

    sqlx::query("DELETE FROM system_admins WHERE user_id = $1")
        .bind(admin)
        .execute(&pool)
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(2500)).await;
    assert!(!admin_event_arrives(&app, &mut ws, admin).await);
}

/// Publish an admin event of this test (other tests share the admin
/// channel), then a marker on the user's own channel through the same
/// connection: the socket's forwarder handles them in that order. Returns
/// whether the admin event was written before the marker.
async fn admin_event_arrives(app: &TestApp, ws: &mut WsClient, user: Uuid) -> bool {
    use futures::StreamExt;
    let subject = Uuid::new_v4();
    let event = ServerEvent::AdminUserUnbanned {
        user_id: subject,
        username: "someone".into(),
    };
    vc_server::ws::broadcast_admin_event(&app.state.redis, &event)
        .await
        .expect("publish");
    let marker = Uuid::new_v4();
    vc_server::ws::broadcast_to_user(
        &app.state.redis,
        user,
        &ServerEvent::Unsubscribed { channel_id: marker },
    )
    .await
    .expect("publish marker");
    let mut arrived = false;
    let wait = async {
        loop {
            match ws.next().await {
                Some(Ok(Message::Text(text))) => {
                    let v: Value = serde_json::from_str(&text).unwrap();
                    if v["type"] == "admin_user_unbanned" && v["user_id"] == subject.to_string() {
                        arrived = true;
                    }
                    if v["type"] == "unsubscribed" && v["channel_id"] == marker.to_string() {
                        return;
                    }
                }
                Some(Ok(_)) => {}
                other => panic!("socket ended before the marker: {other:?}"),
            }
        }
    };
    tokio::time::timeout(EVENT_TIMEOUT, wait)
        .await
        .expect("no marker");
    arrived
}

// ---------------------------------------------------------------------------
// A5: voice
// ---------------------------------------------------------------------------

async fn join_voice(ws: &mut WsClient, channel: Uuid) {
    send(ws, json!({ "type": "voice_join", "channel_id": channel })).await;
    next_ws_event(ws, "voice_room_state", EVENT_TIMEOUT).await;
}

async fn in_room(fx: &Fixture, channel: Uuid, user: Uuid) -> bool {
    match fx.app.state.sfu.get_room(channel).await {
        Some(room) => room.get_peer(user).await.is_some(),
        None => false,
    }
}

#[sqlx::test]
async fn kicked_voice_member_is_removed_with_voice_removed(pool: PgPool) {
    let fx = fixture(&pool).await;
    let voice = create_voice_channel(&pool, fx.guild, "voice").await;
    let mut owner_ws = connect_ready(&fx.server, &fx.owner_token).await;
    let mut member_ws = connect_ready(&fx.server, &fx.member_token).await;
    join_voice(&mut owner_ws, voice).await;
    join_voice(&mut member_ws, voice).await;
    assert!(in_room(&fx, voice, fx.member).await);

    owner_request(
        &fx,
        reqwest::Method::DELETE,
        &format!("/api/guilds/{}/members/{}", fx.guild, fx.member),
        None,
    )
    .await;

    assert!(
        !in_room(&fx, voice, fx.member).await,
        "kicked member still in the room"
    );
    assert!(in_room(&fx, voice, fx.owner).await, "owner was removed too");
    let removed = next_ws_event(&mut member_ws, "voice_removed", EVENT_TIMEOUT).await;
    assert_eq!(
        removed,
        json!({ "type": "voice_removed", "channel_id": voice, "reason": "kicked" })
    );
    let left = next_ws_event(&mut owner_ws, "voice_user_left", EVENT_TIMEOUT).await;
    assert_eq!(left["user_id"], fx.member.to_string());
}

#[sqlx::test]
async fn channel_delete_removes_everyone(pool: PgPool) {
    let fx = fixture(&pool).await;
    let voice = create_voice_channel(&pool, fx.guild, "voice").await;
    let mut owner_ws = connect_ready(&fx.server, &fx.owner_token).await;
    let mut member_ws = connect_ready(&fx.server, &fx.member_token).await;
    join_voice(&mut owner_ws, voice).await;
    join_voice(&mut member_ws, voice).await;

    owner_request(
        &fx,
        reqwest::Method::DELETE,
        &format!("/api/channels/{voice}"),
        None,
    )
    .await;

    assert!(
        fx.app.state.sfu.get_room(voice).await.is_none(),
        "room of the deleted channel still exists"
    );
    for ws in [&mut owner_ws, &mut member_ws] {
        let removed = next_ws_event(ws, "voice_removed", EVENT_TIMEOUT).await;
        assert_eq!(removed["reason"], "channel_deleted");
        assert_eq!(removed["channel_id"], voice.to_string());
    }
}

#[sqlx::test]
async fn role_loss_removes_from_voice(pool: PgPool) {
    let fx = fixture(&pool).await;
    let voice = create_voice_channel(&pool, fx.guild, "voice").await;
    let everyone = everyone_role(&pool, fx.guild).await;
    let role = create_role(&pool, fx.guild, "speakers").await;
    set_override(
        &pool,
        voice,
        everyone,
        GuildPermissions::empty(),
        GuildPermissions::VOICE_CONNECT,
    )
    .await;
    set_override(
        &pool,
        voice,
        role,
        GuildPermissions::VOICE_CONNECT,
        GuildPermissions::empty(),
    )
    .await;
    assign_role(&pool, fx.guild, fx.member, role).await;

    let mut member_ws = connect_ready(&fx.server, &fx.member_token).await;
    join_voice(&mut member_ws, voice).await;

    owner_request(
        &fx,
        reqwest::Method::DELETE,
        &format!(
            "/api/guilds/{}/members/{}/roles/{role}",
            fx.guild, fx.member
        ),
        None,
    )
    .await;

    assert!(!in_room(&fx, voice, fx.member).await);
    let removed = next_ws_event(&mut member_ws, "voice_removed", EVENT_TIMEOUT).await;
    assert_eq!(removed["reason"], "permission_lost");
}

/// A member who keeps access stays in voice when someone else is kicked.
#[sqlx::test]
async fn unaffected_member_stays_in_voice(pool: PgPool) {
    let fx = fixture(&pool).await;
    let (other, _) = create_test_user(&pool).await;
    add_guild_member(&pool, fx.guild, other).await;
    let voice = create_voice_channel(&pool, fx.guild, "voice").await;
    let mut member_ws = connect_ready(&fx.server, &fx.member_token).await;
    join_voice(&mut member_ws, voice).await;

    owner_request(
        &fx,
        reqwest::Method::DELETE,
        &format!("/api/guilds/{}/members/{other}", fx.guild),
        None,
    )
    .await;
    assert!(in_room(&fx, voice, fx.member).await);
    expect_no_ws_event(&mut member_ws, "voice_removed", QUIET).await;
}

/// A voice participant whose access went away without a notice (here: written
/// straight to the database) is removed by the socket's periodic re-check.
#[sqlx::test]
async fn backstop_removes_voice_participant_without_notice(pool: PgPool) {
    let mut config = Config::default_for_test();
    config.session_recheck_secs = 1;
    let fx = fixture_with_config(&pool, config).await;
    let voice = create_voice_channel(&pool, fx.guild, "voice").await;
    let mut member_ws = connect_ready(&fx.server, &fx.member_token).await;
    join_voice(&mut member_ws, voice).await;

    sqlx::query("DELETE FROM guild_members WHERE guild_id = $1 AND user_id = $2")
        .bind(fx.guild)
        .bind(fx.member)
        .execute(&pool)
        .await
        .unwrap();
    let removed = next_ws_event(&mut member_ws, "voice_removed", EVENT_TIMEOUT).await;
    assert_eq!(removed["reason"], "permission_lost");
    assert!(!in_room(&fx, voice, fx.member).await);
}

// ---------------------------------------------------------------------------
// Fix round 1: epochs fence delivery, regain, races, cost, failure, triggers
// ---------------------------------------------------------------------------

/// A private channel (`@everyone` may not view it, `role` may) and a public
/// control channel; the member holds `role` and is subscribed to both.
struct Private {
    private: Uuid,
    control: Uuid,
    role: Uuid,
}

async fn private_channel(pool: &PgPool, fx: &Fixture) -> Private {
    let private = create_channel(pool, fx.guild, "private").await;
    let control = create_channel(pool, fx.guild, "control").await;
    let everyone = everyone_role(pool, fx.guild).await;
    let role = create_role(pool, fx.guild, "insiders").await;
    set_override(
        pool,
        private,
        everyone,
        GuildPermissions::empty(),
        GuildPermissions::VIEW_CHANNEL,
    )
    .await;
    set_override(
        pool,
        private,
        role,
        GuildPermissions::VIEW_CHANNEL,
        GuildPermissions::empty(),
    )
    .await;
    assign_role(pool, fx.guild, fx.member, role).await;
    Private {
        private,
        control,
        role,
    }
}

async fn request_as(
    fx: &Fixture,
    token: &str,
    method: reqwest::Method,
    path: &str,
    body: Option<Value>,
) {
    let mut req = reqwest::Client::new()
        .request(method.clone(), format!("{}{path}", fx.server.url))
        .bearer_auth(token);
    if let Some(body) = body {
        req = req.json(&body);
    }
    let resp = req.send().await.expect("request");
    assert!(
        resp.status().is_success(),
        "{method} {path}: {}",
        resp.status()
    );
}

/// The next `message_new` the socket gets; panics on anything else in the
/// way that is a `message_new` of another content.
async fn next_message(ws: &mut WsClient) -> Value {
    next_ws_event(ws, "message_new", EVENT_TIMEOUT).await
}

/// Post `content` to `cut`, then a control message to `control`: events of
/// one socket keep their order, so receiving the control message first
/// proves the `cut` message was not delivered (no fixed wait needed).
async fn assert_cut_off_before_control(fx: &Fixture, ws: &mut WsClient, cut: Uuid, control: Uuid) {
    post_message(fx, cut, "must not arrive").await;
    post_message(fx, control, "control").await;
    let ev = next_message(ws).await;
    assert_eq!(
        ev["message"]["content"], "control",
        "a message of the revoked channel was delivered: {ev}"
    );
}

async fn remove_role(fx: &Fixture, role: Uuid) {
    owner_request(
        fx,
        reqwest::Method::DELETE,
        &format!(
            "/api/guilds/{}/members/{}/roles/{role}",
            fx.guild, fx.member
        ),
        None,
    )
    .await;
}

async fn grant_role(fx: &Fixture, role: Uuid) {
    owner_request(
        fx,
        reqwest::Method::POST,
        &format!(
            "/api/guilds/{}/members/{}/roles/{role}",
            fx.guild, fx.member
        ),
        None,
    )
    .await;
}

/// Test 1a: the role removal commits and the next message is published
/// before the socket was woken (wake path disabled): delivery itself is
/// fenced by the epoch, no notice involved.
#[sqlx::test]
async fn message_after_commit_is_fenced_without_any_wake(pool: PgPool) {
    let fx = fixture(&pool).await;
    let p = private_channel(&pool, &fx).await;
    vc_server::ws::test_hooks::disable_access_wake(fx.member);
    let mut ws = connect_ready(&fx.server, &fx.member_token).await;
    subscribe(&mut ws, p.private).await;
    subscribe(&mut ws, p.control).await;
    assert_receives(&fx, &mut ws, p.private).await;

    remove_role(&fx, p.role).await;
    assert_cut_off_before_control(&fx, &mut ws, p.private, p.control).await;
}

/// Test 1b: an event already queued for the socket when access shrinks is
/// not written.
#[sqlx::test]
async fn queued_message_is_not_written_after_revocation(pool: PgPool) {
    let fx = fixture(&pool).await;
    let p = private_channel(&pool, &fx).await;
    let mut ws = connect_ready(&fx.server, &fx.member_token).await;
    subscribe(&mut ws, p.private).await;
    subscribe(&mut ws, p.control).await;
    assert_receives(&fx, &mut ws, p.private).await;

    let pause = vc_server::ws::test_hooks::pause_before_channel_write(fx.member);
    post_message(&fx, p.private, "queued").await;
    // The message sits in the socket's queue, its write is held.
    pause.reached.notified().await;
    remove_role(&fx, p.role).await;
    pause.release.notify_one();

    post_message(&fx, p.control, "control").await;
    let ev = next_message(&mut ws).await;
    assert_eq!(
        ev["message"]["content"], "control",
        "queued message written: {ev}"
    );
}

/// Test 2: access that comes back restores the live feed without any
/// client action (no new `subscribe`).
#[sqlx::test]
async fn regained_role_restores_events_without_resubscribe(pool: PgPool) {
    let fx = fixture(&pool).await;
    let p = private_channel(&pool, &fx).await;
    let mut ws = connect_ready(&fx.server, &fx.member_token).await;
    subscribe(&mut ws, p.private).await;
    subscribe(&mut ws, p.control).await;

    remove_role(&fx, p.role).await;
    assert_cut_off_before_control(&fx, &mut ws, p.private, p.control).await;

    grant_role(&fx, p.role).await;
    post_message(&fx, p.private, "back").await;
    assert_eq!(next_message(&mut ws).await["message"]["content"], "back");
}

/// Regain through a channel override: deny, then delete the override.
#[sqlx::test]
async fn removed_override_restores_events(pool: PgPool) {
    let fx = fixture(&pool).await;
    let channel = create_channel(&pool, fx.guild, "general").await;
    let control = create_channel(&pool, fx.guild, "control").await;
    let everyone = everyone_role(&pool, fx.guild).await;
    let mut ws = connect_ready(&fx.server, &fx.member_token).await;
    subscribe(&mut ws, channel).await;
    subscribe(&mut ws, control).await;

    let path = format!("/api/channels/{channel}/overrides/{everyone}");
    owner_request(
        &fx,
        reqwest::Method::PUT,
        &path,
        Some(json!({ "allow": 0, "deny": GuildPermissions::VIEW_CHANNEL.bits() })),
    )
    .await;
    assert_cut_off_before_control(&fx, &mut ws, channel, control).await;

    owner_request(&fx, reqwest::Method::DELETE, &path, None).await;
    post_message(&fx, channel, "back").await;
    assert_eq!(next_message(&mut ws).await["message"]["content"], "back");
}

/// Test 3a: `subscribe` checked access, then a revocation committed before
/// the result was recorded: the subscribe checks again and is refused.
#[sqlx::test]
async fn subscribe_racing_a_revocation_is_refused(pool: PgPool) {
    let fx = fixture(&pool).await;
    let p = private_channel(&pool, &fx).await;
    let mut ws = connect_ready(&fx.server, &fx.member_token).await;
    subscribe(&mut ws, p.control).await;

    let pause = vc_server::ws::test_hooks::pause_subscribe_after_check(fx.member);
    send(
        &mut ws,
        json!({ "type": "subscribe", "channel_id": p.private }),
    )
    .await;
    pause.reached.notified().await;
    remove_role(&fx, p.role).await;
    pause.release.notify_one();

    let err = next_ws_event(&mut ws, "error", EVENT_TIMEOUT).await;
    assert_eq!(err["code"], "FORBIDDEN");
    assert_cut_off_before_control(&fx, &mut ws, p.private, p.control).await;
}

/// Test 3b: the other order: `subscribe` was refused on its first check, a
/// grant committed before the result was recorded: it checks again and is
/// accepted.
#[sqlx::test]
async fn subscribe_racing_a_grant_is_accepted(pool: PgPool) {
    let fx = fixture(&pool).await;
    let p = private_channel(&pool, &fx).await;
    remove_role(&fx, p.role).await;
    let mut ws = connect_ready(&fx.server, &fx.member_token).await;

    let pause = vc_server::ws::test_hooks::pause_subscribe_after_check(fx.member);
    send(
        &mut ws,
        json!({ "type": "subscribe", "channel_id": p.private }),
    )
    .await;
    pause.reached.notified().await;
    grant_role(&fx, p.role).await;
    pause.release.notify_one();

    let ack = next_ws_event(&mut ws, "subscribed", EVENT_TIMEOUT).await;
    assert_eq!(ack["channel_id"], p.private.to_string());
    assert_receives(&fx, &mut ws, p.private).await;
}

/// Test 3c: a revocation right after the subscribe was recorded.
#[sqlx::test]
async fn revocation_after_subscribe_cuts_off(pool: PgPool) {
    let fx = fixture(&pool).await;
    let p = private_channel(&pool, &fx).await;
    let mut ws = connect_ready(&fx.server, &fx.member_token).await;
    subscribe(&mut ws, p.control).await;
    subscribe(&mut ws, p.private).await;
    remove_role(&fx, p.role).await;
    assert_cut_off_before_control(&fx, &mut ws, p.private, p.control).await;
}

/// Test 4: many role edits in a row cost each socket a bounded number of
/// batched reads, independent of the number of edits and subscriptions.
#[sqlx::test]
async fn rapid_role_edits_coalesce_into_few_reads(pool: PgPool) {
    const SOCKETS: usize = 3;
    const CHANNELS: usize = 20;
    const EDITS: usize = 50;
    let fx = fixture(&pool).await;
    let mut channels = Vec::new();
    for i in 0..CHANNELS {
        channels.push(create_channel(&pool, fx.guild, &format!("c{i}")).await);
    }
    let mut sockets = Vec::new();
    for _ in 0..SOCKETS {
        let mut ws = connect_ready(&fx.server, &fx.member_token).await;
        for channel in &channels {
            subscribe(&mut ws, *channel).await;
        }
        sockets.push(ws);
    }

    vc_server::ws::test_hooks::count_access_reads(fx.member);
    for _ in 0..EDITS {
        vc_server::ws::access::access_changed(&fx.app.state, guild_roles_bumped(fx.guild).await)
            .await;
    }
    // Every socket revalidates once after the debounce...
    let deadline = tokio::time::Instant::now() + EVENT_TIMEOUT;
    while vc_server::ws::test_hooks::access_reads(fx.member).0 < SOCKETS {
        assert!(tokio::time::Instant::now() < deadline, "no revalidation");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    // ...and a message afterwards needs no further read.
    post_message(&fx, channels[0], "after").await;
    for ws in &mut sockets {
        assert_eq!(next_message(ws).await["message"]["content"], "after");
    }
    let (revalidations, reads) = vc_server::ws::test_hooks::access_reads(fx.member);
    // At most two coalesced runs per socket (a bump can land during the
    // first), one batched read each (only server channels requested).
    assert!(
        revalidations <= 2 * SOCKETS && reads <= 2 * SOCKETS,
        "{EDITS} edits cost {revalidations} revalidations / {reads} reads for {SOCKETS} sockets"
    );
}

/// A server-wide access change without a database write, through the
/// production wrapper (the only way to get the proof `access_changed` takes).
async fn guild_roles_bumped(guild: Uuid) -> vc_server::ws::access::Bumped {
    let change = vc_server::ws::access::AccessChange::GuildRolesChanged { guild_id: guild };
    let written = std::future::ready(Ok::<(), std::convert::Infallible>(()));
    match vc_server::ws::access::bump_after(change, written).await {
        Ok(((), bumped)) => bumped,
        Err(never) => match never {},
    }
}

/// Send `ping` and read until its `pong`: frames of one socket keep their
/// order, so everything queued for the socket before the ping has been
/// written or dropped by then. Returns the events seen before the pong.
async fn until_pong(ws: &mut WsClient) -> Vec<Value> {
    use futures::StreamExt;
    send(ws, json!({ "type": "ping" })).await;
    let mut events = Vec::new();
    let wait = async {
        loop {
            match ws.next().await {
                Some(Ok(Message::Text(text))) => {
                    let event: Value = serde_json::from_str(&text).unwrap();
                    if event["type"] == "pong" {
                        return;
                    }
                    events.push(event);
                }
                Some(Ok(_)) => {}
                other => panic!("socket ended before the pong: {other:?}"),
            }
        }
    };
    tokio::time::timeout(EVENT_TIMEOUT, wait)
        .await
        .expect("no pong");
    events
}

fn assert_none_of_type(events: &[Value], event_type: &str) {
    assert!(
        !events.iter().any(|e| e["type"] == event_type),
        "{event_type} written on revoked access: {events:?}"
    );
}

/// Frames until the server's Close: the events seen, and the close code and
/// reason.
async fn until_close(ws: &mut WsClient) -> (Vec<Value>, u16, String) {
    use futures::StreamExt;
    let mut events = Vec::new();
    let wait = async {
        loop {
            match ws.next().await {
                Some(Ok(Message::Text(text))) => events.push(serde_json::from_str(&text).unwrap()),
                Some(Ok(Message::Close(Some(frame)))) => {
                    return (u16::from(frame.code), frame.reason.to_string());
                }
                Some(Ok(_)) => {}
                other => panic!("socket ended without a close frame: {other:?}"),
            }
        }
    };
    let (code, reason) = tokio::time::timeout(EVENT_TIMEOUT, wait)
        .await
        .expect("no close frame");
    (events, code, reason)
}

/// Test 5 / item 4: a revalidation that fails (database error) drops the
/// scope's events (fail closed) and retries with backoff; once it recovers,
/// the socket that dropped something closes with 1011 `RESYNC` (spread over
/// up to 3 s) so the client reconnects and catches up — never right away.
#[sqlx::test]
async fn failed_revalidation_fails_closed_then_asks_for_resync(pool: PgPool) {
    let fx = fixture(&pool).await;
    let channel = create_channel(&pool, fx.guild, "general").await;
    let mut ws = connect_ready(&fx.server, &fx.member_token).await;
    subscribe(&mut ws, channel).await;
    assert_receives(&fx, &mut ws, channel).await;

    vc_server::ws::test_hooks::fail_access_reads(fx.member, 2);
    let bumped_at = tokio::time::Instant::now();
    vc_server::ws::access::access_changed(&fx.app.state, guild_roles_bumped(fx.guild).await).await;
    // The first write after the bump fails its revalidation: dropped.
    post_message(&fx, channel, "while failing").await;

    let (events, code, reason) = until_close(&mut ws).await;
    assert!(
        !events
            .iter()
            .any(|e| e["type"] == "message_new" && e["message"]["content"] == "while failing"),
        "dropped message delivered: {events:?}"
    );
    assert_eq!((code, reason.as_str()), (1011, "RESYNC"));
    // Retries at 250 ms and 500 ms: not before the recovery.
    assert!(bumped_at.elapsed() >= Duration::from_millis(700));
}

/// A failing revalidation that dropped nothing recovers without a close.
#[sqlx::test]
async fn failed_revalidation_without_drops_keeps_the_socket(pool: PgPool) {
    let fx = fixture(&pool).await;
    let channel = create_channel(&pool, fx.guild, "general").await;
    let mut ws = connect_ready(&fx.server, &fx.member_token).await;
    subscribe(&mut ws, channel).await;

    vc_server::ws::test_hooks::count_access_reads(fx.member);
    vc_server::ws::test_hooks::fail_access_reads(fx.member, 1);
    vc_server::ws::access::access_changed(&fx.app.state, guild_roles_bumped(fx.guild).await).await;
    // The woken revalidation fails, its retry succeeds.
    let deadline = tokio::time::Instant::now() + EVENT_TIMEOUT;
    while vc_server::ws::test_hooks::access_reads(fx.member).0 < 2 {
        assert!(tokio::time::Instant::now() < deadline, "no retry");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_receives(&fx, &mut ws, channel).await;
    // No resync close within its whole spread window (expect_no_ws_event
    // panics on a close frame); a negative check needs the full window.
    expect_no_ws_event(&mut ws, "error", Duration::from_millis(3200)).await;
}

// --- Test 6: every trigger --------------------------------------------------

/// A role's permission edit (on the role that grants the channel).
#[sqlx::test]
async fn trigger_role_permission_edit(pool: PgPool) {
    let fx = fixture(&pool).await;
    let control = create_channel(&pool, fx.guild, "control").await;
    let channel = create_channel(&pool, fx.guild, "general").await;
    let everyone = everyone_role(&pool, fx.guild).await;
    let mut ws = connect_ready(&fx.server, &fx.member_token).await;
    subscribe(&mut ws, channel).await;
    subscribe(&mut ws, control).await;
    // Overrides keep the control channel visible.
    set_override(
        &pool,
        control,
        everyone,
        GuildPermissions::VIEW_CHANNEL,
        GuildPermissions::empty(),
    )
    .await;
    owner_request(
        &fx,
        reqwest::Method::PATCH,
        &format!("/api/guilds/{}/roles/{everyone}", fx.guild),
        Some(json!({ "permissions": GuildPermissions::SEND_MESSAGES.bits() })),
    )
    .await;
    assert_cut_off_before_control(&fx, &mut ws, channel, control).await;
}

/// Deleting the role that granted the channel.
#[sqlx::test]
async fn trigger_role_delete(pool: PgPool) {
    let fx = fixture(&pool).await;
    let p = private_channel(&pool, &fx).await;
    let mut ws = connect_ready(&fx.server, &fx.member_token).await;
    subscribe(&mut ws, p.private).await;
    subscribe(&mut ws, p.control).await;
    owner_request(
        &fx,
        reqwest::Method::DELETE,
        &format!("/api/guilds/{}/roles/{}", fx.guild, p.role),
        None,
    )
    .await;
    assert_cut_off_before_control(&fx, &mut ws, p.private, p.control).await;
}

/// Leaving the server (own request).
#[sqlx::test]
async fn trigger_leave_server(pool: PgPool) {
    let fx = fixture(&pool).await;
    let channel = create_channel(&pool, fx.guild, "general").await;
    let mut ws = connect_ready(&fx.server, &fx.member_token).await;
    subscribe(&mut ws, channel).await;
    request_as(
        &fx,
        &fx.member_token,
        reqwest::Method::POST,
        &format!("/api/guilds/{}/leave", fx.guild),
        None,
    )
    .await;
    assert_message_cut_off(&fx, &mut ws, channel).await;
}

/// Removing one's own reaction removes a reaction role (and the channel it
/// granted); the member also leaves the voice room it gave access to.
#[sqlx::test]
async fn trigger_reaction_role_removal(pool: PgPool) {
    let fx = fixture(&pool).await;
    let p = private_channel(&pool, &fx).await;
    // The member gets the role through a reaction, not by assignment.
    sqlx::query("DELETE FROM guild_member_roles WHERE user_id = $1")
        .bind(fx.member)
        .execute(&pool)
        .await
        .unwrap();
    let everyone = everyone_role(&pool, fx.guild).await;
    sqlx::query("UPDATE guild_roles SET permissions = $2 WHERE id = $1")
        .bind(everyone)
        .bind((member_perms() | GuildPermissions::ADD_REACTIONS).to_db())
        .execute(&pool)
        .await
        .unwrap();
    let voice = create_voice_channel(&pool, fx.guild, "voice").await;
    set_override(
        &pool,
        voice,
        everyone,
        GuildPermissions::empty(),
        GuildPermissions::VOICE_CONNECT,
    )
    .await;
    set_override(
        &pool,
        voice,
        p.role,
        GuildPermissions::VOICE_CONNECT,
        GuildPermissions::empty(),
    )
    .await;
    let msg = super::helpers::insert_message(&pool, p.control, fx.owner, "pick a role").await;
    sqlx::query(
        "INSERT INTO reaction_role_bindings (guild_id, channel_id, message_id, emoji, role_id, mode)
         VALUES ($1, $2, $3, '🎨', $4, 'toggle')",
    )
    .bind(fx.guild)
    .bind(p.control)
    .bind(msg)
    .bind(p.role)
    .execute(&pool)
    .await
    .unwrap();
    let reactions = format!("/api/channels/{}/messages/{msg}/reactions", p.control);
    request_as(
        &fx,
        &fx.member_token,
        reqwest::Method::PUT,
        &reactions,
        Some(json!({ "emoji": "🎨" })),
    )
    .await;

    let mut ws = connect_ready(&fx.server, &fx.member_token).await;
    subscribe(&mut ws, p.private).await;
    subscribe(&mut ws, p.control).await;
    join_voice(&mut ws, voice).await;

    request_as(
        &fx,
        &fx.member_token,
        reqwest::Method::DELETE,
        &format!("{reactions}/🎨"),
        None,
    )
    .await;
    assert!(!in_room(&fx, voice, fx.member).await, "still in voice");
    let removed = next_ws_event(&mut ws, "voice_removed", EVENT_TIMEOUT).await;
    assert_eq!(removed["reason"], "permission_lost");
    assert_cut_off_before_control(&fx, &mut ws, p.private, p.control).await;
}

/// Leaving a group DM: the leaver's socket stops getting it and leaves its
/// call; the others keep it.
#[sqlx::test]
async fn trigger_group_dm_leave(pool: PgPool) {
    let fx = fixture(&pool).await;
    let (third, _) = create_test_user(&pool).await;
    let dm = vc_server::chat::dm::create_group_dm(&pool, fx.owner, &[fx.member, third], None)
        .await
        .expect("group DM");
    let control = create_channel(&pool, fx.guild, "control").await;
    let mut ws = connect_ready(&fx.server, &fx.member_token).await;
    subscribe(&mut ws, dm.id).await;
    subscribe(&mut ws, control).await;
    join_voice(&mut ws, dm.id).await;
    assert_receives(&fx, &mut ws, dm.id).await;

    request_as(
        &fx,
        &fx.member_token,
        reqwest::Method::POST,
        &format!("/api/dm/{}/leave", dm.id),
        None,
    )
    .await;
    assert!(
        !in_room(&fx, dm.id, fx.member).await,
        "still in the DM call"
    );
    let removed = next_ws_event(&mut ws, "voice_removed", EVENT_TIMEOUT).await;
    assert_eq!(removed["reason"], "permission_lost");
    assert_cut_off_before_control(&fx, &mut ws, dm.id, control).await;
}

/// Ending the admin elevation stops admin events at once (no backstop).
#[sqlx::test]
async fn trigger_de_elevation_stops_admin_events(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let server = spawn_test_server(app.router.clone()).await;
    let (admin, _) = create_test_user(&pool).await;
    make_admin(&pool, admin).await;
    let token = create_elevated_session(&pool, &app.config, admin).await;

    let mut ws = connect_ready(&server, &token).await;
    send(&mut ws, json!({ "type": "admin_subscribe" })).await;
    let event = ServerEvent::AdminUserUnbanned {
        user_id: Uuid::new_v4(),
        username: "someone".into(),
    };
    let deadline = tokio::time::Instant::now() + EVENT_TIMEOUT;
    loop {
        vc_server::ws::broadcast_admin_event(&app.state.redis, &event)
            .await
            .expect("publish");
        let got = tokio::time::timeout(
            Duration::from_millis(200),
            next_ws_event(&mut ws, "admin_user_unbanned", EVENT_TIMEOUT),
        )
        .await;
        if got.is_ok() {
            break;
        }
        assert!(tokio::time::Instant::now() < deadline, "no admin event");
    }

    let resp = reqwest::Client::new()
        .delete(format!("{}/api/admin/elevate", server.url))
        .bearer_auth(&token)
        .send()
        .await
        .expect("de-elevate");
    assert!(resp.status().is_success(), "de-elevate: {}", resp.status());
    vc_server::ws::broadcast_admin_event(&app.state.redis, &event)
        .await
        .expect("publish");
    // A ping round trip after the publish: its pong comes after any admin
    // event that was going to be forwarded.
    send(&mut ws, json!({ "type": "ping" })).await;
    let wait = async {
        use futures::StreamExt;
        loop {
            match ws.next().await {
                Some(Ok(Message::Text(text))) => {
                    let v: Value = serde_json::from_str(&text).unwrap();
                    assert_ne!(
                        v["type"], "admin_user_unbanned",
                        "admin event after de-elevation"
                    );
                    if v["type"] == "pong" {
                        return;
                    }
                }
                Some(Ok(_)) => {}
                other => panic!("socket ended: {other:?}"),
            }
        }
    };
    tokio::time::timeout(EVENT_TIMEOUT, wait)
        .await
        .expect("pong");
}

/// Test 7: a voice join whose permission check passed, then the role that
/// allowed it was removed before the peer was added: the peer is not
/// admitted.
#[sqlx::test]
async fn voice_join_racing_a_revocation_is_not_admitted(pool: PgPool) {
    let fx = fixture(&pool).await;
    let voice = create_voice_channel(&pool, fx.guild, "voice").await;
    let everyone = everyone_role(&pool, fx.guild).await;
    let role = create_role(&pool, fx.guild, "speakers").await;
    set_override(
        &pool,
        voice,
        everyone,
        GuildPermissions::empty(),
        GuildPermissions::VOICE_CONNECT,
    )
    .await;
    set_override(
        &pool,
        voice,
        role,
        GuildPermissions::VOICE_CONNECT,
        GuildPermissions::empty(),
    )
    .await;
    assign_role(&pool, fx.guild, fx.member, role).await;
    let mut ws = connect_ready(&fx.server, &fx.member_token).await;

    let pause = vc_server::ws::test_hooks::pause_voice_join_after_check(fx.member);
    send(
        &mut ws,
        json!({ "type": "voice_join", "channel_id": voice }),
    )
    .await;
    pause.reached.notified().await;
    remove_role(&fx, role).await;
    pause.release.notify_one();

    let err = next_ws_event(&mut ws, "voice_error", EVENT_TIMEOUT).await;
    assert!(
        err["message"].as_str().unwrap_or_default().len() > 0,
        "voice_error without message: {err}"
    );
    assert!(
        !in_room(&fx, voice, fx.member).await,
        "revoked join admitted"
    );
}

// ---------------------------------------------------------------------------
// Fix round 2
// ---------------------------------------------------------------------------

/// The epoch the trigger bumps (user or server).
fn epoch_of(scope_id: Uuid) -> (u64, u64) {
    let r = vc_server::ws::access::registry();
    (
        r.snapshot(scope_id, None).user,
        r.snapshot(Uuid::nil(), Some(scope_id))
            .guild
            .unwrap_or_default(),
    )
}

/// Item 10: run `request` with a pause right after its commit and bump; at
/// that moment (before any post-commit cleanup of the request) the epoch of
/// `scope_id` has already moved, and `check` (e.g. a cut-off check) runs.
/// A trigger whose bump is not the first thing after its commit never
/// reaches the pause and fails on the timeout.
async fn fenced_at_commit<R, C>(scope_id: Uuid, request: R, check: C)
where
    R: std::future::Future<Output = ()> + Send + 'static,
    C: std::future::Future<Output = ()>,
{
    let before = epoch_of(scope_id);
    let pause = vc_server::ws::test_hooks::pause_after_access_bump(scope_id);
    let running = tokio::spawn(request);
    tokio::time::timeout(EVENT_TIMEOUT, pause.reached.notified())
        .await
        .expect("the trigger did not bump right after its commit");
    assert_ne!(epoch_of(scope_id), before, "epoch not moved at the commit");
    check.await;
    pause.release.notify_one();
    running.await.expect("request task");
}

fn spawn_request(
    url: String,
    token: String,
    method: reqwest::Method,
    body: Option<Value>,
) -> impl std::future::Future<Output = ()> + Send + 'static {
    async move {
        let mut req = reqwest::Client::new()
            .request(method.clone(), &url)
            .bearer_auth(token);
        if let Some(body) = body {
            req = req.json(&body);
        }
        let resp = req.send().await.expect("request");
        assert!(
            resp.status().is_success(),
            "{method} {url}: {}",
            resp.status()
        );
    }
}

/// A DM between owner and member as a control channel that survives server
/// changes.
async fn dm_control(pool: &PgPool, fx: &Fixture) -> Uuid {
    let (third, _) = create_test_user(pool).await;
    vc_server::chat::dm::create_group_dm(pool, fx.owner, &[fx.member, third], None)
        .await
        .expect("group DM")
        .id
}

#[sqlx::test]
async fn kick_fences_before_presence_cleanup(pool: PgPool) {
    let fx = fixture(&pool).await;
    let channel = create_channel(&pool, fx.guild, "general").await;
    let control = dm_control(&pool, &fx).await;
    vc_server::ws::test_hooks::disable_access_wake(fx.member);
    let mut ws = connect_ready(&fx.server, &fx.member_token).await;
    subscribe(&mut ws, channel).await;
    subscribe(&mut ws, control).await;
    let url = format!(
        "{}/api/guilds/{}/members/{}",
        fx.server.url, fx.guild, fx.member
    );
    fenced_at_commit(
        fx.member,
        spawn_request(url, fx.owner_token.clone(), reqwest::Method::DELETE, None),
        assert_cut_off_before_control(&fx, &mut ws, channel, control),
    )
    .await;
}

#[sqlx::test]
async fn leave_fences_before_presence_cleanup(pool: PgPool) {
    let fx = fixture(&pool).await;
    let channel = create_channel(&pool, fx.guild, "general").await;
    let control = dm_control(&pool, &fx).await;
    vc_server::ws::test_hooks::disable_access_wake(fx.member);
    let mut ws = connect_ready(&fx.server, &fx.member_token).await;
    subscribe(&mut ws, channel).await;
    subscribe(&mut ws, control).await;
    let url = format!("{}/api/guilds/{}/leave", fx.server.url, fx.guild);
    fenced_at_commit(
        fx.member,
        spawn_request(url, fx.member_token.clone(), reqwest::Method::POST, None),
        assert_cut_off_before_control(&fx, &mut ws, channel, control),
    )
    .await;
}

#[sqlx::test]
async fn role_removal_fences_at_commit(pool: PgPool) {
    let fx = fixture(&pool).await;
    let p = private_channel(&pool, &fx).await;
    vc_server::ws::test_hooks::disable_access_wake(fx.member);
    let mut ws = connect_ready(&fx.server, &fx.member_token).await;
    subscribe(&mut ws, p.private).await;
    subscribe(&mut ws, p.control).await;
    let url = format!(
        "{}/api/guilds/{}/members/{}/roles/{}",
        fx.server.url, fx.guild, fx.member, p.role
    );
    fenced_at_commit(
        fx.member,
        spawn_request(url, fx.owner_token.clone(), reqwest::Method::DELETE, None),
        assert_cut_off_before_control(&fx, &mut ws, p.private, p.control),
    )
    .await;
}

#[sqlx::test]
async fn override_fences_at_commit(pool: PgPool) {
    let fx = fixture(&pool).await;
    let channel = create_channel(&pool, fx.guild, "general").await;
    let control = create_channel(&pool, fx.guild, "control").await;
    let everyone = everyone_role(&pool, fx.guild).await;
    vc_server::ws::test_hooks::disable_access_wake(fx.member);
    let mut ws = connect_ready(&fx.server, &fx.member_token).await;
    subscribe(&mut ws, channel).await;
    subscribe(&mut ws, control).await;
    let url = format!(
        "{}/api/channels/{channel}/overrides/{everyone}",
        fx.server.url
    );
    fenced_at_commit(
        fx.guild,
        spawn_request(
            url,
            fx.owner_token.clone(),
            reqwest::Method::PUT,
            Some(json!({ "allow": 0, "deny": GuildPermissions::VIEW_CHANNEL.bits() })),
        ),
        assert_cut_off_before_control(&fx, &mut ws, channel, control),
    )
    .await;
}

#[sqlx::test]
async fn role_permission_edit_fences_at_commit(pool: PgPool) {
    let fx = fixture(&pool).await;
    let control = create_channel(&pool, fx.guild, "control").await;
    let channel = create_channel(&pool, fx.guild, "general").await;
    let everyone = everyone_role(&pool, fx.guild).await;
    // An override keeps the control channel visible when `@everyone` loses
    // VIEW_CHANNEL.
    set_override(
        &pool,
        control,
        everyone,
        GuildPermissions::VIEW_CHANNEL,
        GuildPermissions::empty(),
    )
    .await;
    vc_server::ws::test_hooks::disable_access_wake(fx.member);
    let mut ws = connect_ready(&fx.server, &fx.member_token).await;
    subscribe(&mut ws, channel).await;
    subscribe(&mut ws, control).await;
    let url = format!("{}/api/guilds/{}/roles/{everyone}", fx.server.url, fx.guild);
    fenced_at_commit(
        fx.guild,
        spawn_request(
            url,
            fx.owner_token.clone(),
            reqwest::Method::PATCH,
            Some(json!({ "permissions": GuildPermissions::SEND_MESSAGES.bits() })),
        ),
        assert_cut_off_before_control(&fx, &mut ws, channel, control),
    )
    .await;
}

#[sqlx::test]
async fn role_delete_fences_at_commit(pool: PgPool) {
    let fx = fixture(&pool).await;
    let p = private_channel(&pool, &fx).await;
    vc_server::ws::test_hooks::disable_access_wake(fx.member);
    let mut ws = connect_ready(&fx.server, &fx.member_token).await;
    subscribe(&mut ws, p.private).await;
    subscribe(&mut ws, p.control).await;
    let url = format!("{}/api/guilds/{}/roles/{}", fx.server.url, fx.guild, p.role);
    fenced_at_commit(
        fx.guild,
        spawn_request(url, fx.owner_token.clone(), reqwest::Method::DELETE, None),
        assert_cut_off_before_control(&fx, &mut ws, p.private, p.control),
    )
    .await;
}

/// The member's own reaction removal takes a reaction role (and the private
/// channel it granted) away.
#[sqlx::test]
async fn reaction_role_removal_fences_at_commit(pool: PgPool) {
    let fx = fixture(&pool).await;
    let p = private_channel(&pool, &fx).await;
    sqlx::query("DELETE FROM guild_member_roles WHERE user_id = $1")
        .bind(fx.member)
        .execute(&pool)
        .await
        .unwrap();
    let everyone = everyone_role(&pool, fx.guild).await;
    sqlx::query("UPDATE guild_roles SET permissions = $2 WHERE id = $1")
        .bind(everyone)
        .bind((member_perms() | GuildPermissions::ADD_REACTIONS).to_db())
        .execute(&pool)
        .await
        .unwrap();
    let msg = super::helpers::insert_message(&pool, p.control, fx.owner, "pick a role").await;
    sqlx::query(
        "INSERT INTO reaction_role_bindings (guild_id, channel_id, message_id, emoji, role_id, mode)
         VALUES ($1, $2, $3, '🎨', $4, 'toggle')",
    )
    .bind(fx.guild)
    .bind(p.control)
    .bind(msg)
    .bind(p.role)
    .execute(&pool)
    .await
    .unwrap();
    let reactions = format!("/api/channels/{}/messages/{msg}/reactions", p.control);
    request_as(
        &fx,
        &fx.member_token,
        reqwest::Method::PUT,
        &reactions,
        Some(json!({ "emoji": "🎨" })),
    )
    .await;

    vc_server::ws::test_hooks::disable_access_wake(fx.member);
    let mut ws = connect_ready(&fx.server, &fx.member_token).await;
    subscribe(&mut ws, p.private).await;
    subscribe(&mut ws, p.control).await;
    assert_receives(&fx, &mut ws, p.private).await;
    let url = format!("{}{reactions}/🎨", fx.server.url);
    fenced_at_commit(
        fx.member,
        spawn_request(url, fx.member_token.clone(), reqwest::Method::DELETE, None),
        assert_cut_off_before_control(&fx, &mut ws, p.private, p.control),
    )
    .await;
}

/// An event of the channel is already queued for the socket (its write is
/// held) when the channel is deleted: released while the delete request is
/// paused right after its bump, it is not written.
#[sqlx::test]
async fn channel_delete_fences_at_commit(pool: PgPool) {
    let fx = fixture(&pool).await;
    let channel = create_channel(&pool, fx.guild, "doomed").await;
    vc_server::ws::test_hooks::disable_access_wake(fx.member);
    let mut ws = connect_ready(&fx.server, &fx.member_token).await;
    subscribe(&mut ws, channel).await;
    assert_receives(&fx, &mut ws, channel).await;

    let held = vc_server::ws::test_hooks::pause_before_channel_write(fx.member);
    post_message(&fx, channel, "queued").await;
    held.reached.notified().await;
    let url = format!("{}/api/channels/{channel}", fx.server.url);
    fenced_at_commit(
        fx.guild,
        spawn_request(url, fx.owner_token.clone(), reqwest::Method::DELETE, None),
        async {
            held.release.notify_one();
            assert_none_of_type(&until_pong(&mut ws).await, "message_new");
        },
    )
    .await;
}

/// Server deletion: a queued channel event and an event of the server's own
/// feed, both released while the delete request is paused right after its
/// bump (before presence cleanup and voice), are not written.
#[sqlx::test]
async fn guild_delete_fences_before_presence_cleanup(pool: PgPool) {
    let fx = fixture(&pool).await;
    let channel = create_channel(&pool, fx.guild, "general").await;
    vc_server::ws::test_hooks::disable_access_wake(fx.member);
    let mut ws = connect_ready(&fx.server, &fx.member_token).await;
    subscribe(&mut ws, channel).await;
    assert_receives(&fx, &mut ws, channel).await;
    assert_feed_reaches(&fx, &mut ws).await;

    let held = vc_server::ws::test_hooks::pause_before_channel_write(fx.member);
    post_message(&fx, channel, "queued").await;
    held.reached.notified().await;
    let url = format!("{}/api/guilds/{}", fx.server.url, fx.guild);
    fenced_at_commit(
        fx.guild,
        spawn_request(url, fx.owner_token.clone(), reqwest::Method::DELETE, None),
        async {
            held.release.notify_one();
            assert_none_of_type(&until_pong(&mut ws).await, "message_new");
            assert_feed_cut_off(&fx, &mut ws).await;
        },
    )
    .await;
}

/// An event of the server's own feed (`guild:{id}`), as any member gets it.
fn feed_event(guild: Uuid) -> ServerEvent {
    ServerEvent::GuildEmojiUpdated {
        guild_id: guild,
        emojis: Vec::new(),
    }
}

async fn assert_feed_reaches(fx: &Fixture, ws: &mut WsClient) {
    vc_server::ws::broadcast_to_guild(&fx.app.state.redis, fx.guild, &feed_event(fx.guild))
        .await
        .expect("publish");
    next_ws_event(ws, "guild_emoji_updated", EVENT_TIMEOUT).await;
}

/// Publish a feed event; once the socket is about to write it, let it decide
/// and check that nothing of it was written.
async fn assert_feed_cut_off(fx: &Fixture, ws: &mut WsClient) {
    let held = vc_server::ws::test_hooks::pause_before_feed_write(fx.member);
    vc_server::ws::broadcast_to_guild(&fx.app.state.redis, fx.guild, &feed_event(fx.guild))
        .await
        .expect("publish");
    tokio::time::timeout(EVENT_TIMEOUT, held.reached.notified())
        .await
        .expect("the feed event did not reach the socket");
    held.release.notify_one();
    assert_none_of_type(&until_pong(ws).await, "guild_emoji_updated");
}

/// A kicked member stops getting the server's own feed at the commit, not
/// when the presence audience unsubscribes.
#[sqlx::test]
async fn kick_fences_the_server_feed_at_commit(pool: PgPool) {
    let fx = fixture(&pool).await;
    vc_server::ws::test_hooks::disable_access_wake(fx.member);
    let mut ws = connect_ready(&fx.server, &fx.member_token).await;
    assert_feed_reaches(&fx, &mut ws).await;
    let url = format!(
        "{}/api/guilds/{}/members/{}",
        fx.server.url, fx.guild, fx.member
    );
    fenced_at_commit(
        fx.member,
        spawn_request(url, fx.owner_token.clone(), reqwest::Method::DELETE, None),
        assert_feed_cut_off(&fx, &mut ws),
    )
    .await;
}

/// A ban: nothing more is written to the banned account's sockets from the
/// commit on (its sessions end with the ban; the sockets close afterwards).
#[sqlx::test]
async fn ban_fences_at_commit(pool: PgPool) {
    let fx = fixture(&pool).await;
    let channel = create_channel(&pool, fx.guild, "general").await;
    let (admin, _) = create_test_user(&pool).await;
    make_admin(&pool, admin).await;
    let admin_token = create_elevated_session(&pool, &fx.app.config, admin).await;
    vc_server::ws::test_hooks::disable_access_wake(fx.member);
    let mut ws = connect_ready(&fx.server, &fx.member_token).await;
    subscribe(&mut ws, channel).await;
    assert_receives(&fx, &mut ws, channel).await;

    let held = vc_server::ws::test_hooks::pause_before_channel_write(fx.member);
    post_message(&fx, channel, "queued").await;
    held.reached.notified().await;
    let url = format!("{}/api/admin/users/{}/ban", fx.server.url, fx.member);
    fenced_at_commit(
        fx.member,
        spawn_request(
            url,
            admin_token,
            reqwest::Method::POST,
            Some(json!({ "reason": "test" })),
        ),
        async {
            held.release.notify_one();
            assert_none_of_type(&until_pong(&mut ws).await, "message_new");
        },
    )
    .await;
}

#[sqlx::test]
async fn group_dm_leave_fences_at_commit(pool: PgPool) {
    let fx = fixture(&pool).await;
    let dm = dm_control(&pool, &fx).await;
    let control = create_channel(&pool, fx.guild, "control").await;
    vc_server::ws::test_hooks::disable_access_wake(fx.member);
    let mut ws = connect_ready(&fx.server, &fx.member_token).await;
    subscribe(&mut ws, dm).await;
    subscribe(&mut ws, control).await;
    let url = format!("{}/api/dm/{dm}/leave", fx.server.url);
    fenced_at_commit(
        fx.member,
        spawn_request(url, fx.member_token.clone(), reqwest::Method::POST, None),
        assert_cut_off_before_control(&fx, &mut ws, dm, control),
    )
    .await;
}

/// De-elevation: an admin event published while the request is paused right
/// after its bump (before the cache is updated) is not written.
#[sqlx::test]
async fn de_elevation_fences_before_cache_update(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let server = spawn_test_server(app.router.clone()).await;
    let (admin, _) = create_test_user(&pool).await;
    make_admin(&pool, admin).await;
    let token = create_elevated_session(&pool, &app.config, admin).await;
    vc_server::ws::test_hooks::disable_access_wake(admin);
    let mut ws = connect_ready(&server, &token).await;
    send(&mut ws, json!({ "type": "admin_subscribe" })).await;
    // The subscribe has no ack: the ping after it is answered once it ran.
    until_pong(&mut ws).await;
    assert!(admin_event_arrives(&app, &mut ws, admin).await);

    let url = format!("{}/api/admin/elevate", server.url);
    fenced_at_commit(
        admin,
        spawn_request(url, token.clone(), reqwest::Method::DELETE, None),
        async {
            assert!(
                !admin_event_arrives(&app, &mut ws, admin).await,
                "admin event written after the de-elevation committed"
            );
        },
    )
    .await;
}

/// Item 2: a channel deleted while one of its events is queued: the
/// revalidation finds it gone (not "allowed by default" once its overrides
/// cascaded away), and the event is not written.
#[sqlx::test]
async fn deleted_channel_is_not_allowed(pool: PgPool) {
    let fx = fixture(&pool).await;
    let channel = create_channel(&pool, fx.guild, "doomed").await;
    let control = create_channel(&pool, fx.guild, "control").await;
    let mut ws = connect_ready(&fx.server, &fx.member_token).await;
    subscribe(&mut ws, channel).await;
    subscribe(&mut ws, control).await;

    let pause = vc_server::ws::test_hooks::pause_before_channel_write(fx.member);
    post_message(&fx, channel, "queued").await;
    pause.reached.notified().await;
    owner_request(
        &fx,
        reqwest::Method::DELETE,
        &format!("/api/channels/{channel}"),
        None,
    )
    .await;
    pause.release.notify_one();
    post_message(&fx, control, "control").await;
    let ev = next_message(&mut ws).await;
    assert_eq!(
        ev["message"]["content"], "control",
        "deleted channel's event written: {ev}"
    );
}

/// Item 3: voice room content queued for a peer that is removed before the
/// frame is written is not written; `voice_removed` still is.
#[sqlx::test]
async fn queued_voice_content_is_not_written_after_removal(pool: PgPool) {
    let fx = fixture(&pool).await;
    let voice = create_voice_channel(&pool, fx.guild, "voice").await;
    let mut member_ws = connect_ready(&fx.server, &fx.member_token).await;
    join_voice(&mut member_ws, voice).await;

    let pause = vc_server::ws::test_hooks::pause_before_channel_write(fx.member);
    let mut owner_ws = connect_ready(&fx.server, &fx.owner_token).await;
    // The member's socket gets `voice_user_joined` for the owner: held.
    send(
        &mut owner_ws,
        json!({ "type": "voice_join", "channel_id": voice }),
    )
    .await;
    pause.reached.notified().await;
    owner_request(
        &fx,
        reqwest::Method::DELETE,
        &format!("/api/guilds/{}/members/{}", fx.guild, fx.member),
        None,
    )
    .await;
    pause.release.notify_one();

    let removed = voice_removed_without(&mut member_ws, "voice_user_joined").await;
    assert_eq!(removed["reason"], "kicked");
}

/// Read until `voice_removed`; no event of type `forbidden` may come first.
async fn voice_removed_without(ws: &mut WsClient, forbidden: &str) -> Value {
    use futures::StreamExt;
    let wait = async {
        loop {
            match ws.next().await {
                Some(Ok(Message::Text(text))) => {
                    let v: Value = serde_json::from_str(&text).unwrap();
                    assert_ne!(v["type"], forbidden, "room content on revoked access");
                    if v["type"] == "voice_removed" {
                        return v;
                    }
                }
                Some(Ok(_)) => {}
                other => panic!("socket ended: {other:?}"),
            }
        }
    };
    tokio::time::timeout(EVENT_TIMEOUT, wait)
        .await
        .expect("voice_removed")
}

/// Invariant B, the window itself: the kick has committed and bumped, the
/// request is paused there — the peer is **still in the room** (nothing has
/// removed it yet). Room content that was queued before, and room content
/// produced at that moment, is not written: the socket's authorization for
/// the room is checked against the epochs when each frame is written, not
/// only whether it still owns the peer.
#[sqlx::test]
async fn voice_content_is_fenced_while_the_kick_is_still_cleaning_up(pool: PgPool) {
    let fx = fixture(&pool).await;
    let voice = create_voice_channel(&pool, fx.guild, "voice").await;
    vc_server::ws::test_hooks::disable_access_wake(fx.member);
    let mut member_ws = connect_ready(&fx.server, &fx.member_token).await;
    join_voice(&mut member_ws, voice).await;
    let mut owner_ws = connect_ready(&fx.server, &fx.owner_token).await;

    // Queued before the kick: `voice_user_joined` for the owner, held.
    let held = vc_server::ws::test_hooks::pause_before_channel_write(fx.member);
    send(
        &mut owner_ws,
        json!({ "type": "voice_join", "channel_id": voice }),
    )
    .await;
    held.reached.notified().await;
    next_ws_event(&mut owner_ws, "voice_room_state", EVENT_TIMEOUT).await;

    let url = format!(
        "{}/api/guilds/{}/members/{}",
        fx.server.url, fx.guild, fx.member
    );
    fenced_at_commit(
        fx.member,
        spawn_request(url, fx.owner_token.clone(), reqwest::Method::DELETE, None),
        async {
            assert!(
                in_room(&fx, voice, fx.member).await,
                "the window under test: bumped, peer not removed yet"
            );
            held.release.notify_one();
            assert_none_of_type(&until_pong(&mut member_ws).await, "voice_user_joined");
            // Produced now, while the request is still paused: the owner
            // mutes; the broadcast goes to every peer in the room.
            if in_room(&fx, voice, fx.member).await {
                let held = vc_server::ws::test_hooks::pause_before_channel_write(fx.member);
                send(
                    &mut owner_ws,
                    json!({ "type": "voice_mute", "channel_id": voice }),
                )
                .await;
                let reached =
                    tokio::time::timeout(Duration::from_millis(200), held.reached.notified()).await;
                if reached.is_ok() {
                    held.release.notify_one();
                }
            }
            assert_none_of_type(&until_pong(&mut member_ws).await, "voice_user_muted");
        },
    )
    .await;

    // The member is told once (by the request, or by its own socket if that
    // was faster) and is out of the room.
    let removed = voice_removed_without(&mut member_ws, "voice_user_muted").await;
    assert!(
        removed["reason"] == "kicked" || removed["reason"] == "permission_lost",
        "{removed}"
    );
    assert!(!in_room(&fx, voice, fx.member).await);
}

/// Item 6: a refused `subscribe` is remembered; when access is granted
/// later, the channel's events arrive without a new `subscribe`.
#[sqlx::test]
async fn refused_subscribe_comes_back_with_access(pool: PgPool) {
    let fx = fixture(&pool).await;
    let p = private_channel(&pool, &fx).await;
    remove_role(&fx, p.role).await;
    let mut ws = connect_ready(&fx.server, &fx.member_token).await;
    send(
        &mut ws,
        json!({ "type": "subscribe", "channel_id": p.private }),
    )
    .await;
    let err = next_ws_event(&mut ws, "error", EVENT_TIMEOUT).await;
    assert_eq!(err["code"], "FORBIDDEN");

    grant_role(&fx, p.role).await;
    post_message(&fx, p.private, "now visible").await;
    assert_eq!(
        next_message(&mut ws).await["message"]["content"],
        "now visible"
    );
}

/// Invariant D: a **forced** revalidation (the backstop) fails although no
/// epoch moved. From that failure until a revalidation succeeds the scopes
/// are failing: the message published meanwhile is dropped, the drop is
/// recorded, and after the recovery the socket closes with 1011 `RESYNC` so
/// the client catches up.
#[sqlx::test]
async fn failed_backstop_without_a_change_asks_for_resync(pool: PgPool) {
    let mut config = Config::default_for_test();
    config.session_recheck_secs = 1;
    let fx = fixture_with_config(&pool, config).await;
    let channel = create_channel(&pool, fx.guild, "general").await;
    let mut ws = connect_ready(&fx.server, &fx.member_token).await;
    subscribe(&mut ws, channel).await;
    assert_receives(&fx, &mut ws, channel).await;

    // The next backstop read and its first two retries fail (250 ms, 500 ms
    // later); the third retry (1 s after that) succeeds.
    vc_server::ws::test_hooks::count_access_reads(fx.member);
    vc_server::ws::test_hooks::fail_access_reads(fx.member, 3);
    let deadline = tokio::time::Instant::now() + EVENT_TIMEOUT;
    while vc_server::ws::test_hooks::access_reads(fx.member).0 < 2 {
        assert!(tokio::time::Instant::now() < deadline, "no backstop");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    // Failing since the first read, for another second at least.
    post_message(&fx, channel, "while failing").await;

    let (events, code, reason) = until_close(&mut ws).await;
    assert_none_of_type(&events, "message_new");
    assert_eq!((code, reason.as_str()), (1011, "RESYNC"));
}

/// The shutdown clock starts at the signal and its first action reaches the
/// sockets: a voice departure that stalls, a socket parked in a slow
/// operation, a socket still in its setup and an HTTP request whose handler
/// does not return delay neither the close frames (1012) nor the return of
/// `serve`; the handler still running near the bound is cancelled (503).
#[sqlx::test]
async fn shutdown_is_bounded_from_the_signal(pool: PgPool) {
    let fx = fixture(&pool).await;
    let channel = create_channel(&pool, fx.guild, "general").await;
    let voice = create_voice_channel(&pool, fx.guild, "voice").await;
    let (other, _) = create_test_user(&pool).await;
    add_guild_member(&pool, fx.guild, other).await;

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (signal_tx, signal_rx) = tokio::sync::oneshot::channel::<()>();
    let drain = vc_server::shutdown::SocketDrain::from_state(&fx.app.state);
    let serving = tokio::spawn(vc_server::shutdown::serve(
        listener,
        fx.app.router.clone(),
        drain,
        async move {
            let _ = signal_rx.await;
        },
    ));
    let server = TestServer::for_addr(addr);

    // A socket in a voice room whose departure will stall.
    let mut in_voice = connect_ready(&server, &fx.owner_token).await;
    join_voice(&mut in_voice, voice).await;
    let stalled_leave = vc_server::ws::test_hooks::pause_voice_leave(fx.owner);
    // A socket parked inside a `subscribe` (after its permission check).
    let mut parked = connect_ready(&server, &fx.member_token).await;
    let park = vc_server::ws::test_hooks::pause_subscribe_after_check(fx.member);
    send(
        &mut parked,
        json!({ "type": "subscribe", "channel_id": channel }),
    )
    .await;
    park.reached.notified().await;
    // A socket that is still being set up (stopped between its reads and
    // its subscriptions).
    let (late, _) = create_test_user(&pool).await;
    let setup = vc_server::ws::test_hooks::pause_after_audience_read(late);
    let mut in_setup = ws_connect(&server, &generate_access_token(&fx.app.config, late)).await;
    setup.reached.notified().await;
    // An HTTP request held right after its commit.
    let hold = vc_server::ws::test_hooks::pause_after_access_bump(other);
    let owner_token = fx.owner_token.clone();
    let kick = format!("http://{addr}/api/guilds/{}/members/{other}", fx.guild);
    let held = tokio::spawn(async move {
        reqwest::Client::new()
            .delete(kick)
            .bearer_auth(owner_token)
            .send()
            .await
            .map(|response| response.status().as_u16())
    });
    hold.reached.notified().await;

    let signalled = tokio::time::Instant::now();
    signal_tx.send(()).unwrap();
    for ws in [&mut parked, &mut in_voice, &mut in_setup] {
        let (_, code, _) = until_close(ws).await;
        assert_eq!(code, 1012);
        assert!(
            signalled.elapsed() < Duration::from_secs(2),
            "close took {:?}",
            signalled.elapsed()
        );
    }
    let bound = vc_server::shutdown::SHUTDOWN_DEADLINE;
    let served = tokio::time::timeout(bound + Duration::from_millis(500), serving)
        .await
        .expect("serve did not return within its bound");
    served.expect("serve task").expect("serve");
    assert!(
        signalled.elapsed() <= bound + Duration::from_millis(500),
        "serve returned after {:?}",
        signalled.elapsed()
    );
    // The held handler was cancelled, not left running: its request ended
    // without anyone releasing it.
    let status = tokio::time::timeout(Duration::from_secs(1), held)
        .await
        .expect("the held request is still running after the bound")
        .expect("request task");
    assert!(
        !matches!(status, Ok(204)),
        "the held handler ran to its end: {status:?}"
    );
    stalled_leave.release.notify_one();
    hold.release.notify_one();
    park.release.notify_one();
    setup.release.notify_one();
}
