//! Integration tests for membership screening (rules gate).
//! Run: `cargo test --test integration screening_http`

use axum::body::Body;
use axum::http::{Method, StatusCode};
use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;
use vc_server::permissions::GuildPermissions;

use crate::helpers::{
    body_to_json, create_channel, create_guild_with_default_role, create_test_user,
    generate_access_token, TestApp,
};

/// Add a member in a specific membership state (simulates a screened join).
async fn add_member_state(pool: &PgPool, guild_id: Uuid, user_id: Uuid, state: &str) {
    sqlx::query(
        "INSERT INTO guild_members (guild_id, user_id, membership_state) VALUES ($1, $2, $3)",
    )
    .bind(guild_id)
    .bind(user_id)
    .bind(state)
    .execute(pool)
    .await
    .expect("add member");
}

async fn enable_screening(pool: &PgPool, guild_id: Uuid) {
    sqlx::query("UPDATE guilds SET screening_enabled = true WHERE id = $1")
        .bind(guild_id)
        .execute(pool)
        .await
        .unwrap();
}

fn post_msg(token: &str, channel: Uuid) -> axum::http::Request<Body> {
    TestApp::request(Method::POST, &format!("/api/messages/channel/{channel}"))
        .header("Authorization", format!("Bearer {token}"))
        .header("Content-Type", "application/json")
        .body(Body::from(
            json!({ "content": "hi", "encrypted": false }).to_string(),
        ))
        .unwrap()
}

#[sqlx::test]
async fn pending_member_is_blocked_then_unlocked_on_accept(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (owner, _) = create_test_user(&pool).await;
    let guild = create_guild_with_default_role(
        &pool,
        owner,
        GuildPermissions::VIEW_CHANNEL | GuildPermissions::SEND_MESSAGES,
    )
    .await;
    enable_screening(&pool, guild).await;
    let channel = create_channel(&pool, guild, "general").await;

    // A pending member — despite @everyone granting view+send.
    let (member, _) = create_test_user(&pool).await;
    add_member_state(&pool, guild, member, "pending").await;
    let token = generate_access_token(&app.config, member);

    // Blocked from posting (resolver short-circuit → empty perms).
    let blocked = app.oneshot(post_msg(&token, channel)).await;
    assert_eq!(blocked.status(), StatusCode::FORBIDDEN);

    // Can read the rules screen.
    let cfg = app
        .oneshot(
            TestApp::request(Method::GET, &format!("/api/guilds/{guild}/screening"))
                .header("Authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(cfg.status(), StatusCode::OK);
    assert_eq!(body_to_json(cfg).await["my_state"], "pending");

    // Accept → active.
    let accept = app
        .oneshot(
            TestApp::request(
                Method::POST,
                &format!("/api/guilds/{guild}/screening/accept"),
            )
            .header("Authorization", format!("Bearer {token}"))
            .body(Body::empty())
            .unwrap(),
        )
        .await;
    assert_eq!(accept.status(), StatusCode::OK);

    // Now posting works immediately (no permission-context cache).
    let ok = app.oneshot(post_msg(&token, channel)).await;
    assert_eq!(ok.status(), StatusCode::CREATED);
}

#[sqlx::test]
async fn active_member_never_gated(pool: PgPool) {
    // Regression guard: an active member in a screening guild keeps full access.
    let app = TestApp::with_pool(pool.clone()).await;
    let (owner, _) = create_test_user(&pool).await;
    let guild = create_guild_with_default_role(
        &pool,
        owner,
        GuildPermissions::VIEW_CHANNEL | GuildPermissions::SEND_MESSAGES,
    )
    .await;
    enable_screening(&pool, guild).await;
    let channel = create_channel(&pool, guild, "general").await;

    let (member, _) = create_test_user(&pool).await;
    add_member_state(&pool, guild, member, "active").await;
    let token = generate_access_token(&app.config, member);

    let res = app.oneshot(post_msg(&token, channel)).await;
    assert_eq!(res.status(), StatusCode::CREATED);
}

#[sqlx::test]
async fn disabling_screening_promotes_pending(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (owner, _) = create_test_user(&pool).await;
    let guild = create_guild_with_default_role(
        &pool,
        owner,
        GuildPermissions::VIEW_CHANNEL | GuildPermissions::MANAGE_GUILD,
    )
    .await;
    enable_screening(&pool, guild).await;
    let (member, _) = create_test_user(&pool).await;
    add_member_state(&pool, guild, member, "pending").await;
    let owner_token = generate_access_token(&app.config, owner);

    // Owner disables screening.
    let res = app
        .oneshot(
            TestApp::request(Method::PUT, &format!("/api/guilds/{guild}/screening"))
                .header("Authorization", format!("Bearer {owner_token}"))
                .header("Content-Type", "application/json")
                .body(Body::from(json!({ "enabled": false }).to_string()))
                .unwrap(),
        )
        .await;
    assert_eq!(res.status(), StatusCode::OK);

    // The formerly-pending member is now active.
    let state: (String,) = sqlx::query_as(
        "SELECT membership_state FROM guild_members WHERE guild_id = $1 AND user_id = $2",
    )
    .bind(guild)
    .bind(member)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(state.0, "active");
}

#[sqlx::test]
async fn accept_when_not_pending_is_rejected(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (owner, _) = create_test_user(&pool).await;
    // Owner (active) — not pending.
    let guild = create_guild_with_default_role(&pool, owner, GuildPermissions::VIEW_CHANNEL).await;
    enable_screening(&pool, guild).await;
    let token = generate_access_token(&app.config, owner);

    let res = app
        .oneshot(
            TestApp::request(
                Method::POST,
                &format!("/api/guilds/{guild}/screening/accept"),
            )
            .header("Authorization", format!("Bearer {token}"))
            .body(Body::empty())
            .unwrap(),
        )
        .await;
    assert_eq!(res.status(), StatusCode::BAD_REQUEST); // NOT_PENDING
}
