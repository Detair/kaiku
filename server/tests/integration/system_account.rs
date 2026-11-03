//! The "Kaiku" system account (C28, spec Part C): fixed id, refused on every
//! auth path, hidden from user lists, search, friends, bots and DM creation.
//!
//! Run with: `cargo test --test integration system_account`

use axum::body::Body;
use axum::http::Method;
use sqlx::PgPool;
use uuid::Uuid;
use vc_server::system_account::{SYSTEM_USERNAME, SYSTEM_USER_ID};

use super::helpers::login::peer_request;
use super::helpers::{
    body_to_json, create_elevated_session, create_guild, create_test_user, generate_access_token,
    make_admin, TestApp,
};

async fn send_json(
    app: &TestApp,
    method: Method,
    uri: &str,
    token: Option<&str>,
    body: serde_json::Value,
) -> (u16, serde_json::Value) {
    let mut req = peer_request(method, uri).header("Content-Type", "application/json");
    if let Some(t) = token {
        req = req.header("Authorization", format!("Bearer {t}"));
    }
    let resp = app
        .oneshot(req.body(Body::from(body.to_string())).unwrap())
        .await;
    let status = resp.status().as_u16();
    (status, body_to_json(resp).await)
}

async fn get_text(app: &TestApp, uri: &str, token: &str) -> (u16, String) {
    let req = TestApp::request(Method::GET, uri)
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await;
    let status = resp.status().as_u16();
    let bytes = http_body_util::BodyExt::collect(resp.into_body())
        .await
        .unwrap()
        .to_bytes();
    (status, String::from_utf8_lossy(&bytes).into_owned())
}

#[sqlx::test]
async fn system_account_exists_with_fixed_id(pool: PgPool) {
    let row: (Uuid, String, String, bool, Option<String>, Option<String>) = sqlx::query_as(
        "SELECT id, username, display_name, is_system, password_hash, avatar_url FROM users WHERE is_system",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(row.0, SYSTEM_USER_ID);
    assert_eq!(row.0.to_string(), "6b61696b-7500-4000-8000-000000000001");
    assert_eq!(row.1, "kaiku");
    assert_eq!(row.1, SYSTEM_USERNAME);
    assert_eq!(row.2, "Kaiku");
    assert!(row.3);
    assert!(row.4.is_none(), "no password");
    assert!(row.5.is_none(), "no avatar");
    assert!(vc_server::system_account::is_system_user(SYSTEM_USER_ID));
    assert!(!vc_server::system_account::is_system_user(Uuid::new_v4()));

    // A second system account cannot exist.
    let err = sqlx::query(
        "INSERT INTO users (id, username, display_name, auth_method, is_system)
         VALUES ($1, 'kaiku2', 'Kaiku', 'local', true)",
    )
    .bind(Uuid::new_v4())
    .execute(&pool)
    .await;
    assert!(err.is_err());
}

#[sqlx::test]
async fn system_account_refused_on_every_auth_path(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;

    // Password login: identical to an unknown user.
    let (s_sys, b_sys) = send_json(
        &app,
        Method::POST,
        "/auth/login",
        None,
        serde_json::json!({"username": "kaiku", "password": "whatever-123"}),
    )
    .await;
    let (s_unknown, b_unknown) = send_json(
        &app,
        Method::POST,
        "/auth/login",
        None,
        serde_json::json!({"username": "nobody_here_x", "password": "whatever-123"}),
    )
    .await;
    assert_eq!(s_sys, 401);
    assert_eq!(s_sys, s_unknown);
    assert_eq!(b_sys, b_unknown);
    assert_eq!(b_sys["error"], "INVALID_CREDENTIALS");

    // A token minted for the system account's id.
    let token = generate_access_token(&app.config, SYSTEM_USER_ID);
    let req = TestApp::request(Method::GET, "/auth/me")
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 401);
    assert_eq!(body_to_json(resp).await["error"], "INVALID_TOKEN");

    // Registration of the reserved name, like any taken name.
    let (s, b) = send_json(
        &app,
        Method::POST,
        "/auth/register",
        None,
        serde_json::json!({"username": "kaiku", "password": "long-enough-pw-1"}),
    )
    .await;
    assert_eq!(s, 409, "{b}");
    assert_eq!(b["error"], "USER_EXISTS");
}

