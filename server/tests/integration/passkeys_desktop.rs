//! Desktop passkey flows in the system browser with the loopback hand-off
//! (LP Task 9; F1 rev 4, C29 steps 1–7). The test plays the browser: it
//! reads the pages, answers with a soft passkey and posts the forms itself.

use axum::body::Body;
use axum::http::Method;
use base64::Engine;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use uuid::Uuid;
use vc_server::auth::passkeys::queries::{self, PasskeyRow, PasskeyState};

use crate::helpers::login::{peer_request, TEST_ORIGIN};
use crate::helpers::passkeys::{limited_router, seed_passkey, SoftAuthenticator};
use crate::helpers::{create_session_with_token, create_test_user, TestApp};

const CALLBACK_PATH: &str = "AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8";
const PORT: u16 = 49_152;
const VERIFIER: &str = "a-verifier-of-at-least-forty-three-characters-0123456789";

fn challenge(verifier: &str) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

struct Resp {
    status: u16,
    headers: axum::http::HeaderMap,
    body: String,
}

impl Resp {
    fn json(&self) -> Value {
        serde_json::from_str(&self.body).unwrap_or(Value::Null)
    }
    fn header(&self, name: &str) -> Option<&str> {
        self.headers.get(name).and_then(|v| v.to_str().ok())
    }
}

async fn send(router: &axum::Router, req: axum::http::Request<Body>) -> Resp {
    let resp = tower::ServiceExt::oneshot(router.clone(), req)
        .await
        .unwrap();
    let status = resp.status().as_u16();
    let headers = resp.headers().clone();
    let bytes = http_body_util::BodyExt::collect(resp.into_body())
        .await
        .unwrap()
        .to_bytes();
    Resp {
        status,
        headers,
        body: String::from_utf8_lossy(&bytes).into_owned(),
    }
}

async fn api(
    router: &axum::Router,
    path: &str,
    body: Value,
    desktop: bool,
    bearer: Option<&str>,
) -> Resp {
    let mut req = peer_request(Method::POST, path).header("Content-Type", "application/json");
    if desktop {
        req = req.header("X-Kaiku-Client", "desktop");
    }
    if let Some(t) = bearer {
        req = req.header("Authorization", format!("Bearer {t}"));
    }
    send(router, req.body(Body::from(body.to_string())).unwrap()).await
}

async fn page(router: &axum::Router, path: &str, lang: Option<&str>) -> Resp {
    let mut req = peer_request(Method::GET, path);
    if let Some(l) = lang {
        req = req.header("Accept-Language", l);
    }
    send(router, req.body(Body::empty()).unwrap()).await
}

async fn form(router: &axum::Router, path: &str, fields: &[(&str, &str)]) -> Resp {
    let body = serde_urlencoded::to_string(fields).unwrap();
    let req = peer_request(Method::POST, path)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(Body::from(body))
        .unwrap();
    send(router, req).await
}

fn start_body(purpose: &str, action: Option<&str>, target: Option<&str>) -> Value {
    json!({
        "purpose": purpose,
        "action": action,
        "target": target,
        "loopback_port": PORT,
        "callback_path": CALLBACK_PATH,
        "client_challenge": challenge(VERIFIER),
    })
}

/// The hidden field `name` of a hand-off page.
fn hidden(html: &str, name: &str) -> Option<String> {
    let needle = format!("name=\"{name}\" value=\"");
    let start = html.find(&needle)? + needle.len();
    let end = html[start..].find('"')? + start;
    Some(html[start..end].to_string())
}

struct Flow {
    id: String,
    path: String,
}

async fn start(router: &axum::Router, body: Value, bearer: Option<&str>) -> Flow {
    let r = api(router, "/auth/passkeys/desktop/flows", body, true, bearer).await;
    assert_eq!(r.status, 201, "{}", r.body);
    let id = r.json()["flow_id"].as_str().unwrap().to_string();
    Flow {
        path: format!("/auth/passkeys/desktop/{id}"),
        id,
    }
}

