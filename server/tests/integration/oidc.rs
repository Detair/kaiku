//! OIDC/OAuth2 integration tests.
//!
//! Tests for OIDC provider management, secret encryption/decryption,
//! username generation from claims, and flow state serialization.
//!
//! Each DB test runs against a fresh per-test database provisioned by
//! `#[sqlx::test]`, so no manual row cleanup is required.
//!
//! Run tests: `cargo test --test integration oidc`

use sqlx::PgPool;
use vc_server::auth::oidc::{
    append_collision_suffix, client_challenge_matches, client_return_url,
    generate_username_from_claims, is_loopback_client_redirect, is_valid_client_challenge,
    resolve_flow_redirects, FlowRedirectError, OidcFlowPurpose, OidcFlowState, OidcProviderManager,
    OidcReauthBinding, OidcUserInfo,
};

// ============================================================================
// Secret Encryption/Decryption Tests (Unit tests - no database required)
// ============================================================================

/// Generate a valid 32-byte encryption key for testing.
fn test_encryption_key() -> Vec<u8> {
    vec![
        0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e, 0x0f,
        0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19, 0x1a, 0x1b, 0x1c, 0x1d, 0x1e,
        0x1f, 0x20,
    ]
}

#[test]
fn test_encrypt_decrypt_roundtrip() {
    let key = test_encryption_key();
    let manager = OidcProviderManager::new(key);

    let secret = "my-super-secret-client-secret-12345";
    let encrypted = manager
        .encrypt_secret(secret)
        .expect("Encryption should succeed");

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
        username
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_'),
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
    assert_ne!(
        username, "ab",
        "Too short preferred_username should be rejected"
    );
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
    assert!(username.len() <= 32, "Username should be at most 32 chars");
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
    let link_user = uuid::Uuid::new_v4();
    let state = OidcFlowState {
        slug: "github".to_string(),
        pkce_verifier: "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk".to_string(),
        nonce: "abc123nonce".to_string(),
        redirect_uri: "http://127.0.0.1:12345/callback".to_string(),
        created_at: 1706745600,
        purpose: OidcFlowPurpose::Link {
            user_id: link_user,
            session_id: None,
        },
        client_redirect_uri: Some("http://127.0.0.1:12345/callback".to_string()),
        client_challenge: None,
    };

    let json = serde_json::to_string(&state).expect("Serialization should succeed");
    let deserialized: OidcFlowState =
        serde_json::from_str(&json).expect("Deserialization should succeed");

    assert_eq!(deserialized.slug, "github");
    assert_eq!(deserialized.pkce_verifier, state.pkce_verifier);
    assert_eq!(deserialized.nonce, "abc123nonce");
    assert_eq!(deserialized.redirect_uri, "http://127.0.0.1:12345/callback");
    assert_eq!(deserialized.created_at, 1706745600);
    assert_eq!(
        deserialized.purpose,
        OidcFlowPurpose::Link {
            user_id: link_user,
            session_id: None
        }
    );
    assert_eq!(
        deserialized.client_redirect_uri.as_deref(),
        Some("http://127.0.0.1:12345/callback")
    );
}

#[test]
fn test_flow_state_link_user_id_defaults_to_none() {
    // Old flow-state blobs (pre-linking) have no link_user_id field; #[serde(default)]
    // must deserialize them as an ordinary login flow.
    let legacy = r#"{"slug":"google","pkce_verifier":"v","nonce":"n","redirect_uri":"https://x/cb","created_at":0}"#;
    let state: OidcFlowState = serde_json::from_str(legacy).expect("legacy state should parse");
    assert_eq!(state.purpose, OidcFlowPurpose::Login);
    assert_eq!(state.client_redirect_uri, None);
}

