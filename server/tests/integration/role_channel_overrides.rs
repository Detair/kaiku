//! Channel overrides resolve in a fixed order: guild base permissions, then the
//! `@everyone` override, then the aggregated role overrides (all role denies,
//! then all role allows). A role allow therefore makes a channel hidden from
//! `@everyone` visible to that role, and a role deny hides a channel that the
//! `@everyone` override opens. Every visibility path (channel list, message
//! fetch, WebSocket subscribe, unread summary, search) follows that order.

use axum::body::Body;
use axum::http::{Method, StatusCode};
use futures::SinkExt;
use serde_json::{json, Value};
use sqlx::PgPool;
use tokio_tungstenite::tungstenite::Message;
use uuid::Uuid;
use vc_server::permissions::{require_channel_access, GuildPermissions};

use super::helpers::{
    add_guild_member, body_to_json, create_channel, create_guild_with_default_role,
    create_test_user, generate_access_token, insert_message, next_ws_event, spawn_test_server,
    ws_connect, TestApp,
};

const EVENT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);

fn member_perms() -> GuildPermissions {
    GuildPermissions::VIEW_CHANNEL | GuildPermissions::SEND_MESSAGES
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

async fn get(app: &TestApp, token: &str, uri: &str) -> (StatusCode, Value) {
    let req = TestApp::request(Method::GET, uri)
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await;
    let status = resp.status();
    let body = if status == StatusCode::OK {
        body_to_json(resp).await
    } else {
        Value::Null
    };
    (status, body)
}

async fn listed_channels(app: &TestApp, token: &str, guild: Uuid) -> Vec<String> {
    let (status, list) = get(app, token, &format!("/api/guilds/{guild}/channels")).await;
    assert_eq!(status, StatusCode::OK);
    list.as_array()
        .unwrap()
        .iter()
        .map(|c| c["id"].as_str().unwrap().to_string())
        .collect()
}

fn unread_channels(aggregate: &Value) -> Vec<String> {
    aggregate["guilds"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|g| g["channels"].as_array().unwrap().iter())
        .map(|c| c["channel_id"].as_str().unwrap().to_string())
        .collect()
}

async fn search_hits(app: &TestApp, token: &str, query: &str) -> u64 {
    let (status, body) = get(app, token, &format!("/api/search?q={query}")).await;
    assert_eq!(status, StatusCode::OK);
    body["total"].as_u64().unwrap()
}

async fn ws_subscribe_result(
    server: &super::helpers::TestServer,
    token: &str,
    channel: Uuid,
) -> String {
    let mut ws = ws_connect(server, token).await;
    next_ws_event(&mut ws, "ready", EVENT_TIMEOUT).await;
    let frame = json!({ "type": "subscribe", "channel_id": channel }).to_string();
    ws.send(Message::Text(frame.into())).await.unwrap();
    // Either "subscribed" or an "error" frame answers the request.
    let wait = async {
        use futures::StreamExt;
        loop {
            match ws.next().await {
                Some(Ok(Message::Text(text))) => {
                    let v: Value = serde_json::from_str(&text).unwrap();
                    match v["type"].as_str() {
                        Some("subscribed") => return "subscribed".to_string(),
                        Some("error") => return format!("error:{}", v["code"].as_str().unwrap()),
                        _ => {}
                    }
                }
                Some(Ok(_)) => {}
                other => panic!("socket ended before subscribe answer: {other:?}"),
            }
        }
    };
    tokio::time::timeout(EVENT_TIMEOUT, wait)
        .await
        .expect("subscribe answer")
}

/// A channel hidden from `@everyone` and opened for one role is visible to the
/// role's members on every path and stays hidden from everyone else. A channel
/// hidden from `@everyone` without any role allow stays hidden from both.
#[sqlx::test]
async fn private_channel_is_visible_to_role_members_only(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let pool = &app.pool;
    let (owner, _) = create_test_user(pool).await;
    let (staff, _) = create_test_user(pool).await;
    let (plain, _) = create_test_user(pool).await;
    let guild = create_guild_with_default_role(pool, owner, member_perms()).await;
    add_guild_member(pool, guild, staff).await;
    add_guild_member(pool, guild, plain).await;
    let everyone = everyone_role(pool, guild).await;
    let staff_role = create_role(pool, guild, "staff").await;
    assign_role(pool, guild, staff, staff_role).await;

    let private = create_channel(pool, guild, "staff-room").await;
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
        staff_role,
        GuildPermissions::VIEW_CHANNEL,
        GuildPermissions::empty(),
    )
    .await;

    let locked = create_channel(pool, guild, "locked").await;
    set_override(
        pool,
        locked,
        everyone,
        GuildPermissions::empty(),
        GuildPermissions::VIEW_CHANNEL,
    )
    .await;

    insert_message(pool, private, owner, "quokkaprivate note").await;
    insert_message(pool, locked, owner, "quokkalocked note").await;

    let staff_token = generate_access_token(&app.config, staff);
    let plain_token = generate_access_token(&app.config, plain);
    let private_id = private.to_string();
    let locked_id = locked.to_string();

    // REST channel list.
    let staff_list = listed_channels(&app, &staff_token, guild).await;
    assert!(
        staff_list.contains(&private_id),
        "role member lists private channel"
    );
    assert!(
        !staff_list.contains(&locked_id),
        "@everyone deny without role allow stays hidden"
    );
    let plain_list = listed_channels(&app, &plain_token, guild).await;
    assert!(!plain_list.contains(&private_id));
    assert!(!plain_list.contains(&locked_id));

    // Message fetch.
    let uri = format!("/api/messages/channel/{private}");
    let (status, page) = get(&app, &staff_token, &uri).await;
    assert_eq!(status, StatusCode::OK);
    assert!(page.to_string().contains("quokkaprivate"), "{page}");
    assert_eq!(get(&app, &plain_token, &uri).await.0, StatusCode::FORBIDDEN);
    let uri = format!("/api/messages/channel/{locked}");
    assert_eq!(get(&app, &staff_token, &uri).await.0, StatusCode::FORBIDDEN);

    // Unread summary.
    let (_, staff_unread) = get(&app, &staff_token, "/api/me/unread").await;
    assert!(
        unread_channels(&staff_unread).contains(&private_id),
        "{staff_unread}"
    );
    assert!(
        !unread_channels(&staff_unread).contains(&locked_id),
        "{staff_unread}"
    );
    let (_, plain_unread) = get(&app, &plain_token, "/api/me/unread").await;
    assert!(
        !unread_channels(&plain_unread).contains(&private_id),
        "{plain_unread}"
    );

    // Global search.
    assert_eq!(search_hits(&app, &staff_token, "quokkaprivate").await, 1);
    assert_eq!(search_hits(&app, &plain_token, "quokkaprivate").await, 0);
    assert_eq!(search_hits(&app, &staff_token, "quokkalocked").await, 0);

    // WebSocket subscribe.
    let server = spawn_test_server(app.router.clone()).await;
    assert_eq!(
        ws_subscribe_result(&server, &staff_token, private).await,
        "subscribed"
    );
    assert_eq!(
        ws_subscribe_result(&server, &plain_token, private).await,
        "error:FORBIDDEN"
    );
    assert_eq!(
        ws_subscribe_result(&server, &staff_token, locked).await,
        "error:FORBIDDEN"
    );
}

