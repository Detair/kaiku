//! Unread counts only cover messages the user can see and has not written.
//!
//! Every unread path (`GET /api/me/unread`, the guild channel list and the DM
//! list) skips the user's own messages, thread replies and deleted messages;
//! a channel without a read cursor counts from when the user joined; and the
//! aggregate never mentions a channel the user may not view.

use axum::body::Body;
use axum::http::{Method, StatusCode};
use serde_json::{json, Value};
use sqlx::PgPool;
use uuid::Uuid;
use vc_server::permissions::GuildPermissions;

use super::helpers::{
    add_guild_member, body_to_json, create_channel, create_dm_channel, create_guild,
    create_guild_with_default_role, create_test_user, create_voice_channel, generate_access_token,
    insert_deleted_message, insert_message, insert_message_at, next_ws_event, spawn_test_server,
    ws_connect, TestApp, WsClient,
};

/// Upper bound for awaited WebSocket events (the full suite runs many servers).
const EVENT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);

fn member_perms() -> GuildPermissions {
    GuildPermissions::VIEW_CHANNEL | GuildPermissions::SEND_MESSAGES
}

async fn get_json(app: &TestApp, token: &str, uri: &str) -> Value {
    let req = TestApp::request(Method::GET, uri)
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), StatusCode::OK, "GET {uri}");
    body_to_json(resp).await
}

async fn post_json(app: &TestApp, token: &str, uri: &str, body: &Value) -> StatusCode {
    let req = TestApp::request(Method::POST, uri)
        .header("Authorization", format!("Bearer {token}"))
        .header("Content-Type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();
    app.oneshot(req).await.status()
}

/// Unread count of `channel_id` in `GET /api/me/unread` (0 when absent).
fn aggregate_count(aggregate: &Value, channel_id: Uuid) -> i64 {
    let id = channel_id.to_string();
    aggregate["guilds"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|g| g["channels"].as_array().unwrap().iter())
        .chain(aggregate["dms"].as_array().unwrap().iter())
        .find(|c| c["channel_id"] == id.as_str())
        .map_or(0, |c| c["unread_count"].as_i64().unwrap())
}

/// Unread count of `channel_id` in `GET /api/guilds/{id}/channels`.
async fn channel_list_count(app: &TestApp, token: &str, guild_id: Uuid, channel_id: Uuid) -> i64 {
    let list = get_json(app, token, &format!("/api/guilds/{guild_id}/channels")).await;
    let id = channel_id.to_string();
    list.as_array()
        .unwrap()
        .iter()
        .find(|c| c["id"] == id.as_str())
        .expect("channel listed")["unread_count"]
        .as_i64()
        .unwrap()
}

/// Unread count of `channel_id` in `GET /api/dm`.
async fn dm_list_count(app: &TestApp, token: &str, channel_id: Uuid) -> i64 {
    let list = get_json(app, token, "/api/dm").await;
    let id = channel_id.to_string();
    list.as_array()
        .unwrap()
        .iter()
        .find(|d| d["id"] == id.as_str() || d["channel"]["id"] == id.as_str())
        .expect("dm listed")["unread_count"]
        .as_i64()
        .unwrap()
}

/// Guild with an owner, a viewing member (returned with token) and one text channel.
async fn guild_setup(pool: &PgPool) -> (TestApp, Uuid, Uuid, Uuid, Uuid, String) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (owner, _) = create_test_user(&app.pool).await;
    let (member, _) = create_test_user(&app.pool).await;
    let guild = create_guild_with_default_role(&app.pool, owner, member_perms()).await;
    add_guild_member(&app.pool, guild, member).await;
    let channel = create_channel(&app.pool, guild, "general").await;
    let token = generate_access_token(&app.config, member);
    (app, owner, member, guild, channel, token)
}

async fn insert_thread_reply(pool: &PgPool, channel_id: Uuid, user_id: Uuid, parent: Uuid) {
    sqlx::query(
        "INSERT INTO messages (id, channel_id, user_id, content, parent_id) VALUES ($1, $2, $3, 'reply', $4)",
    )
    .bind(Uuid::now_v7())
    .bind(channel_id)
    .bind(user_id)
    .bind(parent)
    .execute(pool)
    .await
    .expect("insert thread reply");
}