/// `options` → soft assertion → `finish` form → the hand-off page.
async fn finish_with(
    router: &axum::Router,
    pool: &PgPool,
    flow: &Flow,
    auth: &mut SoftAuthenticator,
    row: &PasskeyRow,
    handle_of: Uuid,
) -> Resp {
    let opts = api(
        router,
        &format!("{}/options", flow.path),
        json!({}),
        false,
        None,
    )
    .await;
    assert_eq!(opts.status, 200, "{}", opts.body);
    assert_eq!(opts.json()["ceremony"], "get");
    let handle = queries::user_handle(pool, handle_of).await.unwrap();
    let cred = auth.assert(
        TEST_ORIGIN,
        &opts.json()["options"],
        &row.credential_id,
        Some(handle),
        true,
    );
    form(
        router,
        &format!("{}/finish", flow.path),
        &[("credential", &cred.to_string())],
    )
    .await
}

async fn redeem(
    router: &axum::Router,
    flow_id: &str,
    code: &str,
    verifier: &str,
    bearer: Option<&str>,
) -> Resp {
    api(
        router,
        "/auth/passkeys/desktop/redeem",
        json!({"flow_id": flow_id, "code": code, "verifier": verifier}),
        true,
        bearer,
    )
    .await
}

fn assert_invalid(r: &Resp) {
    assert_eq!(
        (r.status, r.json()["error"].as_str()),
        (400, Some("PASSKEY_HANDOFF_INVALID")),
        "{}",
        r.body
    );
}

#[sqlx::test]
async fn desktop_sign_in_round_trip(pool: PgPool) {
    let app = TestApp::with_passkeys(pool).await;
    let (user, _) = create_test_user(&app.pool).await;
    let (mut auth, row) = seed_passkey(&app, user, PasskeyState::Active).await;
    let r = api(
        &app.router,
        "/auth/passkeys/desktop/flows",
        start_body("sign_in", None, None),
        true,
        None,
    )
    .await;
    assert_eq!(r.status, 201, "{}", r.body);
    let b = r.json();
    let id = b["flow_id"].as_str().unwrap();
    assert_eq!(Uuid::parse_str(id).unwrap().get_version_num(), 4);
    assert_eq!(
        b["url"],
        format!("{TEST_ORIGIN}/auth/passkeys/desktop/{id}")
    );
    let expires: chrono::DateTime<chrono::Utc> = b["expires_at"].as_str().unwrap().parse().unwrap();
    assert!(expires <= chrono::Utc::now() + chrono::Duration::seconds(601));
    let flow = Flow {
        id: id.to_string(),
        path: format!("/auth/passkeys/desktop/{id}"),
    };

    let p = page(&app.router, &flow.path, None).await;
    assert_eq!(p.status, 200);
    assert_eq!(p.header("cache-control"), Some("no-store"));
    assert_eq!(p.header("referrer-policy"), Some("no-referrer"));
    assert_eq!(
        p.header("content-security-policy"),
        Some("default-src 'none'; script-src 'self'; style-src 'self'; connect-src 'self'; img-src 'self'; form-action 'self'; base-uri 'none'; frame-ancestors 'none'")
    );
    assert!(p
        .body
        .contains("Sign in to the Kaiku desktop app on this computer"));
    assert!(p
        .body
        .contains("If someone sent you this link, close this page."));

    let h = finish_with(&app.router, &app.pool, &flow, &mut auth, &row, user).await;
    assert_eq!(h.status, 200, "{}", h.body);
    assert!(h.header("location").is_none());
    assert_eq!(h.header("cache-control"), Some("no-store"));
    assert_eq!(h.header("referrer-policy"), Some("no-referrer"));
    let url = format!("http://127.0.0.1:{PORT}/callback/{CALLBACK_PATH}");
    let csp = h.header("content-security-policy").unwrap().to_string();
    let script_start = h.body.find("<script>").unwrap() + "<script>".len();
    let script_end = h.body.find("</script>").unwrap();
    let script = &h.body[script_start..script_end];
    let hash = base64::engine::general_purpose::STANDARD.encode(Sha256::digest(script.as_bytes()));
    assert_eq!(
        csp,
        format!("default-src 'none'; script-src 'sha256-{hash}'; form-action {url}; base-uri 'none'; frame-ancestors 'none'")
    );
    assert_eq!(h.body.matches("<form").count(), 1);
    assert!(h.body.contains(&format!("action=\"{url}\"")));
    assert!(h.body.contains("method=\"post\""));
    assert!(h
        .body
        .contains("enctype=\"application/x-www-form-urlencoded\""));
    assert_eq!(
        hidden(&h.body, "flow_id").as_deref(),
        Some(flow.id.as_str())
    );
    let code = hidden(&h.body, "code").expect("code");
    assert!(hidden(&h.body, "error").is_none());
    assert_eq!(code.len(), 43);

    let r = redeem(&app.router, &flow.id, &code, VERIFIER, None).await;
    assert_eq!(r.status, 200, "{}", r.body);
    let b = r.json();
    assert_eq!(b["purpose"], "sign_in");
    assert_eq!(b["user_id"], user.to_string());
    assert!(b["refresh_token"].is_string());
    assert_eq!(b["assurance"], "full");
    let class: String = sqlx::query_scalar("SELECT client_class FROM sessions WHERE id = $1")
        .bind(Uuid::parse_str(b["session_id"].as_str().unwrap()).unwrap())
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(class, "desktop");
}

