//! Integration tests for bot ecosystem (applications, commands, gateway).

mod helpers;

use std::time::Duration;

use axum::body::Body;
use axum::http::Method;
use fred::interfaces::{ClientLike, EventInterface, KeysInterface, PubsubInterface};
use helpers::{create_test_user, delete_user, generate_access_token, TestApp};
use http_body_util::BodyExt;
use serde_json::json;
use vc_server::db;

/// Test creating a bot application.
#[tokio::test]
async fn test_create_bot_application() {
    let app = TestApp::new().await;
    let (user_id, _) = create_test_user(&app.pool).await;
    let token = generate_access_token(&app.config, user_id);

    let request = TestApp::request(Method::POST, "/api/applications")
        .header("Authorization", format!("Bearer {}", token))
        .header("Content-Type", "application/json")
        .body(Body::from(
            serde_json::to_string(&json!({
                "name": "Test Bot",
                "description": "A test bot application"
            }))
            .unwrap(),
        ))
        .unwrap();

    let response = app.oneshot(request).await;
    assert_eq!(response.status(), 201);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let app_response: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(app_response["name"], "Test Bot");
    assert_eq!(app_response["description"], "A test bot application");
    assert!(app_response["id"].is_string());

    delete_user(&app.pool, user_id).await;
}

/// Test creating application with invalid name.
#[tokio::test]
async fn test_create_bot_application_invalid_name() {
    let app = TestApp::new().await;
    let (user_id, _) = create_test_user(&app.pool).await;
    let token = generate_access_token(&app.config, user_id);

    // Name too short
    let request = TestApp::request(Method::POST, "/api/applications")
        .header("Authorization", format!("Bearer {}", token))
        .header("Content-Type", "application/json")
        .body(Body::from(
            serde_json::to_string(&json!({
                "name": "A"
            }))
            .unwrap(),
        ))
        .unwrap();

    let response = app.oneshot(request).await;
    assert_eq!(response.status(), 400);

    delete_user(&app.pool, user_id).await;
}

/// Test listing bot applications.
#[tokio::test]
async fn test_list_bot_applications() {
    let app = TestApp::new().await;
    let (user_id, _) = create_test_user(&app.pool).await;
    let token = generate_access_token(&app.config, user_id);

    // Create two applications
    for i in 1..=2 {
        let request = TestApp::request(Method::POST, "/api/applications")
            .header("Authorization", format!("Bearer {}", token))
            .header("Content-Type", "application/json")
            .body(Body::from(
                serde_json::to_string(&json!({
                    "name": format!("Bot {}", i)
                }))
                .unwrap(),
            ))
            .unwrap();
        let response = app.oneshot(request).await;
        assert_eq!(response.status(), 201);
    }

    // List applications
    let request = TestApp::request(Method::GET, "/api/applications")
        .header("Authorization", format!("Bearer {}", token))
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await;
    assert_eq!(response.status(), 200);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let apps: Vec<serde_json::Value> = serde_json::from_slice(&body).unwrap();

    assert_eq!(apps.len(), 2);
    assert_eq!(apps[0]["name"], "Bot 2"); // Ordered by created_at DESC
    assert_eq!(apps[1]["name"], "Bot 1");

    delete_user(&app.pool, user_id).await;
}

/// Test creating a bot user for an application.
#[tokio::test]
async fn test_create_bot_user() {
    let app = TestApp::new().await;
    let (user_id, _) = create_test_user(&app.pool).await;
    let token = generate_access_token(&app.config, user_id);

    // Create application
    let create_req = TestApp::request(Method::POST, "/api/applications")
        .header("Authorization", format!("Bearer {}", token))
        .header("Content-Type", "application/json")
        .body(Body::from(
            serde_json::to_string(&json!({
                "name": "Test Bot"
            }))
            .unwrap(),
        ))
        .unwrap();

    let create_resp = app.oneshot(create_req).await;
    let body = create_resp.into_body().collect().await.unwrap().to_bytes();
    let app_data: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let app_id = app_data["id"].as_str().unwrap();

    // Create bot user
    let bot_req = TestApp::request(Method::POST, &format!("/api/applications/{}/bot", app_id))
        .header("Authorization", format!("Bearer {}", token))
        .body(Body::empty())
        .unwrap();

    let bot_resp = app.oneshot(bot_req).await;
    assert_eq!(bot_resp.status(), 201);

    let body = bot_resp.into_body().collect().await.unwrap().to_bytes();
    let bot_data: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert!(bot_data["token"].is_string());
    assert!(bot_data["bot_user_id"].is_string());

    // Verify bot user exists in database
    let bot_user_id = bot_data["bot_user_id"].as_str().unwrap();
    let bot_user_id = uuid::Uuid::parse_str(bot_user_id).unwrap();

    let bot_user = sqlx::query!(
        "SELECT is_bot, bot_owner_id FROM users WHERE id = $1",
        bot_user_id
    )
    .fetch_one(&app.pool)
    .await
    .unwrap();

    assert!(bot_user.is_bot);
    assert_eq!(bot_user.bot_owner_id, Some(user_id));

    delete_user(&app.pool, user_id).await;
}

