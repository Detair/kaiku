//! Integration tests for custom status feature.
//!
//! Tests validation logic, database persistence, clearing, and expiry sweep.
//!
//! Run with: `cargo test --test integration custom_status -- --nocapture`

use chrono::{Duration, Utc};
use vc_server::presence::CustomStatus;

use super::helpers::{create_test_user, shared_pool, CleanupGuard};

// ============================================================================
// Validation Tests (pure logic, no DB required)
// ============================================================================

#[test]
fn test_custom_status_validate_valid_text_only() {
    let status = CustomStatus {
        text: "Working on a project".to_string(),
        emoji: None,
        expires_at: None,
    };
    assert!(status.validate().is_ok());
}

#[test]
fn test_custom_status_validate_valid_with_emoji_and_expiry() {
    let status = CustomStatus {
        text: "In a meeting".to_string(),
        emoji: Some("\u{1F4C5}".to_string()),
        expires_at: Some(Utc::now() + Duration::hours(1)),
    };
    assert!(status.validate().is_ok());
}

#[test]
fn test_custom_status_validate_empty_text_rejected() {
    let status = CustomStatus {
        text: String::new(),
        emoji: None,
        expires_at: None,
    };
    let err = status.validate().unwrap_err();
    assert!(
        err.contains("empty"),
        "Error should mention empty text, got: {err}"
    );
}

#[test]
fn test_custom_status_validate_whitespace_only_rejected() {
    let status = CustomStatus {
        text: "   \t  ".to_string(),
        emoji: None,
        expires_at: None,
    };
    let err = status.validate().unwrap_err();
    assert!(
        err.contains("empty"),
        "Whitespace-only text should be rejected as empty, got: {err}"
    );
}

#[test]
fn test_custom_status_validate_text_at_max_length_ok() {
    let status = CustomStatus {
        text: "a".repeat(128),
        emoji: None,
        expires_at: None,
    };
    assert!(
        status.validate().is_ok(),
        "Text at exactly 128 chars should be accepted"
    );
}

#[test]
fn test_custom_status_validate_text_too_long_rejected() {
    let status = CustomStatus {
        text: "a".repeat(129),
        emoji: None,
        expires_at: None,
    };
    let err = status.validate().unwrap_err();
    assert!(
        err.contains("long"),
        "Error should mention text too long, got: {err}"
    );
}

#[test]
fn test_custom_status_validate_expires_at_in_past_rejected() {
    let status = CustomStatus {
        text: "Temporary status".to_string(),
        emoji: None,
        expires_at: Some(Utc::now() - Duration::hours(1)),
    };
    let err = status.validate().unwrap_err();
    assert!(
        err.contains("future"),
        "Error should mention future requirement, got: {err}"
    );
}

#[test]
fn test_custom_status_validate_emoji_at_max_graphemes_ok() {
    // Exactly 10 emoji grapheme clusters
    let status = CustomStatus {
        text: "hi".to_string(),
        emoji: Some(
            "\u{1F3AE}\u{1F3B5}\u{1F3A8}\u{1F3AD}\u{1F3AA}\u{1F3AB}\u{1F3AC}\u{1F3A4}\u{1F3A7}\u{1F3BC}"
                .to_string(),
        ),
        expires_at: None,
    };
    assert!(
        status.validate().is_ok(),
        "Exactly 10 emoji should be accepted"
    );
}

#[test]
fn test_custom_status_validate_emoji_too_many_graphemes_rejected() {
    // 11 emoji grapheme clusters
    let status = CustomStatus {
        text: "hi".to_string(),
        emoji: Some(
            "\u{1F3AE}\u{1F3B5}\u{1F3A8}\u{1F3AD}\u{1F3AA}\u{1F3AB}\u{1F3AC}\u{1F3A4}\u{1F3A7}\u{1F3BC}\u{1F3B9}"
                .to_string(),
        ),
        expires_at: None,
    };
    let err = status.validate().unwrap_err();
    assert!(
        err.contains("Emoji") || err.contains("emoji"),
        "Error should mention emoji limit, got: {err}"
    );
}

