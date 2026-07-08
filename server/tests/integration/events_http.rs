//! Integration tests for scheduled guild events.
//! Run: `cargo test --test integration events_http`

use axum::body::Body;
use axum::http::{Method, StatusCode};
use chrono::{Duration as ChronoDuration, Utc};
use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;
use vc_server::permissions::GuildPermissions;

use crate::helpers::{
    add_guild_member, body_to_json, create_guild_with_default_role, create_test_user,
    generate_access_token, TestApp,
};

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

fn in_hours(h: i64) -> String {
    (Utc::now() + ChronoDuration::hours(h)).to_rfc3339()
}

#[sqlx::test]
async fn owner_creates_event_member_cannot(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (owner, _) = create_test_user(&pool).await;
    let guild = create_guild_with_default_role(&pool, owner, GuildPermissions::VIEW_CHANNEL).await;
    let owner_token = generate_access_token(&app.config, owner);

    // Owner (bypasses perms) creates an event.
    let res = app
        .oneshot(req(
            Method::POST,
            &format!("/api/guilds/{guild}/events"),
            &owner_token,
            Some(json!({ "name": "Game Night", "starts_at": in_hours(24) })),
        ))
        .await;
    assert_eq!(res.status(), StatusCode::CREATED);
    let body = body_to_json(res).await;
    assert_eq!(body["name"], "Game Night");
    assert_eq!(body["going_count"], 0);

    // A plain member without MANAGE_EVENTS cannot.
    let (member, _) = create_test_user(&pool).await;
    add_guild_member(&pool, guild, member).await;
    let member_token = generate_access_token(&app.config, member);
    let res = app
        .oneshot(req(
            Method::POST,
            &format!("/api/guilds/{guild}/events"),
            &member_token,
            Some(json!({ "name": "Nope", "starts_at": in_hours(24) })),
        ))
        .await;
    assert_eq!(res.status(), StatusCode::FORBIDDEN);
}

#[sqlx::test]
async fn rejects_past_start_and_bad_end(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (owner, _) = create_test_user(&pool).await;
    let guild = create_guild_with_default_role(&pool, owner, GuildPermissions::VIEW_CHANNEL).await;
    let token = generate_access_token(&app.config, owner);

    let past = app
        .oneshot(req(
            Method::POST,
            &format!("/api/guilds/{guild}/events"),
            &token,
            Some(json!({ "name": "Past", "starts_at": in_hours(-1) })),
        ))
        .await;
    assert_eq!(past.status(), StatusCode::BAD_REQUEST);

    let bad_end = app
        .oneshot(req(
            Method::POST,
            &format!("/api/guilds/{guild}/events"),
            &token,
            Some(json!({ "name": "BadEnd", "starts_at": in_hours(24), "ends_at": in_hours(23) })),
        ))
        .await;
    assert_eq!(bad_end.status(), StatusCode::BAD_REQUEST);
}

#[sqlx::test]
async fn rsvp_updates_counts_and_clears(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (owner, _) = create_test_user(&pool).await;
    let guild = create_guild_with_default_role(&pool, owner, GuildPermissions::VIEW_CHANNEL).await;
    let token = generate_access_token(&app.config, owner);

    let create = app
        .oneshot(req(
            Method::POST,
            &format!("/api/guilds/{guild}/events"),
            &token,
            Some(json!({ "name": "Stream", "starts_at": in_hours(5) })),
        ))
        .await;
    let event_id = body_to_json(create).await["id"]
        .as_str()
        .unwrap()
        .to_string();

    // RSVP going.
    let rsvp = app
        .oneshot(req(
            Method::PUT,
            &format!("/api/guilds/{guild}/events/{event_id}/rsvp"),
            &token,
            Some(json!({ "response": "going" })),
        ))
        .await;
    assert_eq!(rsvp.status(), StatusCode::OK);
    let body = body_to_json(rsvp).await;
    assert_eq!(body["going_count"], 1);
    assert_eq!(body["my_response"], "going");

    // Change to interested (upsert).
    let change = app
        .oneshot(req(
            Method::PUT,
            &format!("/api/guilds/{guild}/events/{event_id}/rsvp"),
            &token,
            Some(json!({ "response": "interested" })),
        ))
        .await;
    let body = body_to_json(change).await;
    assert_eq!(body["going_count"], 0);
    assert_eq!(body["interested_count"], 1);

    // Clear.
    let clear = app
        .oneshot(req(
            Method::DELETE,
            &format!("/api/guilds/{guild}/events/{event_id}/rsvp"),
            &token,
            None,
        ))
        .await;
    let body = body_to_json(clear).await;
    assert_eq!(body["interested_count"], 0);
    assert!(body["my_response"].is_null());
}

#[sqlx::test]
async fn invalid_rsvp_rejected(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (owner, _) = create_test_user(&pool).await;
    let guild = create_guild_with_default_role(&pool, owner, GuildPermissions::VIEW_CHANNEL).await;
    let token = generate_access_token(&app.config, owner);
    let create = app
        .oneshot(req(
            Method::POST,
            &format!("/api/guilds/{guild}/events"),
            &token,
            Some(json!({ "name": "E", "starts_at": in_hours(5) })),
        ))
        .await;
    let event_id = body_to_json(create).await["id"]
        .as_str()
        .unwrap()
        .to_string();

    let bad = app
        .oneshot(req(
            Method::PUT,
            &format!("/api/guilds/{guild}/events/{event_id}/rsvp"),
            &token,
            Some(json!({ "response": "maybe" })),
        ))
        .await;
    assert_eq!(bad.status(), StatusCode::BAD_REQUEST);
}

#[sqlx::test]
async fn scheduler_transitions_status(pool: PgPool) {
    // An event whose start already passed should flip scheduled → active on tick.
    let (owner, _) = create_test_user(&pool).await;
    let guild = create_guild_with_default_role(&pool, owner, GuildPermissions::VIEW_CHANNEL).await;
    let event_id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO guild_events (id, guild_id, name, starts_at, status)
         VALUES ($1, $2, 'Started', NOW() - INTERVAL '1 minute', 'scheduled')",
    )
    .bind(event_id)
    .bind(guild)
    .execute(&pool)
    .await
    .unwrap();

    let redis = vc_server::db::create_redis_client(
        &vc_server::config::Config::default_for_test().redis_url,
    )
    .await
    .unwrap();
    vc_server::guild::events::scheduler::run_tick(&pool, &redis)
        .await
        .unwrap();

    let status: (String,) = sqlx::query_as("SELECT status FROM guild_events WHERE id = $1")
        .bind(event_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(status.0, "active");
}