#[sqlx::test]
async fn system_account_hidden_everywhere(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (admin, _) = create_test_user(&pool).await;
    make_admin(&pool, admin).await;
    let admin_token = create_elevated_session(&pool, &app.config, admin).await;
    let (user, _) = create_test_user(&pool).await;
    let user_token = generate_access_token(&app.config, user);

    // Admin user list and its count.
    let (s, list) = get_text(&app, "/api/admin/users?limit=100", &admin_token).await;
    assert_eq!(s, 200, "{list}");
    let list: serde_json::Value = serde_json::from_str(&list).unwrap();
    let items = list["items"].as_array().expect("items");
    assert!(items.iter().all(|u| u["id"] != SYSTEM_USER_ID.to_string()));
    assert!(items.iter().any(|u| u["id"] == user.to_string()));
    let total = list["total"].as_i64().unwrap();
    assert_eq!(total, items.len() as i64);
    let (s, list) = get_text(&app, "/api/admin/users?search=kaiku", &admin_token).await;
    assert_eq!(s, 200);
    let list: serde_json::Value = serde_json::from_str(&list).unwrap();
    assert_eq!(list["items"].as_array().unwrap().len(), 0);
    assert_eq!(list["total"], 0);

    // Export.
    let (s, csv) = get_text(&app, "/api/admin/users/export", &admin_token).await;
    assert_eq!(s, 200);
    assert!(!csv.contains(&SYSTEM_USER_ID.to_string()), "{csv}");
    assert!(csv.contains(&user.to_string()));

    // Friend request by name and by id.
    for target in [SYSTEM_USERNAME.to_string(), SYSTEM_USER_ID.to_string()] {
        let (s, b) = send_json(
            &app,
            Method::POST,
            "/api/friends/request",
            Some(&user_token),
            serde_json::json!({"username": target}),
        )
        .await;
        assert_eq!(s, 404, "{b}");
        assert_eq!(b["error"], "USER_NOT_FOUND");
    }

    // DM creation with the system account: not found, like an unknown id.
    let (s_sys, b_sys) = send_json(
        &app,
        Method::POST,
        "/api/dm",
        Some(&user_token),
        serde_json::json!({"participant_ids": [SYSTEM_USER_ID]}),
    )
    .await;
    let (s_unknown, b_unknown) = send_json(
        &app,
        Method::POST,
        "/api/dm",
        Some(&user_token),
        serde_json::json!({"participant_ids": [Uuid::new_v4()]}),
    )
    .await;
    assert_eq!(s_sys, s_unknown);
    assert_eq!(b_sys, b_unknown);

    // Bot paths: the system account is never a bot.
    let guild = create_guild(&pool, user).await;
    let (s, _) = send_json(
        &app,
        Method::POST,
        &format!("/api/guilds/{guild}/bots/{SYSTEM_USER_ID}/add"),
        Some(&user_token),
        serde_json::json!({}),
    )
    .await;
    assert!(s == 404 || s == 400, "bot add answered {s}");

    // Mention resolution never returns the system account.
    let ids = vc_server::chat::mentions::resolve_usernames(&pool, &["kaiku".to_string()])
        .await
        .unwrap();
    assert!(ids.is_empty());
}

/// Review round 1 (Codex m6, Fable m8): a pre-existing `kaiku` account is
/// renamed to a free name, even when the first candidate is taken, and the
/// rename is in the admin audit log.
#[sqlx::test(migrations = false)]
async fn pre_existing_kaiku_is_renamed_to_a_free_name(pool: PgPool) {
    use super::helpers::migrations::{apply_from, apply_until};
    apply_until(&pool, 20_261_120_000_000).await;
    let owner = Uuid::parse_str("0123abcd-4567-4890-8abc-def012345678").unwrap();
    sqlx::query(
        "INSERT INTO users (id, username, display_name, password_hash)
         VALUES ($1, 'kaiku', 'K', 'x'), (gen_random_uuid(), 'kaiku_0123abcd', 'Taken', 'x')",
    )
    .bind(owner)
    .execute(&pool)
    .await
    .unwrap();
    apply_from(&pool, 20_261_120_000_000).await;
    let name: String = sqlx::query_scalar("SELECT username FROM users WHERE id = $1")
        .bind(owner)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(name, "kaiku_0123abcd4567", "the next free candidate");
    let audit: (Uuid, serde_json::Value) = sqlx::query_as(
        "SELECT actor_id, details FROM system_audit_log
          WHERE action = 'system.username_reserved' AND target_id = $1",
    )
    .bind(owner)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(audit.0, SYSTEM_USER_ID);
    assert_eq!(
        audit.1,
        serde_json::json!({"old_username": "kaiku", "new_username": "kaiku_0123abcd4567"})
    );
}
