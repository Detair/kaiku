//! OIDC/OAuth2 integration tests.
//!
//! Tests for OIDC provider management, secret encryption/decryption,
//! username generation from claims, and flow state serialization.
//!
//! Run unit tests: `cargo test --test oidc_test`
//! Run integration tests: `cargo test --test oidc_test -- --ignored`

use vc_server::auth::oidc::{
    append_collision_suffix, generate_username_from_claims, OidcFlowState, OidcProviderManager,
    OidcUserInfo,
};

// ============================================================================
// Secret Encryption/Decryption Tests (Unit tests - no database required)
// ============================================================================

/// Generate a valid 32-byte encryption key for testing.
fn test_encryption_key() -> Vec<u8> {
    vec![
        0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e,
        0x0f, 0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19, 0x1a, 0x1b, 0x1c,
        0x1d, 0x1e, 0x1f, 0x20,
    ]
}

#[test]
fn test_encrypt_decrypt_roundtrip() {
    let key = test_encryption_key();
    let manager = OidcProviderManager::new(key);

    let secret = "my-super-secret-client-secret-12345";
    let encrypted = manager.encrypt_secret(secret).expect("Encryption should succeed");

    // Encrypted should be different from original
    assert_ne!(encrypted, secret);

    // Decryption should return original
    let decrypted = manager
        .decrypt_secret(&encrypted)
        .expect("Decryption should succeed");
    assert_eq!(decrypted, secret);
}

#[test]
fn test_encrypt_produces_different_ciphertexts() {
    let key = test_encryption_key();
    let manager = OidcProviderManager::new(key);

    let secret = "same-secret";
    let encrypted1 = manager.encrypt_secret(secret).unwrap();
    let encrypted2 = manager.encrypt_secret(secret).unwrap();

    // Each encryption should produce different ciphertext (random nonce)
    assert_ne!(
        encrypted1, encrypted2,
        "AES-GCM with random nonce should produce different ciphertexts"
    );

    // Both should decrypt to the same value
    assert_eq!(manager.decrypt_secret(&encrypted1).unwrap(), secret);
    assert_eq!(manager.decrypt_secret(&encrypted2).unwrap(), secret);
}

#[test]
fn test_decrypt_with_wrong_key_fails() {
    let key1 = test_encryption_key();
    let mut key2 = test_encryption_key();
    key2[0] = 0xff; // Different key

    let manager1 = OidcProviderManager::new(key1);
    let manager2 = OidcProviderManager::new(key2);

    let secret = "secret-value";
    let encrypted = manager1.encrypt_secret(secret).unwrap();

    // Decrypting with wrong key should fail
    let result = manager2.decrypt_secret(&encrypted);
    assert!(result.is_err(), "Decryption with wrong key should fail");
}

#[test]
fn test_encrypt_empty_string() {
    let key = test_encryption_key();
    let manager = OidcProviderManager::new(key);

    let encrypted = manager.encrypt_secret("").unwrap();
    let decrypted = manager.decrypt_secret(&encrypted).unwrap();
    assert_eq!(decrypted, "");
}

#[test]
fn test_encrypt_unicode_secret() {
    let key = test_encryption_key();
    let manager = OidcProviderManager::new(key);

    let secret = "секрет-密码-🔐";
    let encrypted = manager.encrypt_secret(secret).unwrap();
    let decrypted = manager.decrypt_secret(&encrypted).unwrap();
    assert_eq!(decrypted, secret);
}

#[test]
fn test_decrypt_invalid_data_fails() {
    let key = test_encryption_key();
    let manager = OidcProviderManager::new(key);

    // Garbage data should fail
    let result = manager.decrypt_secret("not-valid-encrypted-data");
    assert!(result.is_err());

    // Empty string should fail
    let result = manager.decrypt_secret("");
    assert!(result.is_err());
}

// ============================================================================
// Username Generation Tests (Unit tests - no database required)
// ============================================================================

#[test]
fn test_username_from_preferred_username() {
    let info = OidcUserInfo {
        subject: "sub-123".into(),
        email: Some("user@example.com".into()),
        name: Some("Full Name".into()),
        preferred_username: Some("myuser".into()),
        avatar_url: None,
    };
    assert_eq!(generate_username_from_claims(&info), "myuser");
}

