//! Bot Gateway Intent Integration Tests

use axum::body::Body;
use axum::http::{Method, StatusCode};
use sqlx::PgPool;

use super::helpers::*;

// ============================================================================
// Intent Persistence Tests
// ============================================================================

#[sqlx::test]
async fn update_intents_persists(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user_id, _) = create_test_user(&app.pool).await;
    let (app_id, _, _) = create_bot_application(&app.pool, user_id).await;
    let token = generate_access_token(&app.config, user_id);

    let body = serde_json::json!({
        "intents": ["messages", "members", "commands"],
    });

    let req = TestApp::request(Method::PUT, &format!("/api/applications/{app_id}/intents"))
        .header("Authorization", format!("Bearer {token}"))
        .header("Content-Type", "application/json")
        .body(Body::from(serde_json::to_string(&body).unwrap()))
        .unwrap();

    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), StatusCode::OK);

    let json = body_to_json(resp).await;
    let intents = json["gateway_intents"].as_array().unwrap();
    assert_eq!(intents.len(), 3);
    assert!(intents.iter().any(|v| v == "messages"));
    assert!(intents.iter().any(|v| v == "members"));
    assert!(intents.iter().any(|v| v == "commands"));
}

#[sqlx::test]
async fn update_intents_reflects_in_get(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user_id, _) = create_test_user(&app.pool).await;
    let (app_id, _, _) = create_bot_application(&app.pool, user_id).await;
    let token = generate_access_token(&app.config, user_id);

    // Update intents
    let body = serde_json::json!({ "intents": ["messages", "members"] });
    let req = TestApp::request(Method::PUT, &format!("/api/applications/{app_id}/intents"))
        .header("Authorization", format!("Bearer {token}"))
        .header("Content-Type", "application/json")
        .body(Body::from(serde_json::to_string(&body).unwrap()))
        .unwrap();

    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), StatusCode::OK);

    // Verify via GET
    let req = TestApp::request(Method::GET, &format!("/api/applications/{app_id}"))
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), StatusCode::OK);

    let json = body_to_json(resp).await;
    let intents = json["gateway_intents"].as_array().unwrap();
    assert_eq!(intents.len(), 2);
    assert!(intents.iter().any(|v| v == "messages"));
    assert!(intents.iter().any(|v| v == "members"));
}

// ============================================================================
// Intent Validation Tests
// ============================================================================

#[sqlx::test]
async fn invalid_intent_name_rejected(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user_id, _) = create_test_user(&app.pool).await;
    let (app_id, _, _) = create_bot_application(&app.pool, user_id).await;
    let token = generate_access_token(&app.config, user_id);

    let body = serde_json::json!({
        "intents": ["messages", "invalid_intent"],
    });

    let req = TestApp::request(Method::PUT, &format!("/api/applications/{app_id}/intents"))
        .header("Authorization", format!("Bearer {token}"))
        .header("Content-Type", "application/json")
        .body(Body::from(serde_json::to_string(&body).unwrap()))
        .unwrap();

    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[sqlx::test]
async fn empty_intents_allowed(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user_id, _) = create_test_user(&app.pool).await;
    let (app_id, _, _) = create_bot_application(&app.pool, user_id).await;
    let token = generate_access_token(&app.config, user_id);

    let body = serde_json::json!({ "intents": [] });

    let req = TestApp::request(Method::PUT, &format!("/api/applications/{app_id}/intents"))
        .header("Authorization", format!("Bearer {token}"))
        .header("Content-Type", "application/json")
        .body(Body::from(serde_json::to_string(&body).unwrap()))
        .unwrap();

    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), StatusCode::OK);

    let json = body_to_json(resp).await;
    let intents = json["gateway_intents"].as_array().unwrap();
    assert!(intents.is_empty());
}

// ============================================================================
// Ownership Tests
// ============================================================================

