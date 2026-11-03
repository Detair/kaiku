//! Sign in with a passkey (LP Task 7; F1, C29).

use axum::body::Body;
use axum::http::Method;
use serde_json::{json, Value};
use sqlx::PgPool;
use uuid::Uuid;
use vc_server::auth::passkeys::queries::{self, PasskeyRow, PasskeyState};
use vc_server::ratelimit::{LimitConfig, RateLimits};

use crate::helpers::login::{peer_request, TEST_ORIGIN};
use crate::helpers::passkeys::{
    limited_router, limited_router_with, seed_passkey, test_passkey_rp, SoftAuthenticator,
    TEST_APK_ORIGIN,
};
use crate::helpers::{
    body_to_json, create_probation_session_with_token, create_test_user, TestApp,
};

/// `login/start` + `login/finish` with `auth` answering for `row` as
/// `handle_of`'s user handle; `client` sets `X-Kaiku-Client`.
#[allow(clippy::too_many_arguments)]
pub async fn sign_in_with(
    router: &axum::Router,
    pool: &PgPool,
    auth: &mut SoftAuthenticator,
    credential_id: &[u8],
    handle_of: Uuid,
    origin: &str,
    uv: bool,
    username: Option<&str>,
    client: Option<&str>,
) -> (u16, Value, Option<String>) {
    let start = call(router, "/auth/passkeys/login/start", json!({}), client).await;
    assert_eq!(start.0, 200, "{}", start.1);
    let handle = queries::user_handle(pool, handle_of).await.unwrap();
    let cred = auth.assert(origin, &start.1["options"], credential_id, Some(handle), uv);
    call(
        router,
        "/auth/passkeys/login/finish",
        json!({"ceremony_id": start.1["ceremony_id"], "credential": cred, "username": username}),
        client,
    )
    .await
}

/// POST from the test peer → (status, JSON, Set-Cookie).
pub async fn call(
    router: &axum::Router,
    path: &str,
    body: Value,
    client: Option<&str>,
) -> (u16, Value, Option<String>) {
    let mut req = peer_request(Method::POST, path)
        .header("Content-Type", "application/json")
        .header(
            "User-Agent",
            "Mozilla/5.0 (X11; Linux x86_64) Firefox/140.0",
        );
    if let Some(c) = client {
        req = req.header("X-Kaiku-Client", c);
    }
    let resp = tower::ServiceExt::oneshot(
        router.clone(),
        req.body(Body::from(body.to_string())).unwrap(),
    )
    .await
    .unwrap();
    let status = resp.status().as_u16();
    let cookie = resp
        .headers()
        .get_all("set-cookie")
        .iter()
        .filter_map(|v| v.to_str().ok())
        .find(|c| c.contains("kaiku_refresh"))
        .map(str::to_owned);
    (status, body_to_json(resp).await, cookie)
}

async fn signed_in(
    app: &TestApp,
    auth: &mut SoftAuthenticator,
    row: &PasskeyRow,
    client: Option<&str>,
) -> (u16, Value, Option<String>) {
    sign_in_with(
        &app.router,
        &app.pool,
        auth,
        &row.credential_id,
        row.user_id,
        TEST_ORIGIN,
        true,
        None,
        client,
    )
    .await
}

async fn session_count(app: &TestApp, user: Uuid) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM sessions WHERE user_id = $1")
        .bind(user)
        .fetch_one(&app.pool)
        .await
        .unwrap()
}

