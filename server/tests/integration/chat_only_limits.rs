//! What a chat-only (probation) session may no longer do (C27 "What each may
//! do"): list or delete push subscriptions by id, delete a guild, add an
//! end-to-end-encryption device identity or replace the key backup. Reading
//! keys and claiming another user's prekey stay open.
//!
//! Run with: `cargo test --test integration chat_only_limits`

use axum::body::Body;
use axum::http::Method;
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use serde_json::{json, Value};
use sqlx::PgPool;
use uuid::Uuid;

use super::helpers::login::peer_request;
use super::helpers::{
    body_to_json_lenient, create_guild, create_probation_session_with_token,
    create_session_with_token, create_test_user, TestApp,
};

async fn send(app: &TestApp, method: Method, uri: &str, token: &str, body: Value) -> (u16, Value) {
    let req = peer_request(method, uri)
        .header("Authorization", format!("Bearer {token}"))
        .header("Content-Type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();
    let resp = app.oneshot(req).await;
    let status = resp.status().as_u16();
    (status, body_to_json_lenient(resp).await)
}

fn assert_full_required(status: u16, body: &Value, what: &str) {
    assert_eq!(
        (status, body["error"].as_str()),
        (403, Some("FULL_SESSION_REQUIRED")),
        "{what}: {body}"
    );
    // Today's body: the code and the human-readable message, nothing else.
    assert!(body["message"].as_str().is_some(), "{what}: {body}");
}

async fn insert_push_row(pool: &PgPool, user: Uuid, endpoint: &str) -> Uuid {
    sqlx::query_scalar(
        "INSERT INTO push_subscriptions (user_id, provider, endpoint, device_id)
         VALUES ($1, 'unifiedpush', $2, $3) RETURNING id",
    )
    .bind(user)
    .bind(endpoint)
    .bind(Uuid::new_v4())
    .fetch_one(pool)
    .await
    .unwrap()
}

async fn push_rows(pool: &PgPool, user: Uuid) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM push_subscriptions WHERE user_id = $1")
        .bind(user)
        .fetch_one(pool)
        .await
        .unwrap()
}

#[sqlx::test]
async fn probation_cannot_delete_push_subscription(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    let row = insert_push_row(&pool, user, "https://push.example.com/phone").await;
    let (_, chat_only) = create_probation_session_with_token(&pool, &app.config, user).await;

    let uri = format!("/api/me/push-subscriptions/{row}");
    let (status, body) = send(&app, Method::DELETE, &uri, &chat_only, json!({})).await;
    assert_full_required(status, &body, "DELETE by id");
    assert_eq!(push_rows(&pool, user).await, 1, "the row stays");

    let (_, full) = create_session_with_token(&pool, &app.config, user).await;
    let (status, body) = send(&app, Method::DELETE, &uri, &full, json!({})).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(push_rows(&pool, user).await, 0);
}

#[sqlx::test]
async fn probation_cannot_list_push(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    insert_push_row(&pool, user, "https://push.example.com/phone").await;
    let (_, chat_only) = create_probation_session_with_token(&pool, &app.config, user).await;

    let (status, body) = send(
        &app,
        Method::GET,
        "/api/me/push-subscriptions",
        &chat_only,
        json!({}),
    )
    .await;
    assert_full_required(status, &body, "GET list");
    assert!(
        !body.to_string().contains("push.example.com"),
        "nothing of the rows leaks: {body}"
    );

    let (_, full) = create_session_with_token(&pool, &app.config, user).await;
    let (status, body) = send(
        &app,
        Method::GET,
        "/api/me/push-subscriptions",
        &full,
        json!({}),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body.as_array().map(Vec::len), Some(1));

    // The transport list stays open to every session.
    let (status, body) = send(
        &app,
        Method::GET,
        "/api/me/push-subscriptions/config",
        &chat_only,
        json!({}),
    )
    .await;
    assert_eq!(status, 200, "{body}");
}

async fn guild_exists(pool: &PgPool, guild: Uuid) -> bool {
    sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM guilds WHERE id = $1)")
        .bind(guild)
        .fetch_one(pool)
        .await
        .unwrap()
}

#[sqlx::test]
async fn probation_cannot_delete_guild(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (owner, _) = create_test_user(&pool).await;
    let guild = create_guild(&pool, owner).await;
    let (_, chat_only) = create_probation_session_with_token(&pool, &app.config, owner).await;

    let uri = format!("/api/guilds/{guild}");
    let (status, body) = send(&app, Method::DELETE, &uri, &chat_only, json!({})).await;
    assert_full_required(status, &body, "DELETE guild");
    assert!(guild_exists(&pool, guild).await);

    // Reading and renaming stay open to the owner's chat-only session.
    let (status, body) = send(&app, Method::GET, &uri, &chat_only, json!({})).await;
    assert_eq!(status, 200, "{body}");
}