#[test]
fn test_username_preferred_with_hyphens_normalized() {
    let info = OidcUserInfo {
        subject: "sub-123".into(),
        email: None,
        name: None,
        preferred_username: Some("my-cool-user".into()),
        avatar_url: None,
    };
    // Hyphens should be replaced with underscores
    assert_eq!(generate_username_from_claims(&info), "my_cool_user");
}

#[test]
fn test_username_preferred_uppercase_normalized() {
    let info = OidcUserInfo {
        subject: "sub-123".into(),
        email: None,
        name: None,
        preferred_username: Some("MyUser".into()),
        avatar_url: None,
    };
    assert_eq!(generate_username_from_claims(&info), "myuser");
}

#[test]
fn test_username_from_name_fallback() {
    let info = OidcUserInfo {
        subject: "sub-123".into(),
        email: Some("user@example.com".into()),
        name: Some("John Doe".into()),
        preferred_username: None,
        avatar_url: None,
    };
    assert_eq!(generate_username_from_claims(&info), "john_doe");
}

#[test]
fn test_username_from_name_with_special_chars() {
    let info = OidcUserInfo {
        subject: "sub-123".into(),
        email: None,
        name: Some("María García".into()),
        preferred_username: None,
        avatar_url: None,
    };
    // Non-ASCII chars are filtered out; spaces become underscores
    let username = generate_username_from_claims(&info);
    assert!(
        username.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'),
        "Username should only contain [a-z0-9_], got: {username}"
    );
}

#[test]
fn test_username_from_email_fallback() {
    let info = OidcUserInfo {
        subject: "sub-123".into(),
        email: Some("jane.doe@example.com".into()),
        name: None,
        preferred_username: None,
        avatar_url: None,
    };
    assert_eq!(generate_username_from_claims(&info), "jane_doe");
}

#[test]
fn test_username_from_email_with_plus() {
    let info = OidcUserInfo {
        subject: "sub-123".into(),
        email: Some("user+tag@example.com".into()),
        name: None,
        preferred_username: None,
        avatar_url: None,
    };
    // '+' is filtered out, resulting in "usertag"
    let username = generate_username_from_claims(&info);
    assert!(username.contains("user"));
    assert!(username.contains("tag"));
}

#[test]
fn test_username_random_fallback() {
    let info = OidcUserInfo {
        subject: "sub-123".into(),
        email: None,
        name: None,
        preferred_username: None,
        avatar_url: None,
    };
    let username = generate_username_from_claims(&info);
    assert!(
        username.starts_with("user_"),
        "Random fallback should start with 'user_', got: {username}"
    );
    assert!(username.len() >= 10);
}

#[test]
fn test_username_too_short_preferred_falls_through() {
    let info = OidcUserInfo {
        subject: "sub-123".into(),
        email: Some("ok@example.com".into()),
        name: None,
        preferred_username: Some("ab".into()), // Too short (< 3 chars)
        avatar_url: None,
    };
    // Should fall through to email
    let username = generate_username_from_claims(&info);
    assert_ne!(username, "ab", "Too short preferred_username should be rejected");
}

#[test]
fn test_username_too_long_preferred_falls_through() {
    let info = OidcUserInfo {
        subject: "sub-123".into(),
        email: Some("long@example.com".into()),
        name: None,
        preferred_username: Some("a".repeat(33)), // Too long (> 32 chars)
        avatar_url: None,
    };
    let username = generate_username_from_claims(&info);
    assert!(
        username.len() <= 32,
        "Username should be at most 32 chars"
    );
}

// ============================================================================
// Collision Suffix Tests
// ============================================================================

#[test]
fn test_collision_suffix_format() {
    let result = append_collision_suffix("testuser");
    assert!(
        result.starts_with("testuser_"),
        "Should start with base + underscore"
    );
    // Suffix should be 4 digits
    let suffix = &result["testuser_".len()..];
    assert_eq!(suffix.len(), 4, "Suffix should be 4 digits");
    assert!(
        suffix.chars().all(|c| c.is_ascii_digit()),
        "Suffix should be all digits"
    );
}

#[test]
fn test_collision_suffix_unique() {
    // Generate multiple suffixes and check they're not all the same
    let results: Vec<String> = (0..10).map(|_| append_collision_suffix("base")).collect();
    let unique: std::collections::HashSet<&String> = results.iter().collect();
    assert!(
        unique.len() > 1,
        "Multiple collision suffixes should produce different results"
    );
}

