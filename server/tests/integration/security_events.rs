//! Security events (append-only, audited) and the read-only Kaiku DM (C28).
//!
//! Run with: `cargo test --test integration security_events`

use axum::body::Body;
use axum::http::Method;
use sqlx::PgPool;
use uuid::Uuid;
use vc_server::auth::client_class::ClientClass;
use vc_server::security_events::{record, Actor, EventType, NewSecurityEvent};
use vc_server::system_account::{kaiku_dm_tx, SYSTEM_USER_ID};

use super::helpers::{
    body_to_json, create_dm_channel, create_test_user, generate_access_token, insert_message,
    TestApp,
};

fn sign_in_event(subject: Uuid) -> NewSecurityEvent {
    NewSecurityEvent {
        subject_id: subject,
        event_type: EventType::SignIn,
        actor: Actor::User(subject),
        client: Some((ClientClass::Desktop, "Kaiku desktop on Linux".into())),
        ip: Some("203.0.113.7".parse().unwrap()),
        session_id: None,
        authenticator: None,
        request_id: None,
        details: serde_json::json!({"auth_methods": ["qr"], "assurance": "probation"}),
    }
}

async fn counts(pool: &PgPool, subject: Uuid) -> (i64, i64) {
    let events: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM security_events WHERE subject_id = $1")
            .bind(subject)
            .fetch_one(pool)
            .await
            .unwrap();
    let audits: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM system_audit_log WHERE target_id = $1 AND action LIKE 'security.%'",
    )
    .bind(subject)
    .fetch_one(pool)
    .await
    .unwrap();
    (events, audits)
}

