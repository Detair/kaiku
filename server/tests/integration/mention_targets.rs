//! Integration tests for per-user mention targets (contract C21).
//! Run: `cargo test --test integration mention_targets`

use std::collections::HashSet;

use sqlx::PgPool;
use uuid::Uuid;
use vc_server::chat::mentions::{
    extract_mentions, resolve_mention_targets, MENTION_EVERYONE_PUSH_CAP,
};
use vc_server::db;
use vc_server::permissions::GuildPermissions;
use vc_server::social::block_cache;

use crate::helpers::{
    add_guild_member, create_channel, create_guild_with_default_role, create_test_user, TestApp,
};

struct Fixture {
    app: TestApp,
    author: Uuid,
    guild_id: Uuid,
    channel: db::Channel,
}

async fn fixture(pool: PgPool) -> Fixture {
    let app = TestApp::with_pool(pool.clone()).await;
    let (author, _) = create_test_user(&pool).await;
    let guild_id = create_guild_with_default_role(
        &pool,
        author,
        GuildPermissions::VIEW_CHANNEL | GuildPermissions::SEND_MESSAGES,
    )
    .await;
    let channel_id = create_channel(&pool, guild_id, "general").await;
    let channel = db::find_channel_by_id(&pool, channel_id)
        .await
        .unwrap()
        .expect("channel exists");
    Fixture {
        app,
        author,
        guild_id,
        channel,
    }
}

async fn member(f: &Fixture) -> (Uuid, String) {
    let (id, name) = create_test_user(&f.app.pool).await;
    add_guild_member(&f.app.pool, f.guild_id, id).await;
    (id, name)
}

async fn targets(f: &Fixture, content: &str) -> Vec<Uuid> {
    resolve_mention_targets(
        &f.app.state,
        &f.channel,
        f.author,
        &extract_mentions(content),
    )
    .await
}

/// Give `user_id` a role whose channel override denies `VIEW_CHANNEL`.
async fn hide_channel_from(f: &Fixture, user_id: Uuid) {
    let role_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO guild_roles (id, guild_id, name, permissions, position)
         VALUES ($1, $2, 'hidden', 0, 1)",
    )
    .bind(role_id)
    .bind(f.guild_id)
    .execute(&f.app.pool)
    .await
    .unwrap();
    sqlx::query("INSERT INTO guild_member_roles (guild_id, user_id, role_id) VALUES ($1, $2, $3)")
        .bind(f.guild_id)
        .bind(user_id)
        .bind(role_id)
        .execute(&f.app.pool)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO channel_overrides (id, channel_id, role_id, allow_permissions, deny_permissions)
         VALUES ($1, $2, $3, 0, $4)",
    )
    .bind(Uuid::new_v4())
    .bind(f.channel.id)
    .bind(role_id)
    .bind(GuildPermissions::VIEW_CHANNEL.to_db())
    .execute(&f.app.pool)
    .await
    .unwrap();
}

#[sqlx::test]
async fn test_mention_targets_only_members_with_access(pool: PgPool) {
    let f = fixture(pool).await;
    let (member_id, member_name) = member(&f).await;
    let (_outsider_id, outsider_name) = create_test_user(&f.app.pool).await;
    let (hidden_id, hidden_name) = member(&f).await;
    hide_channel_from(&f, hidden_id).await;

    assert_eq!(
        targets(&f, &format!("hi @{member_name}")).await,
        [member_id]
    );
    assert!(targets(&f, &format!("hi @{outsider_name}"))
        .await
        .is_empty());
    assert!(targets(&f, &format!("hi @{hidden_name}")).await.is_empty());
}

#[sqlx::test]
async fn test_mention_targets_exclude_author_and_unknown(pool: PgPool) {
    let f = fixture(pool).await;
    let author_name = db::find_user_by_id(&f.app.pool, f.author)
        .await
        .unwrap()
        .unwrap()
        .username;

    assert!(targets(&f, &format!("@{author_name} @nobody"))
        .await
        .is_empty());
}