#[sqlx::test]
async fn full_can_delete_guild(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (owner, _) = create_test_user(&pool).await;
    let guild = create_guild(&pool, owner).await;
    let (_, full) = create_session_with_token(&pool, &app.config, owner).await;

    let uri = format!("/api/guilds/{guild}");
    // A full session of someone else is still not the owner.
    let (other, _) = create_test_user(&pool).await;
    let (_, other_full) = create_session_with_token(&pool, &app.config, other).await;
    let (status, body) = send(&app, Method::DELETE, &uri, &other_full, json!({})).await;
    assert_eq!(status, 403, "{body}");
    assert_ne!(body["error"], "FULL_SESSION_REQUIRED", "{body}");

    let (status, body) = send(&app, Method::DELETE, &uri, &full, json!({})).await;
    assert_eq!(status, 204, "{body}");
    assert!(!guild_exists(&pool, guild).await);
}

fn key_upload() -> Value {
    let account = vc_crypto::olm::OlmAccount::new();
    let keys = account.identity_keys();
    json!({
        "device_name": "Desktop",
        "identity_key_ed25519": keys.ed25519,
        "identity_key_curve25519": keys.curve25519,
        "one_time_prekeys": [],
    })
}

async fn e2ee_devices(pool: &PgPool, user: Uuid) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM user_devices WHERE user_id = $1")
        .bind(user)
        .fetch_one(pool)
        .await
        .unwrap()
}

#[sqlx::test]
async fn probation_cannot_upload_keys(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    let (_, chat_only) = create_probation_session_with_token(&pool, &app.config, user).await;

    let (status, body) = send(
        &app,
        Method::POST,
        "/api/keys/upload",
        &chat_only,
        key_upload(),
    )
    .await;
    assert_full_required(status, &body, "POST /api/keys/upload");
    assert_eq!(e2ee_devices(&pool, user).await, 0, "no identity was added");

    let (_, full) = create_session_with_token(&pool, &app.config, user).await;
    let (status, body) = send(&app, Method::POST, "/api/keys/upload", &full, key_upload()).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(e2ee_devices(&pool, user).await, 1);

    // Reading the own devices stays open.
    let (status, body) = send(
        &app,
        Method::GET,
        "/api/keys/devices",
        &chat_only,
        json!({}),
    )
    .await;
    assert_eq!(status, 200, "{body}");
}

fn backup(version: i32) -> Value {
    json!({
        "salt": STANDARD.encode([1u8; 16]),
        "nonce": STANDARD.encode([2u8; 12]),
        "ciphertext": STANDARD.encode(b"sealed backup"),
        "version": version,
    })
}

#[sqlx::test]
async fn probation_cannot_replace_backup(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    let (_, full) = create_session_with_token(&pool, &app.config, user).await;
    let (status, body) = send(&app, Method::POST, "/api/keys/backup", &full, backup(1)).await;
    assert!(
        status == 200 || status == 201 || status == 204,
        "{status} {body}"
    );

    let (_, chat_only) = create_probation_session_with_token(&pool, &app.config, user).await;
    let (status, body) = send(
        &app,
        Method::POST,
        "/api/keys/backup",
        &chat_only,
        backup(2),
    )
    .await;
    assert_full_required(status, &body, "POST /api/keys/backup");
    let version: i32 = sqlx::query_scalar("SELECT version FROM key_backups WHERE user_id = $1")
        .bind(user)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(version, 1, "the backup was not replaced");

    // Reading the backup and its status stays open (restoring on a new PC).
    for uri in ["/api/keys/backup", "/api/keys/backup/status"] {
        let (status, body) = send(&app, Method::GET, uri, &chat_only, json!({})).await;
        assert_eq!(status, 200, "GET {uri}: {body}");
    }
}

#[sqlx::test]
async fn probation_can_claim_prekey(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (owner, _) = create_test_user(&pool).await;
    let (claimer, _) = create_test_user(&pool).await;
    let keys = vc_crypto::olm::OlmAccount::new().identity_keys();
    let device: Uuid = sqlx::query_scalar(
        "INSERT INTO user_devices (user_id, device_name, identity_key_ed25519, identity_key_curve25519)
         VALUES ($1, 'Desktop', $2, $3) RETURNING id",
    )
    .bind(owner)
    .bind(&keys.ed25519)
    .bind(&keys.curve25519)
    .fetch_one(&pool)
    .await
    .unwrap();
    sqlx::query("INSERT INTO prekeys (device_id, key_id, public_key) VALUES ($1, 'k1', $2)")
        .bind(device)
        .bind(&keys.curve25519)
        .execute(&pool)
        .await
        .unwrap();
    let (_, chat_only) = create_probation_session_with_token(&pool, &app.config, claimer).await;

    let (status, body) = send(
        &app,
        Method::GET,
        &format!("/api/users/{owner}/keys"),
        &chat_only,
        json!({}),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    let (status, body) = send(
        &app,
        Method::POST,
        &format!("/api/users/{owner}/keys/claim"),
        &chat_only,
        json!({"device_id": device}),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["one_time_prekey"]["key_id"], "k1", "{body}");
}
