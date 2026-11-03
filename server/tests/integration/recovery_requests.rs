//! Self-service recovery (F6) with recovery codes.
//!
//! Run with: `cargo test --test integration recovery_requests`

use serde_json::{json, Value};
use sqlx::PgPool;
use uuid::Uuid;

use super::account_requests::{admin_with_device, post, PASSWORD};
use super::enrollments::{claim_own, Phone};
use super::helpers::approvals::KeyDeviceFixture;
use super::helpers::login::{enable_totp, password_user};
use super::helpers::TestApp;

/// An account with a password, TOTP and fresh recovery codes, plus an admin
/// with a key phone as the canceller.
async fn account(app: &TestApp) -> (Uuid, String, totp_rs::TOTP, Vec<String>) {
    admin_with_device(app).await;
    let (user, name) = password_user(&app.pool, PASSWORD).await;
    let totp = enable_totp(&app.pool, &app.config, user, &name).await;
    let ctx = vc_server::auth::ctx::AuthCtx::from(&app.state);
    let mut tx = app.pool.begin().await.unwrap();
    let codes = vc_server::auth::recovery_codes::replace_tx(&mut tx, &ctx, user)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    (user, name, totp, codes)
}

async fn start(app: &TestApp, name: &str, code: &str, second: Value, lost: &str) -> (u16, Value) {
    post(
        app,
        "/auth/account-requests",
        None,
        &[("X-Kaiku-Client", "android")],
        json!({"username": name, "purpose": "recovery", "recovery_code": code,
               "second": second, "lost": lost}),
    )
    .await
}

async fn codes_left(pool: &PgPool, user: Uuid) -> i64 {
    let mut conn = pool.acquire().await.unwrap();
    vc_server::auth::recovery_codes::left_conn(&mut conn, user)
        .await
        .unwrap()
}

async fn forget_totp_step(app: &TestApp, user: Uuid) {
    use fred::interfaces::KeysInterface;
    let _: Result<(), _> = app
        .state
        .redis
        .del::<(), _>(format!("mfa:totp_step:{user}"))
        .await;
}

#[sqlx::test]
async fn recovery_code_is_checked_first(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, name, _, _) = account(&app).await;
    let (s1, b1) = start(
        &app,
        &name,
        "AAAAA-AAAAA-AAA",
        json!({"type": "password", "password": PASSWORD}),
        "none",
    )
    .await;
    let (s2, b2) = start(
        &app,
        "no_such_user_x",
        "AAAAA-AAAAA-AAA",
        json!({"type": "password", "password": PASSWORD}),
        "none",
    )
    .await;
    assert_eq!(
        (s1, &b1["error"]),
        (401, &json!("INVALID_CREDENTIALS")),
        "{b1}"
    );
    assert_eq!((s1, b1), (s2, b2), "the same answer as an unknown user");
    assert_eq!(codes_left(&pool, user).await, 10);
    let starts: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM account_request_starts WHERE user_id = $1")
            .bind(user)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(starts, 0, "nothing counted per account");
}

#[sqlx::test]
async fn valid_code_is_consumed_when_the_second_factor_fails(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, name, _, codes) = account(&app).await;
    let (s, b) = start(
        &app,
        &name,
        &codes[0],
        json!({"type": "password", "password": "not-the-password"}),
        "none",
    )
    .await;
    assert_eq!(
        (s, b["error"].as_str(), b["recovery_codes_left"].as_i64()),
        (401, Some("REQUEST_FACTOR_INVALID"), Some(9)),
        "{b}"
    );
    assert_eq!(codes_left(&pool, user).await, 9);
}

#[sqlx::test]
async fn totp_counts_in_any_state(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, name, totp, codes) = account(&app).await;
    sqlx::query("UPDATE users SET totp_state = 'throttled' WHERE id = $1")
        .bind(user)
        .execute(&pool)
        .await
        .unwrap();
    forget_totp_step(&app, user).await;
    let (s, b) = start(
        &app,
        &name,
        &codes[0],
        json!({"type": "totp", "code": totp.generate_current().unwrap()}),
        "none",
    )
    .await;
    assert_eq!(s, 201, "{b}");
    assert_eq!(b["request"]["purpose"], "recovery");
    assert!(b["request_secret"].as_str().is_some());
}