#[test]
fn test_custom_status_validate_control_chars_rejected() {
    let status = CustomStatus {
        text: "Hello\x00world".to_string(),
        emoji: None,
        expires_at: None,
    };
    assert!(
        status.validate().is_err(),
        "Control characters should be rejected"
    );
}

#[test]
fn test_custom_status_validate_bidi_override_rejected() {
    let status = CustomStatus {
        text: "Hello\u{202E}world".to_string(),
        emoji: None,
        expires_at: None,
    };
    assert!(
        status.validate().is_err(),
        "Bidi override characters should be rejected"
    );
}

#[test]
fn test_custom_status_validate_zalgo_text_rejected() {
    // 4 combining marks on a single base character (exceeds limit of 3)
    let status = CustomStatus {
        text: "a\u{0301}\u{0302}\u{0303}\u{0304}".to_string(),
        emoji: None,
        expires_at: None,
    };
    assert!(
        status.validate().is_err(),
        "Excessive combining marks (Zalgo) should be rejected"
    );
}

// ============================================================================
// Database Persistence Tests (require PostgreSQL at localhost:5433)
//
// These tests use `CleanupGuard` for RAII-based cleanup that runs even if
// the test panics or the tokio runtime is shutting down. The guard creates
// its own runtime for cleanup, avoiding issues with the shared pool.
// ============================================================================

#[tokio::test]
async fn test_custom_status_set_in_database() {
    let pool = shared_pool().await;
    let (user_id, _) = create_test_user(pool).await;
    let mut guard = CleanupGuard::new(pool.clone());
    guard.delete_user(user_id);

    let status = CustomStatus {
        text: "Testing custom status".to_string(),
        emoji: Some("\u{1F9EA}".to_string()),
        expires_at: None,
    };
    let json_value = serde_json::to_value(&status).expect("Failed to serialize custom status");

    // Set custom_status in DB
    sqlx::query("UPDATE users SET custom_status = $1 WHERE id = $2")
        .bind(&json_value)
        .bind(user_id)
        .execute(pool)
        .await
        .expect("Failed to set custom status in DB");

    // Read back and verify
    let row: (Option<serde_json::Value>,) =
        sqlx::query_as("SELECT custom_status FROM users WHERE id = $1")
            .bind(user_id)
            .fetch_one(pool)
            .await
            .expect("Failed to read custom status from DB");

    let stored = row.0.expect("custom_status should not be NULL");
    let deserialized: CustomStatus =
        serde_json::from_value(stored).expect("Failed to deserialize stored custom status");

    assert_eq!(deserialized.text, "Testing custom status");
    assert_eq!(deserialized.emoji, Some("\u{1F9EA}".to_string()));
    assert!(deserialized.expires_at.is_none());
}

#[tokio::test]
async fn test_custom_status_clear_in_database() {
    let pool = shared_pool().await;
    let (user_id, _) = create_test_user(pool).await;
    let mut guard = CleanupGuard::new(pool.clone());
    guard.delete_user(user_id);

    // First, set a custom status
    let status = CustomStatus {
        text: "Will be cleared".to_string(),
        emoji: None,
        expires_at: None,
    };
    let json_value = serde_json::to_value(&status).expect("Failed to serialize");

    sqlx::query("UPDATE users SET custom_status = $1 WHERE id = $2")
        .bind(&json_value)
        .bind(user_id)
        .execute(pool)
        .await
        .expect("Failed to set custom status");

    // Verify it's set
    let row: (Option<serde_json::Value>,) =
        sqlx::query_as("SELECT custom_status FROM users WHERE id = $1")
            .bind(user_id)
            .fetch_one(pool)
            .await
            .expect("Failed to read custom status");
    assert!(row.0.is_some(), "custom_status should be set before clear");

    // Clear custom_status
    sqlx::query("UPDATE users SET custom_status = NULL WHERE id = $1")
        .bind(user_id)
        .execute(pool)
        .await
        .expect("Failed to clear custom status");

    // Verify it's NULL
    let row: (Option<serde_json::Value>,) =
        sqlx::query_as("SELECT custom_status FROM users WHERE id = $1")
            .bind(user_id)
            .fetch_one(pool)
            .await
            .expect("Failed to read custom status after clear");
    assert!(
        row.0.is_none(),
        "custom_status should be NULL after clearing"
    );

    // guard drops here, runs cleanup even on panic
}