#[sqlx::test]
async fn start_without_desktop_header_is_client_outdated(pool: PgPool) {
    let app = TestApp::with_passkeys(pool).await;
    let r = api(
        &app.router,
        "/auth/passkeys/desktop/flows",
        start_body("sign_in", None, None),
        false,
        None,
    )
    .await;
    assert_eq!(
        (r.status, r.json()["error"].as_str()),
        (400, Some("CLIENT_OUTDATED"))
    );
    let r = api(
        &app.router,
        "/auth/passkeys/desktop/redeem",
        json!({"flow_id": Uuid::new_v4(), "code": "x", "verifier": VERIFIER}),
        false,
        None,
    )
    .await;
    assert_eq!(
        (r.status, r.json()["error"].as_str()),
        (400, Some("CLIENT_OUTDATED"))
    );
}

#[sqlx::test]
async fn start_validation(pool: PgPool) {
    let app = TestApp::with_passkeys(pool).await;
    let (user, _) = create_test_user(&app.pool).await;
    seed_passkey(&app, user, PasskeyState::Active).await;
    let (_, token) = create_session_with_token(&app.pool, &app.config, user).await;
    let mut cases = Vec::new();
    for (k, v) in [
        ("loopback_port", json!(0)),
        ("loopback_port", json!(65_536)),
        ("callback_path", json!(&CALLBACK_PATH[..42])),
        ("callback_path", json!(format!("{}+", &CALLBACK_PATH[..42]))),
        ("callback_path", json!(format!("{CALLBACK_PATH}A"))),
        ("client_challenge", json!("short")),
        ("action", json!("admin.elevate")),
    ] {
        let mut b = start_body("sign_in", None, None);
        b[k] = v;
        cases.push(b);
    }
    for b in cases {
        let r = api(
            &app.router,
            "/auth/passkeys/desktop/flows",
            b.clone(),
            true,
            None,
        )
        .await;
        assert_eq!(r.status, 400, "{b}: {}", r.body);
    }
    // step_up needs a bearer; passkey.add never runs on the desktop (DC12).
    let r = api(
        &app.router,
        "/auth/passkeys/desktop/flows",
        start_body("step_up", Some("admin.elevate"), Some("")),
        true,
        None,
    )
    .await;
    assert_eq!(r.status, 401, "{}", r.body);
    let r = api(
        &app.router,
        "/auth/passkeys/desktop/flows",
        start_body("step_up", Some("passkey.add"), Some("")),
        true,
        Some(&token),
    )
    .await;
    assert_eq!(
        (r.status, r.json()["error"].as_str()),
        (400, Some("VALIDATION_ERROR"))
    );
    let r = api(
        &app.router,
        "/auth/passkeys/desktop/flows",
        start_body("step_up", Some("admin.elevate"), Some("")),
        true,
        Some(&token),
    )
    .await;
    assert_eq!(r.status, 201, "{}", r.body);
}

