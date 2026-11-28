//! Configuration that clients read over HTTP: ICE servers for voice and the
//! minimum client versions in the public settings.

use axum::body::Body;
use axum::http::Method;
use sqlx::PgPool;
use vc_server::config::Config;

use super::helpers::{body_to_json, create_test_user, generate_access_token, TestApp};

async fn get_json(app: &TestApp, uri: &str, token: Option<&str>) -> serde_json::Value {
    let mut request = TestApp::request(Method::GET, uri);
    if let Some(token) = token {
        request = request.header("Authorization", format!("Bearer {token}"));
    }
    let response = app.oneshot(request.body(Body::empty()).unwrap()).await;
    assert_eq!(response.status(), 200, "GET {uri}");
    body_to_json(response).await
}

/// `TURN_SERVER` may list several URLs; clients get all of them in one
/// ICE server entry with one set of credentials.
#[sqlx::test]
async fn ice_servers_list_every_turn_url(pool: PgPool) {
    let mut config = Config::default_for_test();
    config.turn_servers = vec![
        "turn:turn.example.test:3478".to_owned(),
        "turn:turn.example.test:3478?transport=tcp".to_owned(),
        "turns:turn.example.test:443?transport=tcp".to_owned(),
    ];
    config.turn_shared_secret = Some("secret".to_owned());
    let app = TestApp::with_pool_and_config(pool.clone(), config.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    let token = generate_access_token(&app.config, user);

    let body = get_json(&app, "/api/voice/ice-servers", Some(&token)).await;
    let servers = body["ice_servers"].as_array().expect("ice_servers");
    let turn: Vec<&serde_json::Value> = servers
        .iter()
        .filter(|s| s["urls"][0].as_str().is_some_and(|u| u.starts_with("turn")))
        .collect();
    assert_eq!(turn.len(), 1, "{body}");
    assert_eq!(
        turn[0]["urls"],
        serde_json::json!(config.turn_servers),
        "{body}"
    );
    assert!(turn[0]["username"].is_string());
    assert!(turn[0]["credential"].is_string());
}

/// The public settings carry the minimum client version per platform from
/// the configuration.
#[sqlx::test]
async fn settings_return_configured_min_client_versions(pool: PgPool) {
    let mut config = Config::default_for_test();
    config.min_client_version.web = Some("1.0.0".to_owned());
    config.min_client_version.android = Some("1.0.0-rc.6".to_owned());
    let app = TestApp::with_pool_and_config(pool, config).await;

    let body = get_json(&app, "/api/settings", None).await;
    assert_eq!(
        body["min_client_version"],
        serde_json::json!({ "web": "1.0.0", "desktop": null, "android": "1.0.0-rc.6" }),
        "{body}"
    );
}

/// Without configuration every platform's minimum is `null`.
#[sqlx::test]
async fn settings_min_client_versions_are_null_when_unset(pool: PgPool) {
    let app = TestApp::with_pool(pool).await;
    let body = get_json(&app, "/api/settings", None).await;
    assert_eq!(
        body["min_client_version"],
        serde_json::json!({ "web": null, "desktop": null, "android": null }),
        "{body}"
    );
}