/// Test that creating bot user twice fails.
#[tokio::test]
async fn test_create_bot_user_twice_fails() {
    let app = TestApp::new().await;
    let (user_id, _) = create_test_user(&app.pool).await;
    let token = generate_access_token(&app.config, user_id);

    // Create application
    let create_req = TestApp::request(Method::POST, "/api/applications")
        .header("Authorization", format!("Bearer {}", token))
        .header("Content-Type", "application/json")
        .body(Body::from(
            serde_json::to_string(&json!({
                "name": "Test Bot"
            }))
            .unwrap(),
        ))
        .unwrap();

    let create_resp = app.oneshot(create_req).await;
    let body = create_resp.into_body().collect().await.unwrap().to_bytes();
    let app_data: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let app_id = app_data["id"].as_str().unwrap();

    // Create bot user first time
    let bot_req1 = TestApp::request(Method::POST, &format!("/api/applications/{}/bot", app_id))
        .header("Authorization", format!("Bearer {}", token))
        .body(Body::empty())
        .unwrap();
    let bot_resp1 = app.oneshot(bot_req1).await;
    assert_eq!(bot_resp1.status(), 201);

    // Try to create bot user second time
    let bot_req2 = TestApp::request(Method::POST, &format!("/api/applications/{}/bot", app_id))
        .header("Authorization", format!("Bearer {}", token))
        .body(Body::empty())
        .unwrap();
    let bot_resp2 = app.oneshot(bot_req2).await;
    assert_eq!(bot_resp2.status(), 409); // Conflict

    delete_user(&app.pool, user_id).await;
}

/// Test resetting bot token.
#[tokio::test]
async fn test_reset_bot_token() {
    let app = TestApp::new().await;
    let (user_id, _) = create_test_user(&app.pool).await;
    let token = generate_access_token(&app.config, user_id);

    // Create application and bot user
    let create_req = TestApp::request(Method::POST, "/api/applications")
        .header("Authorization", format!("Bearer {}", token))
        .header("Content-Type", "application/json")
        .body(Body::from(
            serde_json::to_string(&json!({
                "name": "Test Bot"
            }))
            .unwrap(),
        ))
        .unwrap();

    let create_resp = app.oneshot(create_req).await;
    let body = create_resp.into_body().collect().await.unwrap().to_bytes();
    let app_data: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let app_id = app_data["id"].as_str().unwrap();

    let bot_req = TestApp::request(Method::POST, &format!("/api/applications/{}/bot", app_id))
        .header("Authorization", format!("Bearer {}", token))
        .body(Body::empty())
        .unwrap();
    let bot_resp = app.oneshot(bot_req).await;
    let body = bot_resp.into_body().collect().await.unwrap().to_bytes();
    let bot_data: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let original_token = bot_data["token"].as_str().unwrap();

    // Reset token
    let reset_req = TestApp::request(
        Method::POST,
        &format!("/api/applications/{}/reset-token", app_id),
    )
    .header("Authorization", format!("Bearer {}", token))
    .body(Body::empty())
    .unwrap();

    let reset_resp = app.oneshot(reset_req).await;
    assert_eq!(reset_resp.status(), 200);

    let body = reset_resp.into_body().collect().await.unwrap().to_bytes();
    let new_data: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let new_token = new_data["token"].as_str().unwrap();

    assert_ne!(original_token, new_token);

    delete_user(&app.pool, user_id).await;
}

/// Test deleting a bot application.
#[tokio::test]
async fn test_delete_bot_application() {
    let app = TestApp::new().await;
    let (user_id, _) = create_test_user(&app.pool).await;
    let token = generate_access_token(&app.config, user_id);

    // Create application
    let create_req = TestApp::request(Method::POST, "/api/applications")
        .header("Authorization", format!("Bearer {}", token))
        .header("Content-Type", "application/json")
        .body(Body::from(
            serde_json::to_string(&json!({
                "name": "Test Bot"
            }))
            .unwrap(),
        ))
        .unwrap();

    let create_resp = app.oneshot(create_req).await;
    let body = create_resp.into_body().collect().await.unwrap().to_bytes();
    let app_data: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let app_id = app_data["id"].as_str().unwrap();

    // Delete application
    let delete_req = TestApp::request(Method::DELETE, &format!("/api/applications/{}", app_id))
        .header("Authorization", format!("Bearer {}", token))
        .body(Body::empty())
        .unwrap();

    let delete_resp = app.oneshot(delete_req).await;
    assert_eq!(delete_resp.status(), 204);

    // Verify it's gone
    let list_req = TestApp::request(Method::GET, "/api/applications")
        .header("Authorization", format!("Bearer {}", token))
        .body(Body::empty())
        .unwrap();

    let list_resp = app.oneshot(list_req).await;
    let body = list_resp.into_body().collect().await.unwrap().to_bytes();
    let apps: Vec<serde_json::Value> = serde_json::from_slice(&body).unwrap();

    assert_eq!(apps.len(), 0);

    delete_user(&app.pool, user_id).await;
}