#[sqlx::test]
async fn test_own_messages_are_not_unread(pool: PgPool) {
    let (app, owner, member, guild, channel, token) = guild_setup(&pool).await;
    insert_message(&app.pool, channel, member, "mine 1").await;
    insert_message(&app.pool, channel, owner, "theirs").await;
    insert_message(&app.pool, channel, member, "mine 2").await;

    let aggregate = get_json(&app, &token, "/api/me/unread").await;
    assert_eq!(aggregate_count(&aggregate, channel), 1);
    assert_eq!(aggregate["total"], 1);
    assert_eq!(channel_list_count(&app, &token, guild, channel).await, 1);
}

#[sqlx::test]
async fn test_thread_replies_not_counted_in_channel(pool: PgPool) {
    let (app, owner, _member, guild, channel, token) = guild_setup(&pool).await;
    let parent = insert_message(&app.pool, channel, owner, "thread start").await;
    insert_thread_reply(&app.pool, channel, owner, parent).await;
    insert_thread_reply(&app.pool, channel, owner, parent).await;

    assert_eq!(channel_list_count(&app, &token, guild, channel).await, 1);
}

#[sqlx::test]
async fn test_aggregate_excludes_thread_replies(pool: PgPool) {
    let (app, owner, _member, _guild, channel, token) = guild_setup(&pool).await;
    let parent = insert_message(&app.pool, channel, owner, "thread start").await;
    insert_thread_reply(&app.pool, channel, owner, parent).await;
    insert_thread_reply(&app.pool, channel, owner, parent).await;

    let aggregate = get_json(&app, &token, "/api/me/unread").await;
    assert_eq!(aggregate_count(&aggregate, channel), 1);
    assert_eq!(aggregate["total"], 1);
}

#[sqlx::test]
async fn test_deleted_messages_not_counted(pool: PgPool) {
    let (app, owner, member, guild, channel, token) = guild_setup(&pool).await;
    insert_message(&app.pool, channel, owner, "kept").await;
    insert_deleted_message(&app.pool, channel, owner, "gone").await;

    let aggregate = get_json(&app, &token, "/api/me/unread").await;
    assert_eq!(aggregate_count(&aggregate, channel), 1);
    assert_eq!(channel_list_count(&app, &token, guild, channel).await, 1);

    // DMs as well.
    let dm = create_dm_channel(&app.pool, member, owner).await;
    insert_message(&app.pool, dm, owner, "kept").await;
    insert_deleted_message(&app.pool, dm, owner, "gone").await;
    assert_eq!(dm_list_count(&app, &token, dm).await, 1);
    let aggregate = get_json(&app, &token, "/api/me/unread").await;
    assert_eq!(aggregate_count(&aggregate, dm), 1);
}

#[sqlx::test]
async fn test_aggregate_hides_channels_without_view_permission(pool: PgPool) {
    let (app, owner, _member, guild, visible, token) = guild_setup(&pool).await;
    let secret_name = format!("secret-{}", &Uuid::new_v4().to_string()[..8]);
    let secret = create_channel(&app.pool, guild, &secret_name).await;
    let everyone_role: Uuid =
        sqlx::query_scalar("SELECT id FROM guild_roles WHERE guild_id = $1 AND is_default")
            .bind(guild)
            .fetch_one(&app.pool)
            .await
            .unwrap();
    sqlx::query(
        "INSERT INTO channel_overrides (id, channel_id, role_id, allow_permissions, deny_permissions)
         VALUES ($1, $2, $3, 0, $4)",
    )
    .bind(Uuid::now_v7())
    .bind(secret)
    .bind(everyone_role)
    .bind(GuildPermissions::VIEW_CHANNEL.to_db())
    .execute(&app.pool)
    .await
    .unwrap();

    insert_message(&app.pool, visible, owner, "hello").await;
    insert_message(&app.pool, secret, owner, "hidden 1").await;
    insert_message(&app.pool, secret, owner, "hidden 2").await;

    let aggregate = get_json(&app, &token, "/api/me/unread").await;
    let raw = aggregate.to_string();
    assert!(
        !raw.contains(&secret_name),
        "hidden channel name leaked: {raw}"
    );
    assert!(
        !raw.contains(&secret.to_string()),
        "hidden channel listed: {raw}"
    );
    assert_eq!(aggregate_count(&aggregate, visible), 1);
    assert_eq!(aggregate["guilds"][0]["total_unread"], 1);
    assert_eq!(aggregate["total"], 1);

    // A guild whose only unread channel is hidden is not listed at all.
    sqlx::query("DELETE FROM messages WHERE channel_id = $1")
        .bind(visible)
        .execute(&app.pool)
        .await
        .unwrap();
    let aggregate = get_json(&app, &token, "/api/me/unread").await;
    assert_eq!(aggregate["guilds"], json!([]));
    assert_eq!(aggregate["total"], 0);
}