#[sqlx::test]
async fn code_is_never_in_a_url_or_api_body(pool: PgPool) {
    let app = TestApp::with_passkeys(pool).await;
    let (user, _) = create_test_user(&app.pool).await;
    let (mut auth, row) = seed_passkey(&app, user, PasskeyState::Active).await;
    let s = api(
        &app.router,
        "/auth/passkeys/desktop/flows",
        start_body("sign_in", None, None),
        true,
        None,
    )
    .await;
    let id = s.json()["flow_id"].as_str().unwrap().to_string();
    let flow = Flow {
        path: format!("/auth/passkeys/desktop/{id}"),
        id,
    };
    let p = page(&app.router, &flow.path, None).await;
    let h = finish_with(&app.router, &app.pool, &flow, &mut auth, &row, user).await;
    let code = hidden(&h.body, "code").unwrap();
    // A reload of the page after the result: no code, 404.
    let p2 = page(&app.router, &flow.path, None).await;
    assert_eq!(p2.status, 404);
    let o2 = api(
        &app.router,
        &format!("{}/options", flow.path),
        json!({}),
        false,
        None,
    )
    .await;
    let r = redeem(&app.router, &flow.id, &code, VERIFIER, None).await;
    assert_eq!(r.status, 200);
    for resp in [&s, &p, &p2, &o2, &r] {
        assert!(!resp.body.contains(&code), "code leaked: {}", resp.body);
        assert!(resp.header("location").is_none());
    }
    assert!(h.header("location").is_none());
    // Exactly once in the hand-off page: the hidden field.
    assert_eq!(h.body.matches(&code).count(), 1);
}

async fn sign_in_code(app: &TestApp) -> (Flow, String) {
    let (user, _) = create_test_user(&app.pool).await;
    let (mut auth, row) = seed_passkey(app, user, PasskeyState::Active).await;
    let flow = start(&app.router, start_body("sign_in", None, None), None).await;
    let h = finish_with(&app.router, &app.pool, &flow, &mut auth, &row, user).await;
    let code = hidden(&h.body, "code").unwrap();
    (flow, code)
}

#[sqlx::test]
async fn wrong_verifier_spends_the_code(pool: PgPool) {
    let app = TestApp::with_passkeys(pool).await;
    let (flow, code) = sign_in_code(&app).await;
    assert_invalid(
        &redeem(
            &app.router,
            &flow.id,
            &code,
            "another-verifier-of-at-least-forty-three-chars-xyz",
            None,
        )
        .await,
    );
    assert_invalid(&redeem(&app.router, &flow.id, &code, VERIFIER, None).await);
}

#[sqlx::test]
async fn flow_id_and_verifier_without_code_never_redeem(pool: PgPool) {
    let app = TestApp::with_passkeys(pool).await;
    let (flow, code) = sign_in_code(&app).await;
    assert_invalid(&redeem(&app.router, &flow.id, "", VERIFIER, None).await);
    assert_invalid(&redeem(&app.router, &flow.id, &code, VERIFIER, None).await);
}

