//! Tenancy & isolation regression suite (Phase 8, Goal 5).
//!
//! Encodes the guild-boundary invariants as release-blocking tests: a user
//! who is not a member of guild A must not be able to read, write, search,
//! or enumerate anything inside it — and content from foreign guilds must
//! never leak through aggregation endpoints like global search.
//!
//! Design reference:
//! `docs/developer-guide/plans/2026-02-15-tenancy-isolation-verification-design.md`
//! (placeholder doc — the invariants are defined here, in executable form).

use axum::body::Body;
use axum::http::{Method, StatusCode};
use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;
use vc_server::permissions::GuildPermissions;

use super::helpers::{
    add_guild_member, body_to_json, create_channel, create_guild, create_guild_with_default_role,
    create_test_user, generate_access_token, insert_message, TestApp,
};

/// Two-tenant fixture: alice owns guild A with a channel and a message;
/// bob owns guild B and is NOT a member of A.
struct TwoGuilds {
    app: TestApp,
    guild_a: Uuid,
    channel_a: Uuid,
    message_a: Uuid,
    alice_token: String,
    bob_token: String,
}

async fn two_guilds(pool: PgPool) -> TwoGuilds {
    let app = TestApp::with_pool(pool.clone()).await;
    let (alice, _) = create_test_user(&pool).await;
    let (bob, _) = create_test_user(&pool).await;

    let guild_a = create_guild(&pool, alice).await;
    let channel_a = create_channel(&pool, guild_a, "general-a").await;
    let message_a = insert_message(
        &pool,
        channel_a,
        alice,
        "tenancy-marker-aurora-borealis-9000",
    )
    .await;

    let guild_b = create_guild(&pool, bob).await;
    let _channel_b = create_channel(&pool, guild_b, "general-b").await;

    let alice_token = generate_access_token(&app.config, alice);
    let bob_token = generate_access_token(&app.config, bob);

    TwoGuilds {
        app,
        guild_a,
        channel_a,
        message_a,
        alice_token,
        bob_token,
    }
}

fn assert_denied(status: StatusCode, what: &str) {
    assert!(
        status == StatusCode::FORBIDDEN || status == StatusCode::NOT_FOUND,
        "{what}: expected 403/404 for cross-guild access, got {status}"
    );
}

// ============================================================================
// Positive control — the suite must not pass because endpoints are broken
// ============================================================================