/// A role deny hides a channel even when the `@everyone` override allows it.
#[sqlx::test]
async fn role_deny_beats_everyone_override_allow(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let pool = &app.pool;
    let (owner, _) = create_test_user(pool).await;
    let (muted, _) = create_test_user(pool).await;
    let (plain, _) = create_test_user(pool).await;
    // @everyone has no VIEW_CHANNEL at guild level; the channel override opens it.
    let guild = create_guild_with_default_role(pool, owner, GuildPermissions::SEND_MESSAGES).await;
    add_guild_member(pool, guild, muted).await;
    add_guild_member(pool, guild, plain).await;
    let everyone = everyone_role(pool, guild).await;
    let muted_role = create_role(pool, guild, "muted").await;
    assign_role(pool, guild, muted, muted_role).await;

    let lobby = create_channel(pool, guild, "lobby").await;
    set_override(
        pool,
        lobby,
        everyone,
        GuildPermissions::VIEW_CHANNEL,
        GuildPermissions::empty(),
    )
    .await;
    set_override(
        pool,
        lobby,
        muted_role,
        GuildPermissions::empty(),
        GuildPermissions::VIEW_CHANNEL,
    )
    .await;

    assert!(require_channel_access(pool, plain, lobby).await.is_ok());
    assert!(require_channel_access(pool, muted, lobby).await.is_err());

    let muted_token = generate_access_token(&app.config, muted);
    let lobby_id = lobby.to_string();
    assert!(!listed_channels(&app, &muted_token, guild)
        .await
        .contains(&lobby_id));
}