#[sqlx::test]
async fn password_must_be_active(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, name, _, codes) = account(&app).await;
    sqlx::query("UPDATE users SET password_state = 'compromised' WHERE id = $1")
        .bind(user)
        .execute(&pool)
        .await
        .unwrap();
    let (s, b) = start(
        &app,
        &name,
        &codes[0],
        json!({"type": "password", "password": PASSWORD}),
        "none",
    )
    .await;
    assert_eq!(
        (s, b["error"].as_str()),
        (401, Some("REQUEST_FACTOR_INVALID")),
        "{b}"
    );
}

#[sqlx::test]
async fn start_budget_is_three_per_delay_period(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, name, _, codes) = account(&app).await;
    for code in &codes[..3] {
        let (s, _) = start(
            &app,
            &name,
            code,
            json!({"type": "password", "password": "wrong-one"}),
            "none",
        )
        .await;
        assert_eq!(s, 401);
    }
    assert_eq!(codes_left(&pool, user).await, 7);
    // The 4th: refused before any code is consumed.
    let (s, b) = start(
        &app,
        &name,
        &codes[3],
        json!({"type": "password", "password": PASSWORD}),
        "none",
    )
    .await;
    assert_eq!(
        (s, b["error"].as_str()),
        (401, Some("INVALID_CREDENTIALS")),
        "{b}"
    );
    assert_eq!(codes_left(&pool, user).await, 7);
}

#[sqlx::test]
async fn all_key_devices_are_named_lost(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, name, _, codes) = account(&app).await;
    let a = KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;
    let b = KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;
    let (s, body) = start(
        &app,
        &name,
        &codes[0],
        json!({"type": "password", "password": PASSWORD}),
        "all_key_devices",
    )
    .await;
    assert_eq!(s, 201, "{body}");
    assert_eq!(body["request"]["lost_key_devices"], 2);
    let lost: Vec<Uuid> = sqlx::query_scalar(
        "SELECT unnest(lost_device_ids) FROM account_requests WHERE user_id = $1",
    )
    .bind(user)
    .fetch_all(&pool)
    .await
    .unwrap();
    assert!(lost.contains(&a.row_id) && lost.contains(&b.row_id));
}

#[sqlx::test]
async fn works_while_the_password_path_is_closed(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, name, totp, codes) = account(&app).await;
    sqlx::query("UPDATE users SET password_state = 'throttled' WHERE id = $1")
        .bind(user)
        .execute(&pool)
        .await
        .unwrap();
    forget_totp_step(&app, user).await;
    let (s, b) = start(
        &app,
        &name,
        &codes[0],
        json!({"type": "totp", "code": totp.generate_current().unwrap()}),
        "none",
    )
    .await;
    assert_eq!(s, 201, "{b}");
}

#[sqlx::test]
async fn full_recovery_to_a_new_phone(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, name, _, codes) = account(&app).await;
    let (s, b) = start(
        &app,
        &name,
        &codes[0],
        json!({"type": "password", "password": PASSWORD}),
        "none",
    )
    .await;
    assert_eq!(s, 201, "{b}");
    let secret = b["request_secret"].as_str().unwrap().to_string();
    let id: Uuid = b["request"]["id"].as_str().unwrap().parse().unwrap();
    sqlx::query("UPDATE account_requests SET ready_at = NOW() - INTERVAL '1 second' WHERE id = $1")
        .bind(id)
        .execute(&pool)
        .await
        .unwrap();
    vc_server::auth::timers::run_once(&app.state).await.unwrap();
    let hdr = [("X-Kaiku-Request-Secret", secret.as_str())];
    let (s, issued) = post(
        &app,
        "/auth/account-requests/current/enrollments",
        None,
        &hdr,
        json!({}),
    )
    .await;
    assert_eq!(s, 201, "{issued}");
    let phone = Phone::new();
    let (s, own) = claim_own(
        &app,
        issued["setup_token"].as_str().unwrap(),
        &phone,
        ("X-Kaiku-Request-Secret", &secret),
    )
    .await;
    assert_eq!(
        (s, own["authenticator_state"].as_str()),
        (201, Some("pending")),
        "{own}"
    );
    let (s, done) = post(
        &app,
        "/auth/account-requests/current/finish",
        None,
        &hdr,
        json!({}),
    )
    .await;
    assert_eq!(s, 200, "{done}");
    assert_eq!(done["recovery_codes"].as_array().unwrap().len(), 10);
    // The new phone signs in.
    let (s, c) = post(
        &app,
        "/auth/device-login/challenge",
        None,
        &[],
        json!({"user_id": user, "device_id": phone.device_id}),
    )
    .await;
    assert_eq!(s, 201, "{c}");
}