#[sqlx::test]
async fn record_writes_event_and_audit_row_in_one_transaction(pool: PgPool) {
    let (user, _) = create_test_user(&pool).await;

    let mut tx = pool.begin().await.unwrap();
    record(&mut tx, sign_in_event(user)).await.unwrap();
    tx.rollback().await.unwrap();
    assert_eq!(counts(&pool, user).await, (0, 0));

    let mut tx = pool.begin().await.unwrap();
    let id = record(&mut tx, sign_in_event(user)).await.unwrap();
    tx.commit().await.unwrap();
    assert_eq!(counts(&pool, user).await, (1, 1));

    let row: (
        String,
        String,
        String,
        Option<Uuid>,
        Option<String>,
        Option<String>,
    ) = sqlx::query_as(
        "SELECT event_type, severity, actor_kind, actor_user_id, client_class, host(ip)
             FROM security_events WHERE id = $1",
    )
    .bind(id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(row.0, "sign_in");
    assert_eq!(row.1, "normal");
    assert_eq!(row.2, "self");
    assert_eq!(row.3, Some(user));
    assert_eq!(row.4.as_deref(), Some("desktop"));
    assert_eq!(row.5.as_deref(), Some("203.0.113.7"));

    let audit: (Option<Uuid>, String, serde_json::Value) = sqlx::query_as(
        "SELECT actor_id, action, details FROM system_audit_log WHERE target_id = $1",
    )
    .bind(user)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(audit.0, Some(user));
    assert_eq!(audit.1, "security.sign_in");
    assert_eq!(audit.2["event_id"], id.to_string());

    // A system or CLI actor is audited as the system account.
    let mut tx = pool.begin().await.unwrap();
    let mut ev = sign_in_event(user);
    ev.event_type = EventType::KeyInvalidated;
    ev.actor = Actor::Cli;
    record(&mut tx, ev).await.unwrap();
    tx.commit().await.unwrap();
    let (actor, severity): (Option<Uuid>, String) = sqlx::query_as(
        "SELECT a.actor_id, e.severity FROM system_audit_log a
           JOIN security_events e ON e.id = (a.details->>'event_id')::uuid
          WHERE a.action = 'security.key_invalidated'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(actor, Some(SYSTEM_USER_ID));
    assert_eq!(severity, "high");
}

#[sqlx::test]
async fn immutable_columns_cannot_change(pool: PgPool) {
    let (user, _) = create_test_user(&pool).await;
    let mut tx = pool.begin().await.unwrap();
    let id = record(&mut tx, sign_in_event(user)).await.unwrap();
    tx.commit().await.unwrap();

    for stmt in [
        "UPDATE security_events SET event_type = 'guessing' WHERE id = $1",
        "UPDATE security_events SET details = '{}' WHERE id = $1",
        "UPDATE security_events SET created_at = NOW() - INTERVAL '1 day' WHERE id = $1",
        "UPDATE security_events SET ip = NULL WHERE id = $1",
    ] {
        let res = sqlx::query(stmt).bind(id).execute(&pool).await;
        assert!(res.is_err(), "{stmt} must fail");
    }
    sqlx::query("UPDATE security_events SET delivered_at = NOW() WHERE id = $1")
        .bind(id)
        .execute(&pool)
        .await
        .expect("delivered_at may be set");
    let again = sqlx::query("UPDATE security_events SET delivered_at = NULL WHERE id = $1")
        .bind(id)
        .execute(&pool)
        .await;
    assert!(again.is_err(), "delivered_at is set once");
    sqlx::query("UPDATE security_events SET repudiated_at = NOW() WHERE id = $1")
        .bind(id)
        .execute(&pool)
        .await
        .expect("repudiated_at may be set");
}

/// Create the user's Kaiku DM and one system message in it.
async fn kaiku_dm_with_message(pool: &PgPool, user: Uuid) -> (Uuid, Uuid) {
    let mut tx = pool.begin().await.unwrap();
    let channel = kaiku_dm_tx(&mut tx, user).await.unwrap();
    let again = kaiku_dm_tx(&mut tx, user).await.unwrap();
    assert_eq!(channel, again, "get or create");
    tx.commit().await.unwrap();
    let message = insert_message(pool, channel, SYSTEM_USER_ID, "A notice").await;
    (channel, message)
}

async fn call(
    app: &TestApp,
    method: Method,
    uri: &str,
    token: &str,
    body: Option<serde_json::Value>,
) -> (u16, serde_json::Value) {
    let mut req = TestApp::request(method, uri).header("Authorization", format!("Bearer {token}"));
    let body = match body {
        Some(b) => {
            req = req.header("Content-Type", "application/json");
            Body::from(b.to_string())
        }
        None => Body::empty(),
    };
    let resp = app.oneshot(req.body(body).unwrap()).await;
    let status = resp.status().as_u16();
    (status, body_to_json(resp).await)
}

#[sqlx::test]
async fn kaiku_dm_is_read_only(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    let (friend, _) = create_test_user(&pool).await;
    let token = generate_access_token(&app.config, user);
    let (dm, message) = kaiku_dm_with_message(&pool, user).await;
    let normal_dm = create_dm_channel(&pool, user, friend).await;

    let read_only: bool = sqlx::query_scalar("SELECT read_only FROM channels WHERE id = $1")
        .bind(dm)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(read_only);

    let cases: Vec<(Method, String, Option<serde_json::Value>)> = vec![
        (
            Method::POST,
            format!("/api/messages/channel/{dm}"),
            Some(serde_json::json!({"content": "hi"})),
        ),
        (
            Method::PATCH,
            format!("/api/messages/{message}"),
            Some(serde_json::json!({"content": "changed"})),
        ),
        (Method::DELETE, format!("/api/messages/{message}"), None),
        (
            Method::PUT,
            format!("/api/channels/{dm}/messages/{message}/reactions"),
            Some(serde_json::json!({"emoji": "👍"})),
        ),
        (
            Method::DELETE,
            format!("/api/channels/{dm}/messages/{message}/reactions/%F0%9F%91%8D"),
            None,
        ),
        (
            Method::PUT,
            format!("/api/channels/{dm}/messages/{message}/pin"),
            None,
        ),
        (
            Method::DELETE,
            format!("/api/channels/{dm}/messages/{message}/pin"),
            None,
        ),
        (Method::POST, format!("/api/dm/{dm}/leave"), None),
        (
            Method::PATCH,
            format!("/api/dm/{dm}/name"),
            Some(serde_json::json!({"name": "Not Kaiku"})),
        ),
        (
            Method::PATCH,
            format!("/api/channels/{dm}"),
            Some(serde_json::json!({"name": "renamed"})),
        ),
        (Method::DELETE, format!("/api/channels/{dm}"), None),
    ];
    for (method, uri, body) in cases {
        let (status, json) = call(&app, method.clone(), &uri, &token, body).await;
        assert_eq!(status, 403, "{method} {uri}: {json}");
        assert_eq!(json["error"], "CHANNEL_READ_ONLY", "{method} {uri}");
    }

    // Icon upload (multipart) is refused before the body is read.
    let boundary = "kaikuboundary";
    let body = format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"a.png\"\r\nContent-Type: image/png\r\n\r\nx\r\n--{boundary}--\r\n"
    );
    let req = TestApp::request(Method::POST, &format!("/api/dm/{dm}/icon"))
        .header("Authorization", format!("Bearer {token}"))
        .header(
            "Content-Type",
            format!("multipart/form-data; boundary={boundary}"),
        )
        .body(Body::from(body))
        .unwrap();
    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 403);
    assert_eq!(body_to_json(resp).await["error"], "CHANNEL_READ_ONLY");

    // The DM list flags the Kaiku DM and no other.
    let (status, list) = call(&app, Method::GET, "/api/dm", &token, None).await;
    assert_eq!(status, 200);
    let list = list.as_array().unwrap();
    let kaiku = list.iter().find(|d| d["id"] == dm.to_string()).unwrap();
    let other = list
        .iter()
        .find(|d| d["id"] == normal_dm.to_string())
        .unwrap();
    assert_eq!(kaiku["read_only"], true);
    assert_eq!(other["read_only"], false);

    // Reading stays possible, and the normal DM stays writable.
    let (status, _) = call(
        &app,
        Method::GET,
        &format!("/api/messages/channel/{dm}"),
        &token,
        None,
    )
    .await;
    assert_eq!(status, 200);
    let (status, _) = call(
        &app,
        Method::POST,
        &format!("/api/messages/channel/{normal_dm}"),
        &token,
        Some(serde_json::json!({"content": "hello"})),
    )
    .await;
    assert_eq!(status, 201);
}

