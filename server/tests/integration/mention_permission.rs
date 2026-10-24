//! Integration tests for @everyone/@here mention permissions.

use axum::body::Body;
use axum::http::Method;
use sqlx::PgPool;
use uuid::Uuid;
use vc_server::db::{self, ChannelType};
use vc_server::permissions::GuildPermissions;

use crate::helpers::{
    add_guild_member, create_channel, create_guild_with_default_role, create_test_user,
    generate_access_token, TestApp,
};

/// Test that users without `MENTION_EVERYONE` permission cannot use @everyone or @here.
#[sqlx::test]
async fn test_mention_everyone_blocked_without_permission(pool: PgPool) -> sqlx::Result<()> {
    // Create guild owner
    let owner = db::create_user(
        &pool,
        "owner",
        "Owner",
        Some("owner@example.com"),
        "password123",
    )
    .await?;

    // Create regular test user (not owner)
    let user = db::create_user(
        &pool,
        "testuser",
        "Test User",
        Some("testuser@example.com"),
        "password123",
    )
    .await?;

    // Create guild with owner
    let guild_id = Uuid::new_v4();
    sqlx::query("INSERT INTO guilds (id, name, owner_id) VALUES ($1, $2, $3)")
        .bind(guild_id)
        .bind("Test Guild")
        .bind(owner.id)
        .execute(&pool)
        .await?;

    // Add regular user as member (not owner)
    sqlx::query("INSERT INTO guild_members (guild_id, user_id) VALUES ($1, $2)")
        .bind(guild_id)
        .bind(user.id)
        .execute(&pool)
        .await?;

    // Create @everyone role without MENTION_EVERYONE permission
    let everyone_perms = GuildPermissions::SEND_MESSAGES | GuildPermissions::VOICE_CONNECT;
    sqlx::query(
        "INSERT INTO guild_roles (id, guild_id, name, permissions, position, is_default)
         VALUES ($1, $2, $3, $4, $5, $6)",
    )
    .bind(Uuid::new_v4())
    .bind(guild_id)
    .bind("@everyone")
    .bind(everyone_perms.to_db())
    .bind(999)
    .bind(true)
    .execute(&pool)
    .await?;

    // Create text channel
    let channel_id = Uuid::new_v4();
    sqlx::query("INSERT INTO channels (id, name, channel_type, guild_id) VALUES ($1, $2, $3, $4)")
        .bind(channel_id)
        .bind("test-channel")
        .bind(ChannelType::Text)
        .bind(guild_id)
        .execute(&pool)
        .await?;

    // Verify user doesn't have MENTION_EVERYONE permission
    let ctx = vc_server::permissions::get_member_permission_context(&pool, guild_id, user.id)
        .await?
        .expect("User should be guild member");

    assert!(
        !ctx.has_permission(GuildPermissions::MENTION_EVERYONE),
        "User should not have MENTION_EVERYONE permission"
    );

    Ok(())
}

/// Test that users with `MENTION_EVERYONE` permission can use @everyone and @here.
#[sqlx::test]
async fn test_mention_everyone_allowed_with_permission(pool: PgPool) -> sqlx::Result<()> {
    // Create test user
    let user = db::create_user(
        &pool,
        "moderator",
        "Moderator",
        Some("mod@example.com"),
        "password123",
    )
    .await?;

    // Create guild
    let guild_id = Uuid::new_v4();
    sqlx::query("INSERT INTO guilds (id, name, owner_id) VALUES ($1, $2, $3)")
        .bind(guild_id)
        .bind("Test Guild")
        .bind(user.id)
        .execute(&pool)
        .await?;

    // Add user as member
    sqlx::query("INSERT INTO guild_members (guild_id, user_id) VALUES ($1, $2)")
        .bind(guild_id)
        .bind(user.id)
        .execute(&pool)
        .await?;

    // Create @everyone role without MENTION_EVERYONE
    let everyone_perms = GuildPermissions::SEND_MESSAGES | GuildPermissions::VOICE_CONNECT;
    sqlx::query(
        "INSERT INTO guild_roles (id, guild_id, name, permissions, position, is_default)
         VALUES ($1, $2, $3, $4, $5, $6)",
    )
    .bind(Uuid::new_v4())
    .bind(guild_id)
    .bind("@everyone")
    .bind(everyone_perms.to_db())
    .bind(999)
    .bind(false)
    .execute(&pool)
    .await?;

    // Create moderator role with MENTION_EVERYONE permission
    let mod_role_id = Uuid::new_v4();
    let mod_perms = GuildPermissions::SEND_MESSAGES
        | GuildPermissions::VOICE_CONNECT
        | GuildPermissions::MENTION_EVERYONE;

    sqlx::query(
        "INSERT INTO guild_roles (id, guild_id, name, permissions, position, is_default)
         VALUES ($1, $2, $3, $4, $5, $6)",
    )
    .bind(mod_role_id)
    .bind(guild_id)
    .bind("Moderator")
    .bind(mod_perms.to_db())
    .bind(1)
    .bind(false)
    .execute(&pool)
    .await?;

    // Assign moderator role to user
    sqlx::query("INSERT INTO guild_member_roles (guild_id, user_id, role_id) VALUES ($1, $2, $3)")
        .bind(guild_id)
        .bind(user.id)
        .bind(mod_role_id)
        .execute(&pool)
        .await?;

    // Verify user has MENTION_EVERYONE permission
    let ctx = vc_server::permissions::get_member_permission_context(&pool, guild_id, user.id)
        .await?
        .expect("User should be guild member");

    assert!(
        ctx.has_permission(GuildPermissions::MENTION_EVERYONE),
        "Moderator should have MENTION_EVERYONE permission"
    );

    Ok(())
}

