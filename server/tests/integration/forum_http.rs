//! Integration tests for forum channels.
//! Run: `cargo test --test integration forum_http`

use axum::body::Body;
use axum::http::{Method, StatusCode};
use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;
use vc_server::permissions::GuildPermissions;

use crate::helpers::{
    add_guild_member, body_to_json, create_guild_with_default_role, create_test_user,
    generate_access_token, TestApp,
};

/// Create a forum channel directly and return its id.
async fn create_forum_channel(pool: &PgPool, guild_id: Uuid) -> Uuid {
    let id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO channels (id, guild_id, name, channel_type) VALUES ($1, $2, 'forum', 'forum')",
    )
    .bind(id)
    .bind(guild_id)
    .execute(pool)
    .await
    .expect("create forum channel");
    id
}

async fn create_tag(pool: &PgPool, channel_id: Uuid, name: &str) -> Uuid {
    let id = Uuid::now_v7();
    sqlx::query("INSERT INTO forum_tags (id, channel_id, name) VALUES ($1, $2, $3)")
        .bind(id)
        .bind(channel_id)
        .bind(name)
        .execute(pool)
        .await
        .expect("create tag");
    id
}

fn req(
    method: Method,
    uri: &str,
    token: &str,
    body: Option<serde_json::Value>,
) -> axum::http::Request<Body> {
    let b = TestApp::request(method, uri).header("Authorization", format!("Bearer {token}"));
    match body {
        Some(j) => b
            .header("Content-Type", "application/json")
            .body(Body::from(j.to_string()))
            .unwrap(),
        None => b.body(Body::empty()).unwrap(),
    }
}

async fn setup(pool: &PgPool, perms: GuildPermissions) -> (TestApp, Uuid, Uuid, String) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (owner, _) = create_test_user(pool).await;
    let guild =
        create_guild_with_default_role(pool, owner, perms | GuildPermissions::VIEW_CHANNEL).await;
    let channel = create_forum_channel(pool, guild).await;
    let token = generate_access_token(&app.config, owner);
    (app, guild, channel, token)
}

#[sqlx::test]
async fn create_and_list_forum_post(pool: PgPool) {
    let (app, _guild, channel, token) = setup(&pool, GuildPermissions::SEND_MESSAGES).await;

    let create = req(
        Method::POST,
        &format!("/api/channels/{channel}/posts"),
        &token,
        Some(json!({ "title": "Hello Forum", "content": "first post body" })),
    );
    let res = app.oneshot(create).await;
    assert_eq!(res.status(), StatusCode::CREATED);
    let j = body_to_json(res).await;
    assert_eq!(j["title"], "Hello Forum");
    assert_eq!(j["reply_count"], 0);

    let list = req(
        Method::GET,
        &format!("/api/channels/{channel}/posts"),
        &token,
        None,
    );
    let res = app.oneshot(list).await;
    assert_eq!(res.status(), StatusCode::OK);
    let arr = body_to_json(res).await;
    assert_eq!(arr.as_array().unwrap().len(), 1);
    assert_eq!(arr[0]["title"], "Hello Forum");
}

#[sqlx::test]
async fn post_rejected_on_non_forum_channel(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (owner, _) = create_test_user(&pool).await;
    let guild = create_guild_with_default_role(
        &pool,
        owner,
        GuildPermissions::SEND_MESSAGES | GuildPermissions::VIEW_CHANNEL,
    )
    .await;
    // A plain text channel, not forum.
    let text = Uuid::now_v7();
    sqlx::query("INSERT INTO channels (id, guild_id, name, channel_type) VALUES ($1, $2, 'general', 'text')")
        .bind(text)
        .bind(guild)
        .execute(&pool)
        .await
        .unwrap();
    let token = generate_access_token(&app.config, owner);

    let res = app
        .oneshot(req(
            Method::POST,
            &format!("/api/channels/{text}/posts"),
            &token,
            Some(json!({ "title": "x", "content": "y" })),
        ))
        .await;
    assert_eq!(res.status(), StatusCode::BAD_REQUEST); // NOT_FORUM
}

#[sqlx::test]
async fn tag_filter_lists_matching_posts(pool: PgPool) {
    let (app, _guild, channel, token) = setup(&pool, GuildPermissions::SEND_MESSAGES).await;
    let tag = create_tag(&pool, channel, "bug").await;

    // Post with the tag.
    app.oneshot(req(
        Method::POST,
        &format!("/api/channels/{channel}/posts"),
        &token,
        Some(json!({ "title": "tagged", "content": "b", "tag_ids": [tag] })),
    ))
    .await;
    // Post without the tag.
    app.oneshot(req(
        Method::POST,
        &format!("/api/channels/{channel}/posts"),
        &token,
        Some(json!({ "title": "untagged", "content": "b" })),
    ))
    .await;

    let res = app
        .oneshot(req(
            Method::GET,
            &format!("/api/channels/{channel}/posts?tag={tag}"),
            &token,
            None,
        ))
        .await;
    let arr = body_to_json(res).await;
    assert_eq!(arr.as_array().unwrap().len(), 1);
    assert_eq!(arr[0]["title"], "tagged");
}

#[sqlx::test]
async fn non_manager_cannot_pin(pool: PgPool) {
    // Owner creates a post; a regular member (no MANAGE_POSTS) tries to pin it.
    let (app, guild, channel, owner_token) = setup(&pool, GuildPermissions::SEND_MESSAGES).await;
    let create = app
        .oneshot(req(
            Method::POST,
            &format!("/api/channels/{channel}/posts"),
            &owner_token,
            Some(json!({ "title": "p", "content": "b" })),
        ))
        .await;
    let post_id = body_to_json(create).await["id"]
        .as_str()
        .unwrap()
        .to_string();

    let (member, _) = create_test_user(&pool).await;
    add_guild_member(&pool, guild, member).await;
    let member_token = generate_access_token(&app.config, member);

    let res = app
        .oneshot(req(
            Method::PATCH,
            &format!("/api/forum/posts/{post_id}"),
            &member_token,
            Some(json!({ "pinned": true })),
        ))
        .await;
    assert_eq!(res.status(), StatusCode::FORBIDDEN);
}

#[sqlx::test]
async fn author_can_delete_own_post(pool: PgPool) {
    let (app, _guild, channel, token) = setup(&pool, GuildPermissions::SEND_MESSAGES).await;
    let create = app
        .oneshot(req(
            Method::POST,
            &format!("/api/channels/{channel}/posts"),
            &token,
            Some(json!({ "title": "mine", "content": "b" })),
        ))
        .await;
    let post_id = body_to_json(create).await["id"]
        .as_str()
        .unwrap()
        .to_string();

    let res = app
        .oneshot(req(
            Method::DELETE,
            &format!("/api/forum/posts/{post_id}"),
            &token,
            None,
        ))
        .await;
    assert_eq!(res.status(), StatusCode::OK);

    // Gone from the list.
    let list = app
        .oneshot(req(
            Method::GET,
            &format!("/api/channels/{channel}/posts"),
            &token,
            None,
        ))
        .await;
    assert_eq!(body_to_json(list).await.as_array().unwrap().len(), 0);
}
