//! OIDC sign-in hand-offs (C27 OIDC variant, C29 step 8): the desktop gets
//! the hand-off page (a form POST of a one-time code to its loopback), the
//! Android app `kaiku://auth/callback?code=…`, the browser a `LoginStep` by
//! `postMessage` with the attempt cookie; the code plus the verifier yields
//! a `LoginStep` at `POST /auth/oidc/desktop/exchange`. Never a session.
//!
//! Uses the mock provider of `oidc_mock` and follows the real redirects.
//!
//! Run tests: `cargo test --test integration oidc_handoff`

use std::collections::HashMap;

use fred::interfaces::KeysInterface;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use uuid::Uuid;
use vc_server::auth::mfa_crypto::{decrypt_mfa_secret, encrypt_mfa_secret};
use vc_server::auth::oidc::{handoff_key, OidcFlowState};

use super::oidc_mock::{
    callback_response, client, create_user, follow, form_fields, fresh, issue, link, query_of,
    setup, setup_with, Ctx, DESKTOP_RETURN,
};

/// RFC 7636 appendix B verifier and its S256 challenge.
const VERIFIER: &str = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
const CHALLENGE: &str = "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM";
/// Per-flow value the desktop puts in its link return address.
const FLOW: &str = "perflowvalue_0123456789abc";
/// The desktop listener's port and per-flow path.
const PORT: &str = "50123";
const CALLBACK_PATH: &str = "cGF0aHBhdGhwYXRocGF0aHBhdGhwYXRocGF0aHBhdGg";

fn desktop_return() -> String {
    format!("{DESKTOP_RETURN}?flow={FLOW}")
}

/// Start a desktop sign-in (C29 step 8); returns `(status, body)`.
async fn start_desktop_login(ctx: &Ctx, challenge: Option<&str>) -> (u16, Value) {
    let mut query = vec![("loopback_port", PORT), ("callback_path", CALLBACK_PATH)];
    if let Some(challenge) = challenge {
        query.push(("client_challenge", challenge));
    }
    let resp = client()
        .get(format!(
            "{}/auth/oidc/authorize/{}",
            ctx.server.url, ctx.slug
        ))
        .query(&query)
        .header("x-kaiku-client", "desktop")
        .send()
        .await
        .expect("authorize");
    let status = resp.status().as_u16();
    (status, resp.json().await.unwrap_or(Value::Null))
}

/// The provider URL and the flow id of a desktop start.
async fn provider_url(ctx: &Ctx) -> (String, String) {
    let (status, body) = start_desktop_login(ctx, Some(CHALLENGE)).await;
    assert_eq!(status, 200, "start desktop login: {body}");
    (
        body["url"].as_str().expect("url").to_string(),
        body["flow_id"].as_str().expect("flow_id").to_string(),
    )
}

/// The hand-off page's fields after the provider round trip.
async fn handoff(ctx: &Ctx, sub: &str) -> (String, HashMap<String, String>) {
    let (url, flow_id) = provider_url(ctx).await;
    issue(ctx, fresh(sub));
    let resp = callback_response(&url).await;
    assert_eq!(resp.status().as_u16(), 200);
    let html = resp.text().await.unwrap();
    (flow_id, form_fields(&html))
}

async fn exchange(ctx: &Ctx, code: &str, verifier: &str) -> (u16, Value) {
    let resp = client()
        .post(format!("{}/auth/oidc/desktop/exchange", ctx.server.url))
        .json(&json!({ "code": code, "verifier": verifier }))
        .send()
        .await
        .expect("exchange");
    let status = resp.status().as_u16();
    (status, resp.json().await.unwrap_or(Value::Null))
}

/// A password user with the identity `sub` linked and TOTP on (so the OIDC
/// first factor leads to `totp_required`).
async fn linked_user(ctx: &Ctx, sub: &str) -> Uuid {
    let user = create_user(&ctx.pool, true).await;
    link(ctx, user, sub).await;
    let name: String = sqlx::query_scalar("SELECT username FROM users WHERE id = $1")
        .bind(user)
        .fetch_one(&ctx.pool)
        .await
        .unwrap();
    super::helpers::login::enable_totp(&ctx.pool, &ctx.config, user, &name).await;
    user
}

async fn redis(ctx: &Ctx) -> fred::clients::Client {
    vc_server::db::create_redis_client(&ctx.config.redis_url)
        .await
        .expect("redis")
}

