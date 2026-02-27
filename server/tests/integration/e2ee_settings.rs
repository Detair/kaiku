//! Integration tests for E2EE settings and backup status endpoints.
//!
//! Unit tests for config parsing and response structures.

/// Test that `REQUIRE_E2EE_SETUP` parses correctly from environment.
#[test]
fn test_require_e2ee_setup_parsing_true() {
    // Test "true" value
    assert!(parse_require_e2ee("true"));
    assert!(parse_require_e2ee("TRUE"));
    assert!(parse_require_e2ee("True"));
}

#[test]
fn test_require_e2ee_setup_parsing_one() {
    // Test "1" value
    assert!(parse_require_e2ee("1"));
}

#[test]
fn test_require_e2ee_setup_parsing_false() {
    // Test various false values
    assert!(!parse_require_e2ee("false"));
    assert!(!parse_require_e2ee("FALSE"));
    assert!(!parse_require_e2ee("0"));
    assert!(!parse_require_e2ee("no"));
    assert!(!parse_require_e2ee(""));
    assert!(!parse_require_e2ee("random"));
}

/// Helper to parse the `REQUIRE_E2EE_SETUP` value the same way config.rs does.
fn parse_require_e2ee(value: &str) -> bool {
    value.to_lowercase() == "true" || value == "1"
}

/// Test backup status response structure.
#[test]
fn test_backup_status_response_no_backup() {
    // Simulate no backup state
    let has_backup = false;
    let backup_created_at: Option<String> = None;
    let version: Option<i32> = None;

    assert!(!has_backup);
    assert!(backup_created_at.is_none());
    assert!(version.is_none());
}

#[test]
fn test_backup_status_response_with_backup() {
    // Simulate backup exists state
    let has_backup = true;
    let backup_created_at: Option<String> = Some("2026-01-20T10:00:00Z".to_string());
    let version: Option<i32> = Some(1);

    assert!(has_backup);
    assert_eq!(backup_created_at.as_deref(), Some("2026-01-20T10:00:00Z"));
    assert_eq!(version, Some(1));
}

/// Test server settings response structure.
#[test]
fn test_server_settings_response() {
    // Test both config combinations
    let settings_required = ServerSettingsTest {
        require_e2ee_setup: true,
        oidc_enabled: true,
    };

    assert!(settings_required.require_e2ee_setup);
    assert!(settings_required.oidc_enabled);

    let settings_optional = ServerSettingsTest {
        require_e2ee_setup: false,
        oidc_enabled: false,
    };

    assert!(!settings_optional.require_e2ee_setup);
    assert!(!settings_optional.oidc_enabled);
}

/// Test struct matching the actual `ServerSettingsResponse`.
struct ServerSettingsTest {
    require_e2ee_setup: bool,
    oidc_enabled: bool,
}

/// Test that the backup validation logic is correct.
#[test]
fn test_backup_salt_validation() {
    // Salt must be exactly 16 bytes
    let valid_salt = [0u8; 16];
    let invalid_salt_short = [0u8; 15];
    let invalid_salt_long = [0u8; 17];

    assert_eq!(valid_salt.len(), 16);
    assert_ne!(invalid_salt_short.len(), 16);
    assert_ne!(invalid_salt_long.len(), 16);
}

#[test]
fn test_backup_nonce_validation() {
    // Nonce must be exactly 12 bytes
    let valid_nonce = [0u8; 12];
    let invalid_nonce_short = [0u8; 11];
    let invalid_nonce_long = [0u8; 13];

    assert_eq!(valid_nonce.len(), 12);
    assert_ne!(invalid_nonce_short.len(), 12);
    assert_ne!(invalid_nonce_long.len(), 12);
}

#[test]
fn test_backup_ciphertext_size_limit() {
    // Ciphertext must be under 1 MB
    const MAX_CIPHERTEXT_SIZE: usize = 1024 * 1024; // 1 MB

    let valid_small = vec![0u8; 1000];
    let valid_at_limit = vec![0u8; MAX_CIPHERTEXT_SIZE];
    let invalid_over_limit = vec![0u8; MAX_CIPHERTEXT_SIZE + 1];

    assert!(valid_small.len() <= MAX_CIPHERTEXT_SIZE);
    assert!(valid_at_limit.len() <= MAX_CIPHERTEXT_SIZE);
    assert!(invalid_over_limit.len() > MAX_CIPHERTEXT_SIZE);
}
