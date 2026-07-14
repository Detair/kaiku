//! Integration tests for incoming (Discord-compatible) webhooks.
//! Run: `cargo test --test integration incoming_webhooks_http`

use axum::body::Body;
use axum::http::{Method, StatusCode};
use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;
use vc_server::permissions::GuildPermissions;

use crate::helpers::{
    add_guild_member, body_to_json, create_channel, create_guild_with_default_role,
    create_test_user, generate_access_token, insert_message, TestApp,
};

/// A valid 32-byte hex key — the default test key is deliberately short and
/// token encryption (like outgoing signing-secret encryption) needs a real one.
const WEBHOOK_TEST_KEY: &str = "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f";

async fn setup(pool: &PgPool) -> (TestApp, Uuid, Uuid, Uuid) {
    let mut config = vc_server::config::Config::default_for_test();
    config.mfa_encryption_key = Some(WEBHOOK_TEST_KEY.to_string());
    let app = TestApp::with_pool_and_config(pool.clone(), config).await;
    let (owner, _) = create_test_user(pool).await;
    let guild = create_guild_with_default_role(
        pool,
        owner,
        GuildPermissions::SEND_MESSAGES | GuildPermissions::VIEW_CHANNEL,
    )
    .await;
    let channel = create_channel(pool, guild, "general").await;
    (app, owner, guild, channel)
}

fn authed(
    method: Method,
    uri: &str,
    token: &str,
    body: Option<serde_json::Value>,
) -> axum::http::Request<Body> {
    TestApp::request(method, uri)
        .header("Authorization", format!("Bearer {token}"))
        .header("Content-Type", "application/json")
        .body(body.map_or_else(Body::empty, |b| Body::from(b.to_string())))
        .unwrap()
}

fn public_post(uri: &str, body: serde_json::Value) -> axum::http::Request<Body> {
    TestApp::request(Method::POST, uri)
        .header("Content-Type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap()
}

/// Create a webhook via the API; returns (id, token, url).
async fn create_webhook(app: &TestApp, owner_token: &str, channel: Uuid) -> (String, String) {
    let res = app
        .oneshot(authed(
            Method::POST,
            &format!("/api/channels/{channel}/webhooks"),
            owner_token,
            Some(json!({ "name": "Game Server" })),
        ))
        .await;
    assert_eq!(res.status(), StatusCode::OK);
    let j = body_to_json(res).await;
    (
        j["id"].as_str().unwrap().to_string(),
        j["token"].as_str().unwrap().to_string(),
    )
}

async fn create_forum_channel(pool: &PgPool, guild_id: Uuid) -> Uuid {
    let channel_id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO channels (id, guild_id, name, channel_type) VALUES ($1, $2, 'forum', 'forum')",
    )
    .bind(channel_id)
    .bind(guild_id)
    .execute(pool)
    .await
    .expect("create forum channel");
    channel_id
}

// ============================================================================
// Management CRUD
// ============================================================================

#[sqlx::test]
async fn owner_can_create_and_list_webhooks(pool: PgPool) {
    let (app, owner, guild, channel) = setup(&pool).await;
    let token = generate_access_token(&app.config, owner);

    let res = app
        .oneshot(authed(
            Method::POST,
            &format!("/api/channels/{channel}/webhooks"),
            &token,
            Some(json!({ "name": "CI Bot", "avatar_url": "https://example.com/a.png" })),
        ))
        .await;
    assert_eq!(res.status(), StatusCode::OK);
    let j = body_to_json(res).await;
    assert_eq!(j["type"], 1);
    assert_eq!(j["name"], "CI Bot");
    assert_eq!(j["avatar"], "https://example.com/a.png");
    assert!(j["application_id"].is_null());
    let webhook_token = j["token"].as_str().unwrap();
    assert!(webhook_token.len() >= 60, "Discord-length token expected");
    let url = j["url"].as_str().unwrap();
    assert!(url.contains(&format!(
        "/api/webhooks/{}/{}",
        j["id"].as_str().unwrap(),
        webhook_token
    )));
    assert_eq!(j["user"]["id"].as_str(), Some(owner.to_string().as_str()));

    // Channel + guild listings both include it, with the token.
    let res = app
        .oneshot(authed(
            Method::GET,
            &format!("/api/channels/{channel}/webhooks"),
            &token,
            None,
        ))
        .await;
    assert_eq!(res.status(), StatusCode::OK);
    let list = body_to_json(res).await;
    assert_eq!(list.as_array().unwrap().len(), 1);
    assert_eq!(list[0]["token"].as_str(), Some(webhook_token));

    let res = app
        .oneshot(authed(
            Method::GET,
            &format!("/api/guilds/{guild}/webhooks"),
            &token,
            None,
        ))
        .await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(body_to_json(res).await.as_array().unwrap().len(), 1);
}