#[sqlx::test]
async fn test_new_member_starts_with_zero_unread(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (owner, _) = create_test_user(&app.pool).await;
    let (newcomer, _) = create_test_user(&app.pool).await;
    let guild = create_guild_with_default_role(&app.pool, owner, member_perms()).await;
    let channel = create_channel(&app.pool, guild, "general").await;
    insert_message_at(&app.pool, channel, owner, "old 1", "2020-01-01T00:00:00Z").await;
    insert_message_at(&app.pool, channel, owner, "old 2", "2020-01-02T00:00:00Z").await;

    // Joined without a read cursor: history before the join is not unread.
    add_guild_member(&app.pool, guild, newcomer).await;
    let token = generate_access_token(&app.config, newcomer);

    let aggregate = get_json(&app, &token, "/api/me/unread").await;
    assert_eq!(aggregate["total"], 0, "{aggregate}");
    assert_eq!(channel_list_count(&app, &token, guild, channel).await, 0);

    // Messages after the join are.
    insert_message(&app.pool, channel, owner, "new").await;
    let aggregate = get_json(&app, &token, "/api/me/unread").await;
    assert_eq!(aggregate_count(&aggregate, channel), 1);
    assert_eq!(channel_list_count(&app, &token, guild, channel).await, 1);
}

#[sqlx::test]
async fn test_dm_own_message_not_unread(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (alice, _) = create_test_user(&app.pool).await;
    let (bob, _) = create_test_user(&app.pool).await;
    let dm = create_dm_channel(&app.pool, alice, bob).await;
    insert_message(&app.pool, dm, alice, "from alice").await;
    insert_message(&app.pool, dm, bob, "from bob").await;
    insert_message(&app.pool, dm, alice, "from alice again").await;

    let token = generate_access_token(&app.config, alice);
    assert_eq!(dm_list_count(&app, &token, dm).await, 1);
    let aggregate = get_json(&app, &token, "/api/me/unread").await;
    assert_eq!(aggregate_count(&aggregate, dm), 1);
}