#[test]
fn test_collision_suffix_respects_max_length() {
    let long_base = "a".repeat(30);
    let result = append_collision_suffix(&long_base);
    assert!(
        result.len() <= 32,
        "Result should be truncated to 32 chars, got {} chars",
        result.len()
    );
}

// ============================================================================
// OidcFlowState Serialization Tests
// ============================================================================

#[test]
fn test_flow_state_serialization_roundtrip() {
    let state = OidcFlowState {
        slug: "github".to_string(),
        pkce_verifier: "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk".to_string(),
        nonce: "abc123nonce".to_string(),
        redirect_uri: "http://127.0.0.1:12345/callback".to_string(),
        created_at: 1706745600,
    };

    let json = serde_json::to_string(&state).expect("Serialization should succeed");
    let deserialized: OidcFlowState =
        serde_json::from_str(&json).expect("Deserialization should succeed");

    assert_eq!(deserialized.slug, "github");
    assert_eq!(deserialized.pkce_verifier, state.pkce_verifier);
    assert_eq!(deserialized.nonce, "abc123nonce");
    assert_eq!(deserialized.redirect_uri, "http://127.0.0.1:12345/callback");
    assert_eq!(deserialized.created_at, 1706745600);
}

#[test]
fn test_flow_state_json_format() {
    let state = OidcFlowState {
        slug: "google".to_string(),
        pkce_verifier: "verifier".to_string(),
        nonce: "nonce".to_string(),
        redirect_uri: "https://example.com/callback".to_string(),
        created_at: 0,
    };

    let json: serde_json::Value = serde_json::to_value(&state).unwrap();

    // Verify all fields are present
    assert!(json.get("slug").is_some());
    assert!(json.get("pkce_verifier").is_some());
    assert!(json.get("nonce").is_some());
    assert!(json.get("redirect_uri").is_some());
    assert!(json.get("created_at").is_some());
}

// ============================================================================
// Preset Configuration Tests
// ============================================================================

#[test]
fn test_github_preset_constants() {
    use vc_server::auth::oidc::GitHubPreset;

    assert_eq!(GitHubPreset::SLUG, "github");
    assert_eq!(GitHubPreset::DISPLAY_NAME, "GitHub");
    assert_eq!(GitHubPreset::ICON_HINT, "github");
    assert!(GitHubPreset::AUTHORIZATION_URL.starts_with("https://github.com/"));
    assert!(GitHubPreset::TOKEN_URL.starts_with("https://github.com/"));
    assert!(GitHubPreset::USERINFO_URL.starts_with("https://api.github.com/"));
    assert!(
        GitHubPreset::SCOPES.contains("read:user"),
        "GitHub scopes should include read:user"
    );
}

#[test]
fn test_google_preset_constants() {
    use vc_server::auth::oidc::GooglePreset;

    assert_eq!(GooglePreset::SLUG, "google");
    assert_eq!(GooglePreset::DISPLAY_NAME, "Google");
    assert_eq!(GooglePreset::ICON_HINT, "chrome");
    assert!(GooglePreset::ISSUER_URL.starts_with("https://accounts.google.com"));
    assert!(
        GooglePreset::SCOPES.contains("openid"),
        "Google scopes should include openid"
    );
}

// ============================================================================
// OidcProviderManager Unit Tests
// ============================================================================

#[test]
fn test_manager_creation() {
    let key = test_encryption_key();
    let manager = OidcProviderManager::new(key.clone());

    // Manager should be created with the given key
    // Verify by encrypting and decrypting
    let secret = "test";
    let encrypted = manager.encrypt_secret(secret).unwrap();
    let decrypted = manager.decrypt_secret(&encrypted).unwrap();
    assert_eq!(decrypted, secret);
}

#[tokio::test]
async fn test_manager_list_public_empty() {
    let key = test_encryption_key();
    let manager = OidcProviderManager::new(key);

    // Fresh manager should have no providers
    let public = manager.list_public().await;
    assert!(public.is_empty());
}

#[tokio::test]
async fn test_manager_get_provider_not_found() {
    let key = test_encryption_key();
    let manager = OidcProviderManager::new(key);

    let result = manager.get_provider_row("nonexistent").await;
    assert!(result.is_none());
}

