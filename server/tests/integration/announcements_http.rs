//! Integration tests for announcement channels (publish gate + follow/crosspost).
//! Run: `cargo test --test integration announcements_http`

use std::time::Duration;

use axum::body::Body;
use axum::http::{Method, StatusCode};
use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;
use vc_server::permissions::GuildPermissions;

use crate::helpers::{
    add_guild_member, body_to_json, create_channel, create_guild_with_default_role,
    create_test_user, generate_access_token, TestApp,
};

async fn create_announcement_channel(pool: &PgPool, guild_id: Uuid) -> Uuid {
    let id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO channels (id, guild_id, name, channel_type) VALUES ($1, $2, 'news', 'announcement')",
    )
    .bind(id)
    .bind(guild_id)
    .execute(pool)
    .await
    .expect("create announcement channel");
    id
}

fn post_msg(token: &str, channel: Uuid, content: &str) -> axum::http::Request<Body> {
    TestApp::request(Method::POST, &format!("/api/messages/channel/{channel}"))
        .header("Authorization", format!("Bearer {token}"))
        .header("Content-Type", "application/json")
        .body(Body::from(
            json!({ "content": content, "encrypted": false }).to_string(),
        ))
        .unwrap()
}

#[sqlx::test]
async fn member_without_send_announcements_is_blocked(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (owner, _) = create_test_user(&pool).await;
    // @everyone can view + send normal messages, but NOT announce.
    let guild = create_guild_with_default_role(
        &pool,
        owner,
        GuildPermissions::SEND_MESSAGES | GuildPermissions::VIEW_CHANNEL,
    )
    .await;
    let channel = create_announcement_channel(&pool, guild).await;
    let (member, _) = create_test_user(&pool).await;
    add_guild_member(&pool, guild, member).await;
    let token = generate_access_token(&app.config, member);

    let res = app.oneshot(post_msg(&token, channel, "hi")).await;
    assert_eq!(res.status(), StatusCode::FORBIDDEN);
}

#[sqlx::test]
async fn member_with_send_announcements_can_publish(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (owner, _) = create_test_user(&pool).await;
    let guild = create_guild_with_default_role(
        &pool,
        owner,
        GuildPermissions::SEND_MESSAGES
            | GuildPermissions::VIEW_CHANNEL
            | GuildPermissions::SEND_ANNOUNCEMENTS,
    )
    .await;
    let channel = create_announcement_channel(&pool, guild).await;
    let (member, _) = create_test_user(&pool).await;
    add_guild_member(&pool, guild, member).await;
    let token = generate_access_token(&app.config, member);

    let res = app
        .oneshot(post_msg(&token, channel, "announcement!"))
        .await;
    assert_eq!(res.status(), StatusCode::CREATED);
}

#[sqlx::test]
async fn follow_requires_announcement_source(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (owner, _) = create_test_user(&pool).await;
    let guild = create_guild_with_default_role(
        &pool,
        owner,
        GuildPermissions::MANAGE_CHANNELS | GuildPermissions::VIEW_CHANNEL,
    )
    .await;
    // Source is a plain text channel, not announcement.
    let text_source = create_channel(&pool, guild, "not-news").await;
    let target = create_channel(&pool, guild, "inbox").await;
    let token = generate_access_token(&app.config, owner);

    let req = TestApp::request(Method::POST, &format!("/api/channels/{target}/follow"))
        .header("Authorization", format!("Bearer {token}"))
        .header("Content-Type", "application/json")
        .body(Body::from(
            json!({ "source_channel_id": text_source }).to_string(),
        ))
        .unwrap();
    let res = app.oneshot(req).await;
    assert_eq!(res.status(), StatusCode::BAD_REQUEST); // NOT_ANNOUNCEMENT
}

#[sqlx::test]
async fn follow_then_crosspost_delivers_to_target(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (owner, _) = create_test_user(&pool).await;
    let guild = create_guild_with_default_role(
        &pool,
        owner,
        GuildPermissions::MANAGE_CHANNELS
            | GuildPermissions::VIEW_CHANNEL
            | GuildPermissions::SEND_ANNOUNCEMENTS
            | GuildPermissions::SEND_MESSAGES,
    )
    .await;
    let source = create_announcement_channel(&pool, guild).await;
    let target = create_channel(&pool, guild, "inbox").await;
    let token = generate_access_token(&app.config, owner);

    // Follow.
    let follow = TestApp::request(Method::POST, &format!("/api/channels/{target}/follow"))
        .header("Authorization", format!("Bearer {token}"))
        .header("Content-Type", "application/json")
        .body(Body::from(
            json!({ "source_channel_id": source }).to_string(),
        ))
        .unwrap();
    let fres = app.oneshot(follow).await;
    assert_eq!(fres.status(), StatusCode::CREATED);
    let follow_id = body_to_json(fres).await["id"].as_str().unwrap().to_string();

    // Publish in the source.
    let pub_res = app.oneshot(post_msg(&token, source, "big news")).await;
    assert_eq!(pub_res.status(), StatusCode::CREATED);

    // The fan-out is spawned; give it a moment, then assert a crosspost landed
    // in the target and is flagged (loop guard).
    tokio::time::sleep(Duration::from_millis(300)).await;
    let crosspost: Option<(bool,)> = sqlx::query_as(
        "SELECT is_crosspost FROM messages WHERE channel_id = $1 AND content = 'big news'",
    )
    .bind(target)
    .fetch_optional(&pool)
    .await
    .unwrap();
    assert_eq!(crosspost, Some((true,)), "crosspost delivered + flagged");

    // Followers list shows the follow; unfollow removes it.
    let followers = app
        .oneshot(
            TestApp::request(Method::GET, &format!("/api/channels/{source}/followers"))
                .header("Authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(body_to_json(followers).await.as_array().unwrap().len(), 1);

    let unfollow = app
        .oneshot(
            TestApp::request(
                Method::DELETE,
                &format!("/api/channels/{target}/follow/{follow_id}"),
            )
            .header("Authorization", format!("Bearer {token}"))
            .body(Body::empty())
            .unwrap(),
        )
        .await;
    assert_eq!(unfollow.status(), StatusCode::OK);
}