/// Test registering slash commands.
#[tokio::test]
async fn test_register_slash_commands() {
    let app = TestApp::new().await;
    let (user_id, _) = create_test_user(&app.pool).await;
    let token = generate_access_token(&app.config, user_id);

    // Create application
    let create_req = TestApp::request(Method::POST, "/api/applications")
        .header("Authorization", format!("Bearer {}", token))
        .header("Content-Type", "application/json")
        .body(Body::from(
            serde_json::to_string(&json!({
                "name": "Test Bot"
            }))
            .unwrap(),
        ))
        .unwrap();

    let create_resp = app.oneshot(create_req).await;
    let body = create_resp.into_body().collect().await.unwrap().to_bytes();
    let app_data: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let app_id = app_data["id"].as_str().unwrap();

    // Register commands
    let register_req = TestApp::request(
        Method::PUT,
        &format!("/api/applications/{}/commands", app_id),
    )
    .header("Authorization", format!("Bearer {}", token))
    .header("Content-Type", "application/json")
    .body(Body::from(
        serde_json::to_string(&json!({
            "commands": [
                {
                    "name": "hello",
                    "description": "Says hello",
                    "options": []
                },
                {
                    "name": "ping",
                    "description": "Pong!",
                    "options": []
                }
            ]
        }))
        .unwrap(),
    ))
    .unwrap();

    let register_resp = app.oneshot(register_req).await;
    assert_eq!(register_resp.status(), 200);

    let body = register_resp
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    let commands: Vec<serde_json::Value> = serde_json::from_slice(&body).unwrap();

    assert_eq!(commands.len(), 2);
    assert_eq!(commands[0]["name"], "hello");
    assert_eq!(commands[1]["name"], "ping");

    delete_user(&app.pool, user_id).await;
}

/// Test command name validation.
#[tokio::test]
async fn test_register_command_invalid_name() {
    let app = TestApp::new().await;
    let (user_id, _) = create_test_user(&app.pool).await;
    let token = generate_access_token(&app.config, user_id);

    // Create application
    let create_req = TestApp::request(Method::POST, "/api/applications")
        .header("Authorization", format!("Bearer {}", token))
        .header("Content-Type", "application/json")
        .body(Body::from(
            serde_json::to_string(&json!({
                "name": "Test Bot"
            }))
            .unwrap(),
        ))
        .unwrap();

    let create_resp = app.oneshot(create_req).await;
    let body = create_resp.into_body().collect().await.unwrap().to_bytes();
    let app_data: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let app_id = app_data["id"].as_str().unwrap();

    // Try to register command with uppercase (invalid)
    let register_req = TestApp::request(
        Method::PUT,
        &format!("/api/applications/{}/commands", app_id),
    )
    .header("Authorization", format!("Bearer {}", token))
    .header("Content-Type", "application/json")
    .body(Body::from(
        serde_json::to_string(&json!({
            "commands": [
                {
                    "name": "HelloWorld",
                    "description": "Invalid name",
                    "options": []
                }
            ]
        }))
        .unwrap(),
    ))
    .unwrap();

    let register_resp = app.oneshot(register_req).await;
    assert_eq!(register_resp.status(), 400);

    delete_user(&app.pool, user_id).await;
}

/// Test listing slash commands.
#[tokio::test]
async fn test_list_slash_commands() {
    let app = TestApp::new().await;
    let (user_id, _) = create_test_user(&app.pool).await;
    let token = generate_access_token(&app.config, user_id);

    // Create application
    let create_req = TestApp::request(Method::POST, "/api/applications")
        .header("Authorization", format!("Bearer {}", token))
        .header("Content-Type", "application/json")
        .body(Body::from(
            serde_json::to_string(&json!({
                "name": "Test Bot"
            }))
            .unwrap(),
        ))
        .unwrap();

    let create_resp = app.oneshot(create_req).await;
    let body = create_resp.into_body().collect().await.unwrap().to_bytes();
    let app_data: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let app_id = app_data["id"].as_str().unwrap();

    // Register commands
    let register_req = TestApp::request(
        Method::PUT,
        &format!("/api/applications/{}/commands", app_id),
    )
    .header("Authorization", format!("Bearer {}", token))
    .header("Content-Type", "application/json")
    .body(Body::from(
        serde_json::to_string(&json!({
            "commands": [
                {
                    "name": "test",
                    "description": "Test command",
                    "options": []
                }
            ]
        }))
        .unwrap(),
    ))
    .unwrap();
    app.oneshot(register_req).await;

    // List commands
    let list_req = TestApp::request(
        Method::GET,
        &format!("/api/applications/{}/commands", app_id),
    )
    .header("Authorization", format!("Bearer {}", token))
    .body(Body::empty())
    .unwrap();

    let list_resp = app.oneshot(list_req).await;
    assert_eq!(list_resp.status(), 200);

    let body = list_resp.into_body().collect().await.unwrap().to_bytes();
    let commands: Vec<serde_json::Value> = serde_json::from_slice(&body).unwrap();

    assert_eq!(commands.len(), 1);
    assert_eq!(commands[0]["name"], "test");

    delete_user(&app.pool, user_id).await;
}