#[sqlx::test]
async fn pending_result_expires_after_60s(pool: PgPool) {
    let app = TestApp::with_passkeys(pool).await;
    let (flow, code) = sign_in_code(&app).await;
    // Age the stored result past 60 s.
    use fred::interfaces::KeysInterface;
    let key = format!("passkey:flow:{}", flow.id);
    let raw: String = app.state.redis.get(&key).await.unwrap();
    let mut v: Value = serde_json::from_str(&raw).unwrap();
    let created = v["result"]["created_at"].as_i64().unwrap();
    v["result"]["created_at"] = json!(created - 61);
    let _: () = app
        .state
        .redis
        .set(
            &key,
            v.to_string(),
            Some(fred::types::Expiration::KEEPTTL),
            None,
            false,
        )
        .await
        .unwrap();
    assert_invalid(&redeem(&app.router, &flow.id, &code, VERIFIER, None).await);
}

#[sqlx::test]
async fn forwarded_link_code_is_useless(pool: PgPool) {
    // The victim's genuine assertion lands on the victim's loopback; an
    // attacker who started the flow has the verifier but never the code,
    // and a code alone with another verifier redeems nothing.
    let app = TestApp::with_passkeys(pool).await;
    let (flow, code) = sign_in_code(&app).await;
    assert_invalid(
        &redeem(
            &app.router,
            &flow.id,
            &code,
            "attacker-verifier-0123456789-0123456789-abcdef",
            None,
        )
        .await,
    );
}

#[sqlx::test]
async fn step_up_by_another_accounts_passkey_is_user_mismatch(pool: PgPool) {
    let app = TestApp::with_passkeys(pool).await;
    let (user, _) = create_test_user(&app.pool).await;
    let (other, _) = create_test_user(&app.pool).await;
    seed_passkey(&app, user, PasskeyState::Active).await;
    let (mut theirs_auth, theirs) = seed_passkey(&app, other, PasskeyState::Active).await;
    let (_, token) = create_session_with_token(&app.pool, &app.config, user).await;
    let flow = start(
        &app.router,
        start_body("step_up", Some("admin.elevate"), Some("")),
        Some(&token),
    )
    .await;
    let h = finish_with(
        &app.router,
        &app.pool,
        &flow,
        &mut theirs_auth,
        &theirs,
        other,
    )
    .await;
    assert_eq!(h.status, 200);
    assert_eq!(
        hidden(&h.body, "error").as_deref(),
        Some("PASSKEY_USER_MISMATCH")
    );
    assert!(hidden(&h.body, "code").is_none());
    assert_eq!(
        page(&app.router, &flow.path, None).await.status,
        404,
        "the flow is gone"
    );
    assert_invalid(&redeem(&app.router, &flow.id, "x", VERIFIER, Some(&token)).await);
}

#[sqlx::test]
async fn step_up_redeem_needs_the_starting_session(pool: PgPool) {
    let app = TestApp::with_passkeys(pool).await;
    let (user, _) = create_test_user(&app.pool).await;
    let (mut auth, row) = seed_passkey(&app, user, PasskeyState::Active).await;
    let (_, b) = seed_passkey(&app, user, PasskeyState::Active).await;
    let (_, token1) = create_session_with_token(&app.pool, &app.config, user).await;
    let (_, token2) = create_session_with_token(&app.pool, &app.config, user).await;
    let target = b.id.to_string();
    // With sid2: refused (and the code is spent).
    let flow = start(
        &app.router,
        start_body("step_up", Some("passkey.remove"), Some(&target)),
        Some(&token1),
    )
    .await;
    let p = page(&app.router, &flow.path, None).await;
    assert!(p.body.contains("to remove the passkey"), "{}", p.body);
    let h = finish_with(&app.router, &app.pool, &flow, &mut auth, &row, user).await;
    let code = hidden(&h.body, "code").unwrap();
    assert_invalid(&redeem(&app.router, &flow.id, &code, VERIFIER, Some(&token2)).await);
    assert_invalid(&redeem(&app.router, &flow.id, &code, VERIFIER, None).await);
    // With sid1: a grant that DELETE accepts.
    let flow = start(
        &app.router,
        start_body("step_up", Some("passkey.remove"), Some(&target)),
        Some(&token1),
    )
    .await;
    let h = finish_with(&app.router, &app.pool, &flow, &mut auth, &row, user).await;
    let code = hidden(&h.body, "code").unwrap();
    let r = redeem(&app.router, &flow.id, &code, VERIFIER, Some(&token1)).await;
    assert_eq!(r.status, 200, "{}", r.body);
    let g = r.json();
    assert_eq!(g["purpose"], "step_up");
    assert_eq!(g["user_id"], user.to_string());
    assert_eq!(g["grant"]["action"], "passkey.remove");
    assert_eq!(g["grant"]["target"], target);
    assert_eq!(g["grant"]["via"], "passkey");
    let del = peer_request(Method::DELETE, &format!("/auth/passkeys/{}", b.id))
        .header("Authorization", format!("Bearer {token1}"))
        .body(Body::empty())
        .unwrap();
    assert_eq!(send(&app.router, del).await.status, 200);
}

