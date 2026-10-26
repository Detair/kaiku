//! Every machine-readable error code on the wire is UPPER_SNAKE
//! (`^[A-Z0-9_]+$`). Hits error paths across modules over HTTP; the per-enum
//! coverage lives in the `error_code_tests` unit module of the server crate.

use reqwest::StatusCode;
use sqlx::PgPool;
use vc_server::permissions::GuildPermissions;

use super::helpers::{
    add_guild_member, create_channel, create_dm_channel, create_elevated_session,
    create_guild_with_default_role, create_test_user, generate_access_token, make_admin,
    spawn_test_server, TestApp, TestServer,
};

fn assert_upper_snake(body: &serde_json::Value, key: &str, expected: &str) {
    let code = body[key]
        .as_str()
        .unwrap_or_else(|| panic!("no string `{key}` in {body}"));
    assert!(
        !code.is_empty()
            && code
                .bytes()
                .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_'),
        "code {code:?} is not UPPER_SNAKE in {body}"
    );
    assert_eq!(code, expected, "{body}");
}

async fn json_of(resp: reqwest::Response, status: StatusCode) -> serde_json::Value {
    assert_eq!(resp.status(), status);
    resp.json().await.expect("error JSON")
}

async fn server(pool: &PgPool) -> (TestApp, TestServer) {
    let app = TestApp::with_pool(pool.clone()).await;
    let server = spawn_test_server(app.router.clone()).await;
    (app, server)
}

#[sqlx::test]
async fn admin_validation_code_is_upper_snake(pool: PgPool) {
    let (app, server) = server(&pool).await;
    let (admin, _) = create_test_user(&pool).await;
    make_admin(&pool, admin).await;
    let token = create_elevated_session(&pool, &app.config, admin).await;

    let resp = reqwest::Client::new()
        .post(format!("{}/api/admin/users", server.url))
        .bearer_auth(&token)
        .json(&serde_json::json!({
            "username": "Bad Name",
            "display_name": null,
            "temporary_password": "correct horse battery",
        }))
        .send()
        .await
        .unwrap();
    let body = json_of(resp, StatusCode::BAD_REQUEST).await;
    assert_upper_snake(&body, "error", "VALIDATION_ERROR");
}

#[sqlx::test]
async fn elevation_required_code_is_upper_snake(pool: PgPool) {
    let (app, server) = server(&pool).await;
    let (admin, _) = create_test_user(&pool).await;
    make_admin(&pool, admin).await;
    let token = generate_access_token(&app.config, admin);

    let resp = reqwest::Client::new()
        .get(format!("{}/api/admin/reports", server.url))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap();
    let body = json_of(resp, StatusCode::FORBIDDEN).await;
    assert_upper_snake(&body, "error", "ELEVATION_REQUIRED");
}

#[sqlx::test]
async fn call_not_found_code_is_upper_snake(pool: PgPool) {
    let (app, server) = server(&pool).await;
    let (a, _) = create_test_user(&pool).await;
    let (b, _) = create_test_user(&pool).await;
    let dm = create_dm_channel(&pool, a, b).await;
    let token = generate_access_token(&app.config, a);

    let resp = reqwest::Client::new()
        .post(format!("{}/api/dm/{dm}/call/join", server.url))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap();
    // The call API carries its machine code in `code`; `error` is free text.
    let body = json_of(resp, StatusCode::NOT_FOUND).await;
    assert_upper_snake(&body, "code", "CALL_NOT_FOUND");
}

#[sqlx::test]
async fn chat_forbidden_code_is_upper_snake(pool: PgPool) {
    let (app, server) = server(&pool).await;
    let (owner, _) = create_test_user(&pool).await;
    let (member, _) = create_test_user(&pool).await;
    let perms = GuildPermissions::VIEW_CHANNEL | GuildPermissions::SEND_MESSAGES;
    let guild = create_guild_with_default_role(&pool, owner, perms).await;
    add_guild_member(&pool, guild, member).await;
    let channel = create_channel(&pool, guild, "codes").await;
    let token = generate_access_token(&app.config, member);

    let resp = reqwest::Client::new()
        .patch(format!("{}/api/channels/{channel}", server.url))
        .bearer_auth(&token)
        .json(&serde_json::json!({ "name": "renamed" }))
        .send()
        .await
        .unwrap();
    let body = json_of(resp, StatusCode::FORBIDDEN).await;
    assert_upper_snake(&body, "error", "FORBIDDEN");
}

#[sqlx::test]
async fn invalid_credentials_code_is_upper_snake(pool: PgPool) {
    let (_app, server) = server(&pool).await;
    let resp = reqwest::Client::new()
        .post(format!("{}/auth/login", server.url))
        .json(&serde_json::json!({ "username": "nobody", "password": "wrong-password-1" }))
        .send()
        .await
        .unwrap();
    let body = json_of(resp, StatusCode::UNAUTHORIZED).await;
    assert_upper_snake(&body, "error", "INVALID_CREDENTIALS");
}
