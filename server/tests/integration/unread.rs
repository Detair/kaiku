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
    add_guild_member, body_to_json, create_channel, create_dm_channel,
    create_guild_with_default_role, create_test_user, generate_access_token,
    insert_deleted_message, insert_message, insert_message_at, TestApp,
};

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
