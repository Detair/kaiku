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
    let event = ServerEvent::AdminUserUnbanned {
        user_id: Uuid::new_v4(),
        username: "someone".into(),
    };
    // The subscribe has no ack: publish until the first event arrives.
    let deadline = tokio::time::Instant::now() + EVENT_TIMEOUT;
    loop {
        vc_server::ws::broadcast_admin_event(&app.state.redis, &event)
            .await
            .expect("publish");
        let got = tokio::time::timeout(
            Duration::from_millis(300),
            next_ws_event(&mut ws, "admin_user_unbanned", EVENT_TIMEOUT),
        )
        .await;
        if got.is_ok() {
            break;
        }
        assert!(tokio::time::Instant::now() < deadline, "no admin event");
    }

    sqlx::query("DELETE FROM system_admins WHERE user_id = $1")
        .bind(admin)
        .execute(&pool)
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(2500)).await;
    vc_server::ws::broadcast_admin_event(&app.state.redis, &event)
        .await
        .expect("publish");
    expect_no_ws_event(&mut ws, "admin_user_unbanned", QUIET).await;
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