/// C29 step 8: the callback answers the desktop with the hand-off page (a
/// form POST to its loopback), never a redirect with the code.
#[sqlx::test]
async fn oidc_desktop_gets_the_handoff_page(pool: PgPool) {
    let ctx = setup(&pool).await;
    linked_user(&ctx, "sub-desktop").await;
    let (url, flow_id) = provider_url(&ctx).await;
    issue(&ctx, fresh("sub-desktop"));
    let resp = callback_response(&url).await;
    assert_eq!(resp.status().as_u16(), 200);
    let h = resp.headers().clone();
    assert!(h.get("location").is_none());
    assert_eq!(h["cache-control"], "no-store");
    assert_eq!(h["referrer-policy"], "no-referrer");
    let csp = h["content-security-policy"].to_str().unwrap().to_string();
    assert!(
        csp.contains(&format!(
            "form-action http://127.0.0.1:{PORT}/callback/{CALLBACK_PATH}"
        )),
        "{csp}"
    );
    let html = resp.text().await.unwrap();
    let fields = form_fields(&html);
    assert_eq!(fields["flow_id"], flow_id);
    let code = &fields["code"];
    assert!(code.len() >= 43, "code has at least 256 bits");
    let ttl: i64 = redis(&ctx).await.ttl(handoff_key(code)).await.expect("ttl");
    assert!((1..=60).contains(&ttl), "ttl {ttl}");
}

/// The code plus the verifier yields a `LoginStep` (never a session),
/// exactly once; the attempt is bound to the same verifier.
#[sqlx::test]
async fn oidc_exchange_returns_a_login_step(pool: PgPool) {
    let ctx = setup(&pool).await;
    let user = linked_user(&ctx, "sub-step").await;
    let (_, fields) = handoff(&ctx, "sub-step").await;
    let (status, step) = exchange(&ctx, &fields["code"], VERIFIER).await;
    assert_eq!(status, 202, "{step}");
    assert_eq!(step["status"], "totp_required");
    assert_eq!(step["attempt"]["kind"], "oidc");
    assert!(step.get("access_token").is_none());
    let sessions: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM sessions WHERE user_id = $1")
        .bind(user)
        .fetch_one(&ctx.pool)
        .await
        .unwrap();
    assert_eq!(sessions, 0);
    let id = step["attempt"]["attempt_id"].as_str().unwrap();
    let resp = client()
        .post(format!("{}/auth/login-attempts/{id}/poll", ctx.server.url))
        .header("x-kaiku-client", "desktop")
        .json(&json!({ "verifier": VERIFIER }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 200);
    let (status, body) = exchange(&ctx, &fields["code"], VERIFIER).await;
    assert_eq!(status, 400);
    assert_eq!(body["error"], "OIDC_HANDOFF_INVALID");
}

/// A provider error reaches the desktop on the hand-off page as `error`.
#[sqlx::test]
async fn test_desktop_error_hand_back_is_the_handoff_page(pool: PgPool) {
    let ctx = setup(&pool).await;
    let (url, flow_id) = provider_url(&ctx).await;
    let state = query_of(&url)["state"].clone();
    let resp = client()
        .get(format!(
            "{}/auth/oidc/callback?error=access_denied&state={state}",
            ctx.server.url
        ))
        .send()
        .await
        .expect("callback");
    assert_eq!(resp.status().as_u16(), 200);
    assert_eq!(resp.headers()["cache-control"], "no-store");
    let fields = form_fields(&resp.text().await.unwrap());
    assert_eq!(fields["flow_id"], flow_id);
    assert_eq!(fields["error"], "OIDC_PROVIDER_ERROR");
    assert!(!fields.contains_key("code"));
}

/// The attempt records the native app's address and user agent.
#[sqlx::test]
async fn test_exchange_records_ip_and_user_agent(pool: PgPool) {
    let ctx = setup(&pool).await;
    let user = linked_user(&ctx, "sub-ua").await;
    let (_, fields) = handoff(&ctx, "sub-ua").await;
    let resp = client()
        .post(format!("{}/auth/oidc/desktop/exchange", ctx.server.url))
        .header("user-agent", "Kaiku-Desktop-Test/1.0")
        .json(&json!({ "code": fields["code"], "verifier": VERIFIER }))
        .send()
        .await
        .expect("exchange");
    assert_eq!(resp.status().as_u16(), 202);
    let (ip, ua, class): (Option<String>, Option<String>, String) = sqlx::query_as(
        "SELECT host(ip), user_agent, client_class FROM login_attempts WHERE user_id = $1",
    )
    .bind(user)
    .fetch_one(&ctx.pool)
    .await
    .expect("attempt");
    assert_eq!(ip.as_deref(), Some("127.0.0.1"));
    assert_eq!(ua.as_deref(), Some("Kaiku-Desktop-Test/1.0"));
    assert_eq!(class, "desktop");
}

/// Without the verifier the code is worthless, and a wrong guess burns it.
#[sqlx::test]
async fn test_desktop_code_needs_the_matching_verifier(pool: PgPool) {
    let ctx = setup(&pool).await;
    linked_user(&ctx, "sub-verifier").await;
    let (_, fields) = handoff(&ctx, "sub-verifier").await;
    let code = &fields["code"];
    let (status, body) = exchange(&ctx, code, &"A".repeat(43)).await;
    assert_eq!(status, 400);
    assert_eq!(body["error"], "OIDC_HANDOFF_INVALID");
    let (status, _) = exchange(&ctx, code, CHALLENGE).await;
    assert_eq!(status, 400);
    let (status, body) = exchange(&ctx, code, VERIFIER).await;
    assert_eq!(status, 400, "code was consumed by the failed attempt");
    assert_eq!(body["error"], "OIDC_HANDOFF_INVALID");
}

/// An account without a second step gets the neutral answer on the
/// hand-off page (no code).
#[sqlx::test]
async fn test_no_second_step_is_neutral(pool: PgPool) {
    let ctx = setup(&pool).await;
    let user = create_user(&ctx.pool, true).await;
    link(&ctx, user, "sub-neutral").await;
    let (_, fields) = handoff(&ctx, "sub-neutral").await;
    assert_eq!(
        fields.get("error").map(String::as_str),
        Some("INVALID_CREDENTIALS")
    );
    assert!(!fields.contains_key("code"));
}

#[sqlx::test]
async fn test_unknown_code_is_refused(pool: PgPool) {
    let ctx = setup(&pool).await;
    let (status, body) = exchange(&ctx, "never-issued", VERIFIER).await;
    assert_eq!(status, 400);
    assert_eq!(body["error"], "OIDC_HANDOFF_INVALID");
}

/// An older desktop app (no `client_challenge`, or the pre-v1.0
/// `redirect_uri`) is refused at the start.
#[sqlx::test]
async fn test_desktop_login_without_challenge_is_outdated(pool: PgPool) {
    let ctx = setup(&pool).await;
    let (status, body) = start_desktop_login(&ctx, None).await;
    assert_eq!(status, 400, "{body}");
    assert_eq!(body["error"], "OIDC_CLIENT_OUTDATED");
    let resp = client()
        .get(format!(
            "{}/auth/oidc/authorize/{}",
            ctx.server.url, ctx.slug
        ))
        .query(&[
            ("redirect_uri", DESKTOP_RETURN),
            ("client_challenge", CHALLENGE),
        ])
        .header("x-kaiku-client", "desktop")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 400);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["error"], "OIDC_CLIENT_OUTDATED");
}