#[sqlx::test]
async fn redeem_rechecks_account_passkey_and_session(pool: PgPool) {
    let app = TestApp::with_passkeys(pool).await;
    // 1. The passkey revoked between finish and redeem.
    let (user, _) = create_test_user(&app.pool).await;
    let (mut auth, row) = seed_passkey(&app, user, PasskeyState::Active).await;
    let flow = start(&app.router, start_body("sign_in", None, None), None).await;
    let code = hidden(
        &finish_with(&app.router, &app.pool, &flow, &mut auth, &row, user)
            .await
            .body,
        "code",
    )
    .unwrap();
    sqlx::query("UPDATE passkeys SET state = 'revoked', revoked_at = NOW() WHERE id = $1")
        .bind(row.id)
        .execute(&app.pool)
        .await
        .unwrap();
    let before: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM sessions WHERE user_id = $1")
        .bind(user)
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_invalid(&redeem(&app.router, &flow.id, &code, VERIFIER, None).await);
    let after: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM sessions WHERE user_id = $1")
        .bind(user)
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(before, after);
    // 2. The user banned.
    let (user, _) = create_test_user(&app.pool).await;
    let (mut auth, row) = seed_passkey(&app, user, PasskeyState::Active).await;
    let flow = start(&app.router, start_body("sign_in", None, None), None).await;
    let code = hidden(
        &finish_with(&app.router, &app.pool, &flow, &mut auth, &row, user)
            .await
            .body,
        "code",
    )
    .unwrap();
    sqlx::query("INSERT INTO global_bans (user_id, reason) VALUES ($1, 'x')")
        .bind(user)
        .execute(&app.pool)
        .await
        .unwrap();
    assert_invalid(&redeem(&app.router, &flow.id, &code, VERIFIER, None).await);
    // 3. step_up: the starting session ended.
    let (user, _) = create_test_user(&app.pool).await;
    let (mut auth, row) = seed_passkey(&app, user, PasskeyState::Active).await;
    let (sid, token) = create_session_with_token(&app.pool, &app.config, user).await;
    let flow = start(
        &app.router,
        start_body("step_up", Some("admin.elevate"), Some("")),
        Some(&token),
    )
    .await;
    let code = hidden(
        &finish_with(&app.router, &app.pool, &flow, &mut auth, &row, user)
            .await
            .body,
        "code",
    )
    .unwrap();
    sqlx::query("UPDATE sessions SET ended_at = NOW(), end_reason = 'logout' WHERE id = $1")
        .bind(sid)
        .execute(&app.pool)
        .await
        .unwrap();
    let r = redeem(&app.router, &flow.id, &code, VERIFIER, Some(&token)).await;
    assert!(matches!(r.status, 400 | 401), "{}", r.body);
    let grants: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM step_up_grants WHERE session_id = $1")
            .bind(sid)
            .fetch_one(&app.pool)
            .await
            .unwrap();
    assert_eq!(grants, 0);
}

