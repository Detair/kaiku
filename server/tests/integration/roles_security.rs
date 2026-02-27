//! Security integration tests for role management.
//!
//! Run with: `cargo test --test integration roles_security -- --ignored`

use sqlx::PgPool;
use vc_server::permissions::GuildPermissions;

/// Helper to create a test database pool.
async fn create_test_pool() -> PgPool {
    let database_url =
        std::env::var("DATABASE_URL").unwrap_or_else(|_| "postgres://localhost/vc_test".into());

    PgPool::connect(&database_url)
        .await
        .expect("Failed to connect to test database")
}

#[tokio::test]
#[ignore] // Requires PostgreSQL
async fn test_cannot_grant_dangerous_permissions_to_everyone() {
    let _pool = create_test_pool().await;

    // 1. Setup: Create a guild and get the @everyone role
    // This part requires recreating some of the app state and handler logic
    // For this test to be truly effective as a regression test, it should use the actual API
    // handler or simulate the database state that the handler interacts with.

    // Since we can't easily spin up the full Axum app here without more scaffolding,
    // we will simulate the Logic check by verifying the GuildPermissions method returns false
    // for the dangerous permissions we care about.

    let dangerous_perms = GuildPermissions::MANAGE_GUILD | GuildPermissions::BAN_MEMBERS;
    assert!(
        !dangerous_perms.validate_for_everyone(),
        "Dangerous permissions must fail validation"
    );

    // To truly test the handler regression, we would need to:
    // 1. Insert a guild
    // 2. Insert the @everyone role
    // 3. Call the update_role handler
    // 4. Verify it returns error

    // For now, this test serves as a documentation of the security requirement.
}

#[test]
fn test_everyone_role_validation_logic() {
    // This is the core logic used by the handler
    let dangerous_combinations = [
        GuildPermissions::MANAGE_GUILD,
        GuildPermissions::MANAGE_ROLES,
        GuildPermissions::BAN_MEMBERS,
        GuildPermissions::KICK_MEMBERS,
        GuildPermissions::MANAGE_CHANNELS,
        GuildPermissions::VIEW_AUDIT_LOG,
    ];

    for perm in dangerous_combinations {
        // Construct a permission set containing this dangerous permission
        let perms = GuildPermissions::SEND_MESSAGES | perm;

        // Assert it fails validation
        assert!(
            !perms.validate_for_everyone(),
            "Permission {perm:?} should be forbidden for @everyone"
        );
    }

    // Assert safe permissions pass
    let safe_perms = GuildPermissions::SEND_MESSAGES | GuildPermissions::VOICE_CONNECT;
    assert!(
        safe_perms.validate_for_everyone(),
        "Safe permissions should pass validation"
    );
}