#[sqlx::test]
async fn member_can_access_own_guild_resources(pool: PgPool) {
    let t = two_guilds(pool).await;

    for uri in [
        format!("/api/guilds/{}/channels", t.guild_a),
        format!("/api/guilds/{}/members", t.guild_a),
        format!("/api/messages/channel/{}", t.channel_a),
    ] {
        let resp = t
            .app
            .oneshot(
                TestApp::request(Method::GET, &uri)
                    .header("Authorization", format!("Bearer {}", t.alice_token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await;
        assert_eq!(resp.status(), StatusCode::OK, "owner blocked from {uri}");
    }
}

// ============================================================================
// Enumeration isolation
// ============================================================================

#[sqlx::test]
async fn outsider_cannot_list_foreign_guild_channels(pool: PgPool) {
    let t = two_guilds(pool).await;
    let resp = t
        .app
        .oneshot(
            TestApp::request(Method::GET, &format!("/api/guilds/{}/channels", t.guild_a))
                .header("Authorization", format!("Bearer {}", t.bob_token))
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_denied(resp.status(), "list foreign guild channels");
}

#[sqlx::test]
async fn outsider_cannot_list_foreign_guild_members(pool: PgPool) {
    let t = two_guilds(pool).await;
    let resp = t
        .app
        .oneshot(
            TestApp::request(Method::GET, &format!("/api/guilds/{}/members", t.guild_a))
                .header("Authorization", format!("Bearer {}", t.bob_token))
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_denied(resp.status(), "list foreign guild members");
}

// ============================================================================
// Message data isolation
// ============================================================================

#[sqlx::test]
async fn outsider_cannot_read_foreign_channel_messages(pool: PgPool) {
    let t = two_guilds(pool).await;
    let resp = t
        .app
        .oneshot(
            TestApp::request(
                Method::GET,
                &format!("/api/messages/channel/{}", t.channel_a),
            )
            .header("Authorization", format!("Bearer {}", t.bob_token))
            .body(Body::empty())
            .unwrap(),
        )
        .await;
    assert_denied(resp.status(), "read foreign channel messages");
}

#[sqlx::test]
async fn outsider_cannot_post_into_foreign_channel(pool: PgPool) {
    let t = two_guilds(pool).await;
    let resp = t
        .app
        .oneshot(
            TestApp::request(
                Method::POST,
                &format!("/api/messages/channel/{}", t.channel_a),
            )
            .header("Authorization", format!("Bearer {}", t.bob_token))
            .header("Content-Type", "application/json")
            .body(Body::from(json!({ "content": "intruder" }).to_string()))
            .unwrap(),
        )
        .await;
    assert_denied(resp.status(), "post into foreign channel");

    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM messages WHERE channel_id = $1 AND content = $2")
            .bind(t.channel_a)
            .bind("intruder")
            .fetch_one(&t.app.pool)
            .await
            .unwrap();
    assert_eq!(count, 0, "intruder message must not be persisted");
}

#[sqlx::test]
async fn outsider_cannot_edit_foreign_message(pool: PgPool) {
    let t = two_guilds(pool).await;
    let resp = t
        .app
        .oneshot(
            TestApp::request(Method::PATCH, &format!("/api/messages/{}", t.message_a))
                .header("Authorization", format!("Bearer {}", t.bob_token))
                .header("Content-Type", "application/json")
                .body(Body::from(json!({ "content": "defaced" }).to_string()))
                .unwrap(),
        )
        .await;
    assert!(
        !resp.status().is_success(),
        "cross-guild edit must not succeed, got {}",
        resp.status()
    );

    let content: String = sqlx::query_scalar("SELECT content FROM messages WHERE id = $1")
        .bind(t.message_a)
        .fetch_one(&t.app.pool)
        .await
        .unwrap();
    assert_eq!(content, "tenancy-marker-aurora-borealis-9000");
}

#[sqlx::test]
async fn outsider_cannot_delete_foreign_message(pool: PgPool) {
    let t = two_guilds(pool).await;
    let resp = t
        .app
        .oneshot(
            TestApp::request(Method::DELETE, &format!("/api/messages/{}", t.message_a))
                .header("Authorization", format!("Bearer {}", t.bob_token))
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert!(
        !resp.status().is_success(),
        "cross-guild delete must not succeed, got {}",
        resp.status()
    );

    let alive: i64 =
        sqlx::query_scalar("SELECT count(*) FROM messages WHERE id = $1 AND deleted_at IS NULL")
            .bind(t.message_a)
            .fetch_one(&t.app.pool)
            .await
            .unwrap();
    assert_eq!(alive, 1, "message must survive foreign delete");
}

// ============================================================================
// Search isolation
// ============================================================================

#[sqlx::test]
async fn outsider_cannot_search_foreign_guild(pool: PgPool) {
    let t = two_guilds(pool).await;
    let resp = t
        .app
        .oneshot(
            TestApp::request(
                Method::GET,
                &format!("/api/guilds/{}/search?q=tenancy", t.guild_a),
            )
            .header("Authorization", format!("Bearer {}", t.bob_token))
            .body(Body::empty())
            .unwrap(),
        )
        .await;
    assert_denied(resp.status(), "search foreign guild");
}

#[sqlx::test]
async fn global_search_does_not_leak_foreign_guild_content(pool: PgPool) {
    let t = two_guilds(pool).await;
    // Bob searches globally for the unique marker that exists ONLY in guild A.
    let resp = t
        .app
        .oneshot(
            TestApp::request(Method::GET, "/api/search?q=aurora-borealis-9000")
                .header("Authorization", format!("Bearer {}", t.bob_token))
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_to_json(resp).await;
    assert_eq!(
        body["total"], 0,
        "global search leaked foreign guild content: {body}"
    );
}

// ============================================================================
// Membership lifecycle — leaving revokes access
// ============================================================================

#[sqlx::test]
async fn leaving_guild_revokes_access(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (owner, _) = create_test_user(&pool).await;
    // Membership alone grants nothing; VIEW_CHANNEL comes from the
    // @everyone role (owners bypass) — so this guild needs a default role.
    let guild = create_guild_with_default_role(&pool, owner, GuildPermissions::VIEW_CHANNEL).await;
    let channel = create_channel(&pool, guild, "general").await;
    insert_message(&pool, channel, owner, "hello").await;

    let (carol, _) = create_test_user(&pool).await;
    let carol_token = generate_access_token(&app.config, carol);
    add_guild_member(&pool, guild, carol).await;

    // Member: access works
    let resp = app
        .oneshot(
            TestApp::request(Method::GET, &format!("/api/messages/channel/{channel}"))
                .header("Authorization", format!("Bearer {carol_token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(resp.status(), StatusCode::OK, "member should have access");

    // Leave the guild
    let resp = app
        .oneshot(
            TestApp::request(Method::POST, &format!("/api/guilds/{guild}/leave"))
                .header("Authorization", format!("Bearer {carol_token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert!(
        resp.status().is_success(),
        "leave failed: {}",
        resp.status()
    );

    // Former member: access revoked
    let resp = app
        .oneshot(
            TestApp::request(Method::GET, &format!("/api/messages/channel/{channel}"))
                .header("Authorization", format!("Bearer {carol_token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_denied(resp.status(), "post-leave channel access");
}
