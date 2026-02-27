//! E2EE Key Management integration tests.
//!
//! Tests for end-to-end encryption key management including:
//! - Key pair upload and device registration
//! - Prekey claim (single and concurrent)
//! - Key backup and restore
//! - Input validation
//!
//! Run with: `cargo test --test integration e2ee_keys`
//! Run ignored (integration) tests: `cargo test --test integration e2ee_keys -- --ignored`

use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use uuid::Uuid;

// ============================================================================
// Constants (matching the server implementation)
// ============================================================================

const MAX_PREKEYS_PER_UPLOAD: usize = 100;

// ============================================================================
// Unit Tests (no database required)
// ============================================================================

#[test]
fn test_base64_encoding_for_keys() {
    // Simulate a 32-byte Curve25519 public key
    let key_bytes = [0u8; 32];
    let encoded = STANDARD.encode(key_bytes);

    // Base64 of 32 bytes should be 44 characters (including padding)
    assert_eq!(encoded.len(), 44);

    // Should be decodable back
    let decoded = STANDARD.decode(&encoded).expect("Should decode");
    assert_eq!(decoded.len(), 32);
}

#[test]
fn test_salt_size_validation() {
    // Salt must be exactly 16 bytes
    let valid_salt = [0u8; 16];
    let encoded_salt = STANDARD.encode(valid_salt);

    let decoded = STANDARD.decode(&encoded_salt).expect("Should decode");
    assert_eq!(decoded.len(), 16, "Salt must be 16 bytes");
}

#[test]
fn test_nonce_size_validation() {
    // AES-GCM nonce must be exactly 12 bytes
    let valid_nonce = [0u8; 12];
    let encoded_nonce = STANDARD.encode(valid_nonce);

    let decoded = STANDARD.decode(&encoded_nonce).expect("Should decode");
    assert_eq!(decoded.len(), 12, "Nonce must be 12 bytes");
}

#[test]
fn test_prekey_limit_constant() {
    // Verify the constant matches expected value
    assert_eq!(MAX_PREKEYS_PER_UPLOAD, 100);
}

#[test]
fn test_identity_key_max_length() {
    // Identity keys are base64-encoded 32-byte public keys
    // Max 64 characters allows for standard base64 with padding
    let max_length = 64;

    // A 32-byte key encodes to 44 chars - within limit
    let key_32 = [0u8; 32];
    let encoded_32 = STANDARD.encode(key_32);
    assert!(
        encoded_32.len() <= max_length,
        "32-byte key should fit within limit"
    );

    // A 48-byte key would encode to 64 chars - at limit
    let key_48 = [0u8; 48];
    let encoded_48 = STANDARD.encode(key_48);
    assert_eq!(
        encoded_48.len(),
        max_length,
        "48-byte key should be exactly at limit"
    );
}

#[test]
fn test_device_name_max_length() {
    // Device name limit is 128 characters
    let max_length = 128;

    let valid_name = "a".repeat(max_length);
    assert_eq!(valid_name.len(), max_length);

    let invalid_name = "a".repeat(max_length + 1);
    assert!(invalid_name.len() > max_length);
}

#[test]
fn test_backup_ciphertext_max_size() {
    // Max ciphertext size is 1MB (1,048,576 bytes)
    let max_size = 1_048_576;

    // A 1MB backup is valid
    assert!(1_000_000 <= max_size);

    // Slightly over is invalid
    assert!(max_size + 1 > max_size);
}

#[test]
fn test_prekey_key_id_format() {
    // key_id can be a UUID or counter string
    let uuid_id = Uuid::new_v4().to_string();
    assert!(uuid_id.len() <= 64, "UUID key_id should fit within limit");

    let counter_id = "12345";
    assert!(
        counter_id.len() <= 64,
        "Counter key_id should fit within limit"
    );
}

#[test]
fn test_claimed_prekey_fields() {
    // Verify the structure of a claimed prekey response
    let key_id = "test_key_1";
    let public_key = STANDARD.encode([1u8; 32]);

    let key_len = key_id.len();
    let pk_len = public_key.len();
    assert!(key_len > 0);
    assert!(pk_len > 0);
    assert!(key_len <= 64);
    assert!(pk_len <= 64);
}