#[sqlx::test]
async fn test_email_and_code_are_not_mentions(pool: PgPool) {
    let f = fixture(pool).await;
    let (_member_id, member_name) = member(&f).await;

    let content = format!("{member_name}@example.com `@{member_name}`");
    assert!(targets(&f, &content).await.is_empty());
}

#[sqlx::test]
async fn test_everyone_targets_members_with_access(pool: PgPool) {
    let f = fixture(pool).await;
    let mut expected = HashSet::new();
    for _ in 0..3 {
        expected.insert(member(&f).await.0);
    }

    let got = targets(&f, "@everyone dinner").await;
    assert_eq!(got.len(), 3, "no duplicates, author excluded");
    assert_eq!(got.into_iter().collect::<HashSet<_>>(), expected);
}

#[sqlx::test]
async fn test_everyone_push_is_capped(pool: PgPool) {
    let f = fixture(pool).await;
    for _ in 0..(MENTION_EVERYONE_PUSH_CAP + 2) {
        member(&f).await;
    }

    let got = targets(&f, "@everyone").await;
    assert_eq!(got.len(), MENTION_EVERYONE_PUSH_CAP);
    assert!(!got.contains(&f.author));
}

#[sqlx::test]
async fn test_here_targets_nobody(pool: PgPool) {
    let f = fixture(pool).await;
    member(&f).await;
    member(&f).await;

    assert!(targets(&f, "@here quick question").await.is_empty());
}

#[sqlx::test]
async fn test_blocked_author_mention_not_targeted(pool: PgPool) {
    use fred::interfaces::KeysInterface;

    let f = fixture(pool).await;
    let (member_id, member_name) = member(&f).await;
    let redis = &f.app.state.redis;
    // Keys are unique to this test's fresh user ids; the TTL guarantees a
    // panicking test leaves nothing behind.
    block_cache::add_block(redis, member_id, f.author)
        .await
        .unwrap();
    for key in [
        format!("blocks:{member_id}"),
        format!("blocked_by:{}", f.author),
    ] {
        let _: () = redis.expire(&key, 60, None).await.unwrap();
    }

    let got = targets(&f, &format!("@{member_name}")).await;
    block_cache::remove_block(redis, member_id, f.author)
        .await
        .unwrap();
    assert!(got.is_empty());
}

/// The Redis block cache expires; a block stored only in Postgres must still
/// suppress the mention target.
#[sqlx::test]
async fn test_db_only_block_not_targeted(pool: PgPool) {
    let f = fixture(pool).await;
    let (blocker_id, blocker_name) = member(&f).await;
    let (blocked_id, blocked_name) = member(&f).await;
    let (free_id, free_name) = member(&f).await;
    // blocker blocked the author; the author blocked `blocked`.
    for (requester, addressee) in [(blocker_id, f.author), (f.author, blocked_id)] {
        sqlx::query(
            "INSERT INTO friendships (id, requester_id, addressee_id, status) \
             VALUES ($1, $2, $3, 'blocked')",
        )
        .bind(Uuid::now_v7())
        .bind(requester)
        .bind(addressee)
        .execute(&f.app.pool)
        .await
        .unwrap();
    }

    let got = targets(&f, &format!("@{blocker_name} @{blocked_name} @{free_name}")).await;
    assert_eq!(got, [free_id]);
}

#[sqlx::test]
async fn test_dm_channel_has_no_mention_targets(pool: PgPool) {
    let f = fixture(pool).await;
    let (other_id, other_name) = create_test_user(&f.app.pool).await;
    let dm_id = crate::helpers::create_dm_channel(&f.app.pool, f.author, other_id).await;
    let dm = db::find_channel_by_id(&f.app.pool, dm_id)
        .await
        .unwrap()
        .unwrap();

    let mentions = extract_mentions(&format!("@{other_name} @everyone"));
    assert!(
        resolve_mention_targets(&f.app.state, &dm, f.author, &mentions)
            .await
            .is_empty()
    );
}
