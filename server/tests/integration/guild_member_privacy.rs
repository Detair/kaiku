//! The guild member list must not reveal a user's status to someone they
//! block or who blocks them.

use axum::body::Body;
use axum::http::Method;
use sqlx::PgPool;
use uuid::Uuid;

use super::helpers::{
    add_guild_member, body_to_json, create_guild, create_test_user, generate_access_token, TestApp,
};

/// `status` and `last_seen_at` of `user` in the member list as seen by `viewer`.
async fn member_entry(app: &TestApp, viewer: Uuid, guild: Uuid, user: Uuid) -> serde_json::Value {
    let token = generate_access_token(&app.config, viewer);
    let req = TestApp::request(Method::GET, &format!("/api/guilds/{guild}/members"))
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 200);
    let list = body_to_json(resp).await;
    list.as_array()
        .unwrap()
        .iter()
        .find(|m| m["user_id"] == user.to_string())
        .cloned()
        .expect("member listed")
}

/// Mark `user` online: stored status plus one live socket (online means "has a
/// live socket", see `presence::connections`).
async fn set_online(app: &TestApp, user: Uuid) {
    sqlx::query("UPDATE users SET status = 'online', last_seen_at = NOW() WHERE id = $1")
        .bind(user)
        .execute(&app.pool)
        .await
        .unwrap();
    let now_ms = chrono::Utc::now().timestamp_millis();
    vc_server::presence::connections::socket_online(&app.state.redis, user, Uuid::now_v7(), now_ms)
        .await
        .unwrap();
}

async fn block(pool: &PgPool, blocker: Uuid, blocked: Uuid) {
    sqlx::query(
        "INSERT INTO friendships (id, requester_id, addressee_id, status) VALUES ($1, $2, $3, 'blocked')",
    )
    .bind(Uuid::now_v7())
    .bind(blocker)
    .bind(blocked)
    .execute(pool)
    .await
    .unwrap();
}

async fn setup(pool: &PgPool) -> (TestApp, Uuid, Uuid, Uuid) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (a, _) = create_test_user(pool).await;
    let (b, _) = create_test_user(pool).await;
    let guild = create_guild(pool, a).await;
    add_guild_member(pool, guild, b).await;
    set_online(&app, a).await;
    set_online(&app, b).await;
    (app, a, b, guild)
}

#[sqlx::test]
async fn member_list_shows_status_without_block(pool: PgPool) {
    let (app, a, b, guild) = setup(&pool).await;
    let entry = member_entry(&app, b, guild, a).await;
    assert_eq!(entry["status"], "online");
    assert!(!entry["last_seen_at"].is_null());
}

#[sqlx::test]
async fn block_only_masks_the_blocked_pair(pool: PgPool) {
    let (app, a, b, guild) = setup(&pool).await;
    let (c, _) = create_test_user(&pool).await;
    add_guild_member(&pool, guild, c).await;
    set_online(&app, c).await;
    block(&pool, a, b).await;
    assert_eq!(member_entry(&app, b, guild, c).await["status"], "online");
    assert_eq!(member_entry(&app, c, guild, a).await["status"], "online");
}

#[sqlx::test]
async fn blocked_user_does_not_see_blockers_status(pool: PgPool) {
    let (app, a, b, guild) = setup(&pool).await;
    block(&pool, a, b).await;
    let entry = member_entry(&app, b, guild, a).await;
    assert_eq!(entry["status"], "offline");
    assert!(entry["last_seen_at"].is_null());
}

#[sqlx::test]
async fn blocker_does_not_see_blocked_users_status(pool: PgPool) {
    let (app, a, b, guild) = setup(&pool).await;
    block(&pool, a, b).await;
    let entry = member_entry(&app, a, guild, b).await;
    assert_eq!(entry["status"], "offline");
    assert!(entry["last_seen_at"].is_null());
}