/// A pre-v1.0 desktop flow state (a loopback `redirect_uri`) hands back an
/// error code, never a code or tokens.
#[sqlx::test]
async fn test_legacy_desktop_state_hands_back_outdated(pool: PgPool) {
    let ctx = setup(&pool).await;
    linked_user(&ctx, "sub-legacy").await;
    let (url, _) = provider_url(&ctx).await;
    let key = format!(
        "oidc:state:{}",
        hex::encode(Sha256::digest(query_of(&url)["state"].as_bytes()))
    );
    let enc_key = hex::decode(ctx.config.mfa_encryption_key.as_ref().expect("key")).expect("hex");
    let redis = redis(&ctx).await;
    let stored: String = redis.get(&key).await.expect("stored state");
    let mut flow: OidcFlowState =
        serde_json::from_str(&decrypt_mfa_secret(&stored, &enc_key).expect("decrypt")).unwrap();
    flow.native = None;
    flow.client_redirect_uri = Some(DESKTOP_RETURN.to_string());
    let enc = encrypt_mfa_secret(&serde_json::to_string(&flow).unwrap(), &enc_key).unwrap();
    redis
        .set::<(), _, _>(&key, enc.as_str(), None, None, false)
        .await
        .unwrap();
    issue(&ctx, fresh("sub-legacy"));
    let chain = follow(&url).await;
    assert_eq!(
        chain.result.get("error").map(String::as_str),
        Some("OIDC_CLIENT_OUTDATED"),
        "{:?}",
        chain.result
    );
    assert!(!chain.result.contains_key("code"));
}

