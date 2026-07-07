//! Integration tests for reaction-role bindings.
//!
//! Run with: `cargo test --test integration reaction_roles`

use axum::body::Body;
use axum::http::{Method, StatusCode};
use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;
use vc_server::permissions::GuildPermissions;

use crate::helpers::{
    add_guild_member, body_to_json, create_channel, create_guild_with_default_role,
    create_test_user, insert_message, TestApp,
};

/// Insert a non-default role with the given permissions/position; return its id.
async fn insert_role(
    pool: &PgPool,
    guild_id: Uuid,
    perms: GuildPermissions,
    position: i32,
) -> Uuid {
    let id = Uuid::now_v7();
    let name = format!("role_{id}");
    sqlx::query(
        "INSERT INTO guild_roles (id, guild_id, name, permissions, position, is_default)
         VALUES ($1, $2, $3, $4, $5, false)",
    )
    .bind(id)
    .bind(guild_id)
    .bind(&name)
    .bind(perms.bits() as i64)
    .bind(position)
    .execute(pool)
    .await
    .expect("insert role");
    id
}

/// Mint an access token for a user via the app config.
fn token_for(app: &TestApp, user_id: Uuid) -> String {
    crate::helpers::generate_access_token(&app.config, user_id)
}

/// Assert a member has (or lacks) a role.
async fn has_role(pool: &PgPool, guild_id: Uuid, user_id: Uuid, role_id: Uuid) -> bool {
    let row: Option<(i32,)> = sqlx::query_as(
        "SELECT 1 FROM guild_member_roles WHERE guild_id = $1 AND user_id = $2 AND role_id = $3",
    )
    .bind(guild_id)
    .bind(user_id)
    .bind(role_id)
    .fetch_optional(pool)
    .await
    .unwrap();
    row.is_some()
}

#[allow(clippy::too_many_arguments)]
async fn insert_binding_row(
    pool: &PgPool,
    guild_id: Uuid,
    channel_id: Uuid,
    message_id: Uuid,
    emoji: &str,
    role_id: Uuid,
    mode: &str,
    group_key: Option<&str>,
) {
    sqlx::query(
        "INSERT INTO reaction_role_bindings
             (guild_id, channel_id, message_id, emoji, role_id, mode, group_key)
         VALUES ($1, $2, $3, $4, $5, $6, $7)",
    )
    .bind(guild_id)
    .bind(channel_id)
    .bind(message_id)
    .bind(emoji)
    .bind(role_id)
    .bind(mode)
    .bind(group_key)
    .execute(pool)
    .await
    .expect("insert binding");
}

#[sqlx::test]
async fn owner_can_create_binding_for_safe_role(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (owner, _) = create_test_user(&pool).await;
    let guild = create_guild_with_default_role(&pool, owner, GuildPermissions::empty()).await;
    let channel = create_channel(&pool, guild, "general").await;
    let msg = insert_message(&pool, channel, owner, "react here").await;
    let role = insert_role(&pool, guild, GuildPermissions::SEND_MESSAGES, 5).await;

    let token = token_for(&app, owner);
    let req = TestApp::request(Method::POST, &format!("/api/guilds/{guild}/reaction-roles"))
        .header("Authorization", format!("Bearer {token}"))
        .header("Content-Type", "application/json")
        .body(Body::from(
            json!({
                "channel_id": channel,
                "message_id": msg,
                "emoji": "🎨",
                "role_id": role,
                "mode": "toggle"
            })
            .to_string(),
        ))
        .unwrap();
    let res = app.oneshot(req).await;
    assert_eq!(res.status(), StatusCode::OK);
    let body = body_to_json(res).await;
    assert_eq!(body["emoji"], "🎨");
    assert_eq!(body["role_id"], role.to_string());
}

#[sqlx::test]
async fn cannot_bind_dangerous_role(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (owner, _) = create_test_user(&pool).await;
    let guild = create_guild_with_default_role(&pool, owner, GuildPermissions::empty()).await;
    let channel = create_channel(&pool, guild, "general").await;
    let msg = insert_message(&pool, channel, owner, "react").await;
    let role = insert_role(&pool, guild, GuildPermissions::BAN_MEMBERS, 5).await;

    let token = token_for(&app, owner);
    let req = TestApp::request(Method::POST, &format!("/api/guilds/{guild}/reaction-roles"))
        .header("Authorization", format!("Bearer {token}"))
        .header("Content-Type", "application/json")
        .body(Body::from(
            json!({ "channel_id": channel, "message_id": msg, "emoji": "🔨", "role_id": role })
                .to_string(),
        ))
        .unwrap();
    let res = app.oneshot(req).await;
    assert_eq!(res.status(), StatusCode::FORBIDDEN);
    let body = body_to_json(res).await;
    assert_eq!(body["error"], "ROLE_NOT_SELF_ASSIGNABLE");
}