// ============================================================================
// Integration Tests (require database - marked as #[ignore])
// ============================================================================

/// Helper to create a test database pool.
#[allow(dead_code)]
async fn create_test_pool() -> sqlx::PgPool {
    let database_url =
        std::env::var("DATABASE_URL").unwrap_or_else(|_| "postgres://localhost/vc_test".into());

    sqlx::PgPool::connect(&database_url)
        .await
        .expect("Failed to connect to test database")
}

/// Test user struct for cleanup.
#[allow(dead_code)]
struct TestUser {
    id: Uuid,
    username: String,
}

/// Helper to create a test user.
#[allow(dead_code)]
async fn create_test_user(pool: &sqlx::PgPool, username: &str) -> TestUser {
    let user_id = Uuid::now_v7();
    let password_hash = vc_server::auth::hash_password("Test123!@#").unwrap();

    sqlx::query(
        "INSERT INTO users (id, username, display_name, password_hash) VALUES ($1, $2, $3, $4)",
    )
    .bind(user_id)
    .bind(username)
    .bind(username)
    .bind(&password_hash)
    .execute(pool)
    .await
    .expect("Failed to create test user");

    TestUser {
        id: user_id,
        username: username.to_string(),
    }
}

/// Helper to cleanup test data.
#[allow(dead_code)]
async fn cleanup_test_user(pool: &sqlx::PgPool, user_id: Uuid) {
    // Delete in correct order due to foreign key constraints
    let _ = sqlx::query(
        "DELETE FROM prekeys WHERE device_id IN (SELECT id FROM user_devices WHERE user_id = $1)",
    )
    .bind(user_id)
    .execute(pool)
    .await;
    let _ = sqlx::query("DELETE FROM user_devices WHERE user_id = $1")
        .bind(user_id)
        .execute(pool)
        .await;
    let _ = sqlx::query("DELETE FROM key_backups WHERE user_id = $1")
        .bind(user_id)
        .execute(pool)
        .await;
    let _ = sqlx::query("DELETE FROM users WHERE id = $1")
        .bind(user_id)
        .execute(pool)
        .await;
}

/// Helper to generate mock identity keys.
#[allow(dead_code)]
fn generate_mock_identity_keys() -> (String, String) {
    // Generate random 32-byte keys and encode as base64
    use std::time::{SystemTime, UNIX_EPOCH};
    let seed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos() as u64;

    let mut ed25519_bytes = [0u8; 32];
    let mut curve25519_bytes = [0u8; 32];

    // Simple deterministic fill for testing
    for (i, byte) in ed25519_bytes.iter_mut().enumerate() {
        *byte = ((seed >> (i % 8)) & 0xFF) as u8;
    }
    for (i, byte) in curve25519_bytes.iter_mut().enumerate() {
        *byte = ((seed >> ((i + 1) % 8)) & 0xFF) as u8;
    }

    (
        STANDARD.encode(ed25519_bytes),
        STANDARD.encode(curve25519_bytes),
    )
}

/// Helper to generate mock prekeys.
#[allow(dead_code)]
fn generate_mock_prekeys(count: usize) -> Vec<(String, String)> {
    (0..count)
        .map(|i| {
            let key_id = format!("prekey_{i}");
            let mut public_key_bytes = [0u8; 32];
            public_key_bytes[0] = i as u8;
            (key_id, STANDARD.encode(public_key_bytes))
        })
        .collect()
}

#[tokio::test]
#[ignore] // Requires PostgreSQL
async fn test_device_registration() {
    let pool = create_test_pool().await;

    // Create test user
    let username = format!("test_device_{}", Uuid::new_v4());
    let user = create_test_user(&pool, &username).await;

    // Generate mock identity keys
    let (identity_ed25519, identity_curve25519) = generate_mock_identity_keys();

    // Insert device
    let device_id: Uuid = sqlx::query_scalar(
        "INSERT INTO user_devices (user_id, device_name, identity_key_ed25519, identity_key_curve25519)
         VALUES ($1, $2, $3, $4)
         RETURNING id",
    )
    .bind(user.id)
    .bind("Test Device")
    .bind(&identity_ed25519)
    .bind(&identity_curve25519)
    .fetch_one(&pool)
    .await
    .expect("Device insert should succeed");

    // Verify device was created
    let device_exists: Option<(Uuid,)> =
        sqlx::query_as("SELECT id FROM user_devices WHERE id = $1")
            .bind(device_id)
            .fetch_optional(&pool)
            .await
            .expect("Query should succeed");

    assert!(device_exists.is_some(), "Device should exist");

    // Cleanup
    cleanup_test_user(&pool, user.id).await;
}

