//! Security integration tests for role management.
//!
//! Run with: `cargo test --test integration roles_security`

use sqlx::PgPool;
use vc_server::permissions::GuildPermissions;

#[sqlx::test]
async fn test_cannot_grant_dangerous_permissions_to_everyone(_pool: PgPool) {
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

#[sqlx::test]
async fn migration_strips_forbidden_bits_from_default_roles_only(pool: PgPool) -> sqlx::Result<()> {
    let owner = vc_server::db::create_user(&pool, "owner", "Owner", None, "password123")
        .await
        .expect("create user");
    let guild_id = uuid::Uuid::new_v4();
    sqlx::query("INSERT INTO guilds (id, name, owner_id) VALUES ($1, $2, $3)")
        .bind(guild_id)
        .bind("Test Guild")
        .bind(owner.id)
        .execute(&pool)
        .await?;

    let july = GuildPermissions::MANAGE_POSTS
        | GuildPermissions::SEND_ANNOUNCEMENTS
        | GuildPermissions::MANAGE_EVENTS
        | GuildPermissions::MANAGE_WEBHOOKS;
    let base = GuildPermissions::SEND_MESSAGES | GuildPermissions::VOICE_CONNECT;
    let everyone_id = uuid::Uuid::new_v4();
    let custom_id = uuid::Uuid::new_v4();
    for (id, name, is_default) in [
        (everyone_id, "@everyone", true),
        (custom_id, "Helpers", false),
    ] {
        sqlx::query(
            "INSERT INTO guild_roles (id, guild_id, name, permissions, position, is_default)
             VALUES ($1, $2, $3, $4, 0, $5)",
        )
        .bind(id)
        .bind(guild_id)
        .bind(name)
        .bind((base | july).to_db())
        .bind(is_default)
        .execute(&pool)
        .await?;
    }

    // Run the migration SQL (twice: it must be idempotent).
    let sql = include_str!("../../migrations/20261110000100_strip_everyone_forbidden.sql");
    sqlx::raw_sql(sql).execute(&pool).await?;
    sqlx::raw_sql(sql).execute(&pool).await?;

    let perms = |id| {
        let pool = pool.clone();
        async move {
            sqlx::query_scalar::<_, i64>("SELECT permissions FROM guild_roles WHERE id = $1")
                .bind(id)
                .fetch_one(&pool)
                .await
        }
    };
    assert_eq!(perms(everyone_id).await?, base.to_db());
    assert_eq!(perms(custom_id).await?, (base | july).to_db());
    Ok(())
}