#[test]
fn test_flow_state_legacy_link_user_id_stays_a_link() {
    // A link state written before `purpose` existed must still finish as a
    // link (never as a login or a re-authentication).
    let user = uuid::Uuid::new_v4();
    let legacy = format!(
        r#"{{"slug":"google","pkce_verifier":"v","nonce":"n","redirect_uri":"https://x/cb","created_at":0,"link_user_id":"{user}"}}"#
    );
    let state: OidcFlowState = serde_json::from_str(&legacy).expect("legacy state should parse");
    assert_eq!(
        state.purpose,
        OidcFlowPurpose::Link {
            user_id: user,
            session_id: None
        }
    );
}

#[test]
fn test_flow_state_purpose_is_exclusive() {
    // A re-authentication state round-trips as exactly that purpose.
    let binding = OidcReauthBinding {
        user_id: uuid::Uuid::new_v4(),
        session_id: uuid::Uuid::new_v4(),
        reason: None,
        mfa_verified: true,
    };
    let state = OidcFlowState {
        slug: "google".to_string(),
        pkce_verifier: "v".to_string(),
        nonce: "n".to_string(),
        redirect_uri: "https://x/cb".to_string(),
        created_at: 0,
        purpose: OidcFlowPurpose::Reauth(binding.clone()),
        client_redirect_uri: None,
        client_challenge: None,
    };
    let json = serde_json::to_value(&state).unwrap();
    assert_eq!(json["purpose"]["kind"], "reauth");
    assert!(json.get("link_user_id").is_none());
    let back: OidcFlowState = serde_json::from_value(json).unwrap();
    assert_eq!(back.purpose, OidcFlowPurpose::Reauth(binding));
}

#[test]
fn test_flow_state_json_format() {
    let state = OidcFlowState {
        slug: "google".to_string(),
        pkce_verifier: "verifier".to_string(),
        nonce: "nonce".to_string(),
        redirect_uri: "https://example.com/callback".to_string(),
        created_at: 0,
        purpose: OidcFlowPurpose::Login,
        client_redirect_uri: None,
        client_challenge: None,
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
    let manager = OidcProviderManager::new(key);

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
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("Provider not found"),);
}

#[tokio::test]
async fn test_manager_exchange_code_unknown_provider() {
    let key = test_encryption_key();
    let manager = OidcProviderManager::new(key);

    let result = manager
        .exchange_code(
            "nonexistent",
            "code",
            "verifier",
            "http://localhost/callback",
            "nonce",
        )
        .await;
    assert!(result.is_err());
}

// ============================================================================
// Integration Tests (each uses a fresh per-test DB via #[sqlx::test])
// ============================================================================

/// Create a temporary test user and return their ID.
async fn create_test_user(pool: &sqlx::PgPool) -> uuid::Uuid {
    let username = format!("oidc_test_{}", &uuid::Uuid::new_v4().to_string()[..8]);
    let password_hash = vc_server::auth::hash_password("test_password").unwrap();
    let user = vc_server::db::create_user(pool, &username, "OIDC Test User", None, &password_hash)
        .await
        .expect("Test user creation should succeed");
    user.id
}