#[tokio::test]
async fn test_custom_status_with_expiry_persists() {
    let pool = shared_pool().await;
    let (user_id, _) = create_test_user(pool).await;
    let mut guard = CleanupGuard::new(pool.clone());
    guard.delete_user(user_id);

    let future_time = Utc::now() + Duration::hours(2);
    let status = CustomStatus {
        text: "Expires soon".to_string(),
        emoji: Some("\u{23F0}".to_string()),
        expires_at: Some(future_time),
    };
    let json_value = serde_json::to_value(&status).expect("Failed to serialize");

    sqlx::query("UPDATE users SET custom_status = $1 WHERE id = $2")
        .bind(&json_value)
        .bind(user_id)
        .execute(pool)
        .await
        .expect("Failed to set custom status with expiry");

    // Read back and verify expiry is preserved
    let row: (Option<serde_json::Value>,) =
        sqlx::query_as("SELECT custom_status FROM users WHERE id = $1")
            .bind(user_id)
            .fetch_one(pool)
            .await
            .expect("Failed to read custom status");

    let stored = row.0.expect("custom_status should not be NULL");
    let deserialized: CustomStatus = serde_json::from_value(stored).expect("Failed to deserialize");

    assert_eq!(deserialized.text, "Expires soon");
    assert!(deserialized.expires_at.is_some());
    // Verify the expiry time is close to what we set (within 1 second)
    let diff = (deserialized.expires_at.unwrap() - future_time)
        .num_seconds()
        .abs();
    assert!(
        diff <= 1,
        "Expiry time should be within 1 second of set value, diff was {diff}s"
    );

    // guard drops here, runs cleanup even on panic
}

// ============================================================================
// Expiry Sweep Tests (Task 13)
//
// These tests verify the SQL queries used by `spawn_custom_status_sweep`
// to find and clear expired custom statuses.
// ============================================================================

#[tokio::test]
async fn test_expiry_sweep_finds_expired_status() {
    let pool = shared_pool().await;
    let (user_id, _) = create_test_user(pool).await;
    let mut guard = CleanupGuard::new(pool.clone());
    guard.delete_user(user_id);

    // Insert an already-expired custom status directly into the DB
    let expired_status = serde_json::json!({
        "text": "This has expired",
        "expires_at": "2020-01-01T00:00:00Z"
    });

    sqlx::query("UPDATE users SET custom_status = $1 WHERE id = $2")
        .bind(&expired_status)
        .bind(user_id)
        .execute(pool)
        .await
        .expect("Failed to set expired custom status");

    // Run the sweep SELECT query (same query used in spawn_custom_status_sweep)
    let expired: Vec<(uuid::Uuid,)> = sqlx::query_as(
        r"
        SELECT id FROM users
        WHERE custom_status IS NOT NULL
          AND custom_status->>'expires_at' IS NOT NULL
          AND (custom_status->>'expires_at')::timestamptz <= NOW()
        ",
    )
    .fetch_all(pool)
    .await
    .expect("Sweep SELECT query failed");

    let expired_ids: Vec<uuid::Uuid> = expired.into_iter().map(|(id,)| id).collect();
    assert!(
        expired_ids.contains(&user_id),
        "Expired user should be found by sweep query"
    );

    // Run the sweep UPDATE query to clear expired statuses
    sqlx::query("UPDATE users SET custom_status = NULL WHERE id = ANY($1)")
        .bind(&expired_ids)
        .execute(pool)
        .await
        .expect("Sweep UPDATE query failed");

    // Verify the custom status is now NULL
    let row: (Option<serde_json::Value>,) =
        sqlx::query_as("SELECT custom_status FROM users WHERE id = $1")
            .bind(user_id)
            .fetch_one(pool)
            .await
            .expect("Failed to read custom status after sweep");

    assert!(
        row.0.is_none(),
        "custom_status should be NULL after sweep clears expired status"
    );

    // guard drops here, runs cleanup even on panic
}