#[sqlx::test]
async fn reloaded_page_gets_fresh_options(pool: PgPool) {
    let app = TestApp::with_passkeys(pool).await;
    let (user, _) = create_test_user(&app.pool).await;
    let (mut auth, row) = seed_passkey(&app, user, PasskeyState::Active).await;
    let flow = start(&app.router, start_body("sign_in", None, None), None).await;
    let o1 = api(
        &app.router,
        &format!("{}/options", flow.path),
        json!({}),
        false,
        None,
    )
    .await
    .json();
    let o2 = api(
        &app.router,
        &format!("{}/options", flow.path),
        json!({}),
        false,
        None,
    )
    .await
    .json();
    assert_ne!(
        o1["options"]["publicKey"]["challenge"],
        o2["options"]["publicKey"]["challenge"]
    );
    let handle = queries::user_handle(&app.pool, user).await.unwrap();
    // The first options' assertion no longer finishes.
    let stale = auth.assert(
        TEST_ORIGIN,
        &o1["options"],
        &row.credential_id,
        Some(handle),
        true,
    );
    let h = form(
        &app.router,
        &format!("{}/finish", flow.path),
        &[("credential", &stale.to_string())],
    )
    .await;
    assert_eq!(
        hidden(&h.body, "error").as_deref(),
        Some("PASSKEY_INVALID"),
        "{}",
        h.body
    );
    // A fresh flow: the second options' assertion completes.
    let flow = start(&app.router, start_body("sign_in", None, None), None).await;
    api(
        &app.router,
        &format!("{}/options", flow.path),
        json!({}),
        false,
        None,
    )
    .await;
    let h = finish_with(&app.router, &app.pool, &flow, &mut auth, &row, user).await;
    assert!(hidden(&h.body, "code").is_some(), "{}", h.body);
}

#[sqlx::test]
async fn finish_is_single_use(pool: PgPool) {
    let app = TestApp::with_passkeys(pool).await;
    let (user, _) = create_test_user(&app.pool).await;
    let (mut auth, row) = seed_passkey(&app, user, PasskeyState::Active).await;
    let flow = start(&app.router, start_body("sign_in", None, None), None).await;
    let opts = api(
        &app.router,
        &format!("{}/options", flow.path),
        json!({}),
        false,
        None,
    )
    .await
    .json();
    let handle = queries::user_handle(&app.pool, user).await.unwrap();
    let cred = auth
        .assert(
            TEST_ORIGIN,
            &opts["options"],
            &row.credential_id,
            Some(handle),
            true,
        )
        .to_string();
    let h = form(
        &app.router,
        &format!("{}/finish", flow.path),
        &[("credential", &cred)],
    )
    .await;
    assert!(hidden(&h.body, "code").is_some());
    let again = form(
        &app.router,
        &format!("{}/finish", flow.path),
        &[("credential", &cred)],
    )
    .await;
    assert_eq!(again.status, 400, "{}", again.body);
    assert!(hidden(&again.body, "code").is_none());
    assert!(again
        .header("content-type")
        .unwrap()
        .starts_with("text/html"));
}

#[sqlx::test]
async fn cancel_hands_back_passkey_cancelled(pool: PgPool) {
    let app = TestApp::with_passkeys(pool).await;
    let flow = start(&app.router, start_body("sign_in", None, None), None).await;
    let h = form(&app.router, &format!("{}/cancel", flow.path), &[]).await;
    assert_eq!(h.status, 200);
    assert_eq!(
        hidden(&h.body, "error").as_deref(),
        Some("PASSKEY_CANCELLED")
    );
    assert_eq!(
        hidden(&h.body, "flow_id").as_deref(),
        Some(flow.id.as_str())
    );
    assert_eq!(page(&app.router, &flow.path, None).await.status, 404);
    assert_eq!(
        form(&app.router, &format!("{}/cancel", flow.path), &[])
            .await
            .status,
        404
    );
}