/// The browser popup gets a `LoginStep` by `postMessage` and the attempt
/// cookie, never tokens.
#[sqlx::test]
async fn oidc_callback_returns_a_login_step(pool: PgPool) {
    let ctx = setup(&pool).await;
    linked_user(&ctx, "sub-browser").await;
    let resp = client()
        .get(format!(
            "{}/auth/oidc/authorize/{}",
            ctx.server.url, ctx.slug
        ))
        .send()
        .await
        .expect("authorize");
    assert_eq!(resp.status().as_u16(), 307);
    let url = resp.headers()["location"].to_str().unwrap().to_string();
    issue(&ctx, fresh("sub-browser"));
    let resp = callback_response(&url).await;
    assert_eq!(resp.status().as_u16(), 200);
    let cookies: Vec<String> = resp
        .headers()
        .get_all("set-cookie")
        .iter()
        .map(|v| v.to_str().unwrap().to_string())
        .collect();
    assert!(
        cookies.iter().any(|c| c.contains("kaiku_attempt=")),
        "{cookies:?}"
    );
    assert!(
        !cookies.iter().any(|c| c.contains("kaiku_refresh=")),
        "{cookies:?}"
    );
    let html = resp.text().await.unwrap();
    assert!(html.contains("\"type\":\"oidc-callback\""), "{html}");
    assert!(html.contains("\"status\":\"totp_required\""), "{html}");
    assert!(html.contains("\"attempt\""), "{html}");
    assert!(!html.contains("access_token"), "{html}");
}

/// Android's Custom Tab gets only `kaiku://auth/callback?code=…`.
#[sqlx::test]
async fn android_oidc_gets_code_only(pool: PgPool) {
    let ctx = setup(&pool).await;
    linked_user(&ctx, "sub-android").await;
    let resp = client()
        .get(format!(
            "{}/auth/oidc/authorize/{}",
            ctx.server.url, ctx.slug
        ))
        .query(&[("client_challenge", CHALLENGE)])
        .header("x-kaiku-client", "android")
        .send()
        .await
        .expect("authorize");
    assert_eq!(resp.status().as_u16(), 200);
    let body: Value = resp.json().await.unwrap();
    issue(&ctx, fresh("sub-android"));
    let resp = callback_response(body["url"].as_str().unwrap()).await;
    assert!(resp.status().is_redirection());
    let location = resp.headers()["location"].to_str().unwrap().to_string();
    assert!(
        location.starts_with("kaiku://auth/callback?code="),
        "{location}"
    );
    let q = query_of(&location);
    assert_eq!(q.keys().collect::<Vec<_>>(), vec!["code"]);
    let resp = client()
        .post(format!("{}/auth/oidc/desktop/exchange", ctx.server.url))
        .header("x-kaiku-client", "android")
        .json(&json!({ "code": q["code"], "verifier": VERIFIER }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 202);
}

/// The identity-link hand-back is unchanged and echoes the per-flow value.
#[sqlx::test]
async fn test_desktop_link_echoes_the_flow_value(pool: PgPool) {
    let ctx = setup(&pool).await;
    let user = create_user(&ctx.pool, true).await;
    let (sid, token) =
        super::helpers::create_session_with_token(&ctx.pool, &ctx.config, user).await;
    super::helpers::grant_step_up(&ctx.pool, sid, "identity.link", "").await;

    let return_to = desktop_return();
    let resp = client()
        .get(format!(
            "{}/auth/me/identities/authorize/{}",
            ctx.server.url, ctx.slug
        ))
        .query(&[("redirect_uri", return_to.as_str()), ("response", "json")])
        .bearer_auth(&token)
        .send()
        .await
        .expect("start link");
    assert_eq!(resp.status().as_u16(), 200);
    let body: Value = resp.json().await.unwrap();

    issue(&ctx, fresh("sub-link-flow"));
    let chain = follow(body["url"].as_str().unwrap()).await;
    assert_eq!(
        chain.result.get("linked"),
        Some(&ctx.slug),
        "{:?}",
        chain.result
    );
    assert_eq!(chain.result.get("flow").map(String::as_str), Some(FLOW));
}

/// The exchange is rate limited like the other OIDC routes.
#[sqlx::test]
async fn test_exchange_is_rate_limited(pool: PgPool) {
    let ctx = setup_with(&pool, true).await;
    let mut limited = false;
    // AuthOther allows 40 requests per minute and IP.
    for _ in 0..60 {
        let (status, _) = exchange(&ctx, "never-issued", VERIFIER).await;
        if status == 429 {
            limited = true;
            break;
        }
        assert_eq!(status, 400);
    }
    assert!(limited, "exchange must be rate limited");
}