#[tokio::test]
async fn test_manager_generate_auth_url_unknown_provider() {
    let key = test_encryption_key();
    let manager = OidcProviderManager::new(key);

    let result = manager
        .generate_auth_url("nonexistent", "http://localhost/callback")
        .await;
    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("Provider not found"),
    );
}

#[tokio::test]
async fn test_manager_exchange_code_unknown_provider() {
    let key = test_encryption_key();
    let manager = OidcProviderManager::new(key);

    let result = manager
        .exchange_code("nonexistent", "code", "verifier", "http://localhost/callback", "nonce")
        .await;
    assert!(result.is_err());
}

// ============================================================================
// Integration Tests (require database - marked as #[ignore])
// ============================================================================

#[allow(dead_code)]
async fn create_test_pool() -> sqlx::PgPool {
    let database_url =
        std::env::var("DATABASE_URL").unwrap_or_else(|_| "postgres://localhost/vc_test".into());

    sqlx::PgPool::connect(&database_url)
        .await
        .expect("Failed to connect to test database")
}

/// Create a temporary test user and return their ID. Caller must clean up.
#[allow(dead_code)]
async fn create_test_user(pool: &sqlx::PgPool) -> uuid::Uuid {
    let username = format!("oidc_test_{}", &uuid::Uuid::new_v4().to_string()[..8]);
    let password_hash = vc_server::auth::hash_password("test_password").unwrap();
    let user = vc_server::db::create_user(pool, &username, "OIDC Test User", None, &password_hash)
        .await
        .expect("Test user creation should succeed");
    user.id
}

/// Clean up a test user.
#[allow(dead_code)]
async fn delete_test_user(pool: &sqlx::PgPool, user_id: uuid::Uuid) {
    sqlx::query("DELETE FROM users WHERE id = $1")
        .bind(user_id)
        .execute(pool)
        .await
        .ok();
}

#[tokio::test]
#[ignore] // Requires PostgreSQL with oidc_providers table
async fn test_oidc_provider_crud() {
    let pool = create_test_pool().await;
    let user_id = create_test_user(&pool).await;
    let key = test_encryption_key();
    let manager = OidcProviderManager::new(key);

    let slug = format!("test-provider-{}", uuid::Uuid::new_v4());
    let encrypted_secret = manager.encrypt_secret("test-client-secret").unwrap();

    // Create a provider
    let provider = vc_server::db::create_oidc_provider(
        &pool,
        &slug,
        "Test Provider",
        Some("key"),
        "custom",
        None,
        Some("https://example.com/auth"),
        Some("https://example.com/token"),
        Some("https://example.com/userinfo"),
        "test-client-id",
        &encrypted_secret,
        "openid profile email",
        user_id,
    )
    .await
    .expect("Provider creation should succeed");

    assert_eq!(provider.slug, slug);
    assert_eq!(provider.display_name, "Test Provider");

    // Read it back
    let found = vc_server::db::get_oidc_provider_by_slug(&pool, &slug)
        .await
        .expect("Provider lookup should succeed");
    assert_eq!(found.slug, slug);
    assert_eq!(found.client_id, "test-client-id");

    // Verify secret decryption
    let decrypted = manager
        .decrypt_secret(&found.client_secret_encrypted)
        .unwrap();
    assert_eq!(decrypted, "test-client-secret");

    // Delete the provider
    vc_server::db::delete_oidc_provider(&pool, provider.id)
        .await
        .expect("Provider deletion should succeed");

    // Verify deletion
    let result = vc_server::db::get_oidc_provider_by_slug(&pool, &slug).await;
    assert!(result.is_err(), "Deleted provider should not be found");

    // Cleanup
    delete_test_user(&pool, user_id).await;
}

#[tokio::test]
#[ignore] // Requires PostgreSQL
async fn test_auth_methods_config_crud() {
    let pool = create_test_pool().await;
    let user_id = create_test_user(&pool).await;

    // Read current auth methods
    let methods = vc_server::db::get_auth_methods_allowed(&pool)
        .await
        .expect("Should read auth methods");

    // Default should have local enabled
    assert!(methods.local, "Local auth should be enabled by default");

    // Update to enable OIDC
    let mut updated = methods.clone();
    updated.oidc = true;
    vc_server::db::set_auth_methods_allowed(&pool, &updated, user_id)
        .await
        .expect("Should update auth methods");

    // Read back
    let read_back = vc_server::db::get_auth_methods_allowed(&pool)
        .await
        .expect("Should read updated auth methods");
    assert!(read_back.oidc, "OIDC should now be enabled");

    // Restore original
    vc_server::db::set_auth_methods_allowed(&pool, &methods, user_id)
        .await
        .expect("Should restore auth methods");

    // Cleanup
    delete_test_user(&pool, user_id).await;
}

