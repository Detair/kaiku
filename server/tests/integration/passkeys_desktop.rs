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
use crate::helpers::{create_session_with_token, create_test_user, make_admin, TestApp};

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
        .header("Origin", TEST_ORIGIN)
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
    let ceremony_id = opts.json()["ceremony_id"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    form(
        router,
        &format!("{}/finish", flow.path),
        &[
            ("credential", &cred.to_string()),
            ("ceremony_id", &ceremony_id),
        ],
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
    make_admin(&app.pool, user).await;
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
    make_admin(&app.pool, user).await;
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
async fn an_expired_bearer_leaves_the_step_up_unspent(pool: PgPool) {
    // The desktop app refreshes and retries once after a 401 (earlier
    // review m5): the 401 comes before the flow is taken.
    let app = TestApp::with_passkeys(pool).await;
    let (user, _) = create_test_user(&app.pool).await;
    let (mut auth, row) = seed_passkey(&app, user, PasskeyState::Active).await;
    let (_, b) = seed_passkey(&app, user, PasskeyState::Active).await;
    let (_, token) = create_session_with_token(&app.pool, &app.config, user).await;
    let flow = start(
        &app.router,
        start_body("step_up", Some("passkey.remove"), Some(&b.id.to_string())),
        Some(&token),
    )
    .await;
    let h = finish_with(&app.router, &app.pool, &flow, &mut auth, &row, user).await;
    let code = hidden(&h.body, "code").unwrap();
    let r = redeem(
        &app.router,
        &flow.id,
        &code,
        VERIFIER,
        Some("not-a-valid-token"),
    )
    .await;
    assert_eq!(r.status, 401, "{}", r.body);
    let r = redeem(&app.router, &flow.id, &code, VERIFIER, Some(&token)).await;
    assert_eq!(r.status, 200, "{}", r.body);
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
    make_admin(&app.pool, user).await;
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
    let stale_id = o1["ceremony_id"].as_str().unwrap().to_string();
    let h = form(
        &app.router,
        &format!("{}/finish", flow.path),
        &[
            ("credential", &stale.to_string()),
            ("ceremony_id", &stale_id),
        ],
    )
    .await;
    // A stale tab gets the "expired" page and hands nothing back; the flow
    // stays usable for the current page.
    assert_eq!(h.status, 400, "{}", h.body);
    assert!(hidden(&h.body, "error").is_none() && hidden(&h.body, "code").is_none());
    let current = auth.assert(
        TEST_ORIGIN,
        &o2["options"],
        &row.credential_id,
        Some(handle),
        true,
    );
    let h = form(
        &app.router,
        &format!("{}/finish", flow.path),
        &[
            ("credential", &current.to_string()),
            ("ceremony_id", o2["ceremony_id"].as_str().unwrap()),
        ],
    )
    .await;
    assert!(hidden(&h.body, "code").is_some(), "{}", h.body);
    // And a fresh flow works as before.
    let flow = start(&app.router, start_body("sign_in", None, None), None).await;
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

fn sign_in_body_with(username: Option<&str>) -> Value {
    let mut b = start_body("sign_in", None, None);
    b["username"] = json!(username);
    b
}

#[sqlx::test]
async fn desktop_sign_in_username_hint_is_enforced(pool: PgPool) {
    // C29 addition (coordinator, 2026-11-03): the typed user name.
    let app = TestApp::with_passkeys(pool).await;
    let (grandma, _) = create_test_user(&app.pool).await;
    let (_, fabian) = create_test_user(&app.pool).await;
    let (mut auth, row) = seed_passkey(&app, grandma, PasskeyState::Active).await;
    let flow = start(&app.router, sign_in_body_with(Some(&fabian)), None).await;
    let h = finish_with(&app.router, &app.pool, &flow, &mut auth, &row, grandma).await;
    assert_eq!(
        hidden(&h.body, "error").as_deref(),
        Some("PASSKEY_USER_MISMATCH"),
        "{}",
        h.body
    );
    assert!(hidden(&h.body, "code").is_none());
    assert_eq!(
        page(&app.router, &flow.path, None).await.status,
        404,
        "the flow is gone"
    );
    // The right name, in another case, passes.
    let name: String = sqlx::query_scalar("SELECT username FROM users WHERE id = $1")
        .bind(grandma)
        .fetch_one(&app.pool)
        .await
        .unwrap();
    let flow = start(
        &app.router,
        sign_in_body_with(Some(&name.to_uppercase())),
        None,
    )
    .await;
    let h = finish_with(&app.router, &app.pool, &flow, &mut auth, &row, grandma).await;
    let code = hidden(&h.body, "code").expect("code");
    assert_eq!(
        redeem(&app.router, &flow.id, &code, VERIFIER, None)
            .await
            .status,
        200
    );
    // A blank name is no hint.
    let flow = start(&app.router, sign_in_body_with(Some("  ")), None).await;
    let h = finish_with(&app.router, &app.pool, &flow, &mut auth, &row, grandma).await;
    assert!(hidden(&h.body, "code").is_some());
}

async fn options_of(app: &TestApp, flow: &Flow) -> Value {
    api(
        &app.router,
        &format!("{}/options", flow.path),
        json!({}),
        false,
        None,
    )
    .await
    .json()
}

#[sqlx::test]
async fn desktop_ceremony_cannot_finish_on_web_routes(pool: PgPool) {
    // Review Fable m2: a desktop ceremony is bound to its flow.
    let app = TestApp::with_passkeys(pool).await;
    let (user, _) = create_test_user(&app.pool).await;
    let (mut auth, row) = seed_passkey(&app, user, PasskeyState::Active).await;
    let handle = queries::user_handle(&app.pool, user).await.unwrap();
    let flow = start(&app.router, start_body("sign_in", None, None), None).await;
    let o = options_of(&app, &flow).await;
    let cred = auth.assert(
        TEST_ORIGIN,
        &o["options"],
        &row.credential_id,
        Some(handle),
        true,
    );
    let r = api(
        &app.router,
        "/auth/passkeys/login/finish",
        json!({"ceremony_id": o["ceremony_id"], "credential": cred}),
        false,
        None,
    )
    .await;
    assert_eq!(
        (r.status, r.json()["error"].as_str()),
        (400, Some("PASSKEY_CEREMONY_INVALID")),
        "{}",
        r.body
    );
    // A web ceremony cannot finish a desktop flow either.
    let web = api(
        &app.router,
        "/auth/passkeys/login/start",
        json!({}),
        false,
        None,
    )
    .await
    .json();
    let flow = start(&app.router, start_body("sign_in", None, None), None).await;
    options_of(&app, &flow).await;
    let cred = auth.assert(
        TEST_ORIGIN,
        &web["options"],
        &row.credential_id,
        Some(handle),
        true,
    );
    let h = form(
        &app.router,
        &format!("{}/finish", flow.path),
        &[
            ("credential", &cred.to_string()),
            ("ceremony_id", web["ceremony_id"].as_str().unwrap()),
        ],
    )
    .await;
    assert!(hidden(&h.body, "code").is_none(), "{}", h.body);
}

async fn flow_ttl(app: &TestApp, flow: &Flow) -> i64 {
    use fred::interfaces::KeysInterface;
    app.state
        .redis
        .ttl(format!("passkey:flow:{}", flow.id))
        .await
        .unwrap()
}

#[sqlx::test]
async fn a_pending_result_is_never_clobbered(pool: PgPool) {
    // Reviews Argon M3 / Argon-a M1: options and cancel after a finish.
    let app = TestApp::with_passkeys(pool).await;
    let (user, _) = create_test_user(&app.pool).await;
    let (mut auth, row) = seed_passkey(&app, user, PasskeyState::Active).await;
    let flow = start(&app.router, start_body("sign_in", None, None), None).await;
    let before = flow_ttl(&app, &flow).await;
    assert!(before > 500, "{before}");
    options_of(&app, &flow).await;
    assert!(
        flow_ttl(&app, &flow).await > 500,
        "options keep the flow's TTL"
    );
    let h = finish_with(&app.router, &app.pool, &flow, &mut auth, &row, user).await;
    let code = hidden(&h.body, "code").unwrap();
    let ttl = flow_ttl(&app, &flow).await;
    assert!(
        (1..=60).contains(&ttl),
        "a pending result lives 60 s: {ttl}"
    );
    let o = api(
        &app.router,
        &format!("{}/options", flow.path),
        json!({}),
        false,
        None,
    )
    .await;
    assert_eq!(o.status, 400, "{}", o.body);
    assert_eq!(
        form(&app.router, &format!("{}/cancel", flow.path), &[])
            .await
            .status,
        404
    );
    assert_eq!(
        redeem(&app.router, &flow.id, &code, VERIFIER, None)
            .await
            .status,
        200
    );
}

#[sqlx::test]
async fn a_finish_near_the_flow_end_still_gets_60_s(pool: PgPool) {
    // Review Argon-a M1: C29 step 4 keeps the pending result 60 s; the TTL
    // is set, not cut down to what the flow had left.
    use fred::interfaces::KeysInterface;
    let app = TestApp::with_passkeys(pool).await;
    let (user, _) = create_test_user(&app.pool).await;
    let (mut auth, row) = seed_passkey(&app, user, PasskeyState::Active).await;
    let flow = start(&app.router, start_body("sign_in", None, None), None).await;
    let _: bool = app
        .state
        .redis
        .expire(format!("passkey:flow:{}", flow.id), 5, None)
        .await
        .unwrap();
    let h = finish_with(&app.router, &app.pool, &flow, &mut auth, &row, user).await;
    assert!(hidden(&h.body, "code").is_some(), "{}", h.body);
    let ttl = flow_ttl(&app, &flow).await;
    assert!((55..=60).contains(&ttl), "pending result TTL {ttl}");
}

#[sqlx::test]
async fn options_without_active_passkeys_end_the_flow(pool: PgPool) {
    // Review Argon-a m3: no reload loop on a live flow that cannot proceed.
    let app = TestApp::with_passkeys(pool).await;
    let (user, _) = create_test_user(&app.pool).await;
    make_admin(&app.pool, user).await;
    let (_, row) = seed_passkey(&app, user, PasskeyState::Active).await;
    let (_, token) = create_session_with_token(&app.pool, &app.config, user).await;
    let flow = start(
        &app.router,
        start_body("step_up", Some("admin.elevate"), Some("")),
        Some(&token),
    )
    .await;
    sqlx::query("UPDATE passkeys SET state = 'revoked', revoked_at = NOW() WHERE id = $1")
        .bind(row.id)
        .execute(&app.pool)
        .await
        .unwrap();
    let o = api(
        &app.router,
        &format!("{}/options", flow.path),
        json!({}),
        false,
        None,
    )
    .await;
    assert_eq!(o.status, 400);
    assert_eq!(page(&app.router, &flow.path, None).await.status, 404);
}

#[sqlx::test]
async fn every_desktop_step_up_page_names_its_action(pool: PgPool) {
    // Review Codex m4: never a generic "security change".
    let app = TestApp::with_passkeys(pool).await;
    let (user, _) = create_test_user(&app.pool).await;
    make_admin(&app.pool, user).await;
    let (_, row) = seed_passkey(&app, user, PasskeyState::Active).await;
    let (_, token) = create_session_with_token(&app.pool, &app.config, user).await;
    let cases: Vec<(&str, String, &str, &str)> = vec![
        (
            "passkey.remove",
            row.id.to_string(),
            "to remove the passkey",
            "den Passkey",
        ),
        (
            "password.set",
            String::new(),
            "to set or change your password",
            "Passwort",
        ),
        (
            "account.delete",
            String::new(),
            "to delete your account",
            "Konto",
        ),
        (
            "sessions.end",
            "others".to_string(),
            "to sign out all your other sessions",
            "Sitzungen",
        ),
        (
            "recovery_codes.create",
            String::new(),
            "recovery codes",
            "Wiederherstellungscodes",
        ),
        (
            "admin.elevate",
            String::new(),
            "to unlock admin actions",
            "Admin-Aktionen",
        ),
    ];
    for (action, target, en, de) in cases {
        let flow = start(
            &app.router,
            start_body("step_up", Some(action), Some(&target)),
            Some(&token),
        )
        .await;
        let p = page(&app.router, &flow.path, None).await;
        assert!(p.body.contains(en), "{action}: {}", p.body);
        assert!(!p.body.contains("this security change"), "{action}");
        let p = page(&app.router, &flow.path, Some("de")).await;
        assert!(p.body.contains(de), "{action}: {}", p.body);
    }
    // The label is the caller's own passkey's, HTML-escaped.
    sqlx::query("UPDATE passkeys SET label = '<b>Laptop</b>' WHERE id = $1")
        .bind(row.id)
        .execute(&app.pool)
        .await
        .unwrap();
    let flow = start(
        &app.router,
        start_body("step_up", Some("passkey.remove"), Some(&row.id.to_string())),
        Some(&token),
    )
    .await;
    let p = page(&app.router, &flow.path, None).await;
    assert!(p.body.contains("&lt;b&gt;Laptop&lt;/b&gt;"), "{}", p.body);
}

#[sqlx::test]
async fn admin_step_up_pages_are_for_admins_only(pool: PgPool) {
    // Review round 2 (Codex 2.1, Fable R1): a non-admin cannot start an
    // admin.* flow and so never sees a foreign label; an admin does.
    let app = TestApp::with_passkeys(pool).await;
    let (user, _) = create_test_user(&app.pool).await;
    let (other, _) = create_test_user(&app.pool).await;
    seed_passkey(&app, user, PasskeyState::Active).await;
    let (_, theirs) = seed_passkey(&app, other, PasskeyState::Active).await;
    sqlx::query("UPDATE passkeys SET label = 'Their secret key' WHERE id = $1")
        .bind(theirs.id)
        .execute(&app.pool)
        .await
        .unwrap();
    let (_, token) = create_session_with_token(&app.pool, &app.config, user).await;
    let body = start_body(
        "step_up",
        Some("admin.authenticator.remove"),
        Some(&theirs.id.to_string()),
    );
    let r = api(
        &app.router,
        "/auth/passkeys/desktop/flows",
        body.clone(),
        true,
        Some(&token),
    )
    .await;
    assert_eq!(
        (r.status, r.json()["error"].as_str()),
        (403, Some("NOT_ADMIN")),
        "{}",
        r.body
    );
    assert!(!r.body.contains("Their secret key"), "{}", r.body);
    make_admin(&app.pool, user).await;
    let flow = start(&app.router, body, Some(&token)).await;
    let p = page(&app.router, &flow.path, None).await;
    assert!(p.body.contains("Their secret key"), "{}", p.body);
}

/// Wait until a query of this test database waits for a row lock.
async fn wait_for_lock_waiter(pool: &PgPool) {
    for _ in 0..250 {
        let waiting: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM pg_stat_activity
              WHERE datname = current_database() AND wait_event_type = 'Lock'",
        )
        .fetch_one(pool)
        .await
        .unwrap();
        if waiting > 0 {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    panic!("no finish waited for the passkey row");
}

#[sqlx::test]
async fn a_finish_that_loses_the_race_hands_back_nothing(pool: PgPool) {
    // Review round 2 (Codex 2.2, Argon A R1): finish A has taken its
    // ceremony when a reloaded page gets ceremony B. A loses the final
    // compare-and-set and gets the stale page, never an error for the
    // app's listener; B still hands back a code that redeems.
    let app = TestApp::with_passkeys(pool).await;
    let (user, _) = create_test_user(&app.pool).await;
    let (mut auth, row) = seed_passkey(&app, user, PasskeyState::Active).await;
    let handle = queries::user_handle(&app.pool, user).await.unwrap();
    let flow = start(&app.router, start_body("sign_in", None, None), None).await;
    let a = options_of(&app, &flow).await;
    let cred_a = auth.assert(
        TEST_ORIGIN,
        &a["options"],
        &row.credential_id,
        Some(handle),
        true,
    );
    // Hold the passkey row: A stops in `record_use`, after its take.
    let mut lock = app.pool.begin().await.unwrap();
    sqlx::query("SELECT 1 FROM passkeys WHERE id = $1 FOR UPDATE")
        .bind(row.id)
        .execute(&mut *lock)
        .await
        .unwrap();
    let finish_a = {
        let router = app.router.clone();
        let path = format!("{}/finish", flow.path);
        let id = a["ceremony_id"].as_str().unwrap().to_string();
        tokio::spawn(async move {
            form(
                &router,
                &path,
                &[("credential", &cred_a.to_string()), ("ceremony_id", &id)],
            )
            .await
        })
    };
    wait_for_lock_waiter(&app.pool).await;
    let b = options_of(&app, &flow).await;
    lock.rollback().await.unwrap();
    let ha = finish_a.await.unwrap();
    assert_eq!(ha.status, 400, "{}", ha.body);
    assert!(
        hidden(&ha.body, "error").is_none() && hidden(&ha.body, "code").is_none(),
        "{}",
        ha.body
    );
    let cred_b = auth.assert(
        TEST_ORIGIN,
        &b["options"],
        &row.credential_id,
        Some(handle),
        true,
    );
    let hb = form(
        &app.router,
        &format!("{}/finish", flow.path),
        &[
            ("credential", &cred_b.to_string()),
            ("ceremony_id", b["ceremony_id"].as_str().unwrap()),
        ],
    )
    .await;
    let code = hidden(&hb.body, "code").unwrap_or_else(|| panic!("{}", hb.body));
    let r = redeem(&app.router, &flow.id, &code, VERIFIER, None).await;
    assert_eq!(r.status, 200, "{}", r.body);
}

#[sqlx::test]
async fn a_foreign_ceremony_id_consumes_nothing(pool: PgPool) {
    // Review round 2 (Argon A R1): a finish naming another ceremony (a web
    // one, or garbage) gets the stale page, takes no ceremony and leaves
    // the flow as it is.
    use fred::interfaces::KeysInterface;
    let app = TestApp::with_passkeys(pool).await;
    let (user, _) = create_test_user(&app.pool).await;
    let (mut auth, row) = seed_passkey(&app, user, PasskeyState::Active).await;
    let handle = queries::user_handle(&app.pool, user).await.unwrap();
    let flow = start(&app.router, start_body("sign_in", None, None), None).await;
    let current = options_of(&app, &flow).await;
    let web = api(
        &app.router,
        "/auth/passkeys/login/start",
        json!({}),
        false,
        None,
    )
    .await
    .json();
    let web_id = web["ceremony_id"].as_str().unwrap().to_string();
    let cred = auth.assert(
        TEST_ORIGIN,
        &web["options"],
        &row.credential_id,
        Some(handle),
        true,
    );
    for id in [web_id.as_str(), "not-a-uuid"] {
        let h = form(
            &app.router,
            &format!("{}/finish", flow.path),
            &[("credential", &cred.to_string()), ("ceremony_id", id)],
        )
        .await;
        assert_eq!(h.status, 400, "{id}: {}", h.body);
        assert!(
            hidden(&h.body, "error").is_none() && hidden(&h.body, "code").is_none(),
            "{id}: {}",
            h.body
        );
    }
    let still: bool = app
        .state
        .redis
        .exists(format!("passkey:ceremony:{web_id}"))
        .await
        .unwrap();
    assert!(still, "the web ceremony was not taken");
    // The flow's own ceremony still finishes.
    let cred = auth.assert(
        TEST_ORIGIN,
        &current["options"],
        &row.credential_id,
        Some(handle),
        true,
    );
    let h = form(
        &app.router,
        &format!("{}/finish", flow.path),
        &[
            ("credential", &cred.to_string()),
            ("ceremony_id", current["ceremony_id"].as_str().unwrap()),
        ],
    )
    .await;
    assert!(hidden(&h.body, "code").is_some(), "{}", h.body);
}

/// Finish the flow's current ceremony while the passkey row is held, break
/// the flow key's type meanwhile (so the final compare-and-set fails with a
/// Valkey error), then let the finish go on.
async fn finish_with_a_failing_cas(app: &TestApp, username: Option<&str>) -> Resp {
    use fred::interfaces::{KeysInterface, ListInterface};
    let (user, _) = create_test_user(&app.pool).await;
    let (mut auth, row) = seed_passkey(app, user, PasskeyState::Active).await;
    let handle = queries::user_handle(&app.pool, user).await.unwrap();
    let flow = start(&app.router, sign_in_body_with(username), None).await;
    let o = options_of(app, &flow).await;
    let cred = auth.assert(
        TEST_ORIGIN,
        &o["options"],
        &row.credential_id,
        Some(handle),
        true,
    );
    let mut lock = app.pool.begin().await.unwrap();
    sqlx::query("SELECT 1 FROM passkeys WHERE id = $1 FOR UPDATE")
        .bind(row.id)
        .execute(&mut *lock)
        .await
        .unwrap();
    let finish = {
        let router = app.router.clone();
        let path = format!("{}/finish", flow.path);
        let id = o["ceremony_id"].as_str().unwrap().to_string();
        tokio::spawn(async move {
            form(
                &router,
                &path,
                &[("credential", &cred.to_string()), ("ceremony_id", &id)],
            )
            .await
        })
    };
    wait_for_lock_waiter(&app.pool).await;
    let key = format!("passkey:flow:{}", flow.id);
    let _: i64 = app.state.redis.del(&key).await.unwrap();
    let _: i64 = app.state.redis.lpush(&key, "not a flow").await.unwrap();
    lock.rollback().await.unwrap();
    finish.await.unwrap()
}

#[sqlx::test]
async fn a_valkey_error_at_the_result_hands_back_nothing(pool: PgPool) {
    // Review round 3 (Codex): the final compare-and-set fails with an error;
    // the page tells the user to go back and makes no loopback callback.
    let app = TestApp::with_passkeys(pool).await;
    let h = finish_with_a_failing_cas(&app, None).await;
    assert_eq!(h.status, 503, "{}", h.body);
    assert!(
        hidden(&h.body, "error").is_none() && hidden(&h.body, "code").is_none(),
        "{}",
        h.body
    );
    assert!(!h.body.contains("127.0.0.1"), "{}", h.body);
    assert!(h.body.contains("not reachable"), "{}", h.body);
}

#[sqlx::test]
async fn a_valkey_error_when_ending_a_flow_hands_back_nothing(pool: PgPool) {
    // Review round 3 (Codex): fail() cannot end the flow (Valkey error); it
    // makes no loopback callback either.
    let app = TestApp::with_passkeys(pool).await;
    let h = finish_with_a_failing_cas(&app, Some("someone_else")).await;
    assert_eq!(h.status, 503, "{}", h.body);
    assert!(
        hidden(&h.body, "error").is_none() && hidden(&h.body, "code").is_none(),
        "{}",
        h.body
    );
    assert!(!h.body.contains("127.0.0.1"), "{}", h.body);
}

// ============================================================================
// "Set up this PC" through the desktop flow (spec Step 5, desktop part)
// ============================================================================

use super::helpers::approvals::KeyDeviceFixture;
use super::helpers::create_probation_session_with_token;
use super::helpers::login::login_config;
use super::helpers::passkeys::test_passkey_rp;
use crate::pc_setup::{
    attempt_state, contains_code, enrollment_state, passkey_start, passkeys_of, with_code, wrong,
    Pc,
};

/// The cookie's name and path prefix (spec Step 5, browser secret).
const PCSETUP_COOKIE: &str = "__Secure-kaiku_pcsetup";

/// A user with a key phone and a probation session; cookies are `Secure`.
async fn desktop_pc(pool: &PgPool) -> Pc {
    let mut config = login_config();
    config.passkeys = Some(std::sync::Arc::new(test_passkey_rp()));
    config.cookie_secure = true;
    let app = TestApp::with_pool_and_config(pool.clone(), config).await;
    let (user, _) = create_test_user(pool).await;
    let dev = KeyDeviceFixture::insert(pool, &app.config, user, "active").await;
    let (sid, token) = create_probation_session_with_token(pool, &app.config, user).await;
    // The desktop app's session: only such a session starts a `pc_setup` flow.
    set_client_class(pool, sid, "desktop").await;
    Pc {
        app,
        user,
        sid,
        token,
        dev,
    }
}

async fn set_client_class(pool: &PgPool, sid: Uuid, class: &str) {
    sqlx::query("UPDATE sessions SET client_class = $2 WHERE id = $1")
        .bind(sid)
        .bind(class)
        .execute(pool)
        .await
        .unwrap();
}

fn pc_setup_body(attempt_id: &str) -> Value {
    json!({
        "purpose": "pc_setup",
        "attempt_id": attempt_id,
        "loopback_port": PORT,
        "callback_path": CALLBACK_PATH,
        "client_challenge": challenge(VERIFIER),
    })
}

/// Start a `pc_setup` flow for attempt `id` with the PC's bearer.
async fn pc_flow(p: &Pc, id: &str) -> Flow {
    let r = api(
        &p.app.router,
        "/auth/passkeys/desktop/flows",
        pc_setup_body(id),
        true,
        Some(&p.token),
    )
    .await;
    assert_eq!(r.status, 201, "{}", r.body);
    let flow_id = r.json()["flow_id"].as_str().unwrap().to_string();
    let path = format!("/auth/passkeys/desktop/flows/{flow_id}");
    assert_eq!(
        r.json()["url"].as_str(),
        Some(format!("{TEST_ORIGIN}{path}").as_str()),
        "the flow's page lives under the cookie's path"
    );
    Flow { id: flow_id, path }
}

/// A browser request from the flow's own page: `cookie` is the
/// `name=value` pair the browser holds.
async fn browser(
    router: &axum::Router,
    method: Method,
    path: &str,
    body: Option<Body>,
    content_type: Option<&str>,
    cookie: Option<&str>,
) -> Resp {
    let from = Sender {
        origin: Some(TEST_ORIGIN),
        site: None,
    };
    browser_from(router, from, method, path, body, content_type, cookie).await
}

/// Where a browser request comes from: its `Origin` and `Sec-Fetch-Site`.
#[derive(Clone, Copy, Debug)]
struct Sender {
    origin: Option<&'static str>,
    site: Option<&'static str>,
}

async fn browser_from(
    router: &axum::Router,
    from: Sender,
    method: Method,
    path: &str,
    body: Option<Body>,
    content_type: Option<&str>,
    cookie: Option<&str>,
) -> Resp {
    let mut req = peer_request(method, path);
    if let Some(o) = from.origin {
        req = req.header("Origin", o);
    }
    if let Some(s) = from.site {
        req = req.header("Sec-Fetch-Site", s);
    }
    if let Some(ct) = content_type {
        req = req.header("Content-Type", ct);
    }
    if let Some(c) = cookie {
        req = req.header("Cookie", c);
    }
    send(router, req.body(body.unwrap_or_else(Body::empty)).unwrap()).await
}

async fn type_code(p: &Pc, flow: &Flow, code: &str, cookie: Option<&str>) -> Resp {
    browser(
        &p.app.router,
        Method::POST,
        &format!("{}/code", flow.path),
        Some(Body::from(json!({ "code": code }).to_string())),
        Some("application/json"),
        cookie,
    )
    .await
}

async fn pc_options(p: &Pc, flow: &Flow, cookie: Option<&str>) -> Resp {
    browser(
        &p.app.router,
        Method::POST,
        &format!("{}/options", flow.path),
        Some(Body::from("{}")),
        Some("application/json"),
        cookie,
    )
    .await
}

async fn pc_finish(p: &Pc, path: &str, fields: &[(&str, &str)], cookie: Option<&str>) -> Resp {
    browser(
        &p.app.router,
        Method::POST,
        &format!("{path}/finish"),
        Some(Body::from(serde_urlencoded::to_string(fields).unwrap())),
        Some("application/x-www-form-urlencoded"),
        cookie,
    )
    .await
}

/// The `name=value` pair of the browser-secret cookie in `r`.
fn pcsetup_cookie(r: &Resp) -> String {
    let set = r
        .headers
        .get_all("set-cookie")
        .iter()
        .filter_map(|v| v.to_str().ok())
        .find(|c| c.starts_with(PCSETUP_COOKIE))
        .unwrap_or_else(|| panic!("no browser secret cookie: {:?}", r.headers));
    set.split(';').next().unwrap().to_string()
}

/// Type the right code: the browser's cookie.
async fn right_code(p: &Pc, flow: &Flow, code: &str) -> String {
    let r = type_code(p, flow, code, None).await;
    assert_eq!(
        (r.status, r.json()["state"].as_str()),
        (200, Some("enrolled")),
        "{}",
        r.body
    );
    pcsetup_cookie(&r)
}

/// A soft registration of `options`' ceremony: the `finish` form fields.
fn registered(opts: &Resp) -> (String, String) {
    assert_eq!(opts.status, 200, "{}", opts.body);
    assert_eq!(opts.json()["ceremony"], "create");
    let cred = SoftAuthenticator::new().register(TEST_ORIGIN, &opts.json()["options"]);
    (
        cred.to_string(),
        opts.json()["ceremony_id"].as_str().unwrap().to_string(),
    )
}

/// A fresh attempt (a restart) up to the hand-off:
/// `(attempt id, flow, hand-off code, phone code)`.
async fn handed_off(p: &Pc) -> (String, Flow, String, String) {
    let (id, code) = with_code(p).await;
    let flow = pc_flow(p, &id).await;
    let cookie = right_code(p, &flow, &code).await;
    let (cred, ceremony_id) = registered(&pc_options(p, &flow, Some(&cookie)).await);
    let h = pc_finish(
        p,
        &flow.path,
        &[("credential", &cred), ("ceremony_id", &ceremony_id)],
        Some(&cookie),
    )
    .await;
    assert_eq!(h.status, 200, "{}", h.body);
    let handoff = hidden(&h.body, "code").expect("hand-off code");
    (id, flow, handoff, code)
}

async fn pc_redeem(p: &Pc, flow: &Flow, code: &str, verifier: &str, bearer: Option<&str>) -> Resp {
    redeem(&p.app.router, &flow.id, code, verifier, bearer).await
}

#[sqlx::test]
async fn desktop_flow_needs_the_starting_session(pool: PgPool) {
    let p = desktop_pc(&pool).await;
    let (id, _) = with_code(&p).await;
    let router = &p.app.router;
    let flows = "/auth/passkeys/desktop/flows";
    // No bearer → 401.
    let r = api(router, flows, pc_setup_body(&id), true, None).await;
    assert_eq!(r.status, 401, "{}", r.body);
    // Another session of the same account does not see the attempt.
    let (_, other) = create_probation_session_with_token(&pool, &p.app.config, p.user).await;
    let r = api(router, flows, pc_setup_body(&id), true, Some(&other)).await;
    assert_eq!(r.status, 404, "{}", r.body);
    // An unknown attempt, and none at all.
    let r = api(
        router,
        flows,
        pc_setup_body(&Uuid::new_v4().to_string()),
        true,
        Some(&p.token),
    )
    .await;
    assert_eq!(r.status, 404, "{}", r.body);
    let mut body = pc_setup_body(&id);
    body.as_object_mut().unwrap().remove("attempt_id");
    let r = api(router, flows, body, true, Some(&p.token)).await;
    assert_eq!(r.status, 400, "{}", r.body);
    // Not from a browser.
    let r = api(router, flows, pc_setup_body(&id), false, Some(&p.token)).await;
    assert_eq!(r.json()["error"], "CLIENT_OUTDATED", "{}", r.body);
    // The requesting session starts it.
    pc_flow(&p, &id).await;
}

#[sqlx::test]
async fn desktop_flow_options_refused_before_the_right_code(pool: PgPool) {
    let p = desktop_pc(&pool).await;
    let (id, _) = with_code(&p).await;
    let flow = pc_flow(&p, &id).await;
    // The page asks for the code first.
    let pg = page(&p.app.router, &flow.path, None).await;
    assert_eq!(pg.status, 200, "{}", pg.body);
    assert!(pg.body.contains("id=\"code-form\""), "{}", pg.body);
    assert!(!pg.body.contains("id=\"start\""), "{}", pg.body);
    // No ceremony before the code, on the flow's routes or F1's.
    let o = pc_options(&p, &flow, None).await;
    assert_eq!(
        (o.status, o.json()["error"].as_str()),
        (400, Some("PASSKEY_CEREMONY_INVALID")),
        "{}",
        o.body
    );
    let legacy = format!("/auth/passkeys/desktop/{}", flow.id);
    let o = api(
        &p.app.router,
        &format!("{legacy}/options"),
        json!({}),
        false,
        None,
    )
    .await;
    assert_eq!(o.status, 400, "{}", o.body);
    assert_eq!(page(&p.app.router, &legacy, None).await.status, 404);
    // A made-up cookie does not help either.
    let fake = format!("{PCSETUP_COOKIE}={}", "A".repeat(43));
    assert_eq!(pc_options(&p, &flow, Some(&fake)).await.status, 400);
    assert_eq!(attempt_state(&pool, &id).await.0, "code");
}

#[sqlx::test]
async fn set_cookie_has_the_exact_attributes(pool: PgPool) {
    let p = desktop_pc(&pool).await;
    let (id, code) = with_code(&p).await;
    let flow = pc_flow(&p, &id).await;
    let r = type_code(&p, &flow, &code, None).await;
    assert_eq!(r.status, 200, "{}", r.body);
    let cookies: Vec<&str> = r
        .headers
        .get_all("set-cookie")
        .iter()
        .filter_map(|v| v.to_str().ok())
        .collect();
    assert_eq!(cookies.len(), 1, "{cookies:?}");
    let (pair, attrs) = cookies[0].split_once("; ").unwrap();
    let value = pair.strip_prefix(&format!("{PCSETUP_COOKIE}=")).unwrap();
    assert_eq!(
        attrs,
        format!(
            "HttpOnly; Secure; SameSite=Strict; Path=/auth/passkeys/desktop/flows/{}",
            flow.id
        )
    );
    // A fresh 256-bit value, b64u, stored only as its keyed hash.
    assert_eq!(
        vc_server::approvals::secrets::b64u_decode(value).map(|b| b.len()),
        Some(32)
    );
    let raw: String = {
        use fred::interfaces::KeysInterface;
        p.app
            .state
            .redis
            .get(format!("passkey:flow:{}", flow.id))
            .await
            .unwrap()
    };
    assert!(!raw.contains(value), "{raw}");
    assert!(raw.contains("browser_secret_hash"), "{raw}");
    // The response body carries no secret and no enrolment id.
    assert_eq!(r.json(), json!({"state": "enrolled"}));
}

#[sqlx::test]
async fn only_the_browser_that_typed_the_code_continues_the_flow(pool: PgPool) {
    let p = desktop_pc(&pool).await;
    let (id, code) = with_code(&p).await;
    let flow = pc_flow(&p, &id).await;
    let cookie = right_code(&p, &flow, &code).await;
    // The legitimate browser runs its ceremony.
    let opts = pc_options(&p, &flow, Some(&cookie)).await;
    let (cred, ceremony_id) = registered(&opts);
    // Without the cookie (or with another) there is no ceremony.
    assert_eq!(pc_options(&p, &flow, None).await.status, 400);
    let fake = format!("{PCSETUP_COOKIE}={}", "A".repeat(43));
    assert_eq!(pc_options(&p, &flow, Some(&fake)).await.status, 400);
    // Its finish of the live ceremony is refused and consumes nothing,
    // on the flow's routes and on F1's.
    let fields = [
        ("credential", cred.as_str()),
        ("ceremony_id", ceremony_id.as_str()),
    ];
    for (path, c) in [
        (flow.path.clone(), None),
        (flow.path.clone(), Some(fake.as_str())),
        (format!("/auth/passkeys/desktop/{}", flow.id), None),
    ] {
        let r = pc_finish(&p, &path, &fields, c).await;
        assert_ne!(r.status, 200, "{}", r.body);
        assert!(hidden(&r.body, "code").is_none(), "{}", r.body);
        assert!(hidden(&r.body, "error").is_none(), "{}", r.body);
    }
    // A second code entry is refused, also with the right code.
    for c in [code.clone(), wrong(&code)] {
        let r = type_code(&p, &flow, &c, None).await;
        assert_eq!(r.status, 409, "{}", r.body);
        assert!(r.headers.get("set-cookie").is_none());
    }
    // The page without the cookie is no use.
    assert_eq!(page(&p.app.router, &flow.path, None).await.status, 404);
    // F1's cancel does not reach this flow; the flow's needs the cookie.
    let c = form(
        &p.app.router,
        &format!("/auth/passkeys/desktop/{}/cancel", flow.id),
        &[],
    )
    .await;
    assert!(hidden(&c.body, "error").is_none(), "{}", c.body);
    let c = form(&p.app.router, &format!("{}/cancel", flow.path), &[]).await;
    assert!(hidden(&c.body, "error").is_none(), "{}", c.body);
    // The legitimate browser still finishes.
    let h = pc_finish(&p, &flow.path, &fields, Some(&cookie)).await;
    assert_eq!(h.status, 200, "{}", h.body);
    let handoff = hidden(&h.body, "code").expect("hand-off code");
    let r = pc_redeem(&p, &flow, &handoff, VERIFIER, Some(&p.token)).await;
    assert_eq!(r.status, 200, "{}", r.body);
}

#[sqlx::test]
async fn a_flow_without_the_phones_code_hands_back_nothing(pool: PgPool) {
    // Without the phone's code there is no ceremony, so the flow hands
    // nothing back to the app.
    let p = desktop_pc(&pool).await;
    let (id, code) = with_code(&p).await;
    let flow = pc_flow(&p, &id).await;
    let mut tries = Vec::new();
    for _ in 0..3 {
        assert_eq!(pc_options(&p, &flow, None).await.status, 400);
        let r = type_code(&p, &flow, &wrong(&code), None).await;
        assert_eq!(r.status, 200, "{}", r.body);
        assert!(r.headers.get("set-cookie").is_none());
        tries.push(r.json()["state"].as_str().unwrap().to_string());
        let f = pc_finish(&p, &flow.path, &[("credential", "{}")], None).await;
        assert!(hidden(&f.body, "code").is_none(), "{}", f.body);
    }
    assert_eq!(tries, ["code", "code", "failed"]);
    // The attempt ended and shows it; the right code is now useless.
    assert_eq!(
        attempt_state(&pool, &id).await,
        ("failed".into(), Some("too_many_codes".into()))
    );
    assert_ne!(type_code(&p, &flow, &code, None).await.status, 200);
    // Nothing to redeem: the flow never held a result.
    let r = pc_redeem(&p, &flow, "made-up", VERIFIER, Some(&p.token)).await;
    assert_invalid(&r);
    assert_eq!(passkeys_of(&pool, p.user).await, 0);
}

#[sqlx::test]
async fn wrong_codes_in_the_browser_count_on_the_attempt(pool: PgPool) {
    let p = desktop_pc(&pool).await;
    let (id, code) = with_code(&p).await;
    let flow = pc_flow(&p, &id).await;
    let r = type_code(&p, &flow, &wrong(&code), None).await;
    assert_eq!(
        r.json(),
        json!({"state": "code", "tries_left": 2}),
        "{}",
        r.body
    );
    // A malformed code spends no try.
    let r = type_code(&p, &flow, "12ab", None).await;
    assert_eq!(r.status, 400, "{}", r.body);
    let r = type_code(&p, &flow, &wrong(&code), None).await;
    assert_eq!(
        r.json(),
        json!({"state": "code", "tries_left": 1}),
        "{}",
        r.body
    );
    let tries: i16 = sqlx::query_scalar("SELECT tries FROM pc_setup_attempts WHERE id = $1::uuid")
        .bind(&id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(tries, 2);
    // The PC's own route shares the counter: its wrong code is the third.
    let (s, body) = crate::pc_setup::enter(&p, &p.token, &id, &wrong(&code)).await;
    assert_eq!((s, body["state"].as_str()), (200, Some("failed")), "{body}");
    let failed: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM security_events WHERE subject_id = $1 AND event_type = 'pc_setup_failed'",
    )
    .bind(p.user)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(failed, 1);
    assert_eq!(type_code(&p, &flow, &code, None).await.status, 409);
}

#[sqlx::test]
async fn nothing_is_stored_before_redeem(pool: PgPool) {
    let p = desktop_pc(&pool).await;
    let (id, flow, handoff, _) = handed_off(&p).await;
    // After `finish`: no passkey row, the enrolment and the attempt open.
    let rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM passkeys WHERE user_id = $1")
        .bind(p.user)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(rows, 0);
    let enrollment: String =
        sqlx::query_scalar("SELECT enrollment_id::text FROM pc_setup_attempts WHERE id = $1::uuid")
            .bind(&id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(enrollment_state(&pool, &enrollment).await, "issued");
    assert_eq!(attempt_state(&pool, &id).await.0, "enrolled");
    let ttl = flow_ttl(&p.app, &flow).await;
    assert!(
        (1..=60).contains(&ttl),
        "the pending result lives 60 s: {ttl}"
    );
    // `redeem` completes it.
    let r = pc_redeem(&p, &flow, &handoff, VERIFIER, Some(&p.token)).await;
    assert_eq!(r.status, 200, "{}", r.body);
    assert_eq!(passkeys_of(&pool, p.user).await, 1);
    assert_eq!(enrollment_state(&pool, &enrollment).await, "done");
    assert_eq!(attempt_state(&pool, &id).await.0, "completed");
    // Named after the browser that made it, never just "Passkey".
    assert_eq!(r.json()["passkey"]["label"], "Web browser", "{}", r.body);
}

#[sqlx::test]
async fn desktop_redeem_needs_code_verifier_and_the_starting_session(pool: PgPool) {
    let p = desktop_pc(&pool).await;
    let (_, other) = create_probation_session_with_token(&pool, &p.app.config, p.user).await;
    let other_verifier = "another-verifier-of-at-least-forty-three-characters-0123";
    // Each refusal spends the flow: every case runs on a fresh attempt.
    for case in ["no bearer", "other session", "wrong code", "wrong verifier"] {
        let (id, flow, handoff, _) = handed_off(&p).await;
        let r = match case {
            "no bearer" => pc_redeem(&p, &flow, &handoff, VERIFIER, None).await,
            "other session" => pc_redeem(&p, &flow, &handoff, VERIFIER, Some(&other)).await,
            "wrong code" => {
                pc_redeem(&p, &flow, "x".repeat(43).as_str(), VERIFIER, Some(&p.token)).await
            }
            _ => pc_redeem(&p, &flow, &handoff, other_verifier, Some(&p.token)).await,
        };
        assert_invalid(&r);
        assert_eq!(passkeys_of(&pool, p.user).await, 0, "{case}");
        assert_eq!(attempt_state(&pool, &id).await.0, "enrolled", "{case}");
        // The flow is spent: the right redemption now fails too.
        assert_invalid(&pc_redeem(&p, &flow, &handoff, VERIFIER, Some(&p.token)).await);
    }
    let (id, flow, handoff, _) = handed_off(&p).await;
    let r = pc_redeem(&p, &flow, &handoff, VERIFIER, Some(&p.token)).await;
    assert_eq!(r.status, 200, "{}", r.body);
    let body = r.json();
    assert_eq!(body["purpose"], "pc_setup");
    assert_eq!(body["user_id"], p.user.to_string());
    assert_eq!(body["passkey"]["state"], "active", "{body}");
    for token in ["access_token", "refresh_token", "grant"] {
        assert!(body.get(token).is_none(), "{body}");
    }
    assert_eq!(attempt_state(&pool, &id).await.0, "completed");
    let via: String = sqlx::query_scalar(
        "SELECT details->>'via' FROM security_events
          WHERE subject_id = $1 AND event_type = 'authenticator_added'",
    )
    .bind(p.user)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(via, "pc_setup");
}

#[sqlx::test]
async fn restart_voids_the_desktop_flow(pool: PgPool) {
    // A restart voids open desktop flows: redeem is refused.
    let p = desktop_pc(&pool).await;
    let (id, flow, handoff, _) = handed_off(&p).await;
    let r = TestApp::request(Method::POST, "/auth/pc-setups")
        .header("Authorization", format!("Bearer {}", p.token))
        .header("Content-Type", "application/json")
        .body(Body::from("{}"))
        .unwrap();
    assert_eq!(p.app.oneshot(r).await.status(), 201);
    assert_eq!(
        attempt_state(&pool, &id).await,
        ("cancelled".into(), Some("restarted".into()))
    );
    assert_invalid(&pc_redeem(&p, &flow, &handoff, VERIFIER, Some(&p.token)).await);
    assert_eq!(passkeys_of(&pool, p.user).await, 0);
}

#[sqlx::test]
async fn ceremony_retry_after_cancel_keeps_browser_secret(pool: PgPool) {
    // Review Focus 4: a cancelled or failed ceremony is retried in the same
    // browser with the same cookie (the code is spent, it cannot be typed
    // again).
    let p = desktop_pc(&pool).await;
    let (id, code) = with_code(&p).await;
    let flow = pc_flow(&p, &id).await;
    let cookie = right_code(&p, &flow, &code).await;
    // The page now offers the passkey step to this browser.
    let pg = browser(
        &p.app.router,
        Method::GET,
        &flow.path,
        None,
        None,
        Some(&cookie),
    )
    .await;
    assert_eq!(pg.status, 200, "{}", pg.body);
    assert!(pg.body.contains("id=\"start\""), "{}", pg.body);
    assert!(!pg.body.contains("id=\"code-form\""), "{}", pg.body);
    // The user cancels the browser's dialog: the retry gets a new ceremony.
    let first = pc_options(&p, &flow, Some(&cookie)).await;
    let (_, first_id) = registered(&first);
    let second = pc_options(&p, &flow, Some(&cookie)).await;
    let (_, second_id) = registered(&second);
    assert_ne!(first_id, second_id);
    // The old ceremony cannot finish any more.
    let stale = pc_finish(
        &p,
        &flow.path,
        &[("credential", "{}"), ("ceremony_id", &first_id)],
        Some(&cookie),
    )
    .await;
    assert_eq!(stale.status, 400, "{}", stale.body);
    // A failed ceremony keeps the flow: the page offers a retry, no hand-off.
    let failed = pc_finish(
        &p,
        &flow.path,
        &[
            ("credential", "{\"id\":\"garbage\"}"),
            ("ceremony_id", &second_id),
        ],
        Some(&cookie),
    )
    .await;
    assert!(hidden(&failed.body, "code").is_none(), "{}", failed.body);
    assert!(hidden(&failed.body, "error").is_none(), "{}", failed.body);
    // A 303 to the flow's page, so the page's base path stays the flow's
    // and a reload never re-posts the spent finish.
    assert_eq!(failed.status, 303, "{}", failed.body);
    let location = failed.header("location").unwrap().to_string();
    assert_eq!(location, format!("{}?failed", flow.path));
    let again = browser(
        &p.app.router,
        Method::GET,
        &location,
        None,
        None,
        Some(&cookie),
    )
    .await;
    assert_eq!(again.status, 200, "{}", again.body);
    assert!(again.body.contains("id=\"start\""), "{}", again.body);
    assert!(
        again.body.contains("That didn&#39;t work"),
        "{}",
        again.body
    );
    assert!(
        again
            .body
            .contains(&format!("action=\"{}/finish\"", flow.path)),
        "{}",
        again.body
    );
    // Without the cookie the redirect target is no use.
    assert_eq!(page(&p.app.router, &location, None).await.status, 404);
    // The retry works with the same cookie.
    let (cred, third_id) = registered(&pc_options(&p, &flow, Some(&cookie)).await);
    let h = pc_finish(
        &p,
        &flow.path,
        &[("credential", &cred), ("ceremony_id", &third_id)],
        Some(&cookie),
    )
    .await;
    let handoff = hidden(&h.body, "code").expect("hand-off code");
    assert_eq!(
        pc_redeem(&p, &flow, &handoff, VERIFIER, Some(&p.token))
            .await
            .status,
        200
    );
}

#[sqlx::test]
async fn phone_code_never_appears_in_the_rendered_pages(pool: PgPool) {
    let p = desktop_pc(&pool).await;
    let (id, code) = with_code(&p).await;
    let flow = pc_flow(&p, &id).await;
    let mut seen = vec![page(&p.app.router, &flow.path, Some("de")).await];
    seen.push(page(&p.app.router, &flow.path, None).await);
    let entry = type_code(&p, &flow, &code, None).await;
    let cookie = pcsetup_cookie(&entry);
    seen.push(entry);
    seen.push(
        browser(
            &p.app.router,
            Method::GET,
            &flow.path,
            None,
            None,
            Some(&cookie),
        )
        .await,
    );
    let opts = pc_options(&p, &flow, Some(&cookie)).await;
    let (cred, ceremony_id) = registered(&opts);
    seen.push(opts);
    let h = pc_finish(
        &p,
        &flow.path,
        &[("credential", &cred), ("ceremony_id", &ceremony_id)],
        Some(&cookie),
    )
    .await;
    assert!(hidden(&h.body, "code").is_some(), "{}", h.body);
    seen.push(h);
    for r in &seen {
        assert!(!contains_code(&r.body, &code), "{}", r.body);
        for v in r.headers.values() {
            assert!(!contains_code(v.to_str().unwrap_or(""), &code));
        }
    }
    // The keypad page names what it is for and keeps the strict CSP.
    let pg = &seen[1];
    assert!(pg.body.contains("ceremony.js"), "{}", pg.body);
    assert!(pg
        .header("content-security-policy")
        .unwrap()
        .contains("script-src 'self'"));
    assert!(seen[0].body.contains("lang=\"de\""));
}

/// The pending result's credential id (b64u) from the stored flow.
async fn pending_cred_id(p: &Pc, flow: &Flow) -> Vec<u8> {
    use fred::interfaces::KeysInterface;
    let raw: String = p
        .app
        .state
        .redis
        .get(format!("passkey:flow:{}", flow.id))
        .await
        .unwrap();
    let v: Value = serde_json::from_str(&raw).unwrap();
    let id = v["result"]["outcome"]["PcSetup"]["verified"]["passkey"]["cred"]["cred_id"]
        .as_str()
        .unwrap_or_else(|| panic!("credential id in {v}"));
    vc_server::approvals::secrets::b64u_decode(id).unwrap()
}

#[sqlx::test]
async fn desktop_redeem_keeps_passkey_errors_readable(pool: PgPool) {
    // The passkey limit and a known credential at redeem answer 409
    // with their own code; the attempt stays `enrolled` and readable.
    let p = desktop_pc(&pool).await;
    let (id, flow, handoff, _) = handed_off(&p).await;
    for _ in 0..vc_server::auth::passkeys::MAX_ACTIVE_PASSKEYS {
        seed_passkey(&p.app, p.user, PasskeyState::Active).await;
    }
    let r = pc_redeem(&p, &flow, &handoff, VERIFIER, Some(&p.token)).await;
    assert_eq!(
        (r.status, r.json()["error"].as_str()),
        (409, Some("PASSKEY_LIMIT")),
        "{}",
        r.body
    );
    assert_eq!(attempt_state(&pool, &id).await, ("enrolled".into(), None));
    let (s, poll) = crate::pc_setup::call(
        &p.app,
        Method::GET,
        &format!("/auth/pc-setups/{id}?wait=0"),
        &p.token,
        None,
    )
    .await;
    assert_eq!(
        (s, poll["state"].as_str()),
        (200, Some("enrolled")),
        "{poll}"
    );

    // A known credential: another account already holds it.
    let q = desktop_pc(&pool).await;
    let (id, flow, handoff, _) = handed_off(&q).await;
    let cred_id = pending_cred_id(&q, &flow).await;
    let (other, _) = create_test_user(&pool).await;
    let (_, row) = seed_passkey(&q.app, other, PasskeyState::Active).await;
    sqlx::query("UPDATE passkeys SET credential_id = $2 WHERE id = $1")
        .bind(row.id)
        .bind(&cred_id)
        .execute(&pool)
        .await
        .unwrap();
    let r = pc_redeem(&q, &flow, &handoff, VERIFIER, Some(&q.token)).await;
    assert_eq!(
        (r.status, r.json()["error"].as_str()),
        (409, Some("PASSKEY_ALREADY_REGISTERED")),
        "{}",
        r.body
    );
    assert_eq!(attempt_state(&pool, &id).await, ("enrolled".into(), None));
    assert_eq!(passkeys_of(&pool, q.user).await, 0);
}

// One path per attempt: the code and the passkey take the same way.

#[sqlx::test]
async fn a_code_typed_in_a_flow_registers_through_that_flow_only(pool: PgPool) {
    let p = desktop_pc(&pool).await;
    let (id, code) = with_code(&p).await;
    let flow = pc_flow(&p, &id).await;
    right_code(&p, &flow, &code).await;
    // The requesting session's web routes refuse the attempt.
    let (s, body) = passkey_start(&p, &p.token, &id).await;
    assert_eq!(
        (s, body["error"].as_str()),
        (409, Some("APPROVAL_STATE")),
        "{body}"
    );
    assert_eq!(passkeys_of(&pool, p.user).await, 0);
    assert_eq!(attempt_state(&pool, &id).await.0, "enrolled");
}

#[sqlx::test]
async fn a_code_typed_on_the_web_cannot_be_used_by_a_flow(pool: PgPool) {
    let p = desktop_pc(&pool).await;
    let (id, code) = with_code(&p).await;
    let flow = pc_flow(&p, &id).await;
    let (s, body) = crate::pc_setup::enter(&p, &p.token, &id, &code).await;
    assert_eq!(
        (s, body["state"].as_str()),
        (200, Some("enrolled")),
        "{body}"
    );
    // The flow never gets a browser secret, so it has no ceremony to run.
    assert_eq!(type_code(&p, &flow, &code, None).await.status, 409);
    assert_eq!(pc_options(&p, &flow, None).await.status, 400);
    let path: (Option<String>, Option<Uuid>) =
        sqlx::query_as("SELECT code_via, code_flow_id FROM pc_setup_attempts WHERE id = $1::uuid")
            .bind(&id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(path, (Some("web".into()), None));
}

#[sqlx::test]
async fn a_session_of_another_client_cannot_start_a_pc_setup_flow(pool: PgPool) {
    let p = desktop_pc(&pool).await;
    let (id, _) = with_code(&p).await;
    for class in ["web", "android"] {
        set_client_class(&pool, p.sid, class).await;
        // The desktop header alone is not enough.
        let r = api(
            &p.app.router,
            "/auth/passkeys/desktop/flows",
            pc_setup_body(&id),
            true,
            Some(&p.token),
        )
        .await;
        assert_eq!(
            (r.status, r.json()["error"].as_str()),
            (403, Some("FORBIDDEN")),
            "{}",
            r.body
        );
    }
}

#[sqlx::test]
async fn a_redeem_without_a_pending_result_consumes_nothing(pool: PgPool) {
    let p = desktop_pc(&pool).await;
    let (id, code) = with_code(&p).await;
    let flow = pc_flow(&p, &id).await;
    let cookie = right_code(&p, &flow, &code).await;
    // Someone who saw the flow URL redeems before any result exists.
    assert_invalid(&pc_redeem(&p, &flow, "x", VERIFIER, None).await);
    // The browser that typed the code still runs its ceremony.
    assert_eq!(pc_options(&p, &flow, Some(&cookie)).await.status, 200);
}

// A refusal reaches the app; the page offers only what can work.

#[sqlx::test]
async fn a_passkey_limit_in_the_browser_reaches_the_app(pool: PgPool) {
    let p = desktop_pc(&pool).await;
    let (id, code) = with_code(&p).await;
    let flow = pc_flow(&p, &id).await;
    let cookie = right_code(&p, &flow, &code).await;
    for _ in 0..vc_server::auth::passkeys::MAX_ACTIVE_PASSKEYS {
        seed_passkey(&p.app, p.user, PasskeyState::Active).await;
    }
    // The options call is refused; the flow stays for the page to hand it on.
    let r = pc_options(&p, &flow, Some(&cookie)).await;
    assert_eq!(
        (r.status, r.json()["error"].as_str()),
        (409, Some("PASSKEY_LIMIT")),
        "{}",
        r.body
    );
    // The page, reloaded, hands the refusal to the app's loopback listener.
    let page = browser(
        &p.app.router,
        Method::GET,
        &flow.path,
        None,
        None,
        Some(&cookie),
    )
    .await;
    assert_eq!(
        hidden(&page.body, "error").as_deref(),
        Some("PASSKEY_LIMIT"),
        "{}",
        page.body
    );
}

#[sqlx::test]
async fn the_right_code_keeps_the_flow_for_the_enrolments_time(pool: PgPool) {
    use fred::interfaces::KeysInterface;
    let p = desktop_pc(&pool).await;
    let (id, code) = with_code(&p).await;
    let flow = pc_flow(&p, &id).await;
    // Most of the flow's time went by before the code was typed.
    let () = p
        .app
        .state
        .redis
        .expire(format!("passkey:flow:{}", flow.id), 60, None)
        .await
        .unwrap();
    right_code(&p, &flow, &code).await;
    let ttl = flow_ttl(&p.app, &flow).await;
    assert!(ttl > 590, "the flow lives as long as the enrolment: {ttl}");
}

#[sqlx::test]
async fn the_code_field_accepts_a_pasted_code_with_spaces(pool: PgPool) {
    let p = desktop_pc(&pool).await;
    let (id, _) = with_code(&p).await;
    let flow = pc_flow(&p, &id).await;
    let page = browser(&p.app.router, Method::GET, &flow.path, None, None, None).await;
    assert_eq!(page.status, 200);
    assert!(
        !page.body.contains("maxlength=\"6\""),
        "a pasted \"123 456\" must not be cut"
    );
    assert!(
        !page.body.contains("pattern="),
        "the page's own check gives the message"
    );
    assert!(page.body.contains("novalidate"));
}

// The code never travels in a URL, also without script (C27).

#[sqlx::test]
async fn the_code_form_posts_and_waits_for_its_script(pool: PgPool) {
    let p = desktop_pc(&pool).await;
    let (id, _) = with_code(&p).await;
    let flow = pc_flow(&p, &id).await;
    let pg = page(&p.app.router, &flow.path, None).await;
    assert_eq!(pg.status, 200, "{}", pg.body);
    let form = pg
        .body
        .split("<form id=\"code-form\"")
        .nth(1)
        .and_then(|f| f.split("</form>").next())
        .expect("code form");
    let head = form.split('>').next().unwrap();
    assert!(head.contains("method=\"post\""), "{head}");
    assert!(
        head.contains(&format!("action=\"{}/code\"", flow.path)),
        "{head}"
    );
    for tag in ["<input", "<button"] {
        let el = form
            .split(tag)
            .nth(1)
            .and_then(|e| e.split('>').next())
            .unwrap();
        assert!(el.contains(" disabled"), "{tag} must start disabled: {el}");
    }
    assert!(pg.body.contains("<noscript>"), "{}", pg.body);
    // The script enables the keypad only after its submit handler is installed.
    let js = page(&p.app.router, "/auth/passkeys/assets/ceremony.js", None).await;
    let handler = js
        .body
        .find("addEventListener(\"submit\"")
        .expect("submit handler");
    let enabled = js
        .body
        .find("input.disabled = false")
        .expect("keypad enabled");
    assert!(handler < enabled, "handler first");
}

// Only the page's own origin changes a flow (R-67).

const OTHER_SITE: Sender = Sender {
    origin: Some("https://other.example.com"),
    site: Some("cross-site"),
};
const SIBLING: Sender = Sender {
    origin: Some("null"),
    site: Some("same-site"),
};
const UNKNOWN: Sender = Sender {
    origin: None,
    site: None,
};

#[sqlx::test]
async fn the_flow_routes_accept_only_the_pages_own_origin(pool: PgPool) {
    let p = desktop_pc(&pool).await;
    let (id, code) = with_code(&p).await;
    let flow = pc_flow(&p, &id).await;
    let r = &p.app.router;
    let json_body = |v: Value| Some(Body::from(v.to_string()));
    let code_path = format!("{}/code", flow.path);
    let cancel_path = format!("{}/cancel", flow.path);
    // Before the code: no entry and no cancel from elsewhere.
    for from in [OTHER_SITE, SIBLING, UNKNOWN] {
        let t = browser_from(
            r,
            from,
            Method::POST,
            &code_path,
            json_body(json!({"code": wrong(&code)})),
            Some("application/json"),
            None,
        )
        .await;
        assert_eq!(t.status, 403, "{from:?}: {}", t.body);
        let c = browser_from(
            r,
            from,
            Method::POST,
            &cancel_path,
            None,
            Some("application/x-www-form-urlencoded"),
            None,
        )
        .await;
        assert_eq!(c.status, 403, "{from:?}: {}", c.body);
        assert!(hidden(&c.body, "error").is_none(), "{}", c.body);
    }
    let tries: i16 = sqlx::query_scalar("SELECT tries FROM pc_setup_attempts WHERE id = $1::uuid")
        .bind(&id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(tries, 0, "no try was spent");
    let cookie = right_code(&p, &flow, &code).await;
    // After it: the browser's cookie alone is not enough.
    let opts = pc_options(&p, &flow, Some(&cookie)).await;
    let (cred, ceremony_id) = registered(&opts);
    let fields = [
        ("credential", cred.as_str()),
        ("ceremony_id", ceremony_id.as_str()),
    ];
    let form_body = || Some(Body::from(serde_urlencoded::to_string(fields).unwrap()));
    for from in [OTHER_SITE, SIBLING, UNKNOWN] {
        let o = browser_from(
            r,
            from,
            Method::POST,
            &format!("{}/options", flow.path),
            json_body(json!({})),
            Some("application/json"),
            Some(&cookie),
        )
        .await;
        assert_eq!(o.status, 403, "{from:?}: {}", o.body);
        let f = browser_from(
            r,
            from,
            Method::POST,
            &format!("{}/finish", flow.path),
            form_body(),
            Some("application/x-www-form-urlencoded"),
            Some(&cookie),
        )
        .await;
        assert_eq!(f.status, 403, "{from:?}: {}", f.body);
        assert!(hidden(&f.body, "code").is_none(), "{}", f.body);
        let c = browser_from(
            r,
            from,
            Method::POST,
            &cancel_path,
            None,
            Some("application/x-www-form-urlencoded"),
            Some(&cookie),
        )
        .await;
        assert_eq!(c.status, 403, "{from:?}: {}", c.body);
        assert!(hidden(&c.body, "error").is_none(), "{}", c.body);
    }
    // The flow is untouched: the page's own form post (its origin hidden by
    // the page's referrer policy) finishes it.
    let own_form = Sender {
        origin: Some("null"),
        site: Some("same-origin"),
    };
    let h = browser_from(
        r,
        own_form,
        Method::POST,
        &format!("{}/finish", flow.path),
        form_body(),
        Some("application/x-www-form-urlencoded"),
        Some(&cookie),
    )
    .await;
    assert_eq!(h.status, 200, "{}", h.body);
    let handoff = hidden(&h.body, "code").expect("hand-off code");
    let rd = pc_redeem(&p, &flow, &handoff, VERIFIER, Some(&p.token)).await;
    assert_eq!(rd.status, 200, "{}", rd.body);
}

// One path per attempt: the web route refuses before it takes a ceremony.

#[sqlx::test]
async fn a_web_finish_of_a_flow_code_is_refused_and_takes_nothing(pool: PgPool) {
    let p = desktop_pc(&pool).await;
    let (id, code) = with_code(&p).await;
    let flow = pc_flow(&p, &id).await;
    let cookie = right_code(&p, &flow, &code).await;
    let (cred, ceremony_id) = registered(&pc_options(&p, &flow, Some(&cookie)).await);
    let body = json!({
        "ceremony_id": ceremony_id,
        "credential": serde_json::from_str::<Value>(&cred).unwrap(),
        "label": "Work PC",
    });
    let (s, b) = crate::pc_setup::passkey_finish(&p, &p.token, &id, body).await;
    assert_eq!(
        (s, b["error"].as_str()),
        (409, Some("APPROVAL_STATE")),
        "{b}"
    );
    // The browser's ceremony is still there.
    let h = pc_finish(
        &p,
        &flow.path,
        &[("credential", &cred), ("ceremony_id", &ceremony_id)],
        Some(&cookie),
    )
    .await;
    assert_eq!(h.status, 200, "{}", h.body);
    assert!(hidden(&h.body, "code").is_some(), "{}", h.body);
}

// The page's own posts carry its origin; a refusal stays a page (R-67).

#[sqlx::test]
async fn the_flow_page_sends_its_origin_and_the_hand_off_no_referrer(pool: PgPool) {
    let p = desktop_pc(&pool).await;
    let (id, code) = with_code(&p).await;
    let flow = pc_flow(&p, &id).await;
    // Same-origin posts from the page carry the real Origin; nothing is sent
    // to another origin.
    let pg = page(&p.app.router, &flow.path, None).await;
    assert_eq!(
        pg.headers
            .get("referrer-policy")
            .and_then(|v| v.to_str().ok()),
        Some("same-origin")
    );
    let cookie = right_code(&p, &flow, &code).await;
    let (cred, ceremony_id) = registered(&pc_options(&p, &flow, Some(&cookie)).await);
    let h = pc_finish(
        &p,
        &flow.path,
        &[("credential", &cred), ("ceremony_id", &ceremony_id)],
        Some(&cookie),
    )
    .await;
    assert_eq!(h.status, 200, "{}", h.body);
    // The hand-off posts to 127.0.0.1 without any referrer.
    assert!(h.body.contains("http://127.0.0.1:"), "{}", h.body);
    assert_eq!(
        h.headers
            .get("referrer-policy")
            .and_then(|v| v.to_str().ok()),
        Some("no-referrer")
    );
}

#[sqlx::test]
async fn a_refused_finish_or_cancel_is_a_page_that_sends_back_to_the_app(pool: PgPool) {
    let p = desktop_pc(&pool).await;
    let (id, code) = with_code(&p).await;
    let flow = pc_flow(&p, &id).await;
    let cookie = right_code(&p, &flow, &code).await;
    let r = &p.app.router;
    for route in ["finish", "cancel"] {
        let x = browser_from(
            r,
            UNKNOWN,
            Method::POST,
            &format!("{}/{route}", flow.path),
            None,
            Some("application/x-www-form-urlencoded"),
            Some(&cookie),
        )
        .await;
        assert_eq!(x.status, 403, "{route}: {}", x.body);
        let ct = x
            .headers
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        assert!(ct.starts_with("text/html"), "{route}: {ct}");
        assert!(
            x.body.contains("Go back to the Kaiku app"),
            "{route}: {}",
            x.body
        );
    }
}