/// Test guild-scoped command operations stay isolated per application.
#[tokio::test]
async fn test_guild_scoped_commands_are_isolated_per_application() {
    let app = TestApp::new().await;
    let (user_id, _) = create_test_user(&app.pool).await;
    let token = generate_access_token(&app.config, user_id);

    let create_guild_req = TestApp::request(Method::POST, "/api/guilds")
        .header("Authorization", format!("Bearer {}", token))
        .header("Content-Type", "application/json")
        .body(Body::from(
            serde_json::to_string(&json!({ "name": "Commands Scope Guild" })).unwrap(),
        ))
        .unwrap();
    let create_guild_resp = app.oneshot(create_guild_req).await;
    assert_eq!(create_guild_resp.status(), 200);
    let guild_body = create_guild_resp
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    let guild_json: serde_json::Value = serde_json::from_slice(&guild_body).unwrap();
    let guild_id = guild_json["id"].as_str().unwrap();

    let mut app_ids = Vec::new();
    for app_name in ["Scoped Bot A", "Scoped Bot B"] {
        let create_req = TestApp::request(Method::POST, "/api/applications")
            .header("Authorization", format!("Bearer {}", token))
            .header("Content-Type", "application/json")
            .body(Body::from(
                serde_json::to_string(&json!({ "name": app_name })).unwrap(),
            ))
            .unwrap();

        let create_resp = app.oneshot(create_req).await;
        assert_eq!(create_resp.status(), 201);
        let body = create_resp.into_body().collect().await.unwrap().to_bytes();
        let app_data: serde_json::Value = serde_json::from_slice(&body).unwrap();
        app_ids.push(app_data["id"].as_str().unwrap().to_string());
    }

    let register_a_req = TestApp::request(
        Method::PUT,
        &format!(
            "/api/applications/{}/commands?guild_id={}",
            app_ids[0], guild_id
        ),
    )
    .header("Authorization", format!("Bearer {}", token))
    .header("Content-Type", "application/json")
    .body(Body::from(
        serde_json::to_string(&json!({
            "commands": [{
                "name": "alpha",
                "description": "alpha",
                "options": []
            }]
        }))
        .unwrap(),
    ))
    .unwrap();
    let register_a_resp = app.oneshot(register_a_req).await;
    assert_eq!(register_a_resp.status(), 200);

    let register_b_req = TestApp::request(
        Method::PUT,
        &format!(
            "/api/applications/{}/commands?guild_id={}",
            app_ids[1], guild_id
        ),
    )
    .header("Authorization", format!("Bearer {}", token))
    .header("Content-Type", "application/json")
    .body(Body::from(
        serde_json::to_string(&json!({
            "commands": [{
                "name": "beta",
                "description": "beta",
                "options": []
            }]
        }))
        .unwrap(),
    ))
    .unwrap();
    let register_b_resp = app.oneshot(register_b_req).await;
    assert_eq!(register_b_resp.status(), 200);

    let list_a_req = TestApp::request(
        Method::GET,
        &format!(
            "/api/applications/{}/commands?guild_id={}",
            app_ids[0], guild_id
        ),
    )
    .header("Authorization", format!("Bearer {}", token))
    .body(Body::empty())
    .unwrap();
    let list_a_resp = app.oneshot(list_a_req).await;
    assert_eq!(list_a_resp.status(), 200);
    let body = list_a_resp.into_body().collect().await.unwrap().to_bytes();
    let commands_a: Vec<serde_json::Value> = serde_json::from_slice(&body).unwrap();
    assert_eq!(commands_a.len(), 1);
    assert_eq!(commands_a[0]["name"], "alpha");

    let delete_a_req = TestApp::request(
        Method::DELETE,
        &format!(
            "/api/applications/{}/commands?guild_id={}",
            app_ids[0], guild_id
        ),
    )
    .header("Authorization", format!("Bearer {}", token))
    .body(Body::empty())
    .unwrap();
    let delete_a_resp = app.oneshot(delete_a_req).await;
    assert_eq!(delete_a_resp.status(), 204);

    let list_b_req = TestApp::request(
        Method::GET,
        &format!(
            "/api/applications/{}/commands?guild_id={}",
            app_ids[1], guild_id
        ),
    )
    .header("Authorization", format!("Bearer {}", token))
    .body(Body::empty())
    .unwrap();
    let list_b_resp = app.oneshot(list_b_req).await;
    assert_eq!(list_b_resp.status(), 200);
    let body = list_b_resp.into_body().collect().await.unwrap().to_bytes();
    let commands_b: Vec<serde_json::Value> = serde_json::from_slice(&body).unwrap();
    assert_eq!(commands_b.len(), 1);
    assert_eq!(commands_b[0]["name"], "beta");

    delete_user(&app.pool, user_id).await;
}