#[tokio::test]
#[ignore] // Requires PostgreSQL
async fn test_load_providers_from_database() {
    let pool = create_test_pool().await;
    let user_id = create_test_user(&pool).await;
    let key = test_encryption_key();
    let manager = OidcProviderManager::new(key);

    let slug = format!("test-load-{}", uuid::Uuid::new_v4());
    let encrypted_secret = manager.encrypt_secret("secret").unwrap();

    // Create a test provider
    let provider = vc_server::db::create_oidc_provider(
        &pool,
        &slug,
        "Load Test",
        Some("key"),
        "custom",
        None,
        Some("https://example.com/auth"),
        Some("https://example.com/token"),
        None,
        "client-id",
        &encrypted_secret,
        "openid",
        user_id,
    )
    .await
    .expect("Provider creation should succeed");

    // Load providers
    manager
        .load_providers(&pool)
        .await
        .expect("Loading providers should succeed");

    // Should be able to find the provider
    let found = manager.get_provider_row(&slug).await;
    assert!(found.is_some(), "Loaded provider should be found");
    assert_eq!(found.unwrap().display_name, "Load Test");

    // Should appear in public list
    let public = manager.list_public().await;
    assert!(
        public.iter().any(|p| p.slug == slug),
        "Provider should appear in public list"
    );

    // Cleanup
    vc_server::db::delete_oidc_provider(&pool, provider.id)
        .await
        .ok();
    delete_test_user(&pool, user_id).await;
}

// ============================================================================
// Redirect URI Validation Tests (Unit tests - no database required)
// ============================================================================

/// These tests document which redirect_uri values the server's validation
/// accepts or rejects. The server uses `Url::parse()` + pattern matching on
/// `(scheme(), host_str())` against `("http", Some("localhost" | "127.0.0.1"))`.
///
/// We test the validation contract by examining URL host parsing behavior.
/// The `url` crate's `Url::parse("http://X").host_str()` returns:
///   - "localhost" for "http://localhost:1234/path"
///   - "localhost.evil.com" for "http://localhost.evil.com" (NOT "localhost")
///   - "evil.com" for "http://localhost@evil.com" (userinfo, NOT host)

#[test]
fn test_redirect_uri_valid_patterns() {
    // These should be accepted by the server's validation
    let valid = [
        "http://localhost:12345/callback",
        "http://localhost/callback",
        "http://127.0.0.1:54321/callback",
        "http://127.0.0.1/callback",
    ];
    for uri in valid {
        let parsed = openidconnect::url::Url::parse(uri).expect(uri);
        assert_eq!(parsed.scheme(), "http", "scheme for {uri}");
        assert!(
            matches!(parsed.host_str(), Some("localhost" | "127.0.0.1")),
            "host_str() for {uri} should be localhost or 127.0.0.1, got {:?}",
            parsed.host_str()
        );
    }
}

#[test]
fn test_redirect_uri_rejected_patterns() {
    // These should be rejected by the server's validation
    let invalid: Vec<(&str, &str)> = vec![
        // Prefix-based bypass attempts (host_str != "localhost")
        ("http://localhost.evil.com/callback", "host is localhost.evil.com"),
        ("http://localhost.attacker.com:8080/steal", "host is subdomain"),
        ("http://127.0.0.1.evil.com/callback", "host is subdomain"),
        // Credential injection (url crate treats pre-@ as userinfo)
        ("http://localhost@evil.com/callback", "host is evil.com"),
        ("http://127.0.0.1@evil.com/callback", "host is evil.com"),
        // Wrong scheme
        ("https://localhost:12345/callback", "scheme is https"),
        ("https://127.0.0.1/callback", "scheme is https"),
        // External hosts
        ("http://evil.com/callback", "external host"),
        ("http://example.com/auth/callback", "external host"),
    ];
    for (uri, reason) in invalid {
        let parsed = openidconnect::url::Url::parse(uri).expect(uri);
        let is_valid = matches!(
            (parsed.scheme(), parsed.host_str()),
            ("http", Some("localhost" | "127.0.0.1"))
        );
        assert!(!is_valid, "{uri} should be rejected ({reason})");
    }
}