#[tokio::test]
async fn test_expiry_sweep_ignores_non_expired_status() {
    let pool = shared_pool().await;
    let (user_id, _) = create_test_user(pool).await;
    let mut guard = CleanupGuard::new(pool.clone());
    guard.delete_user(user_id);

    // Insert a custom status that expires in the future
    let future_status = CustomStatus {
        text: "Still active".to_string(),
        emoji: None,
        expires_at: Some(Utc::now() + Duration::hours(24)),
    };
    let json_value = serde_json::to_value(&future_status).expect("Failed to serialize");

    sqlx::query("UPDATE users SET custom_status = $1 WHERE id = $2")
        .bind(&json_value)
        .bind(user_id)
        .execute(pool)
        .await
        .expect("Failed to set future custom status");

    // Run the sweep SELECT query
    let expired: Vec<(uuid::Uuid,)> = sqlx::query_as(
        r"
        SELECT id FROM users
        WHERE custom_status IS NOT NULL
          AND custom_status->>'expires_at' IS NOT NULL
          AND (custom_status->>'expires_at')::timestamptz <= NOW()
        ",
    )
    .fetch_all(pool)
    .await
    .expect("Sweep SELECT query failed");

    let expired_ids: Vec<uuid::Uuid> = expired.into_iter().map(|(id,)| id).collect();
    assert!(
        !expired_ids.contains(&user_id),
        "Non-expired user should NOT be found by sweep query"
    );

    // Verify the custom status is still set
    let row: (Option<serde_json::Value>,) =
        sqlx::query_as("SELECT custom_status FROM users WHERE id = $1")
            .bind(user_id)
            .fetch_one(pool)
            .await
            .expect("Failed to read custom status");

    assert!(
        row.0.is_some(),
        "Non-expired custom_status should still be present"
    );

    // guard drops here, runs cleanup even on panic
}

#[tokio::test]
async fn test_expiry_sweep_ignores_status_without_expiry() {
    let pool = shared_pool().await;
    let (user_id, _) = create_test_user(pool).await;
    let mut guard = CleanupGuard::new(pool.clone());
    guard.delete_user(user_id);

    // Insert a custom status without expires_at
    let status = CustomStatus {
        text: "Permanent status".to_string(),
        emoji: Some("\u{1F3E0}".to_string()),
        expires_at: None,
    };
    let json_value = serde_json::to_value(&status).expect("Failed to serialize");

    sqlx::query("UPDATE users SET custom_status = $1 WHERE id = $2")
        .bind(&json_value)
        .bind(user_id)
        .execute(pool)
        .await
        .expect("Failed to set permanent custom status");

    // Run the sweep SELECT query
    let expired: Vec<(uuid::Uuid,)> = sqlx::query_as(
        r"
        SELECT id FROM users
        WHERE custom_status IS NOT NULL
          AND custom_status->>'expires_at' IS NOT NULL
          AND (custom_status->>'expires_at')::timestamptz <= NOW()
        ",
    )
    .fetch_all(pool)
    .await
    .expect("Sweep SELECT query failed");

    let expired_ids: Vec<uuid::Uuid> = expired.into_iter().map(|(id,)| id).collect();
    assert!(
        !expired_ids.contains(&user_id),
        "Status without expiry should NOT be found by sweep query"
    );

    // guard drops here, runs cleanup even on panic
}