/// Test deleting a slash command.
#[tokio::test]
async fn test_delete_slash_command() {
    let app = TestApp::new().await;
    let (user_id, _) = create_test_user(&app.pool).await;
    let token = generate_access_token(&app.config, user_id);

    // Create application and command
    let create_req = TestApp::request(Method::POST, "/api/applications")
        .header("Authorization", format!("Bearer {}", token))
        .header("Content-Type", "application/json")
        .body(Body::from(
            serde_json::to_string(&json!({
                "name": "Test Bot"
            }))
            .unwrap(),
        ))
        .unwrap();

    let create_resp = app.oneshot(create_req).await;
    let body = create_resp.into_body().collect().await.unwrap().to_bytes();
    let app_data: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let app_id = app_data["id"].as_str().unwrap();

    let register_req = TestApp::request(
        Method::PUT,
        &format!("/api/applications/{}/commands", app_id),
    )
    .header("Authorization", format!("Bearer {}", token))
    .header("Content-Type", "application/json")
    .body(Body::from(
        serde_json::to_string(&json!({
            "commands": [
                {
                    "name": "test",
                    "description": "Test command",
                    "options": []
                }
            ]
        }))
        .unwrap(),
    ))
    .unwrap();

    let register_resp = app.oneshot(register_req).await;
    let body = register_resp
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    let commands: Vec<serde_json::Value> = serde_json::from_slice(&body).unwrap();
    let cmd_id = commands[0]["id"].as_str().unwrap();

    // Delete command
    let delete_req = TestApp::request(
        Method::DELETE,
        &format!("/api/applications/{}/commands/{}", app_id, cmd_id),
    )
    .header("Authorization", format!("Bearer {}", token))
    .body(Body::empty())
    .unwrap();

    let delete_resp = app.oneshot(delete_req).await;
    assert_eq!(delete_resp.status(), 204);

    // Verify it's gone
    let list_req = TestApp::request(
        Method::GET,
        &format!("/api/applications/{}/commands", app_id),
    )
    .header("Authorization", format!("Bearer {}", token))
    .body(Body::empty())
    .unwrap();

    let list_resp = app.oneshot(list_req).await;
    let body = list_resp.into_body().collect().await.unwrap().to_bytes();
    let commands: Vec<serde_json::Value> = serde_json::from_slice(&body).unwrap();

    assert_eq!(commands.len(), 0);

    delete_user(&app.pool, user_id).await;
}

/// Test that non-owners cannot access applications.
#[tokio::test]
async fn test_application_ownership() {
    let app = TestApp::new().await;
    let (owner_id, _) = create_test_user(&app.pool).await;
    let (other_id, _) = create_test_user(&app.pool).await;
    let owner_token = generate_access_token(&app.config, owner_id);
    let other_token = generate_access_token(&app.config, other_id);

    // Owner creates application
    let create_req = TestApp::request(Method::POST, "/api/applications")
        .header("Authorization", format!("Bearer {}", owner_token))
        .header("Content-Type", "application/json")
        .body(Body::from(
            serde_json::to_string(&json!({
                "name": "Owner's Bot"
            }))
            .unwrap(),
        ))
        .unwrap();

    let create_resp = app.oneshot(create_req).await;
    let body = create_resp.into_body().collect().await.unwrap().to_bytes();
    let app_data: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let app_id = app_data["id"].as_str().unwrap();

    // Other user tries to access it
    let get_req = TestApp::request(Method::GET, &format!("/api/applications/{}", app_id))
        .header("Authorization", format!("Bearer {}", other_token))
        .body(Body::empty())
        .unwrap();

    let get_resp = app.oneshot(get_req).await;
    assert_eq!(get_resp.status(), 403); // Forbidden

    delete_user(&app.pool, owner_id).await;
    delete_user(&app.pool, other_id).await;
}

