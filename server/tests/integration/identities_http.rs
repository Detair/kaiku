//! HTTP integration tests for linked-identity management.
//!
//! Covers GET /auth/me/identities (list), DELETE /auth/me/identities/{id}
//! (unlink, ownership, and last-login-method guard), and auth requirements.
//! The link *authorize*/callback flow needs a live OIDC provider exchange and
//! is exercised at the unit/DB level in `oidc.rs`.
//!
//! Run with: `cargo test --test integration identities_http`

use axum::body::Body;
use axum::http::Method;
use sqlx::PgPool;
use uuid::Uuid;

use super::helpers::{body_to_json, create_test_user, generate_access_token, TestApp};

/// Insert a passwordless OIDC-only user (NULL `password_hash`) and return its ID.
async fn create_passwordless_user(pool: &PgPool, external_id: &str) -> Uuid {
    let suffix = &Uuid::new_v4().to_string()[..8];
    let username = format!("oidconly_{suffix}");
    sqlx::query_scalar::<_, Uuid>(
        "INSERT INTO users (username, display_name, auth_method, external_id)
         VALUES ($1, 'OIDC Only', 'oidc', $2) RETURNING id",
    )
    .bind(&username)
    .bind(external_id)
    .fetch_one(pool)
    .await
    .expect("passwordless user insert should succeed")
}

async fn get_identities(app: &TestApp, token: &str) -> (u16, serde_json::Value) {
    let req = TestApp::request(Method::GET, "/auth/me/identities")
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await;
    let status = resp.status().as_u16();
    (status, body_to_json(resp).await)
}

async fn unlink(app: &TestApp, token: &str, id: Uuid) -> u16 {
    let req = TestApp::request(Method::DELETE, &format!("/auth/me/identities/{id}"))
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();
    app.oneshot(req).await.status().as_u16()
}

#[sqlx::test]
async fn test_list_identities_empty(pool: PgPool) {
    let app = TestApp::with_pool(pool).await;
    let (user_id, _) = create_test_user(&app.pool).await;
    let token = generate_access_token(&app.config, user_id);

    let (status, json) = get_identities(&app, &token).await;
    assert_eq!(status, 200);
    assert_eq!(json["identities"].as_array().unwrap().len(), 0);
}

#[sqlx::test]
async fn test_list_identities_returns_linked(pool: PgPool) {
    let app = TestApp::with_pool(pool).await;
    let (user_id, _) = create_test_user(&app.pool).await;
    let token = generate_access_token(&app.config, user_id);

    vc_server::db::insert_user_identity(&app.pool, user_id, "google", "g-sub", Some("a@x.io"))
        .await
        .unwrap();
    vc_server::db::insert_user_identity(&app.pool, user_id, "github", "gh-sub", None)
        .await
        .unwrap();

    let (status, json) = get_identities(&app, &token).await;
    assert_eq!(status, 200);
    let items = json["identities"].as_array().unwrap();
    assert_eq!(items.len(), 2);
    // No provider rows configured → provider_name falls back to the slug.
    let slugs: Vec<&str> = items
        .iter()
        .map(|i| i["provider_slug"].as_str().unwrap())
        .collect();
    assert!(slugs.contains(&"google") && slugs.contains(&"github"));
    let google = items
        .iter()
        .find(|i| i["provider_slug"] == "google")
        .unwrap();
    assert_eq!(google["provider_name"], "google");
    assert_eq!(google["email"], "a@x.io");
}

#[sqlx::test]
async fn test_unlink_identity_success(pool: PgPool) {
    let app = TestApp::with_pool(pool).await;
    // create_test_user makes a password-backed user, so the guard never trips.
    let (user_id, _) = create_test_user(&app.pool).await;
    let token = generate_access_token(&app.config, user_id);

    let id1 = vc_server::db::insert_user_identity(&app.pool, user_id, "google", "s1", None)
        .await
        .unwrap()
        .id;
    vc_server::db::insert_user_identity(&app.pool, user_id, "github", "s2", None)
        .await
        .unwrap();

    assert_eq!(unlink(&app, &token, id1).await, 204);

    let (_, json) = get_identities(&app, &token).await;
    let items = json["identities"].as_array().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["provider_slug"], "github");
}

#[sqlx::test]
async fn test_unlink_identity_not_found(pool: PgPool) {
    let app = TestApp::with_pool(pool).await;
    let (user_id, _) = create_test_user(&app.pool).await;
    let token = generate_access_token(&app.config, user_id);

    assert_eq!(unlink(&app, &token, Uuid::new_v4()).await, 404);
}

#[sqlx::test]
async fn test_unlink_other_users_identity_is_404(pool: PgPool) {
    let app = TestApp::with_pool(pool).await;
    let (owner, _) = create_test_user(&app.pool).await;
    let (attacker, _) = create_test_user(&app.pool).await;
    let attacker_token = generate_access_token(&app.config, attacker);

    let victim_identity =
        vc_server::db::insert_user_identity(&app.pool, owner, "google", "s", None)
            .await
            .unwrap()
            .id;

    // Ownership failure is reported as 404 (no probing other accounts' IDs).
    assert_eq!(unlink(&app, &attacker_token, victim_identity).await, 404);
    // And the identity is untouched.
    assert_eq!(
        vc_server::db::count_user_identities(&app.pool, owner)
            .await
            .unwrap(),
        1
    );
}

#[sqlx::test]
async fn test_unlink_last_identity_blocked_for_passwordless_user(pool: PgPool) {
    let app = TestApp::with_pool(pool).await;
    let user_id = create_passwordless_user(&app.pool, "google:only").await;
    let token = generate_access_token(&app.config, user_id);

    let id = vc_server::db::insert_user_identity(&app.pool, user_id, "google", "only", None)
        .await
        .unwrap()
        .id;

    // Removing the sole login method of a passwordless account is refused (409).
    assert_eq!(unlink(&app, &token, id).await, 409);
    assert_eq!(
        vc_server::db::count_user_identities(&app.pool, user_id)
            .await
            .unwrap(),
        1
    );
}

#[sqlx::test]
async fn test_unlink_one_of_two_allowed_for_passwordless_user(pool: PgPool) {
    let app = TestApp::with_pool(pool).await;
    let user_id = create_passwordless_user(&app.pool, "google:first").await;
    let token = generate_access_token(&app.config, user_id);

    let id1 = vc_server::db::insert_user_identity(&app.pool, user_id, "google", "first", None)
        .await
        .unwrap()
        .id;
    vc_server::db::insert_user_identity(&app.pool, user_id, "github", "second", None)
        .await
        .unwrap();

    // With two identities, a passwordless user may still drop one.
    assert_eq!(unlink(&app, &token, id1).await, 204);
}

#[sqlx::test]
async fn test_identity_endpoints_require_auth(pool: PgPool) {
    let app = TestApp::with_pool(pool).await;

    let list = TestApp::request(Method::GET, "/auth/me/identities")
        .body(Body::empty())
        .unwrap();
    assert_eq!(app.oneshot(list).await.status().as_u16(), 401);

    let del = TestApp::request(
        Method::DELETE,
        &format!("/auth/me/identities/{}", Uuid::new_v4()),
    )
    .body(Body::empty())
    .unwrap();
    assert_eq!(app.oneshot(del).await.status().as_u16(), 401);

    let link = TestApp::request(Method::GET, "/auth/me/identities/authorize/google")
        .body(Body::empty())
        .unwrap();
    assert_eq!(app.oneshot(link).await.status().as_u16(), 401);
}
