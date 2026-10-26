//! Integration tests for the optional-features server flag
//! (`KAIKU_OPTIONAL_FEATURES`): forum and announcement channels, scheduled
//! events and membership screening are off unless the operator turns them on.
//! Run: `cargo test --test integration optional_features`

use std::time::Duration;

use axum::body::Body;
use axum::http::{Method, StatusCode};
use http_body_util::BodyExt;
use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;
use vc_server::config::Config;
use vc_server::features::OptionalFeatures;
use vc_server::permissions::GuildPermissions;

use crate::helpers::{
    body_to_json, create_channel, create_guild_with_default_role, create_test_user,
    generate_access_token, TestApp,
};

const ALL_ON: OptionalFeatures = OptionalFeatures {
    forum: true,
    announcements: true,
    events: true,
    screening: true,
};

fn config_with(features: OptionalFeatures) -> Config {
    let mut config = Config::default_for_test();
    config.optional_features = features;
    config
}

fn authed(method: Method, uri: &str, token: &str) -> axum::http::request::Builder {
    TestApp::request(method, uri)
        .header("Authorization", format!("Bearer {token}"))
        .header("Content-Type", "application/json")
}

async fn insert_channel_of_type(pool: &PgPool, guild_id: Uuid, name: &str, kind: &str) -> Uuid {
    let id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO channels (id, guild_id, name, channel_type) VALUES ($1, $2, $3, $4::channel_type)",
    )
    .bind(id)
    .bind(guild_id)
    .bind(name)
    .bind(kind)
    .execute(pool)
    .await
    .expect("insert channel");
    id
}

fn all_perms() -> GuildPermissions {
    GuildPermissions::VIEW_CHANNEL
        | GuildPermissions::SEND_MESSAGES
        | GuildPermissions::MANAGE_CHANNELS
        | GuildPermissions::SEND_ANNOUNCEMENTS
}