/// Test guild bot install endpoint requires guild management permissions.
#[tokio::test]
async fn test_add_bot_to_guild() {
    let app = TestApp::new().await;
    let (owner_id, _) = create_test_user(&app.pool).await;
    let owner_token = generate_access_token(&app.config, owner_id);

    let create_app_req = TestApp::request(Method::POST, "/api/applications")
        .header("Authorization", format!("Bearer {}", owner_token))
        .header("Content-Type", "application/json")
        .body(Body::from(
            serde_json::to_string(&json!({ "name": "Guild Bot" })).unwrap(),
        ))
        .unwrap();
    let create_app_resp = app.oneshot(create_app_req).await;
    assert_eq!(create_app_resp.status(), 201);
    let app_body = create_app_resp
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    let app_json: serde_json::Value = serde_json::from_slice(&app_body).unwrap();
    let application_id = uuid::Uuid::parse_str(app_json["id"].as_str().unwrap()).unwrap();

    let create_bot_req = TestApp::request(
        Method::POST,
        &format!("/api/applications/{}/bot", application_id),
    )
    .header("Authorization", format!("Bearer {}", owner_token))
    .body(Body::empty())
    .unwrap();
    let create_bot_resp = app.oneshot(create_bot_req).await;
    assert_eq!(create_bot_resp.status(), 201);
    let bot_body = create_bot_resp
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    let bot_json: serde_json::Value = serde_json::from_slice(&bot_body).unwrap();
    let bot_user_id = uuid::Uuid::parse_str(bot_json["bot_user_id"].as_str().unwrap()).unwrap();

    let create_guild_req = TestApp::request(Method::POST, "/api/guilds")
        .header("Authorization", format!("Bearer {}", owner_token))
        .header("Content-Type", "application/json")
        .body(Body::from(
            serde_json::to_string(&json!({ "name": "Bot Guild" })).unwrap(),
        ))
        .unwrap();
    let create_guild_resp = app.oneshot(create_guild_req).await;
    assert_eq!(create_guild_resp.status(), 200);
    let guild_body = create_guild_resp
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    let guild_json: serde_json::Value = serde_json::from_slice(&guild_body).unwrap();
    let guild_id = uuid::Uuid::parse_str(guild_json["id"].as_str().unwrap()).unwrap();

    let add_bot_req = TestApp::request(
        Method::POST,
        &format!("/api/guilds/{}/bots/{}/add", guild_id, bot_user_id),
    )
    .header("Authorization", format!("Bearer {}", owner_token))
    .body(Body::empty())
    .unwrap();
    let add_bot_resp = app.oneshot(add_bot_req).await;
    assert_eq!(add_bot_resp.status(), 204);

    let installed = sqlx::query!(
        "SELECT id FROM guild_bot_installations WHERE guild_id = $1 AND application_id = $2",
        guild_id,
        application_id
    )
    .fetch_optional(&app.pool)
    .await
    .unwrap();
    assert!(installed.is_some());

    sqlx::query!(
        "UPDATE bot_applications SET public = false WHERE id = $1",
        application_id
    )
    .execute(&app.pool)
    .await
    .unwrap();

    let (other_user_id, _) = create_test_user(&app.pool).await;
    let other_token = generate_access_token(&app.config, other_user_id);

    let create_other_guild_req = TestApp::request(Method::POST, "/api/guilds")
        .header("Authorization", format!("Bearer {}", other_token))
        .header("Content-Type", "application/json")
        .body(Body::from(
            serde_json::to_string(&json!({ "name": "Other Guild" })).unwrap(),
        ))
        .unwrap();
    let create_other_guild_resp = app.oneshot(create_other_guild_req).await;
    assert_eq!(create_other_guild_resp.status(), 200);
    let other_guild_body = create_other_guild_resp
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    let other_guild_json: serde_json::Value = serde_json::from_slice(&other_guild_body).unwrap();
    let other_guild_id = uuid::Uuid::parse_str(other_guild_json["id"].as_str().unwrap()).unwrap();

    let private_bot_req = TestApp::request(
        Method::POST,
        &format!("/api/guilds/{}/bots/{}/add", other_guild_id, bot_user_id),
    )
    .header("Authorization", format!("Bearer {}", other_token))
    .body(Body::empty())
    .unwrap();
    let private_bot_resp = app.oneshot(private_bot_req).await;
    assert_eq!(private_bot_resp.status(), 404);

    let forbidden_req = TestApp::request(
        Method::POST,
        &format!("/api/guilds/{}/bots/{}/add", guild_id, bot_user_id),
    )
    .header("Authorization", format!("Bearer {}", other_token))
    .body(Body::empty())
    .unwrap();
    let forbidden_resp = app.oneshot(forbidden_req).await;
    assert_eq!(forbidden_resp.status(), 403);

    sqlx::query!(
        "DELETE FROM guild_bot_installations WHERE guild_id = $1 AND application_id = $2",
        guild_id,
        application_id
    )
    .execute(&app.pool)
    .await
    .unwrap();
    delete_user(&app.pool, owner_id).await;
    delete_user(&app.pool, other_user_id).await;
}