#[sqlx::test]
async fn passkey_sign_in_returns_session(pool: PgPool) {
    let app = TestApp::with_passkeys(pool).await;
    let (user, _) = create_test_user(&app.pool).await;
    let (mut auth, row) = seed_passkey(&app, user, PasskeyState::Active).await;
    let (s, b, cookie) = signed_in(&app, &mut auth, &row, None).await;
    assert_eq!(s, 200, "{b}");
    assert!(cookie.is_some(), "browser: refresh cookie");
    assert!(b["refresh_token"].is_null(), "{b}");
    let me = TestApp::request(Method::GET, "/auth/me")
        .header(
            "Authorization",
            format!("Bearer {}", b["access_token"].as_str().unwrap()),
        )
        .body(Body::empty())
        .unwrap();
    assert_eq!(app.oneshot(me).await.status(), 200);
    let (s, b, cookie) = signed_in(&app, &mut auth, &row, Some("android")).await;
    assert_eq!(s, 200, "{b}");
    assert!(
        b["refresh_token"].is_string(),
        "native: refresh in the body"
    );
    assert!(cookie.is_none());
}

#[sqlx::test]
async fn passkey_session_is_full(pool: PgPool) {
    let app = TestApp::with_passkeys(pool).await;
    let (user, _) = create_test_user(&app.pool).await;
    let (mut auth, row) = seed_passkey(&app, user, PasskeyState::Active).await;
    let (s, b, _) = signed_in(&app, &mut auth, &row, None).await;
    assert_eq!(s, 200);
    assert_eq!(b["assurance"], "full");
    let sid: Uuid = b["session_id"].as_str().unwrap().parse().unwrap();
    let (assurance, methods, kind, by, class): (
        String,
        Vec<String>,
        Option<String>,
        Option<Uuid>,
        String,
    ) = sqlx::query_as(
        "SELECT assurance, auth_methods, authenticator_type, authenticator_id, client_class
               FROM sessions WHERE id = $1",
    )
    .bind(sid)
    .fetch_one(&app.pool)
    .await
    .unwrap();
    assert_eq!(assurance, "full");
    assert_eq!(methods, vec!["passkey".to_string()]);
    assert_eq!((kind.as_deref(), by), (Some("passkey"), Some(row.id)));
    assert_eq!(class, "web");
    // The C28 sign-in event.
    let ev: (Value,) = sqlx::query_as(
        "SELECT details FROM security_events WHERE subject_id = $1 AND event_type = 'sign_in'",
    )
    .bind(user)
    .fetch_one(&app.pool)
    .await
    .unwrap();
    assert_eq!(ev.0["auth_methods"], json!(["passkey"]));
    assert_eq!(ev.0["assurance"], "full");
}

#[sqlx::test]
async fn android_origin_sign_in_works(pool: PgPool) {
    let app = TestApp::with_passkeys(pool).await;
    let (user, _) = create_test_user(&app.pool).await;
    let (mut auth, row) = seed_passkey(&app, user, PasskeyState::Active).await;
    let (s, b, _) = sign_in_with(
        &app.router,
        &app.pool,
        &mut auth,
        &row.credential_id,
        user,
        TEST_APK_ORIGIN,
        true,
        None,
        Some("android"),
    )
    .await;
    assert_eq!(s, 200, "{b}");
}

#[sqlx::test]
async fn sign_in_creates_a_new_session(pool: PgPool) {
    let app = TestApp::with_passkeys(pool).await;
    let (user, _) = create_test_user(&app.pool).await;
    let (mut auth, row) = seed_passkey(&app, user, PasskeyState::Active).await;
    let (probation, _) = create_probation_session_with_token(&app.pool, &app.config, user).await;
    let (s, b, _) = signed_in(&app, &mut auth, &row, None).await;
    assert_eq!(s, 200);
    assert_ne!(b["session_id"].as_str().unwrap(), probation.to_string());
    let (assurance, ended): (String, Option<chrono::DateTime<chrono::Utc>>) =
        sqlx::query_as("SELECT assurance, ended_at FROM sessions WHERE id = $1")
            .bind(probation)
            .fetch_one(&app.pool)
            .await
            .unwrap();
    assert_eq!(assurance, "probation");
    assert!(ended.is_none());
}