#[sqlx::test]
async fn non_manager_cannot_create_binding(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (owner, _) = create_test_user(&pool).await;
    let (member, _) = create_test_user(&pool).await;
    let guild = create_guild_with_default_role(&pool, owner, GuildPermissions::empty()).await;
    add_guild_member(&pool, guild, member).await;
    let channel = create_channel(&pool, guild, "general").await;
    let msg = insert_message(&pool, channel, owner, "react").await;
    let role = insert_role(&pool, guild, GuildPermissions::SEND_MESSAGES, 5).await;

    let token = token_for(&app, member);
    let req = TestApp::request(Method::POST, &format!("/api/guilds/{guild}/reaction-roles"))
        .header("Authorization", format!("Bearer {token}"))
        .header("Content-Type", "application/json")
        .body(Body::from(
            json!({ "channel_id": channel, "message_id": msg, "emoji": "🎨", "role_id": role })
                .to_string(),
        ))
        .unwrap();
    let res = app.oneshot(req).await;
    assert_eq!(res.status(), StatusCode::FORBIDDEN);
}

#[sqlx::test]
async fn reaction_does_not_grant_role_escalated_after_binding(pool: PgPool) {
    // TOCTOU guard: a role that was safe when bound but has since gained a
    // dangerous permission must NOT be self-grantable by reacting.
    let app = TestApp::with_pool(pool.clone()).await;
    let (owner, _) = create_test_user(&pool).await;
    let guild = create_guild_with_default_role(
        &pool,
        owner,
        GuildPermissions::ADD_REACTIONS | GuildPermissions::VIEW_CHANNEL,
    )
    .await;
    let (member, _) = create_test_user(&pool).await;
    add_guild_member(&pool, guild, member).await;
    let channel = create_channel(&pool, guild, "general").await;
    let msg = insert_message(&pool, channel, owner, "react").await;
    // Bound while safe.
    let role = insert_role(&pool, guild, GuildPermissions::SEND_MESSAGES, 5).await;
    insert_binding_row(&pool, guild, channel, msg, "🎨", role, "toggle", None).await;

    // Later escalated to carry a dangerous permission.
    sqlx::query("UPDATE guild_roles SET permissions = $1 WHERE id = $2")
        .bind(GuildPermissions::MANAGE_GUILD.bits() as i64)
        .bind(role)
        .execute(&pool)
        .await
        .expect("escalate role");

    let token = token_for(&app, member);
    let put = TestApp::request(
        Method::PUT,
        &format!("/api/channels/{channel}/messages/{msg}/reactions"),
    )
    .header("Authorization", format!("Bearer {token}"))
    .header("Content-Type", "application/json")
    .body(Body::from(json!({ "emoji": "🎨" }).to_string()))
    .unwrap();
    // Reaction itself succeeds, but the role is NOT granted.
    let _ = app.oneshot(put).await;
    assert!(
        !has_role(&pool, guild, member, role).await,
        "escalated role must not be self-granted"
    );
}

#[sqlx::test]
async fn toggle_reaction_grants_and_revokes_role(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (owner, _) = create_test_user(&pool).await;
    // @everyone needs ADD_REACTIONS + VIEW_CHANNEL so the member may react.
    let guild = create_guild_with_default_role(
        &pool,
        owner,
        GuildPermissions::ADD_REACTIONS | GuildPermissions::VIEW_CHANNEL,
    )
    .await;
    let (member, _) = create_test_user(&pool).await;
    add_guild_member(&pool, guild, member).await;
    let channel = create_channel(&pool, guild, "general").await;
    let msg = insert_message(&pool, channel, owner, "react").await;
    let role = insert_role(&pool, guild, GuildPermissions::SEND_MESSAGES, 5).await;
    insert_binding_row(&pool, guild, channel, msg, "🎨", role, "toggle", None).await;

    let token = token_for(&app, member);

    // React → role granted.
    let put = TestApp::request(
        Method::PUT,
        &format!("/api/channels/{channel}/messages/{msg}/reactions"),
    )
    .header("Authorization", format!("Bearer {token}"))
    .header("Content-Type", "application/json")
    .body(Body::from(json!({ "emoji": "🎨" }).to_string()))
    .unwrap();
    assert_eq!(app.oneshot(put).await.status(), StatusCode::CREATED);
    assert!(
        has_role(&pool, guild, member, role).await,
        "role granted on react"
    );

    // Un-react → role revoked.
    let del = TestApp::request(
        Method::DELETE,
        &format!("/api/channels/{channel}/messages/{msg}/reactions/🎨"),
    )
    .header("Authorization", format!("Bearer {token}"))
    .body(Body::empty())
    .unwrap();
    assert_eq!(app.oneshot(del).await.status(), StatusCode::NO_CONTENT);
    assert!(
        !has_role(&pool, guild, member, role).await,
        "role revoked on un-react"
    );
}