/// Forum and announcement channels are read-tracked like text channels:
/// `mark_as_read` and "mark all read" both clear them, and the channel list
/// reports their unread count.
#[sqlx::test]
async fn test_text_like_channels_are_read_tracked(pool: PgPool) {
    let (app, owner, _member, guild, _text, token) = guild_setup(&pool).await;
    for kind in ["forum", "announcement"] {
        let channel = Uuid::now_v7();
        sqlx::query(
            "INSERT INTO channels (id, guild_id, name, channel_type) VALUES ($1, $2, $3, $4::channel_type)",
        )
        .bind(channel)
        .bind(guild)
        .bind(kind)
        .bind(kind)
        .execute(&app.pool)
        .await
        .unwrap();
        let first = insert_message(&app.pool, channel, owner, "one").await;
        insert_message(&app.pool, channel, owner, "two").await;

        assert_eq!(
            channel_list_count(&app, &token, guild, channel).await,
            2,
            "{kind}"
        );
        let status = post_json(
            &app,
            &token,
            &format!("/api/channels/{channel}/read"),
            &json!({ "last_read_message_id": first }),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{kind}");
        assert_eq!(
            channel_list_count(&app, &token, guild, channel).await,
            1,
            "{kind}"
        );

        assert_eq!(
            post_json(&app, &token, "/api/me/read-all", &json!({})).await,
            StatusCode::NO_CONTENT
        );
        let aggregate = get_json(&app, &token, "/api/me/unread").await;
        assert_eq!(aggregate_count(&aggregate, channel), 0, "{kind}");
    }
}

/// A channel the user cannot view cannot be marked read.
#[sqlx::test]
async fn test_mark_as_read_requires_view_permission(pool: PgPool) {
    let (app, owner, _member, guild, _visible, token) = guild_setup(&pool).await;
    let secret = create_channel(&app.pool, guild, "secret").await;
    let everyone_role: Uuid =
        sqlx::query_scalar("SELECT id FROM guild_roles WHERE guild_id = $1 AND is_default")
            .bind(guild)
            .fetch_one(&app.pool)
            .await
            .unwrap();
    sqlx::query(
        "INSERT INTO channel_overrides (id, channel_id, role_id, allow_permissions, deny_permissions)
         VALUES ($1, $2, $3, 0, $4)",
    )
    .bind(Uuid::now_v7())
    .bind(secret)
    .bind(everyone_role)
    .bind(GuildPermissions::VIEW_CHANNEL.to_db())
    .execute(&app.pool)
    .await
    .unwrap();
    let msg = insert_message(&app.pool, secret, owner, "hidden").await;

    let status = post_json(
        &app,
        &token,
        &format!("/api/channels/{secret}/read"),
        &json!({ "last_read_message_id": msg }),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

// ============================================================================
// Fix round: hidden channels in read-all, reply cursors, permission parity
// ============================================================================

/// Deny `VIEW_CHANNEL` to `@everyone` on `channel`.
async fn hide_from_everyone(pool: &PgPool, guild: Uuid, channel: Uuid) {
    let everyone_role: Uuid =
        sqlx::query_scalar("SELECT id FROM guild_roles WHERE guild_id = $1 AND is_default")
            .bind(guild)
            .fetch_one(pool)
            .await
            .unwrap();
    sqlx::query(
        "INSERT INTO channel_overrides (id, channel_id, role_id, allow_permissions, deny_permissions)
         VALUES ($1, $2, $3, 0, $4)",
    )
    .bind(Uuid::now_v7())
    .bind(channel)
    .bind(everyone_role)
    .bind(GuildPermissions::VIEW_CHANNEL.to_db())
    .execute(pool)
    .await
    .unwrap();
}

/// Channels with a read cursor row for `user`.
async fn cursor_channels(pool: &PgPool, user: Uuid) -> Vec<Uuid> {
    sqlx::query_scalar("SELECT channel_id FROM channel_read_state WHERE user_id = $1")
        .bind(user)
        .fetch_all(pool)
        .await
        .unwrap()
}

/// Collect the `channel_read` channel ids the socket receives until the
/// sentinel event (published after the request returned) arrives. Events to
/// one user are delivered in order, so everything the request sent comes first.
async fn channel_read_events_until_sentinel(
    ws: &mut WsClient,
    app: &TestApp,
    user: Uuid,
) -> Vec<String> {
    use futures::StreamExt;
    use tokio_tungstenite::tungstenite::Message;

    let sentinel = Uuid::now_v7();
    vc_server::ws::broadcast_to_user(
        &app.state.redis,
        user,
        &vc_server::ws::ServerEvent::ChannelRead {
            channel_id: sentinel,
            last_read_message_id: None,
        },
    )
    .await
    .expect("publish sentinel");

    let collect = async {
        let mut ids = Vec::new();
        loop {
            match ws.next().await {
                Some(Ok(Message::Text(text))) => {
                    let v: Value = serde_json::from_str(&text).unwrap();
                    if v["type"] == "channel_read" {
                        let id = v["channel_id"].as_str().unwrap().to_string();
                        if id == sentinel.to_string() {
                            return ids;
                        }
                        ids.push(id);
                    }
                }
                Some(Ok(_)) => {}
                other => panic!("socket ended while collecting events: {other:?}"),
            }
        }
    };
    tokio::time::timeout(EVENT_TIMEOUT, collect)
        .await
        .expect("sentinel channel_read arrives")
}

#[sqlx::test]
async fn test_read_all_skips_hidden_channels(pool: PgPool) {
    let (app, owner, member, guild, visible, token) = guild_setup(&pool).await;
    let secret = create_channel(&app.pool, guild, "secret").await;
    hide_from_everyone(&app.pool, guild, secret).await;
    insert_message(&app.pool, visible, owner, "hello").await;
    insert_message(&app.pool, secret, owner, "hidden").await;

    let server = spawn_test_server(app.router.clone()).await;
    let mut ws = ws_connect(&server, &token).await;
    next_ws_event(&mut ws, "ready", EVENT_TIMEOUT).await;

    for uri in [
        "/api/me/read-all".to_string(),
        format!("/api/guilds/{guild}/read-all"),
    ] {
        sqlx::query("DELETE FROM channel_read_state WHERE user_id = $1")
            .bind(member)
            .execute(&app.pool)
            .await
            .unwrap();

        let status = post_json(&app, &token, &uri, &json!({})).await;
        assert_eq!(status, StatusCode::NO_CONTENT, "{uri}");

        assert_eq!(
            cursor_channels(&app.pool, member).await,
            vec![visible],
            "{uri}"
        );
        let events = channel_read_events_until_sentinel(&mut ws, &app, member).await;
        assert_eq!(events, vec![visible.to_string()], "{uri}");
    }
}

#[sqlx::test]
async fn test_join_setup_skips_hidden_channels(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (owner, _) = create_test_user(&app.pool).await;
    let (newcomer, _) = create_test_user(&app.pool).await;
    let guild = create_guild_with_default_role(&app.pool, owner, member_perms()).await;
    let visible = create_channel(&app.pool, guild, "general").await;
    let secret = create_channel(&app.pool, guild, "secret").await;
    hide_from_everyone(&app.pool, guild, secret).await;
    let last = insert_message(&app.pool, visible, owner, "hello").await;
    add_guild_member(&app.pool, guild, newcomer).await;

    vc_server::guild::queries::core::initialize_channel_read_state(&app.pool, guild, newcomer)
        .await
        .expect("initialize read state");

    assert_eq!(cursor_channels(&app.pool, newcomer).await, vec![visible]);
    let cursor: Option<Uuid> = sqlx::query_scalar(
        "SELECT last_read_message_id FROM channel_read_state WHERE user_id = $1",
    )
    .bind(newcomer)
    .fetch_one(&app.pool)
    .await
    .unwrap();
    assert_eq!(cursor, Some(last));
}

/// `20261112000000_cursor_to_top_level.sql` moves cursors that point at a
/// thread reply to the channel's top-level feed without changing unread
/// counts, and is safe to run again.
#[sqlx::test]
async fn test_cursor_migration_moves_reply_cursors_to_top_level(pool: PgPool) {
    const MIGRATION: &str = include_str!("../../migrations/20261112000000_cursor_to_top_level.sql");

    let (app, owner, member, guild, channel, token) = guild_setup(&pool).await;
    let parent = insert_message(&app.pool, channel, owner, "thread start").await;
    let reply = insert_reply_returning(&app.pool, channel, owner, parent).await;

    // A channel whose only top-level message is deleted: the cursor is cleared.
    let lonely = create_channel(&app.pool, guild, "lonely").await;
    let gone = insert_deleted_message(&app.pool, lonely, owner, "gone").await;
    let lonely_reply = insert_reply_returning(&app.pool, lonely, owner, gone).await;

    // A DM with a reply cursor.
    let dm = create_dm_channel(&app.pool, member, owner).await;
    let dm_parent = insert_message(&app.pool, dm, owner, "dm start").await;
    let dm_reply = insert_reply_returning(&app.pool, dm, owner, dm_parent).await;

    for (sql, ch, cursor) in [
        (
            "INSERT INTO channel_read_state (user_id, channel_id, last_read_at, last_read_message_id) VALUES ($1, $2, NOW(), $3)",
            channel,
            reply,
        ),
        (
            "INSERT INTO channel_read_state (user_id, channel_id, last_read_at, last_read_message_id) VALUES ($1, $2, NOW(), $3)",
            lonely,
            lonely_reply,
        ),
        (
            "INSERT INTO dm_read_state (user_id, channel_id, last_read_at, last_read_message_id) VALUES ($1, $2, NOW(), $3)",
            dm,
            dm_reply,
        ),
    ] {
        sqlx::query(sql)
            .bind(member)
            .bind(ch)
            .bind(cursor)
            .execute(&app.pool)
            .await
            .unwrap();
    }

    for _ in 0..2 {
        sqlx::raw_sql(MIGRATION)
            .execute(&app.pool)
            .await
            .expect("migration SQL");
    }

    let list = get_json(&app, &token, &format!("/api/guilds/{guild}/channels")).await;
    let entry = |id: Uuid| {
        list.as_array()
            .unwrap()
            .iter()
            .find(|c| c["id"] == id.to_string().as_str())
            .cloned()
            .expect("channel listed")
    };
    let main = entry(channel);
    assert_eq!(main["last_read_message_id"], parent.to_string().as_str());
    assert_eq!(main["last_read_message_id"], main["last_message_id"]);
    assert_eq!(main["unread_count"], 0);
    let lonely_entry = entry(lonely);
    assert!(lonely_entry["last_read_message_id"].is_null());
    assert!(lonely_entry["last_message_id"].is_null());
    assert_eq!(lonely_entry["unread_count"], 0);

    let dm_cursor: Option<Uuid> = sqlx::query_scalar(
        "SELECT last_read_message_id FROM dm_read_state WHERE user_id = $1 AND channel_id = $2",
    )
    .bind(member)
    .bind(dm)
    .fetch_one(&app.pool)
    .await
    .unwrap();
    assert_eq!(dm_cursor, Some(dm_parent));
    assert_eq!(dm_list_count(&app, &token, dm).await, 0);

    let aggregate = get_json(&app, &token, "/api/me/unread").await;
    assert_eq!(aggregate["total"], 0, "{aggregate}");
}

async fn insert_reply_returning(
    pool: &PgPool,
    channel_id: Uuid,
    user_id: Uuid,
    parent: Uuid,
) -> Uuid {
    let id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO messages (id, channel_id, user_id, content, parent_id) VALUES ($1, $2, $3, 'reply', $4)",
    )
    .bind(id)
    .bind(channel_id)
    .bind(user_id)
    .bind(parent)
    .execute(pool)
    .await
    .expect("insert thread reply");
    id
}

/// The batched visibility filter used by the unread summary and read-all
/// decides exactly like `require_channel_access`.
#[sqlx::test]
async fn test_viewable_filter_matches_require_channel_access(pool: PgPool) {
    use vc_server::permissions::{filter_viewable_channels_in_guilds, require_channel_access};

    // Screening must be enforced for the pending fixture to stay hidden.
    let app = TestApp::with_all_features(pool.clone()).await;
    let pool = &app.pool;
    let (user, _) = create_test_user(pool).await;
    let (other, _) = create_test_user(pool).await;

    // Owned guild: sees even a channel hidden from @everyone.
    let owned = create_guild_with_default_role(pool, user, GuildPermissions::empty()).await;
    let owned_hidden = create_channel(pool, owned, "owned-hidden").await;
    hide_from_everyone(pool, owned, owned_hidden).await;

    // Member guild with role overrides.
    let member_guild = create_guild_with_default_role(pool, other, member_perms()).await;
    add_guild_member(pool, member_guild, user).await;
    let open = create_channel(pool, member_guild, "open").await;
    let denied = create_channel(pool, member_guild, "denied").await;
    hide_from_everyone(pool, member_guild, denied).await;
    let role_allowed = create_channel(pool, member_guild, "role-allowed").await;
    hide_from_everyone(pool, member_guild, role_allowed).await;
    let role = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO guild_roles (id, guild_id, name, permissions, position) VALUES ($1, $2, 'mods', 0, 1)",
    )
    .bind(role)
    .bind(member_guild)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query("INSERT INTO guild_member_roles (guild_id, user_id, role_id) VALUES ($1, $2, $3)")
        .bind(member_guild)
        .bind(user)
        .bind(role)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO channel_overrides (id, channel_id, role_id, allow_permissions, deny_permissions)
         VALUES ($1, $2, $3, $4, 0)",
    )
    .bind(Uuid::now_v7())
    .bind(role_allowed)
    .bind(role)
    .bind(GuildPermissions::VIEW_CHANNEL.to_db())
    .execute(pool)
    .await
    .unwrap();
    let role_only = create_channel(pool, member_guild, "role-only").await;
    sqlx::query(
        "INSERT INTO channel_overrides (id, channel_id, role_id, allow_permissions, deny_permissions)
         VALUES ($1, $2, $3, $4, 0)",
    )
    .bind(Uuid::now_v7())
    .bind(role_only)
    .bind(role)
    .bind(GuildPermissions::VIEW_CHANNEL.to_db())
    .execute(pool)
    .await
    .unwrap();

    // Screening guild where the user has not accepted the rules yet.
    let screened = create_guild_with_default_role(pool, other, member_perms()).await;
    sqlx::query("UPDATE guilds SET screening_enabled = true WHERE id = $1")
        .bind(screened)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO guild_members (guild_id, user_id, membership_state) VALUES ($1, $2, 'pending')",
    )
    .bind(screened)
    .bind(user)
    .execute(pool)
    .await
    .unwrap();
    let screened_channel = create_channel(pool, screened, "rules-first").await;

    // Guild the user is not a member of.
    let foreign = create_guild(pool, other).await;
    let foreign_channel = create_channel(pool, foreign, "foreign").await;

    let pairs = vec![
        (owned, owned_hidden),
        (member_guild, open),
        (member_guild, denied),
        (member_guild, role_allowed),
        (member_guild, role_only),
        (screened, screened_channel),
        (foreign, foreign_channel),
    ];
    let batched = filter_viewable_channels_in_guilds(pool, user, &pairs)
        .await
        .expect("batched filter");

    let mut expected = Vec::new();
    for (_, channel) in &pairs {
        let single = require_channel_access(pool, user, *channel).await.is_ok();
        assert_eq!(batched.contains(channel), single, "channel {channel}");
        expected.push(single);
    }
    // The fixtures exercise both outcomes: owner bypass, plain member access,
    // role-only allow; @everyone deny, pending screening, non-member. (The
    // role allow vs @everyone deny case is only checked for parity.)
    for (index, want) in [
        (0, true),
        (1, true),
        (4, true),
        (2, false),
        (5, false),
        (6, false),
    ] {
        assert_eq!(expected[index], want, "fixture {index}");
    }
}

