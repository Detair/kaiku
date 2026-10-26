//! Desktop SSO hand-off: the server hands the desktop loopback a one-time
//! code (never tokens) and the desktop redeems it with its verifier at
//! `POST /auth/oidc/desktop/exchange`.
//!
//! Uses the mock provider of `reauth_oidc` and follows the real redirect
//! chain like a browser would.
//!
//! Run tests: `cargo test --test integration oidc_handoff`

use fred::interfaces::KeysInterface;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use uuid::Uuid;
use vc_server::auth::mfa_crypto::{decrypt_mfa_secret, encrypt_mfa_secret};
use vc_server::auth::oidc::{handoff_key, OidcFlowState};

use super::reauth_oidc::{
    client, create_user, follow, fresh, issue, link, query_of, setup, setup_with, Ctx,
    DESKTOP_RETURN,
};

/// RFC 7636 appendix B verifier and its S256 challenge.
const VERIFIER: &str = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
const CHALLENGE: &str = "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM";
/// Per-flow value the desktop puts in its return address.
const FLOW: &str = "perflowvalue_0123456789abc";

fn desktop_return() -> String {
    format!("{DESKTOP_RETURN}?flow={FLOW}")
}

/// Start a desktop login (`response=json`); returns `(status, body)`.
async fn start_desktop_login(ctx: &Ctx, challenge: Option<&str>) -> (u16, Value) {
    let return_to = desktop_return();
    let mut query = vec![("redirect_uri", return_to.as_str()), ("response", "json")];
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

async fn provider_url(ctx: &Ctx) -> String {
    let (status, body) = start_desktop_login(ctx, Some(CHALLENGE)).await;
    assert_eq!(status, 200, "start desktop login: {body}");
    body["url"].as_str().expect("url").to_string()
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

/// A password user with the identity `sub` linked.
async fn linked_user(ctx: &Ctx, sub: &str) -> Uuid {
    let user = create_user(&ctx.pool, true).await;
    link(ctx, user, sub).await;
    user
}

async fn redis(ctx: &Ctx) -> fred::clients::Client {
    vc_server::db::create_redis_client(&ctx.config.redis_url)
        .await
        .expect("redis")
}

/// The loopback gets only the per-flow value and a short-lived code; the code
/// plus the verifier yields the same token response as a login, exactly once.
#[sqlx::test]
async fn test_desktop_login_hands_back_only_a_code(pool: PgPool) {
    let ctx = setup(&pool).await;
    let user = linked_user(&ctx, "sub-desktop").await;

    let url = provider_url(&ctx).await;
    issue(&ctx, fresh("sub-desktop"));
    let chain = follow(&url).await;

    let mut keys: Vec<&str> = chain.result.keys().map(String::as_str).collect();
    keys.sort_unstable();
    assert_eq!(keys, ["code", "flow"], "{:?}", chain.result);
    assert_eq!(
        chain.result["flow"], FLOW,
        "per-flow value echoed unchanged"
    );
    let code = chain.result["code"].clone();
    assert!(code.len() >= 43, "code has at least 256 bits");

    // Single use, at most 60 s.
    let ttl: i64 = redis(&ctx)
        .await
        .ttl(handoff_key(&code))
        .await
        .expect("ttl");
    assert!((1..=60).contains(&ttl), "ttl {ttl}");

    let (status, tokens) = exchange(&ctx, &code, VERIFIER).await;
    assert_eq!(status, 200, "{tokens}");
    let access = tokens["access_token"].as_str().expect("access_token");
    assert!(tokens["refresh_token"]
        .as_str()
        .is_some_and(|t| !t.is_empty()));
    assert_eq!(tokens["token_type"], "Bearer");
    assert!(tokens["expires_in"].as_i64().is_some());
    assert!(tokens["setup_required"].is_boolean());
    assert!(tokens["password_change_required"].is_boolean());

    let me: Value = client()
        .get(format!("{}/auth/me", ctx.server.url))
        .bearer_auth(access)
        .send()
        .await
        .expect("me")
        .json()
        .await
        .expect("json");
    assert_eq!(me["id"], user.to_string());

    let (status, body) = exchange(&ctx, &code, VERIFIER).await;
    assert_eq!(status, 400);
    assert_eq!(body["error"], "OIDC_HANDOFF_INVALID");
}

/// The hand-back redirect to the loopback (it carries the code) is never
/// cached and never sent on as a referrer.
#[sqlx::test]
async fn test_desktop_hand_back_is_no_store_no_referrer(pool: PgPool) {
    let ctx = setup(&pool).await;
    linked_user(&ctx, "sub-headers").await;
    let url = provider_url(&ctx).await;
    issue(&ctx, fresh("sub-headers"));

    let resp = client().get(&url).send().await.expect("provider");
    let callback = resp.headers()["location"].to_str().unwrap().to_string();
    let resp = client().get(&callback).send().await.expect("callback");
    assert_eq!(resp.status().as_u16(), 307);
    let h = resp.headers();
    assert!(h["location"].to_str().unwrap().starts_with(DESKTOP_RETURN));
    assert_eq!(h["cache-control"], "no-store");
    assert_eq!(h["referrer-policy"], "no-referrer");
}

/// Error hand-backs to the loopback carry the same headers.
#[sqlx::test]
async fn test_desktop_error_hand_back_is_no_store_no_referrer(pool: PgPool) {
    let ctx = setup(&pool).await;
    let url = provider_url(&ctx).await;
    let state = query_of(&url)["state"].clone();
    let resp = client()
        .get(format!(
            "{}/auth/oidc/callback?error=access_denied&state={state}",
            ctx.server.url
        ))
        .send()
        .await
        .expect("callback");
    assert_eq!(resp.status().as_u16(), 307);
    let h = resp.headers();
    assert!(h["location"]
        .to_str()
        .unwrap()
        .contains("error=OIDC_PROVIDER_ERROR"));
    assert_eq!(h["cache-control"], "no-store");
    assert_eq!(h["referrer-policy"], "no-referrer");
}

/// The session created at redemption records the desktop's address and
/// user agent.
#[sqlx::test]
async fn test_exchange_records_ip_and_user_agent(pool: PgPool) {
    let ctx = setup(&pool).await;
    let user = linked_user(&ctx, "sub-ua").await;
    let url = provider_url(&ctx).await;
    issue(&ctx, fresh("sub-ua"));
    let code = follow(&url).await.result["code"].clone();
    let resp = client()
        .post(format!("{}/auth/oidc/desktop/exchange", ctx.server.url))
        .header("user-agent", "Kaiku-Desktop-Test/1.0")
        .json(&json!({ "code": code, "verifier": VERIFIER }))
        .send()
        .await
        .expect("exchange");
    assert_eq!(resp.status().as_u16(), 200);
    let (ip, ua): (Option<String>, Option<String>) =
        sqlx::query_as("SELECT host(ip_address), user_agent FROM sessions WHERE user_id = $1")
            .bind(user)
            .fetch_one(&ctx.pool)
            .await
            .expect("session");
    assert_eq!(ip.as_deref(), Some("127.0.0.1"));
    assert_eq!(ua.as_deref(), Some("Kaiku-Desktop-Test/1.0"));
}

/// Without the verifier the code is worthless, and a wrong guess burns it.
#[sqlx::test]
async fn test_desktop_code_needs_the_matching_verifier(pool: PgPool) {
    let ctx = setup(&pool).await;
    linked_user(&ctx, "sub-verifier").await;

    let url = provider_url(&ctx).await;
    issue(&ctx, fresh("sub-verifier"));
    let code = follow(&url).await.result["code"].clone();

    let (status, body) = exchange(&ctx, &code, &"A".repeat(43)).await;
    assert_eq!(status, 400);
    assert_eq!(body["error"], "OIDC_HANDOFF_INVALID");
    // The challenge itself is not a verifier ("plain" is not accepted).
    let (status, _) = exchange(&ctx, &code, CHALLENGE).await;
    assert_eq!(status, 400);
    let (status, body) = exchange(&ctx, &code, VERIFIER).await;
    assert_eq!(status, 400, "code was consumed by the failed attempt");
    assert_eq!(body["error"], "OIDC_HANDOFF_INVALID");
}

#[sqlx::test]
async fn test_unknown_code_is_refused(pool: PgPool) {
    let ctx = setup(&pool).await;
    let (status, body) = exchange(&ctx, "never-issued", VERIFIER).await;
    assert_eq!(status, 400);
    assert_eq!(body["error"], "OIDC_HANDOFF_INVALID");
}

/// An older desktop app (no `client_challenge`) is refused at the start.
#[sqlx::test]
async fn test_desktop_login_without_challenge_is_outdated(pool: PgPool) {
    let ctx = setup(&pool).await;
    let (status, body) = start_desktop_login(&ctx, None).await;
    assert_eq!(status, 400, "{body}");
    assert_eq!(body["error"], "OIDC_CLIENT_OUTDATED");
}

/// A desktop flow state stored without a challenge (started before the
/// upgrade) hands back an error code, never a code or tokens.
#[sqlx::test]
async fn test_legacy_desktop_state_hands_back_outdated(pool: PgPool) {
    let ctx = setup(&pool).await;
    linked_user(&ctx, "sub-legacy").await;
    let url = provider_url(&ctx).await;

    // Strip the challenge from the stored state.
    let key = format!(
        "oidc:state:{}",
        hex::encode(Sha256::digest(query_of(&url)["state"].as_bytes()))
    );
    let enc_key = hex::decode(ctx.config.mfa_encryption_key.as_ref().expect("key")).expect("hex");
    let redis = redis(&ctx).await;
    let stored: String = redis.get(&key).await.expect("stored state");
    let mut flow: OidcFlowState =
        serde_json::from_str(&decrypt_mfa_secret(&stored, &enc_key).expect("decrypt")).unwrap();
    assert!(flow.client_challenge.is_some());
    flow.client_challenge = None;
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
    assert!(!chain.result.contains_key("access_token"));
}

/// The browser popup flow is unchanged: tokens via `postMessage` and the
/// refresh cookie, no hand-off code.
#[sqlx::test]
async fn test_browser_login_flow_unchanged(pool: PgPool) {
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
    let provider = resp.headers()["location"].to_str().unwrap().to_string();

    issue(&ctx, fresh("sub-browser"));
    let resp = client().get(&provider).send().await.expect("provider");
    assert!(resp.status().is_redirection());
    let callback = resp.headers()["location"].to_str().unwrap().to_string();
    assert!(callback.starts_with(&format!("{}/auth/oidc/callback", ctx.server.url)));

    let resp = client().get(&callback).send().await.expect("callback");
    assert_eq!(resp.status().as_u16(), 200);
    let cookie = resp
        .headers()
        .get("set-cookie")
        .and_then(|c| c.to_str().ok())
        .unwrap_or_default()
        .to_string();
    assert!(!cookie.is_empty(), "refresh cookie is set");
    let html = resp.text().await.unwrap();
    assert!(html.contains("\"type\":\"oidc-callback\""), "{html}");
    assert!(html.contains("\"access_token\""), "{html}");
    assert!(!html.contains("\"code\""), "{html}");
}

/// The identity-link hand-back is unchanged and echoes the per-flow value.
#[sqlx::test]
async fn test_desktop_link_echoes_the_flow_value(pool: PgPool) {
    let ctx = setup(&pool).await;
    let user = create_user(&ctx.pool, true).await;
    let (_, token) = super::helpers::create_session_with_token(&ctx.pool, &ctx.config, user).await;

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