#[sqlx::test]
async fn unknown_credential_is_reported(pool: PgPool) {
    let app = TestApp::with_passkeys(pool).await;
    let (user, _) = create_test_user(&app.pool).await;
    // A credential the server never stored.
    let rp = test_passkey_rp();
    let mut auth = SoftAuthenticator::new();
    let handle = queries::user_handle(&app.pool, user).await.unwrap();
    let (ccr, st) = rp.start_registration(handle, "u", "U", vec![]).unwrap();
    let cred = auth.register(TEST_ORIGIN, &serde_json::to_value(&ccr).unwrap());
    let v = rp
        .finish_registration(&serde_json::from_value(cred).unwrap(), &st)
        .unwrap();
    let id = v.passkey.cred_id().as_ref().to_vec();
    let (s, b, _) = sign_in_with(
        &app.router,
        &app.pool,
        &mut auth,
        &id,
        user,
        TEST_ORIGIN,
        true,
        None,
        None,
    )
    .await;
    assert_eq!((s, b["error"].as_str()), (400, Some("PASSKEY_UNKNOWN")));
}

#[sqlx::test]
async fn revoked_passkey_cannot_sign_in(pool: PgPool) {
    let app = TestApp::with_passkeys(pool).await;
    let (user, _) = create_test_user(&app.pool).await;
    let (mut auth, row) = seed_passkey(&app, user, PasskeyState::Revoked).await;
    let (s, b, _) = signed_in(&app, &mut auth, &row, None).await;
    assert_eq!((s, b["error"].as_str()), (400, Some("PASSKEY_UNKNOWN")));
}

#[sqlx::test]
async fn pending_passkey_gets_ready_at(pool: PgPool) {
    let app = TestApp::with_passkeys(pool).await;
    let (user, _) = create_test_user(&app.pool).await;
    let (mut auth, row) = seed_passkey(&app, user, PasskeyState::Pending).await;
    let before = session_count(&app, user).await;
    let (s, b, _) = signed_in(&app, &mut auth, &row, None).await;
    assert_eq!(
        (s, b["error"].as_str()),
        (409, Some("PASSKEY_PENDING")),
        "{b}"
    );
    let ready: chrono::DateTime<chrono::Utc> = b["ready_at"].as_str().unwrap().parse().unwrap();
    assert_eq!(ready.timestamp(), row.ready_at.unwrap().timestamp());
    assert_eq!(session_count(&app, user).await, before);
}

#[sqlx::test]
async fn pending_passkey_with_bad_signature_is_invalid(pool: PgPool) {
    let app = TestApp::with_passkeys(pool).await;
    let (user, _) = create_test_user(&app.pool).await;
    let (mut auth, row) = seed_passkey(&app, user, PasskeyState::Pending).await;
    let start = call(&app.router, "/auth/passkeys/login/start", json!({}), None).await;
    let handle = queries::user_handle(&app.pool, user).await.unwrap();
    let mut cred = auth.assert(
        TEST_ORIGIN,
        &start.1["options"],
        &row.credential_id,
        Some(handle),
        true,
    );
    // A broken signature: the holder is not proven, so no ready time.
    let mut sig =
        vc_server::approvals::secrets::b64u_decode(cred["response"]["signature"].as_str().unwrap())
            .unwrap();
    let last = sig.len() - 1;
    sig[last] ^= 0x01;
    cred["response"]["signature"] = vc_server::approvals::secrets::b64u_encode(&sig).into();
    let (s, b, _) = call(
        &app.router,
        "/auth/passkeys/login/finish",
        json!({"ceremony_id": start.1["ceremony_id"], "credential": cred}),
        None,
    )
    .await;
    assert_eq!(
        (s, b["error"].as_str()),
        (400, Some("PASSKEY_INVALID")),
        "{b}"
    );
    assert!(b.get("ready_at").is_none());
}