#[sqlx::test]
async fn failed_assertion_hands_back_error(pool: PgPool) {
    let app = TestApp::with_passkeys(pool).await;
    let (user, _) = create_test_user(&app.pool).await;
    let (mut auth, row) = seed_passkey(&app, user, PasskeyState::Active).await;
    let flow = start(&app.router, start_body("sign_in", None, None), None).await;
    let opts = api(
        &app.router,
        &format!("{}/options", flow.path),
        json!({}),
        false,
        None,
    )
    .await
    .json();
    let handle = queries::user_handle(&app.pool, user).await.unwrap();
    let cred = auth.assert(
        TEST_ORIGIN,
        &opts["options"],
        &row.credential_id,
        Some(handle),
        false,
    );
    let h = form(
        &app.router,
        &format!("{}/finish", flow.path),
        &[("credential", &cred.to_string())],
    )
    .await;
    assert_eq!(hidden(&h.body, "error").as_deref(), Some("PASSKEY_INVALID"));
    assert_eq!(
        page(&app.router, &flow.path, None).await.status,
        404,
        "a failed finish ends the flow"
    );
}

#[sqlx::test]
async fn page_for_unknown_flow_is_404_html(pool: PgPool) {
    let app = TestApp::with_passkeys(pool).await;
    let p = page(
        &app.router,
        &format!("/auth/passkeys/desktop/{}", Uuid::new_v4()),
        None,
    )
    .await;
    assert_eq!(p.status, 404);
    assert!(p.header("content-type").unwrap().starts_with("text/html"));
    assert!(p
        .body
        .contains("This link has expired or was already used. Start again in the Kaiku app."));
    let p = page(&app.router, "/auth/passkeys/desktop/not-a-uuid", None).await;
    assert_eq!(p.status, 404);
}

#[sqlx::test]
async fn assets_are_served_with_content_types(pool: PgPool) {
    let app = TestApp::with_passkeys(pool).await;
    let js = page(&app.router, "/auth/passkeys/assets/ceremony.js", None).await;
    assert_eq!(js.status, 200);
    assert!(js
        .header("content-type")
        .unwrap()
        .starts_with("text/javascript"));
    assert_eq!(js.header("cache-control"), Some("no-cache"));
    assert!(js.body.contains("navigator.credentials"));
    let css = page(&app.router, "/auth/passkeys/assets/ceremony.css", None).await;
    assert_eq!(css.status, 200);
    assert!(css.header("content-type").unwrap().starts_with("text/css"));
}

#[sqlx::test]
async fn page_text_follows_accept_language(pool: PgPool) {
    let app = TestApp::with_passkeys(pool).await;
    let flow = start(&app.router, start_body("sign_in", None, None), None).await;
    let de = page(&app.router, &flow.path, Some("de-DE,de;q=0.9,en;q=0.5")).await;
    assert!(
        de.body
            .contains("Bei der Kaiku-Desktop-App auf diesem Computer anmelden"),
        "{}",
        de.body
    );
    assert!(de
        .body
        .contains("Wenn dir jemand diesen Link geschickt hat, schließe diese Seite."));
    assert!(de.body.contains("lang=\"de\""));
    let en = page(&app.router, &flow.path, Some("fr-FR")).await;
    assert!(en
        .body
        .contains("Sign in to the Kaiku desktop app on this computer"));
}

#[sqlx::test]
async fn failed_redeems_never_block_the_ip(pool: PgPool) {
    let app = TestApp::with_passkeys(pool).await;
    let (router, limiter) = limited_router(&app).await;
    for _ in 0..20 {
        let r = redeem(&router, &Uuid::new_v4().to_string(), "x", VERIFIER, None).await;
        assert_invalid(&r);
    }
    assert!(!limiter.is_blocked("203.0.113.9").await.unwrap());
}