#[sqlx::test]
async fn is_system_flag_on_profiles(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    let token = generate_access_token(&app.config, user);
    let (dm, _) = kaiku_dm_with_message(&pool, user).await;

    let (_, list) = call(&app, Method::GET, "/api/dm", &token, None).await;
    let kaiku = list
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["id"] == dm.to_string())
        .unwrap()
        .clone();
    let parts = kaiku["participants"].as_array().unwrap();
    let sys = parts
        .iter()
        .find(|p| p["user_id"] == SYSTEM_USER_ID.to_string())
        .unwrap();
    let me = parts
        .iter()
        .find(|p| p["user_id"] == user.to_string())
        .unwrap();
    assert_eq!(sys["is_system"], true);
    assert_eq!(sys["display_name"], "Kaiku");
    assert_eq!(me["is_system"], false);

    let (status, page) = call(
        &app,
        Method::GET,
        &format!("/api/messages/channel/{dm}"),
        &token,
        None,
    )
    .await;
    assert_eq!(status, 200);
    let items = page["items"].as_array().unwrap();
    assert_eq!(items[0]["author"]["is_system"], true);

    let (status, me) = call(&app, Method::GET, "/auth/me", &token, None).await;
    assert_eq!(status, 200);
    assert_eq!(me["is_system"], false);
}
