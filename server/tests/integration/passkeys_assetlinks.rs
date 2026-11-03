//! `/.well-known/assetlinks.json` (LP Task 10; C29).

use axum::body::Body;
use axum::http::Method;
use serde_json::{json, Value};
use sqlx::PgPool;
use vc_server::approvals::secrets::b64u_encode;

use crate::helpers::passkeys::{test_passkey_rp, TEST_CERT_HEX};
use crate::helpers::{body_to_json, TestApp};

async fn get(app: &TestApp) -> (u16, Option<String>, Option<String>, Value) {
    let req = TestApp::request(Method::GET, "/.well-known/assetlinks.json")
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await;
    let status = resp.status().as_u16();
    let h = |n: &str| {
        resp.headers()
            .get(n)
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned)
    };
    let (ct, cc) = (h("content-type"), h("cache-control"));
    (status, ct, cc, body_to_json(resp).await)
}

fn statement(app_id: &str) -> Value {
    json!({
        "relation": [
            "delegate_permission/common.get_login_creds",
            "delegate_permission/common.handle_all_urls"
        ],
        "target": {
            "namespace": "android_app",
            "package_name": app_id,
            "sha256_cert_fingerprints": [TEST_CERT_HEX]
        }
    })
}

#[sqlx::test]
async fn assetlinks_lists_both_apps_with_fingerprint(pool: PgPool) {
    let app = TestApp::with_passkeys(pool).await;
    let (s, _, _, b) = get(&app).await;
    assert_eq!(s, 200);
    assert_eq!(
        b,
        json!([
            statement("io.wolftown.kaiku"),
            statement("io.wolftown.kaiku.simple")
        ])
    );
}

#[sqlx::test]
async fn assetlinks_is_json(pool: PgPool) {
    let app = TestApp::with_passkeys(pool).await;
    let (_, ct, cc, _) = get(&app).await;
    assert_eq!(ct.as_deref(), Some("application/json"));
    assert_eq!(cc.as_deref(), Some("public, max-age=3600"));
}

#[sqlx::test]
async fn assetlinks_empty_when_android_disabled(pool: PgPool) {
    // No passkey RP at all.
    let app = TestApp::with_pool(pool.clone()).await;
    assert_eq!(get(&app).await.3, json!([]));
    // An RP with Android turned off.
    let mut config = crate::helpers::login::login_config();
    let rp = vc_server::auth::passkeys::rp::PasskeyRp::new(
        vc_server::auth::passkeys::config::PasskeyConfig::new(
            &config.public_origin,
            Some("none"),
            None,
        )
        .unwrap(),
    )
    .unwrap();
    config.passkeys = Some(std::sync::Arc::new(rp));
    let app = TestApp::with_pool_and_config(pool, config).await;
    assert_eq!(get(&app).await.3, json!([]));
}

#[sqlx::test]
async fn assetlinks_and_origin_use_the_same_certificate(pool: PgPool) {
    let app = TestApp::with_passkeys(pool).await;
    let rp = test_passkey_rp();
    for st in get(&app).await.3.as_array().unwrap() {
        for fp in st["target"]["sha256_cert_fingerprints"].as_array().unwrap() {
            let bytes = hex::decode(fp.as_str().unwrap().replace(':', "")).unwrap();
            let origin = format!("android:apk-key-hash:{}", b64u_encode(&bytes));
            assert!(rp.allowed_origins().contains(&origin), "{origin}");
        }
    }
}