#[sqlx::test]
async fn member_without_permission_cannot_manage(pool: PgPool) {
    let (app, owner, guild, channel) = setup(&pool).await;
    let owner_token = generate_access_token(&app.config, owner);
    let (webhook_id, _) = create_webhook(&app, &owner_token, channel).await;

    let (member, _) = create_test_user(&pool).await;
    add_guild_member(&pool, guild, member).await;
    let member_token = generate_access_token(&app.config, member);

    for req in [
        authed(
            Method::POST,
            &format!("/api/channels/{channel}/webhooks"),
            &member_token,
            Some(json!({ "name": "nope" })),
        ),
        authed(
            Method::GET,
            &format!("/api/channels/{channel}/webhooks"),
            &member_token,
            None,
        ),
        authed(
            Method::DELETE,
            &format!("/api/webhooks/{webhook_id}"),
            &member_token,
            None,
        ),
    ] {
        let res = app.oneshot(req).await;
        assert_eq!(res.status(), StatusCode::FORBIDDEN);
    }
}

#[sqlx::test]
async fn webhook_can_be_moved_within_guild_only(pool: PgPool) {
    let (app, owner, guild, channel) = setup(&pool).await;
    let token = generate_access_token(&app.config, owner);
    let (webhook_id, _) = create_webhook(&app, &token, channel).await;
    let other_channel = create_channel(&pool, guild, "other").await;

    let res = app
        .oneshot(authed(
            Method::PATCH,
            &format!("/api/webhooks/{webhook_id}"),
            &token,
            Some(json!({ "channel_id": other_channel, "name": "Renamed" })),
        ))
        .await;
    assert_eq!(res.status(), StatusCode::OK);
    let j = body_to_json(res).await;
    assert_eq!(
        j["channel_id"].as_str(),
        Some(other_channel.to_string().as_str())
    );
    assert_eq!(j["name"], "Renamed");

    // Cross-guild move is rejected.
    let (other_owner, _) = create_test_user(&pool).await;
    let foreign_guild =
        create_guild_with_default_role(&pool, other_owner, GuildPermissions::VIEW_CHANNEL).await;
    let foreign_channel = create_channel(&pool, foreign_guild, "foreign").await;
    let res = app
        .oneshot(authed(
            Method::PATCH,
            &format!("/api/webhooks/{webhook_id}"),
            &token,
            Some(json!({ "channel_id": foreign_channel })),
        ))
        .await;
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
}

#[sqlx::test]
async fn webhooks_rejected_on_voice_channels(pool: PgPool) {
    let (app, owner, guild, _) = setup(&pool).await;
    let token = generate_access_token(&app.config, owner);
    let voice = crate::helpers::create_voice_channel(&pool, guild, "voice").await;

    let res = app
        .oneshot(authed(
            Method::POST,
            &format!("/api/channels/{voice}/webhooks"),
            &token,
            Some(json!({ "name": "nope" })),
        ))
        .await;
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
}

// ============================================================================
// Token-authenticated routes
// ============================================================================