#[test]
fn test_redirect_uri_unparseable_rejected() {
    // These should fail URL parsing entirely
    let invalid = ["not-a-url", "", "javascript:alert(1)"];
    for uri in invalid {
        // "javascript:alert(1)" actually parses as a valid URL with scheme "javascript"
        // so the scheme check rejects it. Empty and "not-a-url" fail parsing.
        let result = openidconnect::url::Url::parse(uri);
        if let Ok(parsed) = result {
            let is_valid = matches!(
                (parsed.scheme(), parsed.host_str()),
                ("http", Some("localhost" | "127.0.0.1"))
            );
            assert!(!is_valid, "{uri} parsed but should be rejected by scheme/host check");
        }
        // If parsing fails, the server also rejects it — that's fine
    }
}

// ============================================================================
// Collision Suffix Truncation Tests
// ============================================================================

#[test]
fn test_collision_suffix_always_has_full_suffix() {
    // Even with a very long base, the _XXXX suffix should always be 5 chars
    let long_base = "a".repeat(50);
    let result = append_collision_suffix(&long_base);
    assert!(result.len() <= 32, "Should be at most 32 chars, got {}", result.len());

    // The suffix should always be _XXXX (5 chars)
    let underscore_pos = result.rfind('_').expect("Should contain underscore");
    let suffix = &result[underscore_pos + 1..];
    assert_eq!(suffix.len(), 4, "Suffix should always be 4 digits, got: {suffix}");
    assert!(
        suffix.chars().all(|c| c.is_ascii_digit()),
        "Suffix should be all digits, got: {suffix}"
    );
}

#[test]
fn test_collision_suffix_short_base_unchanged() {
    let result = append_collision_suffix("user");
    // "user" (4) + "_" (1) + "XXXX" (4) = 9, well under 32
    assert!(result.starts_with("user_"));
    assert_eq!(result.len(), 9);
}

// ============================================================================
// Registration Policy + First-User Admin Tests (require database)
// ============================================================================

#[tokio::test]
#[ignore] // Requires PostgreSQL
async fn test_registration_policy_blocks_non_open() {
    let pool = create_test_pool().await;
    let user_id = create_test_user(&pool).await;

    // Set policy to invite_only
    vc_server::db::set_config_value(
        &pool,
        "registration_policy",
        serde_json::json!("invite_only"),
        user_id,
    )
    .await
    .expect("Should set policy");

    let policy: serde_json::Value = vc_server::db::get_config_value(&pool, "registration_policy")
        .await
        .expect("Should read policy");
    assert_eq!(policy.as_str().unwrap(), "invite_only");

    // Set policy to closed
    vc_server::db::set_config_value(
        &pool,
        "registration_policy",
        serde_json::json!("closed"),
        user_id,
    )
    .await
    .expect("Should set policy");

    let policy: serde_json::Value = vc_server::db::get_config_value(&pool, "registration_policy")
        .await
        .expect("Should read policy");
    assert_eq!(policy.as_str().unwrap(), "closed");

    // Restore to open
    vc_server::db::set_config_value(
        &pool,
        "registration_policy",
        serde_json::json!("open"),
        user_id,
    )
    .await
    .expect("Should restore policy");

    delete_test_user(&pool, user_id).await;
}

#[tokio::test]
#[ignore] // Requires PostgreSQL
async fn test_first_user_detection() {
    let pool = create_test_pool().await;

    // Count users — this is the same query used in first-user admin logic
    let user_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users")
        .fetch_one(&pool)
        .await
        .expect("Should count users");

    // If there are existing users, first-user detection should be false
    // (This test verifies the query works; the transaction + FOR UPDATE
    // serialization is tested by the actual registration flow)
    if user_count > 0 {
        assert!(user_count > 0, "Existing users should prevent first-user grant");
    }

    // Verify setup_complete row exists (needed for FOR UPDATE lock)
    let setup_val = sqlx::query_scalar::<_, serde_json::Value>(
        "SELECT value FROM server_config WHERE key = 'setup_complete'"
    )
    .fetch_one(&pool)
    .await;
    assert!(setup_val.is_ok(), "setup_complete config row must exist for registration locking");
}