#[sqlx::test]
async fn user_handle_of_other_account_is_refused(pool: PgPool) {
    let app = TestApp::with_passkeys(pool).await;
    let (user, _) = create_test_user(&app.pool).await;
    let (other, _) = create_test_user(&app.pool).await;
    let (mut auth, row) = seed_passkey(&app, user, PasskeyState::Active).await;
    let (s, b, _) = sign_in_with(
        &app.router,
        &app.pool,
        &mut auth,
        &row.credential_id,
        other,
        TEST_ORIGIN,
        true,
        None,
        None,
    )
    .await;
    assert_eq!((s, b["error"].as_str()), (400, Some("PASSKEY_UNKNOWN")));
    assert_eq!(
        session_count(&app, other).await,
        1,
        "only the fixture session"
    );
}

#[sqlx::test]
async fn username_hint_mismatch_is_refused(pool: PgPool) {
    let app = TestApp::with_passkeys(pool).await;
    let (grandma, _) = create_test_user(&app.pool).await;
    let (_, fabian) = create_test_user(&app.pool).await;
    let (mut auth, row) = seed_passkey(&app, grandma, PasskeyState::Active).await;
    let before = session_count(&app, grandma).await;
    let (s, b, _) = sign_in_with(
        &app.router,
        &app.pool,
        &mut auth,
        &row.credential_id,
        grandma,
        TEST_ORIGIN,
        true,
        Some(&fabian),
        None,
    )
    .await;
    assert_eq!(
        (s, b["error"].as_str()),
        (400, Some("PASSKEY_USER_MISMATCH"))
    );
    assert_eq!(session_count(&app, grandma).await, before);
}

#[sqlx::test]
async fn username_hint_is_case_insensitive(pool: PgPool) {
    let app = TestApp::with_passkeys(pool).await;
    let (user, username) = create_test_user(&app.pool).await;
    let (mut auth, row) = seed_passkey(&app, user, PasskeyState::Active).await;
    let hint = username.to_uppercase();
    let (s, b, _) = sign_in_with(
        &app.router,
        &app.pool,
        &mut auth,
        &row.credential_id,
        user,
        TEST_ORIGIN,
        true,
        Some(&hint),
        None,
    )
    .await;
    assert_eq!(s, 200, "{b}");
}

#[sqlx::test]
async fn banned_user_is_refused(pool: PgPool) {
    let app = TestApp::with_passkeys(pool).await;
    let (user, _) = create_test_user(&app.pool).await;
    let (mut auth, row) = seed_passkey(&app, user, PasskeyState::Active).await;
    sqlx::query("INSERT INTO global_bans (user_id, reason) VALUES ($1, 'x')")
        .bind(user)
        .execute(&app.pool)
        .await
        .unwrap();
    let (s, b, _) = signed_in(&app, &mut auth, &row, None).await;
    assert_eq!((s, b["error"].as_str()), (403, Some("ACCOUNT_BANNED")));
}

#[sqlx::test]
async fn uv_zero_is_refused(pool: PgPool) {
    let app = TestApp::with_passkeys(pool).await;
    let (user, _) = create_test_user(&app.pool).await;
    let (mut auth, row) = seed_passkey(&app, user, PasskeyState::Active).await;
    let (s, b, _) = sign_in_with(
        &app.router,
        &app.pool,
        &mut auth,
        &row.credential_id,
        user,
        TEST_ORIGIN,
        false,
        None,
        None,
    )
    .await;
    assert_eq!((s, b["error"].as_str()), (400, Some("PASSKEY_INVALID")));
}