// ---------------------------------------------------------------------------
// A fresh OIDC sign-in as the second factor (C27)
// ---------------------------------------------------------------------------

mod oidc_factor {
    use serde_json::{json, Value};
    use sqlx::PgPool;
    use uuid::Uuid;

    use super::super::helpers::approvals::KeyDeviceFixture;
    use super::super::helpers::{create_test_user, make_admin};
    use super::super::oidc_mock::{
        callback_response, client, create_user, form_fields, fresh, issue, link, query_of, setup,
        Ctx, NextToken,
    };

    /// An OIDC-linked account with one recovery code, and an admin canceller.
    async fn account(ctx: &Ctx, sub: &str) -> (Uuid, String, Vec<String>) {
        let (admin, _) = create_test_user(&ctx.pool).await;
        make_admin(&ctx.pool, admin).await;
        KeyDeviceFixture::insert(&ctx.pool, &ctx.config, admin, "active").await;
        let user = create_user(&ctx.pool, true).await;
        link(ctx, user, sub).await;
        let (shown, hashes) = vc_server::auth::recovery_codes::generate();
        for h in &hashes[..3] {
            sqlx::query(
                "INSERT INTO recovery_codes (id, user_id, generation, code_hash)
                 SELECT gen_random_uuid(), $1, recovery_codes_generation, $2 FROM users WHERE id = $1",
            )
            .bind(user)
            .bind(h)
            .execute(&ctx.pool)
            .await
            .unwrap();
        }
        let name: String = sqlx::query_scalar("SELECT username FROM users WHERE id = $1")
            .bind(user)
            .fetch_one(&ctx.pool)
            .await
            .unwrap();
        (user, name, shown[..3].to_vec())
    }

    async fn authorize(ctx: &Ctx, desktop: Option<Value>) -> Value {
        let mut req = client().post(format!(
            "{}/auth/account-requests/oidc/authorize/{}",
            ctx.server.url, ctx.slug
        ));
        if let Some(body) = desktop {
            req = req.header("x-kaiku-client", "desktop").json(&body);
        }
        let resp = req.send().await.expect("authorize");
        assert_eq!(resp.status().as_u16(), 200);
        resp.json().await.expect("json")
    }

    /// The `oidc_code` the browser popup page posts to its opener.
    fn posted_code(html: &str) -> String {
        let v: Value = serde_json::from_str(
            html.split("postMessage(")
                .nth(1)
                .and_then(|r| r.split(", window.location.origin").next())
                .expect("postMessage payload"),
        )
        .expect("payload json");
        assert_eq!(v["type"], "oidc-request-callback", "{v}");
        v["oidc_code"].as_str().expect("oidc_code").to_string()
    }

    async fn start(ctx: &Ctx, name: &str, code: &str, second: Value) -> (u16, Value) {
        let resp = client()
            .post(format!("{}/auth/account-requests", ctx.server.url))
            .header("x-kaiku-client", "android")
            .json(
                &json!({"username": name, "purpose": "recovery", "recovery_code": code,
                          "second": second, "lost": "none"}),
            )
            .send()
            .await
            .expect("start");
        let status = resp.status().as_u16();
        (status, resp.json().await.unwrap_or(Value::Null))
    }