/// Test slash command routing publishes invocation to bot gateway channel.
#[tokio::test]
async fn test_slash_command_invocation_publishes_to_bot_channel() {
    let app = TestApp::new().await;
    let (user_id, _) = create_test_user(&app.pool).await;
    let token = generate_access_token(&app.config, user_id);

    let create_app_req = TestApp::request(Method::POST, "/api/applications")
        .header("Authorization", format!("Bearer {}", token))
        .header("Content-Type", "application/json")
        .body(Body::from(
            serde_json::to_string(&json!({
                "name": "Routing Bot"
            }))
            .unwrap(),
        ))
        .unwrap();
    let create_app_resp = app.oneshot(create_app_req).await;
    assert_eq!(create_app_resp.status(), 201);
    let app_body = create_app_resp
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    let app_json: serde_json::Value = serde_json::from_slice(&app_body).unwrap();
    let application_id = uuid::Uuid::parse_str(app_json["id"].as_str().unwrap()).unwrap();

    let create_bot_req = TestApp::request(
        Method::POST,
        &format!("/api/applications/{}/bot", application_id),
    )
    .header("Authorization", format!("Bearer {}", token))
    .body(Body::empty())
    .unwrap();
    let create_bot_resp = app.oneshot(create_bot_req).await;
    assert_eq!(create_bot_resp.status(), 201);
    let bot_body = create_bot_resp
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    let bot_json: serde_json::Value = serde_json::from_slice(&bot_body).unwrap();
    let bot_user_id = uuid::Uuid::parse_str(bot_json["bot_user_id"].as_str().unwrap()).unwrap();

    let register_cmd_req = TestApp::request(
        Method::PUT,
        &format!("/api/applications/{}/commands", application_id),
    )
    .header("Authorization", format!("Bearer {}", token))
    .header("Content-Type", "application/json")
    .body(Body::from(
        serde_json::to_string(&json!({
            "commands": [{
                "name": "hello",
                "description": "Say hello",
                "options": []
            }]
        }))
        .unwrap(),
    ))
    .unwrap();
    let register_cmd_resp = app.oneshot(register_cmd_req).await;
    assert_eq!(register_cmd_resp.status(), 200);

    let guild_id = uuid::Uuid::new_v4();
    let channel_id = uuid::Uuid::new_v4();

    sqlx::query("INSERT INTO guilds (id, name, owner_id) VALUES ($1, $2, $3)")
        .bind(guild_id)
        .bind("Bot Routing Guild")
        .bind(user_id)
        .execute(&app.pool)
        .await
        .unwrap();

    sqlx::query("INSERT INTO guild_members (guild_id, user_id) VALUES ($1, $2)")
        .bind(guild_id)
        .bind(user_id)
        .execute(&app.pool)
        .await
        .unwrap();

    sqlx::query(
        "INSERT INTO channels (id, guild_id, name, channel_type) VALUES ($1, $2, $3, 'text')",
    )
    .bind(channel_id)
    .bind(guild_id)
    .bind("bot-commands")
    .execute(&app.pool)
    .await
    .unwrap();

    sqlx::query(
        "INSERT INTO guild_bot_installations (guild_id, application_id, installed_by) VALUES ($1, $2, $3)",
    )
    .bind(guild_id)
    .bind(application_id)
    .bind(user_id)
    .execute(&app.pool)
    .await
    .unwrap();

    let subscriber = vc_server::db::create_redis_client(&app.config.redis_url)
        .await
        .unwrap();
    let _connect_handle = subscriber.connect();
    subscriber.wait_for_connect().await.unwrap();

    let mut pubsub_stream = subscriber.message_rx();
    subscriber
        .subscribe(format!("bot:{}", bot_user_id))
        .await
        .unwrap();

    let create_msg_req = TestApp::request(
        Method::POST,
        &format!("/api/messages/channel/{}", channel_id),
    )
    .header("Authorization", format!("Bearer {}", token))
    .header("Content-Type", "application/json")
    .body(Body::from(
        serde_json::to_string(&json!({
            "content": "/hello world"
        }))
        .unwrap(),
    ))
    .unwrap();

    let create_msg_resp = app.oneshot(create_msg_req).await;
    let create_msg_status = create_msg_resp.status();
    let create_msg_body = create_msg_resp
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    assert_eq!(
        create_msg_status,
        202,
        "expected 202 for slash invocation, got body: {}",
        String::from_utf8_lossy(&create_msg_body)
    );

    let persisted_count = sqlx::query_scalar!(
        "SELECT COUNT(*) as \"count!\" FROM messages WHERE channel_id = $1",
        channel_id
    )
    .fetch_one(&app.pool)
    .await
    .unwrap();
    assert_eq!(persisted_count, 0);

    let message = tokio::time::timeout(Duration::from_secs(2), pubsub_stream.recv())
        .await
        .expect("timed out waiting for bot command event")
        .expect("bot pubsub stream closed unexpectedly");

    let payload = String::from_utf8(message.value.as_bytes().unwrap().to_vec()).unwrap();
    let event: serde_json::Value = serde_json::from_str(&payload).unwrap();

    assert_eq!(event["type"], "command_invoked");
    assert_eq!(event["command_name"], "hello");
    assert_eq!(event["guild_id"], guild_id.to_string());
    assert_eq!(event["channel_id"], channel_id.to_string());
    assert_eq!(event["user_id"], user_id.to_string());

    let interaction_id = event["interaction_id"].as_str().unwrap();
    let owner_key = format!("interaction:{}:owner", interaction_id);
    let redis = db::create_redis_client(&app.config.redis_url)
        .await
        .unwrap();
    let stored_owner = redis.get::<Option<String>, _>(&owner_key).await.unwrap();
    let bot_user_id_str = bot_user_id.to_string();
    assert_eq!(stored_owner.as_deref(), Some(bot_user_id_str.as_str()));

    sqlx::query("DELETE FROM guild_bot_installations WHERE guild_id = $1")
        .bind(guild_id)
        .execute(&app.pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM channels WHERE id = $1")
        .bind(channel_id)
        .execute(&app.pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM guild_members WHERE guild_id = $1 AND user_id = $2")
        .bind(guild_id)
        .bind(user_id)
        .execute(&app.pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM guilds WHERE id = $1")
        .bind(guild_id)
        .execute(&app.pool)
        .await
        .unwrap();
    delete_user(&app.pool, user_id).await;
}

/// Test slash command fails when multiple bots provide same command in same scope.
#[tokio::test]
async fn test_slash_command_invocation_ambiguous() {
    let app = TestApp::new().await;
    let (user_id, _) = create_test_user(&app.pool).await;
    let token = generate_access_token(&app.config, user_id);

    let create_app_1_req = TestApp::request(Method::POST, "/api/applications")
        .header("Authorization", format!("Bearer {}", token))
        .header("Content-Type", "application/json")
        .body(Body::from(
            serde_json::to_string(&json!({ "name": "Ambiguous Bot A" })).unwrap(),
        ))
        .unwrap();
    let create_app_1_resp = app.oneshot(create_app_1_req).await;
    assert_eq!(create_app_1_resp.status(), 201);
    let app_1_body = create_app_1_resp
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    let app_1_json: serde_json::Value = serde_json::from_slice(&app_1_body).unwrap();
    let application_id_1 = uuid::Uuid::parse_str(app_1_json["id"].as_str().unwrap()).unwrap();

    let create_app_2_req = TestApp::request(Method::POST, "/api/applications")
        .header("Authorization", format!("Bearer {}", token))
        .header("Content-Type", "application/json")
        .body(Body::from(
            serde_json::to_string(&json!({ "name": "Ambiguous Bot B" })).unwrap(),
        ))
        .unwrap();
    let create_app_2_resp = app.oneshot(create_app_2_req).await;
    assert_eq!(create_app_2_resp.status(), 201);
    let app_2_body = create_app_2_resp
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    let app_2_json: serde_json::Value = serde_json::from_slice(&app_2_body).unwrap();
    let application_id_2 = uuid::Uuid::parse_str(app_2_json["id"].as_str().unwrap()).unwrap();

    let create_bot_1_req = TestApp::request(
        Method::POST,
        &format!("/api/applications/{}/bot", application_id_1),
    )
    .header("Authorization", format!("Bearer {}", token))
    .body(Body::empty())
    .unwrap();
    let create_bot_1_resp = app.oneshot(create_bot_1_req).await;
    assert_eq!(create_bot_1_resp.status(), 201);

    let create_bot_2_req = TestApp::request(
        Method::POST,
        &format!("/api/applications/{}/bot", application_id_2),
    )
    .header("Authorization", format!("Bearer {}", token))
    .body(Body::empty())
    .unwrap();
    let create_bot_2_resp = app.oneshot(create_bot_2_req).await;
    assert_eq!(create_bot_2_resp.status(), 201);

    for app_id in [application_id_1, application_id_2] {
        let register_cmd_req = TestApp::request(
            Method::PUT,
            &format!("/api/applications/{}/commands", app_id),
        )
        .header("Authorization", format!("Bearer {}", token))
        .header("Content-Type", "application/json")
        .body(Body::from(
            serde_json::to_string(&json!({
                "commands": [{
                    "name": "hello",
                    "description": "Say hello",
                    "options": []
                }]
            }))
            .unwrap(),
        ))
        .unwrap();
        let register_cmd_resp = app.oneshot(register_cmd_req).await;
        assert_eq!(register_cmd_resp.status(), 200);
    }

    let guild_id = uuid::Uuid::new_v4();
    let channel_id = uuid::Uuid::new_v4();

    sqlx::query("INSERT INTO guilds (id, name, owner_id) VALUES ($1, $2, $3)")
        .bind(guild_id)
        .bind("Ambiguous Command Guild")
        .bind(user_id)
        .execute(&app.pool)
        .await
        .unwrap();

    sqlx::query("INSERT INTO guild_members (guild_id, user_id) VALUES ($1, $2)")
        .bind(guild_id)
        .bind(user_id)
        .execute(&app.pool)
        .await
        .unwrap();

    sqlx::query(
        "INSERT INTO channels (id, guild_id, name, channel_type) VALUES ($1, $2, $3, 'text')",
    )
    .bind(channel_id)
    .bind(guild_id)
    .bind("bot-commands")
    .execute(&app.pool)
    .await
    .unwrap();

    for app_id in [application_id_1, application_id_2] {
        sqlx::query(
            "INSERT INTO guild_bot_installations (guild_id, application_id, installed_by) VALUES ($1, $2, $3)",
        )
        .bind(guild_id)
        .bind(app_id)
        .bind(user_id)
        .execute(&app.pool)
        .await
        .unwrap();
    }

    let create_msg_req = TestApp::request(
        Method::POST,
        &format!("/api/messages/channel/{}", channel_id),
    )
    .header("Authorization", format!("Bearer {}", token))
    .header("Content-Type", "application/json")
    .body(Body::from(
        serde_json::to_string(&json!({
            "content": "/hello world"
        }))
        .unwrap(),
    ))
    .unwrap();

    let create_msg_resp = app.oneshot(create_msg_req).await;
    assert_eq!(create_msg_resp.status(), 400);

    let persisted_count = sqlx::query_scalar!(
        "SELECT COUNT(*) as \"count!\" FROM messages WHERE channel_id = $1",
        channel_id
    )
    .fetch_one(&app.pool)
    .await
    .unwrap();
    assert_eq!(persisted_count, 0);

    sqlx::query("DELETE FROM guild_bot_installations WHERE guild_id = $1")
        .bind(guild_id)
        .execute(&app.pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM channels WHERE id = $1")
        .bind(channel_id)
        .execute(&app.pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM guild_members WHERE guild_id = $1 AND user_id = $2")
        .bind(guild_id)
        .bind(user_id)
        .execute(&app.pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM guilds WHERE id = $1")
        .bind(guild_id)
        .execute(&app.pool)
        .await
        .unwrap();
    delete_user(&app.pool, user_id).await;
}