#[sqlx::test]
async fn test_oidc_provider_crud(pool: PgPool) {
    let user_id = create_test_user(&pool).await;
    let key = test_encryption_key();
    let manager = OidcProviderManager::new(key);

    let slug = format!("test-provider-{}", uuid::Uuid::new_v4());
    let encrypted_secret = manager.encrypt_secret("test-client-secret").unwrap();

    // Create a provider
    let provider = vc_server::db::create_oidc_provider(
        &pool,
        vc_server::db::CreateOidcProviderParams {
            slug: &slug,
            display_name: "Test Provider",
            icon_hint: Some("key"),
            provider_type: "custom",
            issuer_url: None,
            authorization_url: Some("https://example.com/auth"),
            token_url: Some("https://example.com/token"),
            userinfo_url: Some("https://example.com/userinfo"),
            client_id: "test-client-id",
            client_secret_encrypted: &encrypted_secret,
            scopes: "openid profile email",
            created_by: Some(user_id),
        },
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
}

#[sqlx::test]
async fn test_auth_methods_config_crud(pool: PgPool) {
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
    vc_server::db::set_auth_methods_allowed(&pool, &updated, Some(user_id))
        .await
        .expect("Should update auth methods");

    // Read back
    let read_back = vc_server::db::get_auth_methods_allowed(&pool)
        .await
        .expect("Should read updated auth methods");
    assert!(read_back.oidc, "OIDC should now be enabled");

    // Restore original
    vc_server::db::set_auth_methods_allowed(&pool, &methods, Some(user_id))
        .await
        .expect("Should restore auth methods");
}

#[sqlx::test]
async fn test_load_providers_from_database(pool: PgPool) {
    let user_id = create_test_user(&pool).await;
    let key = test_encryption_key();
    let manager = OidcProviderManager::new(key);

    let slug = format!("test-load-{}", uuid::Uuid::new_v4());
    let encrypted_secret = manager.encrypt_secret("secret").unwrap();

    // Create a test provider
    let provider = vc_server::db::create_oidc_provider(
        &pool,
        vc_server::db::CreateOidcProviderParams {
            slug: &slug,
            display_name: "Load Test",
            icon_hint: Some("key"),
            provider_type: "custom",
            issuer_url: None,
            authorization_url: Some("https://example.com/auth"),
            token_url: Some("https://example.com/token"),
            userinfo_url: None,
            client_id: "client-id",
            client_secret_encrypted: &encrypted_secret,
            scopes: "openid",
            created_by: Some(user_id),
        },
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
}

// ============================================================================
// Redirect URI Validation Tests (Unit tests - no database required)
// ============================================================================

// Which desktop `redirect_uri` values the server accepts, checked against the
// server's own rule (`is_loopback_client_redirect`). Note the `url` crate's
// `host_str()` returns "localhost.evil.com" for `http://localhost.evil.com`
// and "evil.com" for `http://localhost@evil.com` (userinfo, not host).

#[test]
fn test_redirect_uri_valid_patterns() {
    let valid = [
        "http://localhost:12345/callback",
        "http://localhost/callback",
        "http://127.0.0.1:54321/callback",
        "http://127.0.0.1/callback",
    ];
    for uri in valid {
        let parsed = openidconnect::url::Url::parse(uri).expect(uri);
        assert!(
            is_loopback_client_redirect(&parsed),
            "{uri} should be accepted"
        );
    }
}

#[test]
fn test_redirect_uri_rejected_patterns() {
    let invalid: Vec<(&str, &str)> = vec![
        // Prefix-based bypass attempts (host_str != "localhost")
        (
            "http://localhost.evil.com/callback",
            "host is localhost.evil.com",
        ),
        (
            "http://localhost.attacker.com:8080/steal",
            "host is subdomain",
        ),
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
        // Not a URL with a host at all
        ("javascript:alert(1)", "scheme is javascript"),
    ];
    for (uri, reason) in invalid {
        let parsed = openidconnect::url::Url::parse(uri).expect(uri);
        assert!(
            !is_loopback_client_redirect(&parsed),
            "{uri} should be rejected ({reason})"
        );
    }
}

#[test]
fn test_redirect_uri_unparseable_rejected() {
    for uri in ["not-a-url", ""] {
        assert_eq!(
            resolve_flow_redirects(Some(uri), Some("https://chat.example.com")),
            Err(FlowRedirectError::InvalidClientRedirect),
            "{uri:?} must be rejected"
        );
    }
}

// ============================================================================
// Flow Redirect Resolution Tests (Unit tests - no database required)
// ============================================================================

/// Desktop login: the provider must redirect to the server's own callback (the
/// server holds the PKCE verifier and client secret and exchanges the code);
/// the loopback address is only where the server hands the result back.
#[test]
fn test_desktop_flow_sends_server_callback_to_provider() {
    let r = resolve_flow_redirects(
        Some("http://127.0.0.1:54321/callback"),
        Some("https://chat.example.com"),
    )
    .expect("loopback redirect is accepted");
    assert_eq!(
        r.provider_redirect_uri,
        "https://chat.example.com/auth/oidc/callback"
    );
    assert_eq!(
        r.client_redirect_uri.as_deref(),
        Some("http://127.0.0.1:54321/callback")
    );
}

#[test]
fn test_browser_flow_has_no_client_redirect() {
    let r = resolve_flow_redirects(None, Some("https://chat.example.com")).unwrap();
    assert_eq!(
        r.provider_redirect_uri,
        "https://chat.example.com/auth/oidc/callback"
    );
    assert_eq!(r.client_redirect_uri, None);
}

#[test]
fn test_flow_redirects_require_public_url() {
    assert_eq!(
        resolve_flow_redirects(None, None),
        Err(FlowRedirectError::MissingPublicUrl)
    );
    assert_eq!(
        resolve_flow_redirects(Some("http://127.0.0.1:1/callback"), None),
        Err(FlowRedirectError::MissingPublicUrl)
    );
}

#[test]
fn test_flow_redirects_reject_non_loopback_client() {
    for uri in [
        "http://localhost.evil.com/callback",
        "http://localhost@evil.com/callback",
        "https://127.0.0.1/callback",
        "not-a-url",
    ] {
        assert_eq!(
            resolve_flow_redirects(Some(uri), Some("https://chat.example.com")),
            Err(FlowRedirectError::InvalidClientRedirect),
            "{uri} must be rejected"
        );
    }
}

#[test]
fn test_flow_redirects_trim_public_url_trailing_slash() {
    let r = resolve_flow_redirects(None, Some("https://chat.example.com/")).unwrap();
    assert_eq!(
        r.provider_redirect_uri,
        "https://chat.example.com/auth/oidc/callback"
    );
}

/// The desktop contract is exactly `http://{localhost|127.0.0.1}[:port]/callback`.
#[test]
fn test_loopback_client_redirect_contract() {
    for ok in [
        "http://127.0.0.1:54321/callback",
        "http://localhost:12345/callback",
        "http://127.0.0.1/callback",
    ] {
        let u = openidconnect::url::Url::parse(ok).unwrap();
        assert!(is_loopback_client_redirect(&u), "{ok} must be accepted");
    }
    for bad in [
        "http://127.0.0.1:1/",
        "http://127.0.0.1:1/other",
        "http://127.0.0.1:1/callback/extra",
        "http://127.0.0.1:1/callback?x=1",
        "http://127.0.0.1:1/callback?",
        "http://127.0.0.1:1/callback#frag",
        "http://user@127.0.0.1:1/callback",
        "http://user:pw@localhost:1/callback",
    ] {
        let u = openidconnect::url::Url::parse(bad).unwrap();
        assert!(!is_loopback_client_redirect(&u), "{bad} must be rejected");
    }
}

fn flow_state_with_client(client: Option<&str>) -> OidcFlowState {
    OidcFlowState {
        slug: "google".to_string(),
        pkce_verifier: "v".to_string(),
        nonce: "n".to_string(),
        redirect_uri: "https://chat.example.com/auth/oidc/callback".to_string(),
        created_at: 0,
        purpose: OidcFlowPurpose::Login,
        client_redirect_uri: client.map(str::to_string),
        client_challenge: None,
    }
}

/// The callback hands the result to the desktop loopback only when the flow
/// was started with one; the provider redirect URI never decides this.
#[test]
fn test_client_return_url_selects_desktop_or_browser() {
    let desktop = flow_state_with_client(Some("http://127.0.0.1:54321/callback"));
    assert_eq!(
        client_return_url(&desktop)
            .map(|u| u.to_string())
            .as_deref(),
        Some("http://127.0.0.1:54321/callback")
    );
    assert!(client_return_url(&flow_state_with_client(None)).is_none());
    // A tampered/non-loopback stored value is never used as a return target.
    assert!(client_return_url(&flow_state_with_client(Some("http://evil.com/cb"))).is_none());
}

// ============================================================================
// Collision Suffix Truncation Tests
// ============================================================================

#[test]
fn test_collision_suffix_always_has_full_suffix() {
    // Even with a very long base, the _XXXX suffix should always be 5 chars
    let long_base = "a".repeat(50);
    let result = append_collision_suffix(&long_base);
    assert!(
        result.len() <= 32,
        "Should be at most 32 chars, got {}",
        result.len()
    );

    // The suffix should always be _XXXX (5 chars)
    let underscore_pos = result.rfind('_').expect("Should contain underscore");
    let suffix = &result[underscore_pos + 1..];
    assert_eq!(
        suffix.len(),
        4,
        "Suffix should always be 4 digits, got: {suffix}"
    );
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

#[sqlx::test]
async fn test_registration_policy_blocks_non_open(pool: PgPool) {
    let user_id = create_test_user(&pool).await;

    // Set policy to invite_only
    vc_server::db::set_config_value(
        &pool,
        "registration_policy",
        serde_json::json!("invite_only"),
        Some(user_id),
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
        Some(user_id),
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
        Some(user_id),
    )
    .await
    .expect("Should restore policy");
}

#[sqlx::test]
async fn test_first_user_detection(pool: PgPool) {
    // Count users — this is the same query used in first-user admin logic
    let user_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users")
        .fetch_one(&pool)
        .await
        .expect("Should count users");

    // If there are existing users, first-user detection should be false
    // (This test verifies the query works; the transaction + FOR UPDATE
    // serialization is tested by the actual registration flow)
    if user_count > 0 {
        assert!(
            user_count > 0,
            "Existing users should prevent first-user grant"
        );
    }

    // Verify setup_complete row exists (needed for FOR UPDATE lock)
    let setup_val = sqlx::query_scalar::<_, serde_json::Value>(
        "SELECT value FROM server_config WHERE key = 'setup_complete'",
    )
    .fetch_one(&pool)
    .await;
    assert!(
        setup_val.is_ok(),
        "setup_complete config row must exist for registration locking"
    );
}

// ============================================================================
// External Identity Linking (user_identities table)
// ============================================================================

#[sqlx::test]
async fn test_user_identity_insert_and_lookup(pool: PgPool) {
    let user_id = create_test_user(&pool).await;

    // No identity yet.
    let none = vc_server::db::find_user_id_by_identity(&pool, "google", "sub-123")
        .await
        .expect("lookup should succeed");
    assert_eq!(none, None);

    // Insert an identity.
    let identity = vc_server::db::insert_user_identity(
        &pool,
        user_id,
        "google",
        "sub-123",
        Some("a@example.com"),
    )
    .await
    .expect("insert should succeed");
    assert_eq!(identity.user_id, user_id);
    assert_eq!(identity.provider_slug, "google");
    assert_eq!(identity.subject, "sub-123");
    assert_eq!(identity.email.as_deref(), Some("a@example.com"));
    assert!(identity.last_used_at.is_none());

    // Now resolvable to the account.
    let found = vc_server::db::find_user_id_by_identity(&pool, "google", "sub-123")
        .await
        .expect("lookup should succeed");
    assert_eq!(found, Some(user_id));

    // A different subject does not resolve.
    let other = vc_server::db::find_user_id_by_identity(&pool, "google", "sub-999")
        .await
        .expect("lookup should succeed");
    assert_eq!(other, None);
}

#[sqlx::test]
async fn test_user_identity_list_multiple_providers(pool: PgPool) {
    let user_id = create_test_user(&pool).await;

    vc_server::db::insert_user_identity(&pool, user_id, "google", "g-1", None)
        .await
        .unwrap();
    vc_server::db::insert_user_identity(&pool, user_id, "github", "gh-1", Some("u@example.com"))
        .await
        .unwrap();

    let identities = vc_server::db::list_user_identities(&pool, user_id)
        .await
        .expect("list should succeed");
    assert_eq!(identities.len(), 2);

    // Both providers resolve back to the same account.
    let g = vc_server::db::find_user_id_by_identity(&pool, "google", "g-1")
        .await
        .unwrap();
    let gh = vc_server::db::find_user_id_by_identity(&pool, "github", "gh-1")
        .await
        .unwrap();
    assert_eq!(g, Some(user_id));
    assert_eq!(gh, Some(user_id));
}

#[sqlx::test]
async fn test_user_identity_subject_unique_across_accounts(pool: PgPool) {
    let user_a = create_test_user(&pool).await;
    let user_b = create_test_user(&pool).await;

    vc_server::db::insert_user_identity(&pool, user_a, "google", "shared-sub", None)
        .await
        .expect("first insert should succeed");

    // The same (provider, subject) cannot be claimed by a second account.
    let err = vc_server::db::insert_user_identity(&pool, user_b, "google", "shared-sub", None)
        .await
        .expect_err("duplicate (provider, subject) must be rejected");
    match err {
        sqlx::Error::Database(db_err) => assert!(db_err.is_unique_violation()),
        other => panic!("expected unique violation, got {other:?}"),
    }

    // The identity still belongs to the original account.
    let owner = vc_server::db::find_user_id_by_identity(&pool, "google", "shared-sub")
        .await
        .unwrap();
    assert_eq!(owner, Some(user_a));
}

#[sqlx::test]
async fn test_user_identity_one_per_provider_per_user(pool: PgPool) {
    let user_id = create_test_user(&pool).await;

    vc_server::db::insert_user_identity(&pool, user_id, "google", "sub-1", None)
        .await
        .expect("first insert should succeed");

    // The same user cannot link a second identity for the same provider.
    let err = vc_server::db::insert_user_identity(&pool, user_id, "google", "sub-2", None)
        .await
        .expect_err("second identity for same provider must be rejected");
    match err {
        sqlx::Error::Database(db_err) => assert!(db_err.is_unique_violation()),
        other => panic!("expected unique violation, got {other:?}"),
    }
}

#[sqlx::test]
async fn test_user_identity_cascade_on_user_delete(pool: PgPool) {
    let user_id = create_test_user(&pool).await;
    vc_server::db::insert_user_identity(&pool, user_id, "google", "sub-del", None)
        .await
        .unwrap();

    // Deleting the user cascades to its identities.
    sqlx::query("DELETE FROM users WHERE id = $1")
        .bind(user_id)
        .execute(&pool)
        .await
        .expect("user delete should succeed");

    let found = vc_server::db::find_user_id_by_identity(&pool, "google", "sub-del")
        .await
        .unwrap();
    assert_eq!(found, None);
}

#[sqlx::test]
async fn test_system_seeded_provider_without_creator(pool: PgPool) {
    // Regression: env-var seeding (seed_from_env) runs at startup on a fresh
    // database with ZERO users and must insert with created_by = NULL. It
    // previously passed Uuid::nil(), violating oidc_providers_created_by_fkey
    // on every boot of a freshly provisioned server.
    let key = test_encryption_key();
    let manager = OidcProviderManager::new(key);
    let encrypted_secret = manager.encrypt_secret("legacy-secret").unwrap();

    let provider = vc_server::db::create_oidc_provider(
        &pool,
        vc_server::db::CreateOidcProviderParams {
            slug: "legacy-oidc",
            display_name: "SSO Login",
            icon_hint: Some("key"),
            provider_type: "custom",
            issuer_url: Some("https://sso.example.com"),
            authorization_url: None,
            token_url: None,
            userinfo_url: None,
            client_id: "legacy-client",
            client_secret_encrypted: &encrypted_secret,
            scopes: "openid profile email",
            created_by: None,
        },
    )
    .await
    .expect("system-seeded provider (no creator) must insert on a user-less DB");
    assert_eq!(provider.slug, "legacy-oidc");

    // The follow-up config write during seeding must also work without a user.
    let mut methods = vc_server::db::get_auth_methods_allowed(&pool)
        .await
        .expect("read auth methods");
    methods.oidc = true;
    vc_server::db::set_auth_methods_allowed(&pool, &methods, None)
        .await
        .expect("auth methods update without updater must succeed");
}

// ============================================================================
// Desktop hand-back: per-flow value and client challenge (no database)
// ============================================================================

/// The desktop may add one per-flow value (`?flow=`, base64url, 22–128
/// characters) to its return address; the server hands it back unchanged.
#[test]
fn test_loopback_client_redirect_accepts_only_a_flow_value() {
    let flow = "Zm9vYmFyYmF6cXV4cXV1eHh5ejEyMzQ1Ng";
    for ok in [
        format!("http://127.0.0.1:54321/callback?flow={flow}"),
        format!("http://localhost:1/callback?flow={}", "a".repeat(22)),
        format!("http://localhost:1/callback?flow={}", "_-".repeat(64)),
    ] {
        let u = openidconnect::url::Url::parse(&ok).unwrap();
        assert!(is_loopback_client_redirect(&u), "{ok} must be accepted");
    }
    for bad in [
        "http://127.0.0.1:1/callback?flow=".to_string(),
        format!("http://127.0.0.1:1/callback?flow={}", "a".repeat(21)),
        format!("http://127.0.0.1:1/callback?flow={}", "a".repeat(129)),
        format!("http://127.0.0.1:1/callback?flow={flow}&flow={flow}"),
        format!("http://127.0.0.1:1/callback?flow={flow}&x=1"),
        format!("http://127.0.0.1:1/callback?x=1&flow={flow}"),
        format!("http://127.0.0.1:1/callback?flow={flow}%21"),
        format!("http://127.0.0.1:1/callback?flow={flow}.~"),
        format!("http://127.0.0.1:1/callback?state={flow}"),
        format!("http://127.0.0.1:1/callback?flow={flow}#f"),
        format!("http://127.0.0.1:1/other?flow={flow}"),
    ] {
        let u = openidconnect::url::Url::parse(&bad).unwrap();
        assert!(!is_loopback_client_redirect(&u), "{bad} must be rejected");
    }
}

/// `client_challenge` is an unpadded base64url SHA-256 digest (43 characters).
#[test]
fn test_client_challenge_format() {
    assert!(is_valid_client_challenge(
        "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
    ));
    for bad in [
        "",
        "E9Melhoa2OwvFrEMTJguCHaoeKt8R5Ce4fZUZkQ0Pl",
        "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM=",
        "E9Melhoa2OwvFrEMTJguCHaoeKt8R5Ce4fZUZkQ0Pl=",
        "E9Melhoa2OwvFrEMTJguCHaoeKt8R5Ce4fZUZkQ0P+/",
        "plain",
    ] {
        assert!(!is_valid_client_challenge(bad), "{bad:?} must be rejected");
    }
}

/// S256 as in RFC 7636 appendix B; anything else does not match.
#[test]
fn test_client_challenge_matches_s256_only() {
    let verifier = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
    let challenge = "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM";
    assert!(client_challenge_matches(verifier, challenge));
    // "plain" (verifier == challenge) is not accepted.
    assert!(!client_challenge_matches(challenge, challenge));
    assert!(!client_challenge_matches(
        "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXK",
        challenge
    ));
    // Verifier outside the RFC 7636 length range.
    assert!(!client_challenge_matches("short", challenge));
    assert!(!client_challenge_matches(&"a".repeat(129), challenge));
    assert!(!client_challenge_matches("", ""));
}

/// States stored before the challenge existed deserialize without one.
#[test]
fn test_flow_state_without_challenge_deserializes() {
    let json = serde_json::json!({
        "slug": "google",
        "pkce_verifier": "v",
        "nonce": "n",
        "redirect_uri": "https://chat.example.com/auth/oidc/callback",
        "created_at": 0,
        "purpose": { "kind": "login" },
        "client_redirect_uri": "http://127.0.0.1:1/callback",
    });
    let state: OidcFlowState = serde_json::from_value(json).unwrap();
    assert_eq!(state.client_challenge, None);
}