    #[sqlx::test]
    async fn fresh_oidc_is_a_factor(pool: PgPool) {
        let ctx = setup(&pool).await;
        let (_, name, codes) = account(&ctx, "sub-fresh").await;
        let code = codes[0].clone();
        let answer = authorize(&ctx, None).await;
        let url = answer["url"].as_str().unwrap();
        let params = query_of(url);
        assert_eq!(params.get("prompt").map(String::as_str), Some("login"));
        assert_eq!(params.get("max_age").map(String::as_str), Some("0"));
        issue(&ctx, fresh("sub-fresh"));
        let html = callback_response(url).await.text().await.unwrap();
        let oidc_code = posted_code(&html);
        let (s, b) = start(
            &ctx,
            &name,
            &code,
            json!({"type": "oidc", "oidc_code": oidc_code}),
        )
        .await;
        assert_eq!(s, 201, "{b}");
        assert!(b["request"]["factors"]
            .as_array()
            .unwrap()
            .contains(&json!("oidc")));
    }

    #[sqlx::test]
    async fn stale_oidc_is_refused(pool: PgPool) {
        let ctx = setup(&pool).await;
        let (_, name, codes) = account(&ctx, "sub-stale").await;
        let code = codes[0].clone();
        let answer = authorize(&ctx, None).await;
        let url = answer["url"].as_str().unwrap();
        let mut stale = fresh("sub-stale");
        stale.auth_time = Some(chrono::Utc::now().timestamp() - 600);
        issue(&ctx, stale);
        let html = callback_response(url).await.text().await.unwrap();
        // A stale sign-in yields no code, or one that is refused.
        let second = html
            .contains("\"oidc_code\"")
            .then(|| posted_code(&html))
            .unwrap_or_else(|| "nothing".into());
        let (s, b) = start(
            &ctx,
            &name,
            &code,
            json!({"type": "oidc", "oidc_code": second}),
        )
        .await;
        assert_eq!(
            (s, b["error"].as_str()),
            (401, Some("REQUEST_FACTOR_INVALID")),
            "{b}"
        );
        let _ = NextToken::default;
    }

    #[sqlx::test]
    async fn desktop_gets_url_and_flow_id_and_needs_the_verifier(pool: PgPool) {
        let ctx = setup(&pool).await;
        let (_, name, codes) = account(&ctx, "sub-desk").await;
        let verifier = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
        let challenge = "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM";
        let answer = authorize(
            &ctx,
            Some(json!({"loopback_port": 50123,
                        "callback_path": "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
                        "client_challenge": challenge})),
        )
        .await;
        let flow_id = answer["flow_id"].as_str().expect("flow_id").to_string();
        issue(&ctx, fresh("sub-desk"));
        let html = callback_response(answer["url"].as_str().unwrap())
            .await
            .text()
            .await
            .unwrap();
        let fields = form_fields(&html);
        assert_eq!(fields.get("flow_id"), Some(&flow_id));
        let oidc_code = fields.get("code").expect("code").clone();
        // Without the verifier: refused (the code is spent).
        let (s, b) = start(
            &ctx,
            &name,
            &codes[0],
            json!({"type": "oidc", "oidc_code": oidc_code}),
        )
        .await;
        assert_eq!(
            (s, b["error"].as_str()),
            (401, Some("REQUEST_FACTOR_INVALID")),
            "{b}"
        );
        // A new flow with the verifier.
        let answer = authorize(
            &ctx,
            Some(json!({"loopback_port": 50123,
                        "callback_path": "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
                        "client_challenge": challenge})),
        )
        .await;
        issue(&ctx, fresh("sub-desk"));
        let html = callback_response(answer["url"].as_str().unwrap())
            .await
            .text()
            .await
            .unwrap();
        let oidc_code = form_fields(&html).get("code").expect("code").clone();
        let (s, b) = start(
            &ctx,
            &name,
            &codes[1],
            json!({"type": "oidc", "oidc_code": oidc_code, "verifier": verifier}),
        )
        .await;
        assert_eq!(s, 201, "{b}");
    }

    async fn web_code(ctx: &Ctx, sub: &str) -> String {
        let answer = authorize(ctx, None).await;
        issue(ctx, fresh(sub));
        let html = callback_response(answer["url"].as_str().unwrap())
            .await
            .text()
            .await
            .unwrap();
        posted_code(&html)
    }