#[sqlx::test]
async fn unique_group_swaps_roles(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (owner, _) = create_test_user(&pool).await;
    let guild = create_guild_with_default_role(
        &pool,
        owner,
        GuildPermissions::ADD_REACTIONS | GuildPermissions::VIEW_CHANNEL,
    )
    .await;
    let (member, _) = create_test_user(&pool).await;
    add_guild_member(&pool, guild, member).await;
    let channel = create_channel(&pool, guild, "general").await;
    let msg = insert_message(&pool, channel, owner, "pick a color").await;
    let red = insert_role(&pool, guild, GuildPermissions::empty(), 5).await;
    let blue = insert_role(&pool, guild, GuildPermissions::empty(), 6).await;
    insert_binding_row(
        &pool,
        guild,
        channel,
        msg,
        "🔴",
        red,
        "unique",
        Some("color"),
    )
    .await;
    insert_binding_row(
        &pool,
        guild,
        channel,
        msg,
        "🔵",
        blue,
        "unique",
        Some("color"),
    )
    .await;

    let token = token_for(&app, member);
    let react = |emoji: &str| {
        TestApp::request(
            Method::PUT,
            &format!("/api/channels/{channel}/messages/{msg}/reactions"),
        )
        .header("Authorization", format!("Bearer {token}"))
        .header("Content-Type", "application/json")
        .body(Body::from(json!({ "emoji": emoji }).to_string()))
        .unwrap()
    };

    app.oneshot(react("🔴")).await;
    assert!(has_role(&pool, guild, member, red).await);

    app.oneshot(react("🔵")).await;
    assert!(
        has_role(&pool, guild, member, blue).await,
        "new group role granted"
    );
    assert!(
        !has_role(&pool, guild, member, red).await,
        "old group role revoked"
    );

    // The losing reaction row was cleared.
    let red_react: Option<(i32,)> = sqlx::query_as(
        "SELECT 1 FROM message_reactions WHERE message_id = $1 AND user_id = $2 AND emoji = '🔴'",
    )
    .bind(msg)
    .bind(member)
    .fetch_optional(&pool)
    .await
    .unwrap();
    assert!(red_react.is_none(), "losing reaction cleared");
}

#[sqlx::test]
async fn non_member_reaction_does_not_grant(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (owner, _) = create_test_user(&pool).await;
    let guild = create_guild_with_default_role(
        &pool,
        owner,
        GuildPermissions::ADD_REACTIONS | GuildPermissions::VIEW_CHANNEL,
    )
    .await;
    let channel = create_channel(&pool, guild, "general").await;
    let msg = insert_message(&pool, channel, owner, "react").await;
    let role = insert_role(&pool, guild, GuildPermissions::empty(), 5).await;
    insert_binding_row(&pool, guild, channel, msg, "🎨", role, "toggle", None).await;

    // A user who is NOT a guild member: the hook's is_member guard is the
    // backstop — no role grant regardless of whether the endpoint lets the
    // reaction through.
    let (outsider, _) = create_test_user(&pool).await;
    let token = token_for(&app, outsider);
    let put = TestApp::request(
        Method::PUT,
        &format!("/api/channels/{channel}/messages/{msg}/reactions"),
    )
    .header("Authorization", format!("Bearer {token}"))
    .header("Content-Type", "application/json")
    .body(Body::from(json!({ "emoji": "🎨" }).to_string()))
    .unwrap();
    let _ = app.oneshot(put).await;
    assert!(
        !has_role(&pool, guild, outsider, role).await,
        "non-member never granted"
    );
}

#[sqlx::test]
async fn admin_assign_role_persists(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (owner, _) = create_test_user(&pool).await;
    let guild = create_guild_with_default_role(&pool, owner, GuildPermissions::empty()).await;
    let (member, _) = create_test_user(&pool).await;
    add_guild_member(&pool, guild, member).await;
    let role = insert_role(&pool, guild, GuildPermissions::SEND_MESSAGES, 5).await;

    let token = token_for(&app, owner);
    let req = TestApp::request(
        Method::POST,
        &format!("/api/guilds/{guild}/members/{member}/roles/{role}"),
    )
    .header("Authorization", format!("Bearer {token}"))
    .body(Body::empty())
    .unwrap();
    assert_eq!(app.oneshot(req).await.status(), StatusCode::OK);
    assert!(has_role(&pool, guild, member, role).await);
}