#[sqlx::test]
async fn non_owner_cannot_update_intents(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (owner_id, _) = create_test_user(&app.pool).await;
    let (other_id, _) = create_test_user(&app.pool).await;
    let (app_id, _, _) = create_bot_application(&app.pool, owner_id).await;
    let other_token = generate_access_token(&app.config, other_id);

    let body = serde_json::json!({ "intents": ["messages"] });

    let req = TestApp::request(Method::PUT, &format!("/api/applications/{app_id}/intents"))
        .header("Authorization", format!("Bearer {other_token}"))
        .header("Content-Type", "application/json")
        .body(Body::from(serde_json::to_string(&body).unwrap()))
        .unwrap();

    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

// ============================================================================
// Default Intent Behavior Tests
// ============================================================================

#[sqlx::test]
async fn new_application_has_default_intents(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user_id, _) = create_test_user(&app.pool).await;
    let token = generate_access_token(&app.config, user_id);

    // Create application via API
    let body = serde_json::json!({
        "name": "IntentTestBot",
        "description": "Testing default intents",
    });

    let req = TestApp::request(Method::POST, "/api/applications")
        .header("Authorization", format!("Bearer {token}"))
        .header("Content-Type", "application/json")
        .body(Body::from(serde_json::to_string(&body).unwrap()))
        .unwrap();

    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), StatusCode::CREATED);

    let json = body_to_json(resp).await;
    // New applications should have empty gateway_intents by default (from DB default)
    let intents = json["gateway_intents"].as_array().unwrap();
    assert!(intents.is_empty());
}

// ============================================================================
// Intent Logic Unit Tests (via shared events module)
// ============================================================================

#[sqlx::test]
async fn intents_permit_event_messages(_pool: PgPool) {
    use vc_server::webhooks::events::{BotEventType, GatewayIntent};

    let intents = vec!["messages".to_string()];
    assert!(GatewayIntent::intents_permit_event(
        &intents,
        &BotEventType::MessageCreated
    ));
    assert!(!GatewayIntent::intents_permit_event(
        &intents,
        &BotEventType::MemberJoined
    ));
    assert!(!GatewayIntent::intents_permit_event(
        &intents,
        &BotEventType::MemberLeft
    ));
    // Commands always permitted
    assert!(GatewayIntent::intents_permit_event(
        &intents,
        &BotEventType::CommandInvoked
    ));
}

#[sqlx::test]
async fn intents_permit_event_members(_pool: PgPool) {
    use vc_server::webhooks::events::{BotEventType, GatewayIntent};

    let intents = vec!["members".to_string()];
    assert!(!GatewayIntent::intents_permit_event(
        &intents,
        &BotEventType::MessageCreated
    ));
    assert!(GatewayIntent::intents_permit_event(
        &intents,
        &BotEventType::MemberJoined
    ));
    assert!(GatewayIntent::intents_permit_event(
        &intents,
        &BotEventType::MemberLeft
    ));
    assert!(GatewayIntent::intents_permit_event(
        &intents,
        &BotEventType::CommandInvoked
    ));
}

#[sqlx::test]
async fn intents_permit_event_combined(_pool: PgPool) {
    use vc_server::webhooks::events::{BotEventType, GatewayIntent};

    let intents = vec!["messages".to_string(), "members".to_string()];
    assert!(GatewayIntent::intents_permit_event(
        &intents,
        &BotEventType::MessageCreated
    ));
    assert!(GatewayIntent::intents_permit_event(
        &intents,
        &BotEventType::MemberJoined
    ));
    assert!(GatewayIntent::intents_permit_event(
        &intents,
        &BotEventType::MemberLeft
    ));
    assert!(GatewayIntent::intents_permit_event(
        &intents,
        &BotEventType::CommandInvoked
    ));
}

#[sqlx::test]
async fn no_intents_still_permits_commands(_pool: PgPool) {
    use vc_server::webhooks::events::{BotEventType, GatewayIntent};

    let intents: Vec<String> = vec![];
    assert!(!GatewayIntent::intents_permit_event(
        &intents,
        &BotEventType::MessageCreated
    ));
    assert!(!GatewayIntent::intents_permit_event(
        &intents,
        &BotEventType::MemberJoined
    ));
    // Commands always permitted even with no intents
    assert!(GatewayIntent::intents_permit_event(
        &intents,
        &BotEventType::CommandInvoked
    ));
}