    #[sqlx::test]
    async fn an_identity_of_another_account_is_refused(pool: PgPool) {
        let ctx = setup(&pool).await;
        let (_, name, codes) = account(&ctx, "sub-own").await;
        // Someone else's identity: the code redeems, but not to this account.
        let other = create_user(&ctx.pool, true).await;
        link(&ctx, other, "sub-other").await;
        let oidc_code = web_code(&ctx, "sub-other").await;
        let (s, b) = start(
            &ctx,
            &name,
            &codes[0],
            json!({"type": "oidc", "oidc_code": oidc_code}),
        )
        .await;
        assert_eq!(
            (s, b["error"].as_str()),
            (401, Some("REQUEST_FACTOR_INVALID")),
            "{b}"
        );
        let open: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM account_requests WHERE state IN ('waiting', 'ready')",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(open, 0, "nothing started");
    }

    #[sqlx::test]
    async fn a_code_redeems_once(pool: PgPool) {
        let ctx = setup(&pool).await;
        let (_, name, codes) = account(&ctx, "sub-once").await;
        let oidc_code = web_code(&ctx, "sub-once").await;
        let (s, b) = start(
            &ctx,
            &name,
            &codes[0],
            json!({"type": "oidc", "oidc_code": oidc_code}),
        )
        .await;
        assert_eq!(s, 201, "{b}");
        // Withdraw, then the same code again: spent.
        let secret = b["request_secret"].as_str().unwrap().to_string();
        let resp = client()
            .delete(format!("{}/auth/account-requests/current", ctx.server.url))
            .header("X-Kaiku-Request-Secret", &secret)
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status().as_u16(), 204);
        let (s, b) = start(
            &ctx,
            &name,
            &codes[1],
            json!({"type": "oidc", "oidc_code": oidc_code}),
        )
        .await;
        assert_eq!(
            (s, b["error"].as_str()),
            (401, Some("REQUEST_FACTOR_INVALID")),
            "{b}"
        );
    }

    #[sqlx::test]
    async fn bootstrap_with_oidc_as_the_first_factor(pool: PgPool) {
        let ctx = setup(&pool).await;
        let (user, name, _) = account(&ctx, "sub-boot").await;
        let totp =
            super::super::helpers::login::enable_totp(&ctx.pool, &ctx.config, user, &name).await;
        sqlx::query("UPDATE users SET bootstrap_open = true WHERE id = $1")
            .bind(user)
            .execute(&ctx.pool)
            .await
            .unwrap();
        let oidc_code = web_code(&ctx, "sub-boot").await;
        let resp = client()
            .post(format!("{}/auth/account-requests", ctx.server.url))
            .header("x-kaiku-client", "android")
            .json(&json!({"username": name, "purpose": "bootstrap",
                          "first": {"type": "oidc", "oidc_code": oidc_code},
                          "totp_code": totp.generate_current().unwrap()}))
            .send()
            .await
            .unwrap();
        let status = resp.status().as_u16();
        let b: Value = resp.json().await.unwrap_or(Value::Null);
        assert_eq!(status, 201, "{b}");
        assert!(
            b["request"]["factors"]
                .as_array()
                .unwrap()
                .contains(&json!("oidc")),
            "{b}"
        );
    }

    #[sqlx::test]
    async fn a_desktop_provider_error_reaches_the_app(pool: PgPool) {
        let ctx = setup(&pool).await;
        let answer = authorize(
            &ctx,
            Some(json!({"loopback_port": 50123,
                        "callback_path": "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
                        "client_challenge": "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"})),
        )
        .await;
        let state = query_of(answer["url"].as_str().unwrap())
            .get("state")
            .cloned()
            .unwrap();
        let resp = client()
            .get(format!("{}/auth/oidc/callback", ctx.server.url))
            .query(&[("state", state.as_str()), ("error", "access_denied")])
            .send()
            .await
            .unwrap();
        let html = resp.text().await.unwrap();
        let fields = form_fields(&html);
        assert_eq!(
            fields.get("flow_id").map(String::as_str),
            answer["flow_id"].as_str()
        );
        assert!(fields.contains_key("error"), "{html}");
    }

    #[sqlx::test]
    async fn a_code_whose_sign_in_aged_is_refused(pool: PgPool) {
        use fred::interfaces::KeysInterface;
        let ctx = setup(&pool).await;
        let (user, name, codes) = account(&ctx, "sub-aged").await;
        // A grant whose sign-in is now 301 s old (redeemed late).
        let grant = vc_server::auth::account_requests::recovery::RequestOidcGrant {
            user_id: Some(user),
            slug: ctx.slug.clone(),
            auth_time: Some(chrono::Utc::now() - chrono::Duration::seconds(301)),
            client_challenge: None,
        };
        let code = "aged-code-for-the-test-0000000000000000000";
        let redis = fred::clients::Client::new(
            fred::types::config::Config::from_url(&ctx.config.redis_url).unwrap(),
            None,
            None,
            None,
        );
        {
            use fred::interfaces::ClientLike;
            redis.connect();
            redis.wait_for_connect().await.unwrap();
        }
        let _: () = redis
            .set(
                vc_server::auth::account_requests::recovery::request_code_key(code),
                serde_json::to_string(&grant).unwrap(),
                Some(fred::types::Expiration::EX(60)),
                None,
                false,
            )
            .await
            .unwrap();
        let (s, b) = start(
            &ctx,
            &name,
            &codes[0],
            json!({"type": "oidc", "oidc_code": code}),
        )
        .await;
        assert_eq!(
            (s, b["error"].as_str()),
            (401, Some("REQUEST_FACTOR_INVALID")),
            "{b}"
        );
    }

    /// Codex r3: an OIDC first factor that turns stale while the start
    /// waits for the account lock is refused.
    #[sqlx::test]
    async fn bootstrap_oidc_freshness_is_rechecked_after_the_lock_wait(pool: PgPool) {
        use fred::interfaces::{ClientLike, KeysInterface};
        let ctx = setup(&pool).await;
        let (user, name, _) = account(&ctx, "sub-wait").await;
        let totp =
            super::super::helpers::login::enable_totp(&ctx.pool, &ctx.config, user, &name).await;
        sqlx::query("UPDATE users SET bootstrap_open = true WHERE id = $1")
            .bind(user)
            .execute(&ctx.pool)
            .await
            .unwrap();
        let grant = vc_server::auth::account_requests::recovery::RequestOidcGrant {
            user_id: Some(user),
            slug: ctx.slug.clone(),
            auth_time: Some(chrono::Utc::now() - chrono::Duration::seconds(298)),
            client_challenge: None,
        };
        let code = "wait-code-for-the-test-00000000000000000000";
        let redis = fred::clients::Client::new(
            fred::types::config::Config::from_url(&ctx.config.redis_url).unwrap(),
            None,
            None,
            None,
        );
        redis.connect();
        redis.wait_for_connect().await.unwrap();
        let _: () = redis
            .set(
                vc_server::auth::account_requests::recovery::request_code_key(code),
                serde_json::to_string(&grant).unwrap(),
                Some(fred::types::Expiration::EX(60)),
                None,
                false,
            )
            .await
            .unwrap();
        let mut hold = pool.begin().await.unwrap();
        vc_server::approvals::transition::lock_accounts(&mut hold, &[user])
            .await
            .unwrap();
        let start = async {
            client()
                .post(format!("{}/auth/account-requests", ctx.server.url))
                .header("x-kaiku-client", "android")
                .json(&json!({"username": name, "purpose": "bootstrap",
                              "first": {"type": "oidc", "oidc_code": code},
                              "totp_code": totp.generate_current().unwrap()}))
                .send()
                .await
                .unwrap()
                .status()
                .as_u16()
        };
        let release = async {
            super::super::helpers::wait_for_lock_waiters(&pool, 1).await;
            tokio::time::sleep(std::time::Duration::from_secs(3)).await;
            hold.commit().await.unwrap();
        };
        let (status, ()) = tokio::join!(start, release);
        assert_eq!(status, 401, "stale after the wait");
    }
}