/// Test that guild owner always has `MENTION_EVERYONE` permission.
#[sqlx::test]
async fn test_guild_owner_can_mention_everyone(pool: PgPool) -> sqlx::Result<()> {
    // Create test user
    let owner = db::create_user(
        &pool,
        "owner",
        "Guild Owner",
        Some("owner@example.com"),
        "password123",
    )
    .await?;

    // Create guild
    let guild_id = Uuid::new_v4();
    sqlx::query("INSERT INTO guilds (id, name, owner_id) VALUES ($1, $2, $3)")
        .bind(guild_id)
        .bind("Test Guild")
        .bind(owner.id)
        .execute(&pool)
        .await?;

    // Add owner as member
    sqlx::query("INSERT INTO guild_members (guild_id, user_id) VALUES ($1, $2)")
        .bind(guild_id)
        .bind(owner.id)
        .execute(&pool)
        .await?;

    // Create @everyone role without MENTION_EVERYONE
    let everyone_perms = GuildPermissions::SEND_MESSAGES | GuildPermissions::VOICE_CONNECT;
    sqlx::query(
        "INSERT INTO guild_roles (id, guild_id, name, permissions, position, is_default)
         VALUES ($1, $2, $3, $4, $5, $6)",
    )
    .bind(Uuid::new_v4())
    .bind(guild_id)
    .bind("@everyone")
    .bind(everyone_perms.to_db())
    .bind(999)
    .bind(true)
    .execute(&pool)
    .await?;

    // Verify owner has all permissions including MENTION_EVERYONE
    let ctx = vc_server::permissions::get_member_permission_context(&pool, guild_id, owner.id)
        .await?
        .expect("Owner should be guild member");

    assert!(ctx.is_owner, "User should be guild owner");
    assert!(
        ctx.has_permission(GuildPermissions::MENTION_EVERYONE),
        "Owner should have MENTION_EVERYONE permission"
    );
    assert_eq!(
        ctx.computed_permissions,
        GuildPermissions::all(),
        "Owner should have all permissions"
    );

    Ok(())
}

/// A channel override that denies `MENTION_EVERYONE` wins over the guild-level
/// role grant on the send path; other channels keep the grant.
#[sqlx::test]
async fn test_channel_deny_overrides_guild_mention_everyone(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (owner, _) = create_test_user(&pool).await;
    let (member, _) = create_test_user(&pool).await;
    let guild_id = create_guild_with_default_role(
        &pool,
        owner,
        GuildPermissions::VIEW_CHANNEL | GuildPermissions::SEND_MESSAGES,
    )
    .await;
    add_guild_member(&pool, guild_id, member).await;

    let role_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO guild_roles (id, guild_id, name, permissions, position)
         VALUES ($1, $2, 'announcer', $3, 1)",
    )
    .bind(role_id)
    .bind(guild_id)
    .bind(GuildPermissions::MENTION_EVERYONE.to_db())
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query("INSERT INTO guild_member_roles (guild_id, user_id, role_id) VALUES ($1, $2, $3)")
        .bind(guild_id)
        .bind(member)
        .bind(role_id)
        .execute(&pool)
        .await
        .unwrap();

    let quiet = create_channel(&pool, guild_id, "quiet").await;
    let open = create_channel(&pool, guild_id, "open").await;
    sqlx::query(
        "INSERT INTO channel_overrides (id, channel_id, role_id, allow_permissions, deny_permissions)
         VALUES ($1, $2, $3, 0, $4)",
    )
    .bind(Uuid::new_v4())
    .bind(quiet)
    .bind(role_id)
    .bind(GuildPermissions::MENTION_EVERYONE.to_db())
    .execute(&pool)
    .await
    .unwrap();

    let token = generate_access_token(&app.config, member);
    let send = |channel_id: Uuid| {
        TestApp::request(Method::POST, &format!("/api/messages/channel/{channel_id}"))
            .header("Authorization", format!("Bearer {token}"))
            .header("Content-Type", "application/json")
            .body(Body::from(r#"{"content":"@everyone dinner"}"#))
            .unwrap()
    };

    let denied = app.oneshot(send(quiet)).await;
    assert_eq!(
        denied.status(),
        400,
        "channel deny of MENTION_EVERYONE must reject @everyone"
    );
    let allowed = app.oneshot(send(open)).await;
    assert_eq!(
        allowed.status(),
        201,
        "guild-level MENTION_EVERYONE still applies in other channels"
    );
}