#[sqlx::test]
async fn ceremony_is_single_use(pool: PgPool) {
    let app = TestApp::with_passkeys(pool).await;
    let (user, _) = create_test_user(&app.pool).await;
    let (mut auth, row) = seed_passkey(&app, user, PasskeyState::Active).await;
    let start = call(&app.router, "/auth/passkeys/login/start", json!({}), None).await;
    let handle = queries::user_handle(&app.pool, user).await.unwrap();
    let cred = auth.assert(
        TEST_ORIGIN,
        &start.1["options"],
        &row.credential_id,
        Some(handle),
        true,
    );
    let body = json!({"ceremony_id": start.1["ceremony_id"], "credential": cred});
    let (s, _, _) = call(
        &app.router,
        "/auth/passkeys/login/finish",
        body.clone(),
        None,
    )
    .await;
    assert_eq!(s, 200);
    let (s, b, _) = call(&app.router, "/auth/passkeys/login/finish", body, None).await;
    assert_eq!(
        (s, b["error"].as_str()),
        (400, Some("PASSKEY_CEREMONY_INVALID"))
    );
}

#[sqlx::test]
async fn failed_passkey_sign_ins_never_block_the_ip(pool: PgPool) {
    let app = TestApp::with_passkeys(pool).await;
    let limits = RateLimits {
        auth_passkey: LimitConfig {
            requests: 100,
            window_secs: 60,
        },
        ..RateLimits::default()
    };
    let (router, limiter) = limited_router_with(&app, limits).await;
    let (user, username) = create_test_user(&app.pool).await;
    let (mut auth, row) = seed_passkey(&app, user, PasskeyState::Active).await;
    for _ in 0..20 {
        let (s, _, _) = sign_in_with(
            &router,
            &app.pool,
            &mut auth,
            &row.credential_id,
            user,
            TEST_ORIGIN,
            false,
            None,
            None,
        )
        .await;
        assert_eq!(s, 400);
    }
    assert!(!limiter.is_blocked("203.0.113.9").await.unwrap());
    let (s, _, _) = call(
        &router,
        "/auth/login",
        json!({"username": username, "password": "wrong-password"}),
        None,
    )
    .await;
    assert_eq!(s, 401, "a normal answer, not an IP block");
}

#[sqlx::test]
async fn passkey_requests_have_their_own_limit(pool: PgPool) {
    let app = TestApp::with_passkeys(pool).await;
    let (router, _) = limited_router(&app).await;
    let (_, username) = create_test_user(&app.pool).await;
    for _ in 0..60 {
        let (s, _, _) = call(&router, "/auth/passkeys/login/start", json!({}), None).await;
        assert_eq!(s, 200);
    }
    let (s, b, _) = call(&router, "/auth/passkeys/login/start", json!({}), None).await;
    assert_eq!(
        (s, b["error"].as_str()),
        (429, Some("TOO_MANY_ATTEMPTS")),
        "{b}"
    );
    let (s, _, _) = call(
        &router,
        "/auth/login",
        json!({"username": username, "password": "wrong-password"}),
        None,
    )
    .await;
    assert_eq!(s, 401, "password sign-in is unaffected");
}

async fn stored_counter(app: &TestApp, id: Uuid) -> i64 {
    sqlx::query_scalar(
        "SELECT (credential #>> '{cred,counter}')::bigint FROM passkeys WHERE id = $1",
    )
    .bind(id)
    .fetch_one(&app.pool)
    .await
    .unwrap()
}

async fn set_counter(app: &TestApp, id: Uuid, n: i64) {
    sqlx::query("UPDATE passkeys SET credential = jsonb_set(credential, '{cred,counter}', to_jsonb($2::bigint)) WHERE id = $1")
        .bind(id)
        .bind(n)
        .execute(&app.pool)
        .await
        .unwrap();
}