#[sqlx::test]
async fn token_routes_work_without_session(pool: PgPool) {
    let (app, owner, _, channel) = setup(&pool).await;
    let owner_token = generate_access_token(&app.config, owner);
    let (id, token) = create_webhook(&app, &owner_token, channel).await;

    // GET returns the object without the creator (Discord parity).
    let res = app
        .oneshot(
            TestApp::request(Method::GET, &format!("/api/webhooks/{id}/{token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(res.status(), StatusCode::OK);
    let j = body_to_json(res).await;
    assert_eq!(j["name"], "Game Server");
    assert!(j.get("user").is_none());

    // PATCH modifies name; channel_id is ignored on the token route.
    let res = app
        .oneshot(
            TestApp::request(Method::PATCH, &format!("/api/webhooks/{id}/{token}"))
                .header("Content-Type", "application/json")
                .body(Body::from(
                    json!({ "name": "Better Name", "channel_id": Uuid::now_v7() }).to_string(),
                ))
                .unwrap(),
        )
        .await;
    assert_eq!(res.status(), StatusCode::OK);
    let j = body_to_json(res).await;
    assert_eq!(j["name"], "Better Name");
    assert_eq!(j["channel_id"].as_str(), Some(channel.to_string().as_str()));

    // DELETE via token, then the webhook is gone.
    let res = app
        .oneshot(
            TestApp::request(Method::DELETE, &format!("/api/webhooks/{id}/{token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(res.status(), StatusCode::NO_CONTENT);
    let res = app
        .oneshot(public_post(
            &format!("/api/webhooks/{id}/{token}"),
            json!({ "content": "hi" }),
        ))
        .await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
}

// ============================================================================
// Execute
// ============================================================================

#[sqlx::test]
async fn execute_returns_204_and_stores_message(pool: PgPool) {
    let (app, owner, _, channel) = setup(&pool).await;
    let owner_token = generate_access_token(&app.config, owner);
    let (id, token) = create_webhook(&app, &owner_token, channel).await;

    let res = app
        .oneshot(public_post(
            &format!("/api/webhooks/{id}/{token}"),
            json!({ "content": "server started" }),
        ))
        .await;
    assert_eq!(res.status(), StatusCode::NO_CONTENT);

    // Message history renders the webhook author (no user row involved).
    let res = app
        .oneshot(authed(
            Method::GET,
            &format!("/api/messages/channel/{channel}"),
            &owner_token,
            None,
        ))
        .await;
    assert_eq!(res.status(), StatusCode::OK);
    let j = body_to_json(res).await;
    let msg = &j["items"][0];
    assert_eq!(msg["content"], "server started");
    assert_eq!(msg["author"]["display_name"], "Game Server");
    assert_eq!(msg["webhook_id"].as_str(), Some(id.as_str()));
}

#[sqlx::test]
async fn execute_wait_returns_message_with_overrides(pool: PgPool) {
    let (app, owner, _, channel) = setup(&pool).await;
    let owner_token = generate_access_token(&app.config, owner);
    let (id, token) = create_webhook(&app, &owner_token, channel).await;

    let res = app
        .oneshot(public_post(
            &format!("/api/webhooks/{id}/{token}?wait=true"),
            json!({
                "content": "match ended",
                "username": "Scoreboard",
                "avatar_url": "https://example.com/score.png"
            }),
        ))
        .await;
    assert_eq!(res.status(), StatusCode::OK);
    let j = body_to_json(res).await;
    assert_eq!(j["author"]["display_name"], "Scoreboard");
    assert_eq!(j["author"]["avatar_url"], "https://example.com/score.png");
    assert_eq!(j["webhook_id"].as_str(), Some(id.as_str()));
    assert!(j.get("mention_type").is_none());
}

#[sqlx::test]
async fn execute_error_bodies_match_discord(pool: PgPool) {
    let (app, owner, _, channel) = setup(&pool).await;
    let owner_token = generate_access_token(&app.config, owner);
    let (id, token) = create_webhook(&app, &owner_token, channel).await;

    // Empty body → 400 code 50006.
    let res = app
        .oneshot(public_post(
            &format!("/api/webhooks/{id}/{token}"),
            json!({}),
        ))
        .await;
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    assert_eq!(body_to_json(res).await["code"], 50006);

    // Unknown id → 404 code 10015.
    let res = app
        .oneshot(public_post(
            &format!("/api/webhooks/{}/{token}", Uuid::now_v7()),
            json!({ "content": "x" }),
        ))
        .await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
    assert_eq!(body_to_json(res).await["code"], 10015);

    // Wrong token → 401 code 50027.
    let res = app
        .oneshot(public_post(
            &format!("/api/webhooks/{id}/not-the-token"),
            json!({ "content": "x" }),
        ))
        .await;
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(body_to_json(res).await["code"], 50027);
}

#[sqlx::test]
async fn execute_ignores_unknown_discord_fields(pool: PgPool) {
    let (app, owner, _, channel) = setup(&pool).await;
    let owner_token = generate_access_token(&app.config, owner);
    let (id, token) = create_webhook(&app, &owner_token, channel).await;

    // Everything Discord senders may include but Kaiku v1 doesn't support.
    let res = app
        .oneshot(public_post(
            &format!("/api/webhooks/{id}/{token}"),
            json!({
                "content": "hello",
                "tts": true,
                "allowed_mentions": { "parse": [] },
                "components": [{ "type": 1, "components": [] }],
                "flags": 4096,
                "poll": { "question": { "text": "?" } },
                "applied_tags": ["123"],
                "made_up_field": { "nested": true }
            }),
        ))
        .await;
    assert_eq!(res.status(), StatusCode::NO_CONTENT);
}

#[sqlx::test]
async fn execute_accepts_real_world_discord_embed_payload(pool: PgPool) {
    let (app, owner, _, channel) = setup(&pool).await;
    let owner_token = generate_access_token(&app.config, owner);
    let (id, token) = create_webhook(&app, &owner_token, channel).await;

    // Grafana/Uptime-Kuma style payload: object-form image/thumbnail,
    // footer icon, timestamp, decimal color, provider/type fields.
    let res = app
        .oneshot(public_post(
            &format!("/api/webhooks/{id}/{token}?wait=true"),
            json!({
                "username": "Grafana",
                "embeds": [{
                    "type": "rich",
                    "title": "[Alerting] CPU usage",
                    "url": "https://grafana.example.com/alert/1",
                    "description": "CPU above 90% for 5m",
                    "color": 14037554,
                    "timestamp": "2026-07-14T10:00:00.000Z",
                    "provider": { "name": "should-be-ignored" },
                    "image": { "url": "https://grafana.example.com/render/panel.png", "width": 800 },
                    "thumbnail": { "url": "https://grafana.example.com/logo.png" },
                    "footer": { "text": "Grafana v11", "icon_url": "https://grafana.example.com/fav.png" },
                    "author": { "name": "Alerts", "url": "https://grafana.example.com" },
                    "fields": [
                        { "name": "Instance", "value": "web-1", "inline": true },
                        { "name": "Value", "value": "93%", "inline": true }
                    ]
                }]
            }),
        ))
        .await;
    assert_eq!(res.status(), StatusCode::OK);
    let j = body_to_json(res).await;
    let e = &j["embeds"][0];
    assert_eq!(e["title"], "[Alerting] CPU usage");
    assert_eq!(e["color"], 14037554);
    assert_eq!(e["image"], "https://grafana.example.com/render/panel.png");
    assert_eq!(e["thumbnail"], "https://grafana.example.com/logo.png");
    assert_eq!(
        e["footer"]["icon_url"],
        "https://grafana.example.com/fav.png"
    );
    assert_eq!(e["fields"][1]["value"], "93%");
    assert_eq!(e["timestamp"], "2026-07-14T10:00:00.000Z");
    assert_eq!(j["author"]["display_name"], "Grafana");
}

#[sqlx::test]
async fn execute_embed_limits_enforced(pool: PgPool) {
    let (app, owner, _, channel) = setup(&pool).await;
    let owner_token = generate_access_token(&app.config, owner);
    let (id, token) = create_webhook(&app, &owner_token, channel).await;

    // 11 embeds → 400.
    let embeds: Vec<_> = (0..11)
        .map(|i| json!({ "title": format!("e{i}") }))
        .collect();
    let res = app
        .oneshot(public_post(
            &format!("/api/webhooks/{id}/{token}"),
            json!({ "embeds": embeds }),
        ))
        .await;
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);

    // Non-https image is dropped, not rejected.
    let res = app
        .oneshot(public_post(
            &format!("/api/webhooks/{id}/{token}?wait=true"),
            json!({ "embeds": [{ "title": "t", "image": { "url": "http://plain.example/x.png" } }] }),
        ))
        .await;
    assert_eq!(res.status(), StatusCode::OK);
    let j = body_to_json(res).await;
    assert_eq!(j["embeds"][0]["title"], "t");
    assert!(j["embeds"][0].get("image").is_none());
}

// ============================================================================
// Forum channels
// ============================================================================

#[sqlx::test]
async fn forum_webhook_thread_name_creates_post(pool: PgPool) {
    let (app, owner, guild, _) = setup(&pool).await;
    let owner_token = generate_access_token(&app.config, owner);
    let forum = create_forum_channel(&pool, guild).await;
    let (id, token) = create_webhook(&app, &owner_token, forum).await;

    // No thread_name/thread_id → 400.
    let res = app
        .oneshot(public_post(
            &format!("/api/webhooks/{id}/{token}"),
            json!({ "content": "no thread" }),
        ))
        .await;
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);

    let res = app
        .oneshot(public_post(
            &format!("/api/webhooks/{id}/{token}?wait=true"),
            json!({ "content": "patch notes body", "thread_name": "Patch 1.2" }),
        ))
        .await;
    assert_eq!(res.status(), StatusCode::OK);
    let root_id = body_to_json(res).await["id"].as_str().unwrap().to_string();

    let (title, author_id): (String, Option<Uuid>) =
        sqlx::query_as("SELECT title, author_id FROM forum_posts WHERE root_message_id = $1")
            .bind(Uuid::parse_str(&root_id).unwrap())
            .fetch_one(&pool)
            .await
            .expect("forum post row");
    assert_eq!(title, "Patch 1.2");
    assert_eq!(author_id, None);
}

#[sqlx::test]
async fn forum_webhook_thread_id_replies_into_post(pool: PgPool) {
    let (app, owner, guild, _) = setup(&pool).await;
    let owner_token = generate_access_token(&app.config, owner);
    let forum = create_forum_channel(&pool, guild).await;
    let (id, token) = create_webhook(&app, &owner_token, forum).await;

    // Create the post via the webhook, then reply by post id AND by root id.
    let res = app
        .oneshot(public_post(
            &format!("/api/webhooks/{id}/{token}?wait=true"),
            json!({ "content": "opening", "thread_name": "Server Status" }),
        ))
        .await;
    let root_id = body_to_json(res).await["id"].as_str().unwrap().to_string();
    let (post_id,): (Uuid,) =
        sqlx::query_as("SELECT id FROM forum_posts WHERE root_message_id = $1")
            .bind(Uuid::parse_str(&root_id).unwrap())
            .fetch_one(&pool)
            .await
            .unwrap();

    for thread_ref in [post_id.to_string(), root_id.clone()] {
        let res = app
            .oneshot(public_post(
                &format!("/api/webhooks/{id}/{token}?wait=true&thread_id={thread_ref}"),
                json!({ "content": "update" }),
            ))
            .await;
        assert_eq!(res.status(), StatusCode::OK);
        let j = body_to_json(res).await;
        assert_eq!(j["parent_id"].as_str(), Some(root_id.as_str()));
    }

    let (count,): (i32,) = sqlx::query_as("SELECT thread_reply_count FROM messages WHERE id = $1")
        .bind(Uuid::parse_str(&root_id).unwrap())
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 2);

    // Locked post rejects webhook replies.
    sqlx::query("UPDATE forum_posts SET locked = true WHERE id = $1")
        .bind(post_id)
        .execute(&pool)
        .await
        .unwrap();
    let res = app
        .oneshot(public_post(
            &format!("/api/webhooks/{id}/{token}?thread_id={post_id}"),
            json!({ "content": "nope" }),
        ))
        .await;
    assert_eq!(res.status(), StatusCode::FORBIDDEN);
}

#[sqlx::test]
async fn thread_id_from_another_channel_rejected(pool: PgPool) {
    let (app, owner, guild, channel) = setup(&pool).await;
    let owner_token = generate_access_token(&app.config, owner);
    let (id, token) = create_webhook(&app, &owner_token, channel).await;

    // A root message in a DIFFERENT channel must not be reachable.
    let other = create_channel(&pool, guild, "elsewhere").await;
    let foreign_root = insert_message(&pool, other, owner, "root").await;

    let res = app
        .oneshot(public_post(
            &format!("/api/webhooks/{id}/{token}?thread_id={foreign_root}"),
            json!({ "content": "cross-channel" }),
        ))
        .await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
}

// ============================================================================
// Slack compatibility
// ============================================================================

#[sqlx::test]
async fn slack_route_accepts_json_and_form(pool: PgPool) {
    let (app, owner, _, channel) = setup(&pool).await;
    let owner_token = generate_access_token(&app.config, owner);
    let (id, token) = create_webhook(&app, &owner_token, channel).await;

    // JSON variant with attachment → embed mapping.
    let payload = json!({
        "text": "deploy <https://ci.example.com/1|finished>",
        "username": "CI",
        "attachments": [{
            "color": "#36a64f",
            "title": "Build 42",
            "title_link": "https://ci.example.com/42",
            "text": "All green",
            "fields": [{ "title": "Branch", "value": "main", "short": true }],
            "footer": "ci-bot",
            "ts": 1700000000
        }]
    });
    let res = app
        .oneshot(public_post(
            &format!("/api/webhooks/{id}/{token}/slack"),
            payload.clone(),
        ))
        .await;
    assert_eq!(res.status(), StatusCode::OK);

    // Form-encoded variant (Slack's classic payload= format).
    let form = format!(
        "payload={}",
        url_escape(&json!({ "text": "form variant" }).to_string())
    );
    let res = app
        .oneshot(
            TestApp::request(Method::POST, &format!("/api/webhooks/{id}/{token}/slack"))
                .header("Content-Type", "application/x-www-form-urlencoded")
                .body(Body::from(form))
                .unwrap(),
        )
        .await;
    assert_eq!(res.status(), StatusCode::OK);

    // Verify mapping through history.
    let res = app
        .oneshot(authed(
            Method::GET,
            &format!("/api/messages/channel/{channel}"),
            &owner_token,
            None,
        ))
        .await;
    let j = body_to_json(res).await;
    let items = j["items"].as_array().unwrap();
    assert_eq!(items.len(), 2);
    // Newest first: the form variant.
    assert_eq!(items[0]["content"], "form variant");
    let slack_msg = &items[1];
    assert_eq!(
        slack_msg["content"],
        "deploy [finished](https://ci.example.com/1)"
    );
    assert_eq!(slack_msg["author"]["display_name"], "CI");
    let e = &slack_msg["embeds"][0];
    assert_eq!(e["title"], "Build 42");
    assert_eq!(e["color"], 0x36A64F);
    assert_eq!(e["fields"][0]["name"], "Branch");
    assert_eq!(e["fields"][0]["inline"], true);
    assert_eq!(e["footer"]["text"], "ci-bot");
}

fn url_escape(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            other => format!("%{other:02X}"),
        })
        .collect()
}

// ============================================================================
// Webhook message CRUD via token
// ============================================================================

#[sqlx::test]
async fn webhook_message_get_edit_delete(pool: PgPool) {
    let (app, owner, _, channel) = setup(&pool).await;
    let owner_token = generate_access_token(&app.config, owner);
    let (id, token) = create_webhook(&app, &owner_token, channel).await;

    let res = app
        .oneshot(public_post(
            &format!("/api/webhooks/{id}/{token}?wait=true"),
            json!({ "content": "v1" }),
        ))
        .await;
    let message_id = body_to_json(res).await["id"].as_str().unwrap().to_string();

    // GET
    let res = app
        .oneshot(
            TestApp::request(
                Method::GET,
                &format!("/api/webhooks/{id}/{token}/messages/{message_id}"),
            )
            .body(Body::empty())
            .unwrap(),
        )
        .await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(body_to_json(res).await["content"], "v1");

    // PATCH content + embeds.
    let res = app
        .oneshot(
            TestApp::request(
                Method::PATCH,
                &format!("/api/webhooks/{id}/{token}/messages/{message_id}"),
            )
            .header("Content-Type", "application/json")
            .body(Body::from(
                json!({ "content": "v2", "embeds": [{ "title": "added later" }] }).to_string(),
            ))
            .unwrap(),
        )
        .await;
    assert_eq!(res.status(), StatusCode::OK);
    let j = body_to_json(res).await;
    assert_eq!(j["content"], "v2");
    assert_eq!(j["embeds"][0]["title"], "added later");
    assert!(j["edited_at"].is_string());

    // A message NOT created by this webhook is unreachable (404 code 10008).
    let foreign = insert_message(&pool, channel, owner, "user message").await;
    let res = app
        .oneshot(
            TestApp::request(
                Method::DELETE,
                &format!("/api/webhooks/{id}/{token}/messages/{foreign}"),
            )
            .body(Body::empty())
            .unwrap(),
        )
        .await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
    assert_eq!(body_to_json(res).await["code"], 10008);

    // DELETE own message → 204, then GET → 404.
    let res = app
        .oneshot(
            TestApp::request(
                Method::DELETE,
                &format!("/api/webhooks/{id}/{token}/messages/{message_id}"),
            )
            .body(Body::empty())
            .unwrap(),
        )
        .await;
    assert_eq!(res.status(), StatusCode::NO_CONTENT);
    let res = app
        .oneshot(
            TestApp::request(
                Method::GET,
                &format!("/api/webhooks/{id}/{token}/messages/{message_id}"),
            )
            .body(Body::empty())
            .unwrap(),
        )
        .await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
}

// ============================================================================
// Rate limiting (Discord-shaped 429)
// ============================================================================

/// The 6th request inside the 5/2s per-webhook budget returns Discord's 429
/// body (float `retry_after`, `global: false`) plus a Retry-After header.
/// Needs the Redis-backed limiter, so this uses a real spawned server.
#[sqlx::test]
async fn execute_rate_limit_returns_discord_429(pool: PgPool) {
    use std::collections::HashSet;
    use std::sync::Arc;

    use vc_server::api::{create_router, AppState, AppStateConfig};
    use vc_server::config::Config;
    use vc_server::ratelimit::{LimitConfig, RateLimitConfig, RateLimiter, RateLimits};
    use vc_server::voice::sfu::SfuServer;

    let mut config = Config::default_for_test();
    config.mfa_encryption_key = Some(WEBHOOK_TEST_KEY.to_string());
    let redis = vc_server::db::create_redis_client(&config.redis_url)
        .await
        .expect("test Redis");
    let sfu = SfuServer::new(Arc::new(config.clone()), None).expect("SfuServer");
    let mut limiter = RateLimiter::new(
        redis.clone(),
        RateLimitConfig {
            enabled: true,
            redis_key_prefix: format!("test:rl:{}", Uuid::new_v4()),
            fail_open: false,
            trust_proxy: false,
            allowlist: HashSet::new(),
            limits: RateLimits {
                webhook_execute: LimitConfig {
                    requests: 5,
                    window_secs: 2,
                },
                // Keep the coarse per-IP Write limit out of the way.
                write: LimitConfig {
                    requests: 1000,
                    window_secs: 60,
                },
                ..RateLimits::default()
            },
        },
    );
    limiter.init().await.expect("init limiter");

    let state = AppState::new(AppStateConfig {
        db: pool.clone(),
        redis,
        config: config.clone(),
        s3: None,
        sfu,
        rate_limiter: Some(limiter),
        screen_share_limiter: None,
        email: None,
        oidc_manager: None,
        http_client: reqwest::Client::new(),
    });
    let server = crate::helpers::spawn_test_server(create_router(state)).await;

    // Seed guild/channel/webhook directly (the server is rate limited).
    let (owner, _) = create_test_user(&pool).await;
    let guild = create_guild_with_default_role(&pool, owner, GuildPermissions::VIEW_CHANNEL).await;
    let channel = create_channel(&pool, guild, "alerts").await;
    let webhook_id = Uuid::now_v7();
    let webhook_token = "t".repeat(68);
    sqlx::query(
        "INSERT INTO incoming_webhooks (id, guild_id, channel_id, name, token, created_by)
         VALUES ($1, $2, $3, 'RL Test', $4, $5)",
    )
    .bind(webhook_id)
    .bind(guild)
    .bind(channel)
    .bind(&webhook_token)
    .bind(owner)
    .execute(&pool)
    .await
    .expect("seed webhook");

    let client = reqwest::Client::new();
    let url = format!("{}/api/webhooks/{webhook_id}/{webhook_token}", server.url);
    for i in 0..5 {
        let res = client
            .post(&url)
            .json(&json!({ "content": format!("msg {i}") }))
            .send()
            .await
            .expect("send");
        assert_eq!(res.status(), 204, "request {i} should pass");
    }
    let res = client
        .post(&url)
        .json(&json!({ "content": "over budget" }))
        .send()
        .await
        .expect("send");
    assert_eq!(res.status(), 429);
    assert!(res.headers().get("retry-after").is_some());
    let body: serde_json::Value = res.json().await.expect("json body");
    assert_eq!(body["global"], false);
    assert!(body["retry_after"].is_f64() || body["retry_after"].is_u64());
    assert_eq!(body["message"], "You are being rate limited.");
}

// ============================================================================
// Author snapshot survives webhook deletion
// ============================================================================

#[sqlx::test]
async fn history_keeps_author_after_webhook_deletion(pool: PgPool) {
    let (app, owner, _, channel) = setup(&pool).await;
    let owner_token = generate_access_token(&app.config, owner);
    let (id, token) = create_webhook(&app, &owner_token, channel).await;

    let res = app
        .oneshot(public_post(
            &format!("/api/webhooks/{id}/{token}"),
            json!({ "content": "before deletion", "username": "Snapshot Name" }),
        ))
        .await;
    assert_eq!(res.status(), StatusCode::NO_CONTENT);

    let res = app
        .oneshot(authed(
            Method::DELETE,
            &format!("/api/webhooks/{id}"),
            &owner_token,
            None,
        ))
        .await;
    assert_eq!(res.status(), StatusCode::NO_CONTENT);

    let res = app
        .oneshot(authed(
            Method::GET,
            &format!("/api/messages/channel/{channel}"),
            &owner_token,
            None,
        ))
        .await;
    let j = body_to_json(res).await;
    let msg = &j["items"][0];
    assert_eq!(msg["content"], "before deletion");
    // webhook_id was SET NULL, but the snapshot columns keep the identity.
    assert_eq!(msg["author"]["display_name"], "Snapshot Name");
}