#[sqlx::test]
async fn test_voice_channels_have_no_read_state(pool: PgPool) {
    let (app, owner, _member, guild, _text, token) = guild_setup(&pool).await;
    let voice = create_voice_channel(&app.pool, guild, "voice").await;
    let msg = insert_message(&app.pool, voice, owner, "stray").await;

    let aggregate = get_json(&app, &token, "/api/me/unread").await;
    assert!(
        !aggregate.to_string().contains(&voice.to_string()),
        "voice channel in unread summary: {aggregate}"
    );

    let status = post_json(
        &app,
        &token,
        &format!("/api/channels/{voice}/read"),
        &json!({ "last_read_message_id": msg }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[sqlx::test]
async fn test_mark_as_read_requires_top_level_message_of_channel(pool: PgPool) {
    let (app, owner, member, guild, channel, token) = guild_setup(&pool).await;
    let other_channel = create_channel(&app.pool, guild, "other").await;
    let top = insert_message(&app.pool, channel, owner, "top").await;
    let reply = insert_reply_returning(&app.pool, channel, owner, top).await;
    let elsewhere = insert_message(&app.pool, other_channel, owner, "elsewhere").await;

    let mark = |target: Uuid, msg: Uuid| {
        let app = &app;
        let token = &token;
        async move {
            post_json(
                app,
                token,
                &format!("/api/channels/{target}/read"),
                &json!({ "last_read_message_id": msg }),
            )
            .await
        }
    };
    assert_eq!(mark(channel, reply).await, StatusCode::BAD_REQUEST);
    assert_eq!(mark(channel, elsewhere).await, StatusCode::BAD_REQUEST);
    assert_eq!(mark(channel, Uuid::now_v7()).await, StatusCode::BAD_REQUEST);
    assert_eq!(mark(channel, top).await, StatusCode::OK);

    // DMs follow the same rule.
    let dm = create_dm_channel(&app.pool, member, owner).await;
    let dm_top = insert_message(&app.pool, dm, owner, "dm top").await;
    let dm_reply = insert_reply_returning(&app.pool, dm, owner, dm_top).await;
    let dm_status = |msg: Uuid| {
        let app = &app;
        let token = &token;
        async move {
            post_json(
                app,
                token,
                &format!("/api/dm/{dm}/read"),
                &json!({ "last_read_message_id": msg }),
            )
            .await
        }
    };
    assert_eq!(dm_status(dm_reply).await, StatusCode::BAD_REQUEST);
    assert_eq!(dm_status(dm_top).await, StatusCode::OK);
}