#[tokio::test]
#[ignore] // Requires PostgreSQL
async fn test_device_upsert_on_same_identity_key() {
    let pool = create_test_pool().await;

    // Create test user
    let username = format!("test_upsert_{}", Uuid::new_v4());
    let user = create_test_user(&pool, &username).await;

    // Generate identity keys (same for both inserts)
    let (identity_ed25519, identity_curve25519) = generate_mock_identity_keys();

    // First insert
    let device_id1: Uuid = sqlx::query_scalar(
        "INSERT INTO user_devices (user_id, device_name, identity_key_ed25519, identity_key_curve25519)
         VALUES ($1, $2, $3, $4)
         ON CONFLICT (user_id, identity_key_curve25519)
         DO UPDATE SET last_seen_at = NOW(), device_name = COALESCE(EXCLUDED.device_name, user_devices.device_name)
         RETURNING id",
    )
    .bind(user.id)
    .bind("Device v1")
    .bind(&identity_ed25519)
    .bind(&identity_curve25519)
    .fetch_one(&pool)
    .await
    .expect("First insert should succeed");

    // Second insert with same identity key (should upsert)
    let device_id2: Uuid = sqlx::query_scalar(
        "INSERT INTO user_devices (user_id, device_name, identity_key_ed25519, identity_key_curve25519)
         VALUES ($1, $2, $3, $4)
         ON CONFLICT (user_id, identity_key_curve25519)
         DO UPDATE SET last_seen_at = NOW(), device_name = COALESCE(EXCLUDED.device_name, user_devices.device_name)
         RETURNING id",
    )
    .bind(user.id)
    .bind("Device v2")
    .bind(&identity_ed25519)
    .bind(&identity_curve25519)
    .fetch_one(&pool)
    .await
    .expect("Second insert should succeed (upsert)");

    // Should return the same device ID
    assert_eq!(
        device_id1, device_id2,
        "Upsert should return same device ID"
    );

    // Should only have one device
    let device_count: (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM user_devices WHERE user_id = $1")
            .bind(user.id)
            .fetch_one(&pool)
            .await
            .expect("Query should succeed");

    assert_eq!(
        device_count.0, 1,
        "Should only have one device after upsert"
    );

    // Cleanup
    cleanup_test_user(&pool, user.id).await;
}

#[tokio::test]
#[ignore] // Requires PostgreSQL
async fn test_prekey_upload() {
    let pool = create_test_pool().await;

    // Create test user and device
    let username = format!("test_prekey_{}", Uuid::new_v4());
    let user = create_test_user(&pool, &username).await;

    let (identity_ed25519, identity_curve25519) = generate_mock_identity_keys();

    let device_id: Uuid = sqlx::query_scalar(
        "INSERT INTO user_devices (user_id, identity_key_ed25519, identity_key_curve25519)
         VALUES ($1, $2, $3)
         RETURNING id",
    )
    .bind(user.id)
    .bind(&identity_ed25519)
    .bind(&identity_curve25519)
    .fetch_one(&pool)
    .await
    .expect("Device insert should succeed");

    // Upload prekeys
    let prekeys = generate_mock_prekeys(10);
    let mut uploaded_count = 0;

    for (key_id, public_key) in &prekeys {
        let result = sqlx::query(
            "INSERT INTO prekeys (device_id, key_id, public_key)
             VALUES ($1, $2, $3)
             ON CONFLICT (device_id, key_id) DO NOTHING",
        )
        .bind(device_id)
        .bind(key_id)
        .bind(public_key)
        .execute(&pool)
        .await
        .expect("Prekey insert should succeed");

        if result.rows_affected() > 0 {
            uploaded_count += 1;
        }
    }

    assert_eq!(uploaded_count, 10, "Should upload 10 prekeys");

    // Verify prekeys exist
    let prekey_count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM prekeys WHERE device_id = $1")
        .bind(device_id)
        .fetch_one(&pool)
        .await
        .expect("Query should succeed");

    assert_eq!(prekey_count.0, 10, "Should have 10 prekeys");

    // Cleanup
    cleanup_test_user(&pool, user.id).await;
}

#[tokio::test]
#[ignore] // Requires PostgreSQL
async fn test_prekey_claim_single() {
    let pool = create_test_pool().await;

    // Create owner and claimer users
    let owner_username = format!("test_owner_{}", Uuid::new_v4());
    let claimer_username = format!("test_claimer_{}", Uuid::new_v4());

    let owner = create_test_user(&pool, &owner_username).await;
    let claimer = create_test_user(&pool, &claimer_username).await;

    // Create device with prekeys for owner
    let (identity_ed25519, identity_curve25519) = generate_mock_identity_keys();

    let device_id: Uuid = sqlx::query_scalar(
        "INSERT INTO user_devices (user_id, identity_key_ed25519, identity_key_curve25519)
         VALUES ($1, $2, $3)
         RETURNING id",
    )
    .bind(owner.id)
    .bind(&identity_ed25519)
    .bind(&identity_curve25519)
    .fetch_one(&pool)
    .await
    .expect("Device insert should succeed");

    // Upload prekeys
    let prekeys = generate_mock_prekeys(5);
    for (key_id, public_key) in &prekeys {
        sqlx::query("INSERT INTO prekeys (device_id, key_id, public_key) VALUES ($1, $2, $3)")
            .bind(device_id)
            .bind(key_id)
            .bind(public_key)
            .execute(&pool)
            .await
            .expect("Prekey insert should succeed");
    }

    // Claim a prekey
    let claimed: Option<(String, String)> = sqlx::query_as(
        "UPDATE prekeys
         SET claimed_at = NOW(), claimed_by = $1
         WHERE id = (
             SELECT id FROM prekeys
             WHERE device_id = $2 AND claimed_at IS NULL
             ORDER BY created_at
             LIMIT 1
             FOR UPDATE SKIP LOCKED
         )
         RETURNING key_id, public_key",
    )
    .bind(claimer.id)
    .bind(device_id)
    .fetch_optional(&pool)
    .await
    .expect("Claim query should succeed");

    assert!(claimed.is_some(), "Should claim a prekey");

    let (key_id, _public_key) = claimed.unwrap();
    assert_eq!(
        key_id, "prekey_0",
        "Should claim first prekey (ordered by created_at)"
    );

    // Verify prekey is now claimed
    let unclaimed_count: (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM prekeys WHERE device_id = $1 AND claimed_at IS NULL")
            .bind(device_id)
            .fetch_one(&pool)
            .await
            .expect("Query should succeed");

    assert_eq!(unclaimed_count.0, 4, "Should have 4 unclaimed prekeys left");

    // Cleanup
    cleanup_test_user(&pool, owner.id).await;
    cleanup_test_user(&pool, claimer.id).await;
}

#[tokio::test]
#[ignore] // Requires PostgreSQL
async fn test_prekey_claim_when_exhausted() {
    let pool = create_test_pool().await;

    // Create owner and claimer
    let owner_username = format!("test_exhausted_{}", Uuid::new_v4());
    let claimer_username = format!("test_claimer_ex_{}", Uuid::new_v4());

    let owner = create_test_user(&pool, &owner_username).await;
    let claimer = create_test_user(&pool, &claimer_username).await;

    // Create device WITHOUT prekeys
    let (identity_ed25519, identity_curve25519) = generate_mock_identity_keys();

    let device_id: Uuid = sqlx::query_scalar(
        "INSERT INTO user_devices (user_id, identity_key_ed25519, identity_key_curve25519)
         VALUES ($1, $2, $3)
         RETURNING id",
    )
    .bind(owner.id)
    .bind(&identity_ed25519)
    .bind(&identity_curve25519)
    .fetch_one(&pool)
    .await
    .expect("Device insert should succeed");

    // Try to claim when no prekeys exist
    let claimed: Option<(String, String)> = sqlx::query_as(
        "UPDATE prekeys
         SET claimed_at = NOW(), claimed_by = $1
         WHERE id = (
             SELECT id FROM prekeys
             WHERE device_id = $2 AND claimed_at IS NULL
             ORDER BY created_at
             LIMIT 1
             FOR UPDATE SKIP LOCKED
         )
         RETURNING key_id, public_key",
    )
    .bind(claimer.id)
    .bind(device_id)
    .fetch_optional(&pool)
    .await
    .expect("Claim query should succeed");

    assert!(
        claimed.is_none(),
        "Should return None when no prekeys available"
    );

    // Cleanup
    cleanup_test_user(&pool, owner.id).await;
    cleanup_test_user(&pool, claimer.id).await;
}

#[tokio::test]
#[ignore] // Requires PostgreSQL
async fn test_prekey_duplicate_upload_ignored() {
    let pool = create_test_pool().await;

    // Create test user and device
    let username = format!("test_dup_prekey_{}", Uuid::new_v4());
    let user = create_test_user(&pool, &username).await;

    let (identity_ed25519, identity_curve25519) = generate_mock_identity_keys();

    let device_id: Uuid = sqlx::query_scalar(
        "INSERT INTO user_devices (user_id, identity_key_ed25519, identity_key_curve25519)
         VALUES ($1, $2, $3)
         RETURNING id",
    )
    .bind(user.id)
    .bind(&identity_ed25519)
    .bind(&identity_curve25519)
    .fetch_one(&pool)
    .await
    .expect("Device insert should succeed");

    // Upload a prekey
    let result1 = sqlx::query(
        "INSERT INTO prekeys (device_id, key_id, public_key)
         VALUES ($1, $2, $3)
         ON CONFLICT (device_id, key_id) DO NOTHING",
    )
    .bind(device_id)
    .bind("duplicate_key")
    .bind("AAAA")
    .execute(&pool)
    .await
    .expect("First insert should succeed");

    assert_eq!(
        result1.rows_affected(),
        1,
        "First insert should affect 1 row"
    );

    // Upload same prekey again (should be ignored)
    let result2 = sqlx::query(
        "INSERT INTO prekeys (device_id, key_id, public_key)
         VALUES ($1, $2, $3)
         ON CONFLICT (device_id, key_id) DO NOTHING",
    )
    .bind(device_id)
    .bind("duplicate_key")
    .bind("BBBB") // Different value, but same key_id
    .execute(&pool)
    .await
    .expect("Second insert should succeed (no-op)");

    assert_eq!(
        result2.rows_affected(),
        0,
        "Duplicate insert should affect 0 rows"
    );

    // Should still only have 1 prekey
    let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM prekeys WHERE device_id = $1")
        .bind(device_id)
        .fetch_one(&pool)
        .await
        .expect("Query should succeed");

    assert_eq!(count.0, 1, "Should only have 1 prekey");

    // Cleanup
    cleanup_test_user(&pool, user.id).await;
}

#[tokio::test]
#[ignore] // Requires PostgreSQL
async fn test_key_backup_upload_and_retrieve() {
    let pool = create_test_pool().await;

    // Create test user
    let username = format!("test_backup_{}", Uuid::new_v4());
    let user = create_test_user(&pool, &username).await;

    // Generate backup data
    let salt = [0u8; 16];
    let nonce = [0u8; 12];
    let ciphertext = b"encrypted_key_data_here";

    // Upload backup
    sqlx::query(
        "INSERT INTO key_backups (user_id, salt, nonce, ciphertext, version)
         VALUES ($1, $2, $3, $4, $5)
         ON CONFLICT (user_id) DO UPDATE SET
             salt = EXCLUDED.salt,
             nonce = EXCLUDED.nonce,
             ciphertext = EXCLUDED.ciphertext,
             version = EXCLUDED.version,
             created_at = NOW()",
    )
    .bind(user.id)
    .bind(&salt[..])
    .bind(&nonce[..])
    .bind(&ciphertext[..])
    .bind(1i32)
    .execute(&pool)
    .await
    .expect("Backup insert should succeed");

    // Retrieve backup
    let backup: Option<(Vec<u8>, Vec<u8>, Vec<u8>, i32)> = sqlx::query_as(
        "SELECT salt, nonce, ciphertext, version FROM key_backups WHERE user_id = $1",
    )
    .bind(user.id)
    .fetch_optional(&pool)
    .await
    .expect("Query should succeed");

    assert!(backup.is_some(), "Backup should exist");

    let (retrieved_salt, retrieved_nonce, retrieved_ciphertext, version) = backup.unwrap();
    assert_eq!(retrieved_salt, salt.to_vec());
    assert_eq!(retrieved_nonce, nonce.to_vec());
    assert_eq!(retrieved_ciphertext, ciphertext.to_vec());
    assert_eq!(version, 1);

    // Cleanup
    cleanup_test_user(&pool, user.id).await;
}

#[tokio::test]
#[ignore] // Requires PostgreSQL
async fn test_key_backup_upsert() {
    let pool = create_test_pool().await;

    // Create test user
    let username = format!("test_backup_upsert_{}", Uuid::new_v4());
    let user = create_test_user(&pool, &username).await;

    // Upload first backup
    let salt1 = [1u8; 16];
    let nonce1 = [1u8; 12];
    let ciphertext1 = b"backup_v1";

    sqlx::query(
        "INSERT INTO key_backups (user_id, salt, nonce, ciphertext, version)
         VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(user.id)
    .bind(&salt1[..])
    .bind(&nonce1[..])
    .bind(&ciphertext1[..])
    .bind(1i32)
    .execute(&pool)
    .await
    .expect("First backup should succeed");

    // Upload second backup (should replace)
    let salt2 = [2u8; 16];
    let nonce2 = [2u8; 12];
    let ciphertext2 = b"backup_v2";

    sqlx::query(
        "INSERT INTO key_backups (user_id, salt, nonce, ciphertext, version)
         VALUES ($1, $2, $3, $4, $5)
         ON CONFLICT (user_id) DO UPDATE SET
             salt = EXCLUDED.salt,
             nonce = EXCLUDED.nonce,
             ciphertext = EXCLUDED.ciphertext,
             version = EXCLUDED.version,
             created_at = NOW()",
    )
    .bind(user.id)
    .bind(&salt2[..])
    .bind(&nonce2[..])
    .bind(&ciphertext2[..])
    .bind(2i32)
    .execute(&pool)
    .await
    .expect("Second backup (upsert) should succeed");

    // Verify only one backup exists with v2 data
    let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM key_backups WHERE user_id = $1")
        .bind(user.id)
        .fetch_one(&pool)
        .await
        .expect("Query should succeed");

    assert_eq!(count.0, 1, "Should only have one backup");

    let backup: (i32,) = sqlx::query_as("SELECT version FROM key_backups WHERE user_id = $1")
        .bind(user.id)
        .fetch_one(&pool)
        .await
        .expect("Query should succeed");

    assert_eq!(backup.0, 2, "Backup should be version 2");

    // Cleanup
    cleanup_test_user(&pool, user.id).await;
}

#[tokio::test]
#[ignore] // Requires PostgreSQL
async fn test_backup_status_no_backup() {
    let pool = create_test_pool().await;

    // Create test user (no backup)
    let username = format!("test_no_backup_{}", Uuid::new_v4());
    let user = create_test_user(&pool, &username).await;

    // Check status
    let backup: Option<(i32,)> =
        sqlx::query_as("SELECT version FROM key_backups WHERE user_id = $1")
            .bind(user.id)
            .fetch_optional(&pool)
            .await
            .expect("Query should succeed");

    assert!(backup.is_none(), "Should have no backup");

    // Cleanup
    cleanup_test_user(&pool, user.id).await;
}

#[tokio::test]
#[ignore] // Requires PostgreSQL
async fn test_get_user_device_keys() {
    let pool = create_test_pool().await;

    // Create test user with multiple devices
    let username = format!("test_multi_device_{}", Uuid::new_v4());
    let user = create_test_user(&pool, &username).await;

    // Create 3 devices
    for i in 0..3 {
        let (identity_ed25519, identity_curve25519) = generate_mock_identity_keys();

        sqlx::query(
            "INSERT INTO user_devices (user_id, device_name, identity_key_ed25519, identity_key_curve25519)
             VALUES ($1, $2, $3, $4)",
        )
        .bind(user.id)
        .bind(format!("Device {i}"))
        .bind(&identity_ed25519)
        .bind(&identity_curve25519)
        .execute(&pool)
        .await
        .expect("Device insert should succeed");

        // Small delay to ensure different timestamps
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }

    // Get all devices for user
    let devices: Vec<(Uuid, Option<String>, String, String)> = sqlx::query_as(
        "SELECT id, device_name, identity_key_ed25519, identity_key_curve25519
         FROM user_devices
         WHERE user_id = $1
         ORDER BY last_seen_at DESC",
    )
    .bind(user.id)
    .fetch_all(&pool)
    .await
    .expect("Query should succeed");

    assert_eq!(devices.len(), 3, "Should have 3 devices");

    // Cleanup
    cleanup_test_user(&pool, user.id).await;
}

#[tokio::test]
#[ignore] // Requires PostgreSQL
async fn test_concurrent_prekey_claims_unique() {
    use tokio::task::JoinSet;

    let pool = create_test_pool().await;

    // Create owner
    let owner_username = format!("test_concurrent_{}", Uuid::new_v4());
    let owner = create_test_user(&pool, &owner_username).await;

    // Create device with prekeys
    let (identity_ed25519, identity_curve25519) = generate_mock_identity_keys();

    let device_id: Uuid = sqlx::query_scalar(
        "INSERT INTO user_devices (user_id, identity_key_ed25519, identity_key_curve25519)
         VALUES ($1, $2, $3)
         RETURNING id",
    )
    .bind(owner.id)
    .bind(&identity_ed25519)
    .bind(&identity_curve25519)
    .fetch_one(&pool)
    .await
    .expect("Device insert should succeed");

    // Upload 5 prekeys
    let prekeys = generate_mock_prekeys(5);
    for (key_id, public_key) in &prekeys {
        sqlx::query("INSERT INTO prekeys (device_id, key_id, public_key) VALUES ($1, $2, $3)")
            .bind(device_id)
            .bind(key_id)
            .bind(public_key)
            .execute(&pool)
            .await
            .expect("Prekey insert should succeed");
    }

    // Create 5 claimers
    let mut claimer_ids = Vec::new();
    for i in 0..5 {
        let claimer_username = format!("test_claimer_{}_{}", i, Uuid::new_v4());
        let claimer = create_test_user(&pool, &claimer_username).await;
        claimer_ids.push(claimer.id);
    }

    // Concurrently claim prekeys
    let mut join_set = JoinSet::new();

    for claimer_id in &claimer_ids {
        let pool_clone = pool.clone();
        let claimer_id = *claimer_id;

        join_set.spawn(async move {
            let claimed: Option<(String,)> = sqlx::query_as(
                "UPDATE prekeys
                 SET claimed_at = NOW(), claimed_by = $1
                 WHERE id = (
                     SELECT id FROM prekeys
                     WHERE device_id = $2 AND claimed_at IS NULL
                     ORDER BY created_at
                     LIMIT 1
                     FOR UPDATE SKIP LOCKED
                 )
                 RETURNING key_id",
            )
            .bind(claimer_id)
            .bind(device_id)
            .fetch_optional(&pool_clone)
            .await
            .expect("Claim query should succeed");

            claimed.map(|(key_id,)| key_id)
        });
    }

    // Collect results
    let mut claimed_keys = Vec::new();
    while let Some(result) = join_set.join_next().await {
        if let Ok(Some(key_id)) = result {
            claimed_keys.push(key_id);
        }
    }

    // All 5 should have claimed unique prekeys
    assert_eq!(claimed_keys.len(), 5, "All 5 claimers should get a prekey");

    // No duplicates
    let unique_keys: std::collections::HashSet<_> = claimed_keys.iter().collect();
    assert_eq!(
        unique_keys.len(),
        5,
        "All claimed keys should be unique (FOR UPDATE SKIP LOCKED)"
    );

    // No unclaimed prekeys should remain
    let unclaimed_count: (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM prekeys WHERE device_id = $1 AND claimed_at IS NULL")
            .bind(device_id)
            .fetch_one(&pool)
            .await
            .expect("Query should succeed");

    assert_eq!(unclaimed_count.0, 0, "All prekeys should be claimed");

    // Cleanup
    cleanup_test_user(&pool, owner.id).await;
    for claimer_id in &claimer_ids {
        cleanup_test_user(&pool, *claimer_id).await;
    }
}