#[sqlx::test]
async fn test_settings_reports_features_off_by_default(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let res = app
        .oneshot(
            TestApp::request(Method::GET, "/api/settings")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(res.status(), StatusCode::OK);
    let body = body_to_json(res).await;
    assert_eq!(
        body["features"],
        json!({
            "forum_channels": false,
            "announcement_channels": false,
            "scheduled_events": false,
            "membership_screening": false,
        })
    );

    // Turning one on is reflected in the same field.
    let app = TestApp::with_pool_and_config(
        pool.clone(),
        config_with(OptionalFeatures {
            events: true,
            ..OptionalFeatures::default()
        }),
    )
    .await;
    let res = app
        .oneshot(
            TestApp::request(Method::GET, "/api/settings")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    let body = body_to_json(res).await;
    assert_eq!(body["features"]["scheduled_events"], true);
    assert_eq!(body["features"]["forum_channels"], false);
}

#[sqlx::test]
async fn test_create_forum_channel_rejected_when_disabled(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (owner, _) = create_test_user(&pool).await;
    let guild = create_guild_with_default_role(&pool, owner, all_perms()).await;
    let token = generate_access_token(&app.config, owner);

    for kind in ["forum", "announcement"] {
        let res = app
            .oneshot(
                authed(Method::POST, "/api/channels", &token)
                    .body(Body::from(
                        json!({ "name": format!("x-{kind}"), "channel_type": kind, "guild_id": guild })
                            .to_string(),
                    ))
                    .unwrap(),
            )
            .await;
        assert_eq!(res.status(), StatusCode::BAD_REQUEST, "{kind}");
        assert_eq!(
            body_to_json(res).await["error"],
            "FEATURE_DISABLED",
            "{kind}"
        );
    }

    // Plain text channels are unaffected.
    let res = app
        .oneshot(
            authed(Method::POST, "/api/channels", &token)
                .body(Body::from(
                    json!({ "name": "general", "channel_type": "text", "guild_id": guild })
                        .to_string(),
                ))
                .unwrap(),
        )
        .await;
    assert_eq!(res.status(), StatusCode::CREATED);

    // With the flag on, the forum channel is created.
    let features = OptionalFeatures {
        forum: true,
        ..OptionalFeatures::default()
    };
    let app = TestApp::with_pool_and_config(pool.clone(), config_with(features)).await;
    let res = app
        .oneshot(
            authed(Method::POST, "/api/channels", &token)
                .body(Body::from(
                    json!({ "name": "board", "channel_type": "forum", "guild_id": guild })
                        .to_string(),
                ))
                .unwrap(),
        )
        .await;
    assert_eq!(res.status(), StatusCode::CREATED);
}

#[sqlx::test]
async fn test_forum_follow_events_screening_routes_404_when_disabled(pool: PgPool) {
    let off = TestApp::with_pool(pool.clone()).await;
    let on = TestApp::with_pool_and_config(pool.clone(), config_with(ALL_ON)).await;
    let (owner, _) = create_test_user(&pool).await;
    let guild = create_guild_with_default_role(&pool, owner, all_perms()).await;
    let forum = insert_channel_of_type(&pool, guild, "board", "forum").await;
    let news = insert_channel_of_type(&pool, guild, "news", "announcement").await;
    let text = create_channel(&pool, guild, "inbox").await;
    let token = generate_access_token(&off.config, owner);
    let x = Uuid::now_v7();

    let routes: Vec<(Method, String)> = vec![
        (Method::GET, format!("/api/channels/{forum}/posts")),
        (Method::POST, format!("/api/channels/{forum}/posts")),
        (Method::GET, format!("/api/channels/{forum}/tags")),
        (Method::POST, format!("/api/channels/{forum}/tags")),
        (Method::DELETE, format!("/api/channels/{forum}/tags/{x}")),
        (Method::PATCH, format!("/api/forum/posts/{x}")),
        (Method::DELETE, format!("/api/forum/posts/{x}")),
        (Method::POST, format!("/api/channels/{text}/follow")),
        (Method::DELETE, format!("/api/channels/{text}/follow/{x}")),
        (Method::GET, format!("/api/channels/{news}/followers")),
        (Method::GET, format!("/api/guilds/{guild}/events")),
        (Method::POST, format!("/api/guilds/{guild}/events")),
        (Method::PATCH, format!("/api/guilds/{guild}/events/{x}")),
        (Method::DELETE, format!("/api/guilds/{guild}/events/{x}")),
        (Method::PUT, format!("/api/guilds/{guild}/events/{x}/rsvp")),
        (
            Method::DELETE,
            format!("/api/guilds/{guild}/events/{x}/rsvp"),
        ),
        (Method::GET, format!("/api/guilds/{guild}/screening")),
        (Method::PUT, format!("/api/guilds/{guild}/screening")),
        (
            Method::POST,
            format!("/api/guilds/{guild}/screening/accept"),
        ),
    ];

    for (method, uri) in &routes {
        let res = off
            .oneshot(
                authed(method.clone(), uri, &token)
                    .body(Body::from("{}"))
                    .unwrap(),
            )
            .await;
        assert_eq!(res.status(), StatusCode::NOT_FOUND, "{method} {uri}");
        let body = body_to_json(res).await;
        assert_eq!(body["error"], "FEATURE_DISABLED", "{method} {uri}");
        assert_eq!(
            body["message"], "This feature is turned off on this server.",
            "{method} {uri}"
        );

        // With every feature on, the same route reaches its handler.
        let res = on
            .oneshot(
                authed(method.clone(), uri, &token)
                    .body(Body::from("{}"))
                    .unwrap(),
            )
            .await;
        // (Handlers may reject the empty body; any answer but the gate counts.)
        let status = res.status();
        let raw = res.into_body().collect().await.unwrap().to_bytes();
        let raw = String::from_utf8_lossy(&raw);
        assert!(
            !raw.contains("FEATURE_DISABLED"),
            "{method} {uri} ({status}): {raw}"
        );
    }

    // Unauthenticated callers still get 401, not a feature hint.
    let res = off
        .oneshot(
            TestApp::request(Method::GET, &format!("/api/guilds/{guild}/events"))
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
}

#[sqlx::test]
async fn test_existing_announcement_channel_works_without_crosspost_when_disabled(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (owner, _) = create_test_user(&pool).await;
    let guild = create_guild_with_default_role(&pool, owner, all_perms()).await;
    let source = insert_channel_of_type(&pool, guild, "news", "announcement").await;
    let target = create_channel(&pool, guild, "inbox").await;
    sqlx::query(
        "INSERT INTO announcement_follows (source_channel_id, target_channel_id, created_by)
         VALUES ($1, $2, $3)",
    )
    .bind(source)
    .bind(target)
    .bind(owner)
    .execute(&pool)
    .await
    .expect("insert follow");
    let token = generate_access_token(&app.config, owner);

    let res = app
        .oneshot(
            authed(
                Method::POST,
                &format!("/api/messages/channel/{source}"),
                &token,
            )
            .body(Body::from(
                json!({ "content": "big news", "encrypted": false }).to_string(),
            ))
            .unwrap(),
        )
        .await;
    assert_eq!(res.status(), StatusCode::CREATED);

    // The fan-out would be spawned; give it time, then assert nothing landed.
    tokio::time::sleep(Duration::from_millis(400)).await;
    let crossposts: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM messages WHERE channel_id = $1")
        .bind(target)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(crossposts, 0, "no crosspost while announcements are off");

    // The follow row is kept.
    let follows: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM announcement_follows WHERE source_channel_id = $1",
    )
    .bind(source)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(follows, 1);

    // Channel listing still works and still contains the channel.
    let res = app
        .oneshot(
            authed(
                Method::GET,
                &format!("/api/guilds/{guild}/channels"),
                &token,
            )
            .body(Body::empty())
            .unwrap(),
        )
        .await;
    assert_eq!(res.status(), StatusCode::OK);
    let list = body_to_json(res).await;
    let ids: Vec<&str> = list
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|c| c["id"].as_str())
        .collect();
    assert!(ids.contains(&source.to_string().as_str()));
}

async fn membership_state(pool: &PgPool, guild: Uuid, user: Uuid) -> String {
    sqlx::query_scalar(
        "SELECT membership_state FROM guild_members WHERE guild_id = $1 AND user_id = $2",
    )
    .bind(guild)
    .bind(user)
    .fetch_one(pool)
    .await
    .expect("membership row")
}

/// Join `guild` once via invite and once via discovery (two fresh users) and
/// return both membership states.
async fn join_both_ways(app: &TestApp, pool: &PgPool, owner: Uuid, guild: Uuid) -> [String; 2] {
    let owner_token = generate_access_token(&app.config, owner);
    let res = app
        .oneshot(
            authed(
                Method::POST,
                &format!("/api/guilds/{guild}/invites"),
                &owner_token,
            )
            .body(Body::from(r#"{"expires_in": "7d"}"#))
            .unwrap(),
        )
        .await;
    assert_eq!(res.status(), StatusCode::OK);
    let code = body_to_json(res).await["code"]
        .as_str()
        .unwrap()
        .to_string();

    let (via_invite, _) = create_test_user(pool).await;
    let token = generate_access_token(&app.config, via_invite);
    let res = app
        .oneshot(
            authed(Method::POST, &format!("/api/invites/{code}/join"), &token)
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert!(res.status().is_success(), "invite join: {}", res.status());

    let (via_discovery, _) = create_test_user(pool).await;
    let token = generate_access_token(&app.config, via_discovery);
    let res = app
        .oneshot(
            authed(
                Method::POST,
                &format!("/api/discover/guilds/{guild}/join"),
                &token,
            )
            .body(Body::empty())
            .unwrap(),
        )
        .await;
    assert!(
        res.status().is_success(),
        "discovery join: {}",
        res.status()
    );

    [
        membership_state(pool, guild, via_invite).await,
        membership_state(pool, guild, via_discovery).await,
    ]
}

#[sqlx::test]
async fn test_join_never_pending_when_screening_disabled(pool: PgPool) {
    let (owner, _) = create_test_user(&pool).await;
    let guild = create_guild_with_default_role(&pool, owner, all_perms()).await;
    sqlx::query("UPDATE guilds SET screening_enabled = true, discoverable = true WHERE id = $1")
        .bind(guild)
        .execute(&pool)
        .await
        .unwrap();

    let mut off = Config::default_for_test();
    off.enable_guild_discovery = true;
    let app = TestApp::with_pool_and_config(pool.clone(), off).await;
    assert_eq!(
        join_both_ways(&app, &pool, owner, guild).await,
        ["active".to_string(), "active".to_string()]
    );

    // Control: with screening on, the same guild screens new members.
    let mut on = config_with(ALL_ON);
    on.enable_guild_discovery = true;
    let app = TestApp::with_pool_and_config(pool.clone(), on).await;
    assert_eq!(
        join_both_ways(&app, &pool, owner, guild).await,
        ["pending".to_string(), "pending".to_string()]
    );
}

#[sqlx::test]
async fn test_forum_webhooks_and_sends_rejected_when_disabled(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (owner, _) = create_test_user(&pool).await;
    let guild = create_guild_with_default_role(
        &pool,
        owner,
        all_perms() | GuildPermissions::MANAGE_WEBHOOKS,
    )
    .await;
    let forum = insert_channel_of_type(&pool, guild, "board", "forum").await;
    let token = generate_access_token(&app.config, owner);

    // A new webhook into an existing forum channel is refused.
    let res = app
        .oneshot(
            authed(
                Method::POST,
                &format!("/api/channels/{forum}/webhooks"),
                &token,
            )
            .body(Body::from(json!({ "name": "hook" }).to_string()))
            .unwrap(),
        )
        .await;
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    assert_eq!(body_to_json(res).await["error"], "FEATURE_DISABLED");

    // A plain message into the forum channel is refused too.
    let res = app
        .oneshot(
            authed(
                Method::POST,
                &format!("/api/messages/channel/{forum}"),
                &token,
            )
            .body(Body::from(
                json!({ "content": "hi", "encrypted": false }).to_string(),
            ))
            .unwrap(),
        )
        .await;
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    assert_eq!(body_to_json(res).await["error"], "FEATURE_DISABLED");
}

#[sqlx::test]
async fn test_crosspost_skips_forum_targets_when_forum_disabled(pool: PgPool) {
    let app = TestApp::with_pool_and_config(
        pool.clone(),
        config_with(OptionalFeatures {
            announcements: true,
            ..OptionalFeatures::default()
        }),
    )
    .await;
    let (owner, _) = create_test_user(&pool).await;
    let guild = create_guild_with_default_role(&pool, owner, all_perms()).await;
    let source = insert_channel_of_type(&pool, guild, "news", "announcement").await;
    let forum = insert_channel_of_type(&pool, guild, "board", "forum").await;
    let text = create_channel(&pool, guild, "inbox").await;
    let token = generate_access_token(&app.config, owner);

    // A new follow into a forum channel is refused; a text target is fine.
    let follow = |target: Uuid| {
        authed(
            Method::POST,
            &format!("/api/channels/{target}/follow"),
            &token,
        )
        .body(Body::from(
            json!({ "source_channel_id": source }).to_string(),
        ))
        .unwrap()
    };
    let res = app.oneshot(follow(forum)).await;
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    assert_eq!(body_to_json(res).await["error"], "FEATURE_DISABLED");
    let res = app.oneshot(follow(text)).await;
    assert_eq!(res.status(), StatusCode::CREATED);

    // An existing follow row into the forum (from before) is kept but skipped.
    sqlx::query(
        "INSERT INTO announcement_follows (source_channel_id, target_channel_id, created_by)
         VALUES ($1, $2, $3)",
    )
    .bind(source)
    .bind(forum)
    .bind(owner)
    .execute(&pool)
    .await
    .unwrap();

    let res = app
        .oneshot(
            authed(
                Method::POST,
                &format!("/api/messages/channel/{source}"),
                &token,
            )
            .body(Body::from(
                json!({ "content": "big news", "encrypted": false }).to_string(),
            ))
            .unwrap(),
        )
        .await;
    assert_eq!(res.status(), StatusCode::CREATED);

    let count = |channel: Uuid| {
        let pool = pool.clone();
        async move {
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM messages WHERE channel_id = $1")
                .bind(channel)
                .fetch_one(&pool)
                .await
                .unwrap()
        }
    };
    // Wait for the text target's crosspost, then check the forum got nothing.
    for _ in 0..50 {
        if count(text).await == 1 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(count(text).await, 1, "text target gets the crosspost");
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(count(forum).await, 0, "forum target is skipped");
    let rows: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM announcement_follows WHERE source_channel_id = $1",
    )
    .bind(source)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(rows, 2, "follow rows are kept");
}

#[sqlx::test]
async fn test_pin_in_forum_rejected_when_disabled(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (owner, _) = create_test_user(&pool).await;
    let guild = create_guild_with_default_role(&pool, owner, all_perms()).await;
    let forum = insert_channel_of_type(&pool, guild, "board", "forum").await;
    let message = crate::helpers::insert_message(&pool, forum, owner, "old post").await;
    let token = generate_access_token(&app.config, owner);

    let res = app
        .oneshot(
            authed(
                Method::PUT,
                &format!("/api/channels/{forum}/messages/{message}/pin"),
                &token,
            )
            .body(Body::empty())
            .unwrap(),
        )
        .await;
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    assert_eq!(body_to_json(res).await["error"], "FEATURE_DISABLED");
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM messages WHERE channel_id = $1")
        .bind(forum)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 1, "no system message added");
}