#[sqlx::test]
async fn regressed_counter_is_a_signal_not_a_lockout(pool: PgPool) {
    // Spec B.1 (coordinator decision 2): a valid assertion with a counter
    // that went backwards signs in, is audited, and the stored counter stays.
    let app = TestApp::with_passkeys(pool).await;
    let (user, _) = create_test_user(&app.pool).await;
    let (mut auth, row) = seed_passkey(&app, user, PasskeyState::Active).await;
    set_counter(&app, row.id, 1000).await;
    let (s, b, _) = signed_in(&app, &mut auth, &row, None).await;
    assert_eq!(s, 200, "{b}");
    let audited: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM system_audit_log WHERE action = 'auth.passkey.counter_regression' AND target_id = $1",
    )
    .bind(row.id)
    .fetch_one(&app.pool)
    .await
    .unwrap();
    assert_eq!(audited, 1);
    assert_eq!(
        stored_counter(&app, row.id).await,
        1000,
        "the high-water mark stays"
    );
    // A broken signature is still refused.
    let start = call(&app.router, "/auth/passkeys/login/start", json!({}), None).await;
    let handle = queries::user_handle(&app.pool, user).await.unwrap();
    let mut cred = auth.assert(
        TEST_ORIGIN,
        &start.1["options"],
        &row.credential_id,
        Some(handle),
        true,
    );
    let mut sig =
        vc_server::approvals::secrets::b64u_decode(cred["response"]["signature"].as_str().unwrap())
            .unwrap();
    let last = sig.len() - 1;
    sig[last] ^= 0x01;
    cred["response"]["signature"] = vc_server::approvals::secrets::b64u_encode(&sig).into();
    let (s, b, _) = call(
        &app.router,
        "/auth/passkeys/login/finish",
        json!({"ceremony_id": start.1["ceremony_id"], "credential": cred}),
        None,
    )
    .await;
    assert_eq!((s, b["error"].as_str()), (400, Some("PASSKEY_INVALID")));
}

#[sqlx::test]
async fn stored_counter_never_decreases(pool: PgPool) {
    // Review Codex m3: two ceremonies completing in reversed order.
    let app = TestApp::with_passkeys(pool).await;
    let (user, _) = create_test_user(&app.pool).await;
    let (mut auth, row) = seed_passkey(&app, user, PasskeyState::Active).await;
    let handle = queries::user_handle(&app.pool, user).await.unwrap();
    let s1 = call(&app.router, "/auth/passkeys/login/start", json!({}), None)
        .await
        .1;
    let s2 = call(&app.router, "/auth/passkeys/login/start", json!({}), None)
        .await
        .1;
    let a1 = auth.assert(
        TEST_ORIGIN,
        &s1["options"],
        &row.credential_id,
        Some(handle),
        true,
    );
    let a2 = auth.assert(
        TEST_ORIGIN,
        &s2["options"],
        &row.credential_id,
        Some(handle),
        true,
    );
    let (s, _, _) = call(
        &app.router,
        "/auth/passkeys/login/finish",
        json!({"ceremony_id": s2["ceremony_id"], "credential": a2}),
        None,
    )
    .await;
    assert_eq!(s, 200);
    let high = stored_counter(&app, row.id).await;
    assert!(high >= 2, "{high}");
    let (s, _, _) = call(
        &app.router,
        "/auth/passkeys/login/finish",
        json!({"ceremony_id": s1["ceremony_id"], "credential": a1}),
        None,
    )
    .await;
    assert_eq!(s, 200, "the older assertion is a signal, not a refusal");
    assert_eq!(stored_counter(&app, row.id).await, high);
}

#[sqlx::test]
async fn the_ceremony_page_is_outside_the_passkey_limit(pool: PgPool) {
    // Review Fable m5: page loads and assets never push a hand-off into 429.
    let app = TestApp::with_passkeys(pool).await;
    let (router, _) = limited_router(&app).await;
    for path in [
        "/auth/passkeys/assets/ceremony.js",
        &format!("/auth/passkeys/desktop/{}", Uuid::new_v4()),
    ] {
        for _ in 0..70 {
            let req = peer_request(Method::GET, path).body(Body::empty()).unwrap();
            let resp = tower::ServiceExt::oneshot(router.clone(), req)
                .await
                .unwrap();
            assert_ne!(resp.status().as_u16(), 429, "{path}");
        }
    }
}