/// The `@everyone` override can never grant a bit outside the `@everyone`
/// allow list, also when it is applied in its own step.
#[sqlx::test]
async fn everyone_override_cannot_allow_forbidden_bits(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let pool = &app.pool;
    let (owner, _) = create_test_user(pool).await;
    let (member, _) = create_test_user(pool).await;
    let guild = create_guild_with_default_role(pool, owner, member_perms()).await;
    add_guild_member(pool, guild, member).await;
    let everyone = everyone_role(pool, guild).await;
    let channel = create_channel(pool, guild, "general").await;
    // Written directly, bypassing the API validation that refuses these bits.
    set_override(
        pool,
        channel,
        everyone,
        GuildPermissions::MANAGE_MESSAGES
            | GuildPermissions::MANAGE_WEBHOOKS
            | GuildPermissions::ATTACH_FILES,
        GuildPermissions::empty(),
    )
    .await;

    let ctx = require_channel_access(pool, member, channel).await.unwrap();
    assert!(ctx.has_permission(GuildPermissions::ATTACH_FILES));
    assert!(!ctx.has_permission(GuildPermissions::MANAGE_MESSAGES));
    assert!(!ctx.has_permission(GuildPermissions::MANAGE_WEBHOOKS));
}

/// Role overrides are aggregated before they apply: when one of a member's
/// roles allows a bit and another denies it, the allow wins.
#[sqlx::test]
async fn role_allow_wins_over_other_role_deny(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let pool = &app.pool;
    let (owner, _) = create_test_user(pool).await;
    let (member, _) = create_test_user(pool).await;
    let guild = create_guild_with_default_role(pool, owner, member_perms()).await;
    add_guild_member(pool, guild, member).await;
    let allow_role = create_role(pool, guild, "allow").await;
    let deny_role = create_role(pool, guild, "deny").await;
    assign_role(pool, guild, member, allow_role).await;
    assign_role(pool, guild, member, deny_role).await;
    let channel = create_channel(pool, guild, "mixed").await;
    set_override(
        pool,
        channel,
        allow_role,
        GuildPermissions::VIEW_CHANNEL,
        GuildPermissions::empty(),
    )
    .await;
    set_override(
        pool,
        channel,
        deny_role,
        GuildPermissions::empty(),
        GuildPermissions::VIEW_CHANNEL,
    )
    .await;

    assert!(require_channel_access(pool, member, channel).await.is_ok());
}

/// A member who has not accepted the server rules yet gets no channel access,
/// even when the `@everyone` override of a channel allows it. All three
/// resolution paths agree; the owner still sees the channel.
#[sqlx::test]
async fn pending_member_ignores_everyone_override_allow(pool: PgPool) {
    use vc_server::permissions::{filter_accessible_channels, filter_viewable_channels_in_guilds};

    // Screening must be enforced for the pending member to be held back.
    let app = TestApp::with_all_features(pool.clone()).await;
    let pool = &app.pool;
    let (owner, _) = create_test_user(pool).await;
    let (pending, _) = create_test_user(pool).await;
    let guild = create_guild_with_default_role(pool, owner, GuildPermissions::empty()).await;
    sqlx::query("UPDATE guilds SET screening_enabled = true WHERE id = $1")
        .bind(guild)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO guild_members (guild_id, user_id, membership_state) VALUES ($1, $2, 'pending')",
    )
    .bind(guild)
    .bind(pending)
    .execute(pool)
    .await
    .unwrap();
    let everyone = everyone_role(pool, guild).await;
    let channel = create_channel(pool, guild, "welcome").await;
    set_override(
        pool,
        channel,
        everyone,
        GuildPermissions::VIEW_CHANNEL | GuildPermissions::SEND_MESSAGES,
        GuildPermissions::empty(),
    )
    .await;

    assert!(require_channel_access(pool, pending, channel)
        .await
        .is_err());
    let accessible = filter_accessible_channels(pool, guild, pending, &[channel])
        .await
        .unwrap();
    assert!(accessible.is_empty(), "{accessible:?}");
    let viewable = filter_viewable_channels_in_guilds(pool, pending, &[(guild, channel)])
        .await
        .unwrap();
    assert!(viewable.is_empty(), "{viewable:?}");

    let token = generate_access_token(&app.config, pending);
    let uri = format!("/api/messages/channel/{channel}");
    assert_eq!(get(&app, &token, &uri).await.0, StatusCode::FORBIDDEN);

    // Owner bypass is unaffected.
    assert!(require_channel_access(pool, owner, channel).await.is_ok());
    assert!(
        filter_viewable_channels_in_guilds(pool, owner, &[(guild, channel)])
            .await
            .unwrap()
            .contains(&channel)
    );
}
