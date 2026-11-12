//! Attachment access follows channel access: download, variants and
//! presigned URLs need the same right as reading the message's channel.

use axum::body::Body;
use axum::http::Method;
use sqlx::PgPool;
use uuid::Uuid;
use vc_server::permissions::GuildPermissions;

use super::helpers::{
    add_guild_member, create_channel, create_dm_channel, create_guild_with_default_role,
    create_test_user, fresh_test_app_with_s3_and_pool, generate_access_token, insert_message,
    rustfs_available, TestApp,
};

fn member_perms() -> GuildPermissions {
    GuildPermissions::VIEW_CHANNEL | GuildPermissions::SEND_MESSAGES
}

async fn everyone_role(pool: &PgPool, guild: Uuid) -> Uuid {
    sqlx::query_scalar("SELECT id FROM guild_roles WHERE guild_id = $1 AND is_default")
        .bind(guild)
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn create_role(pool: &PgPool, guild: Uuid) -> Uuid {
    let id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO guild_roles (id, guild_id, name, permissions, position) VALUES ($1, $2, 'insiders', 0, 1)",
    )
    .bind(id)
    .bind(guild)
    .execute(pool)
    .await
    .unwrap();
    id
}

async fn assign_role(pool: &PgPool, guild: Uuid, user: Uuid, role: Uuid) {
    sqlx::query("INSERT INTO guild_member_roles (guild_id, user_id, role_id) VALUES ($1, $2, $3)")
        .bind(guild)
        .bind(user)
        .bind(role)
        .execute(pool)
        .await
        .unwrap();
}

async fn set_override(
    pool: &PgPool,
    channel: Uuid,
    role: Uuid,
    allow: GuildPermissions,
    deny: GuildPermissions,
) {
    sqlx::query(
        "INSERT INTO channel_overrides (id, channel_id, role_id, allow_permissions, deny_permissions)
         VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(Uuid::now_v7())
    .bind(channel)
    .bind(role)
    .bind(allow.to_db())
    .bind(deny.to_db())
    .execute(pool)
    .await
    .unwrap();
}

/// Store an attachment (original + thumbnail objects) on `message` and
/// return `(attachment_id, keys)`.
async fn stored_attachment(app: &TestApp, message: Uuid) -> (Uuid, Vec<String>) {
    let s3 = app.state.s3.as_ref().expect("s3 configured");
    let base = format!("attachments/test/{message}/{}", Uuid::now_v7());
    let original = format!("{base}.txt");
    let thumb = format!("{base}_thumb.webp");
    s3.upload(&original, b"hello attachment".to_vec(), "text/plain")
        .await
        .expect("upload original");
    s3.upload(&thumb, b"thumb".to_vec(), "image/webp")
        .await
        .expect("upload thumbnail");
    let id: Uuid = sqlx::query_scalar(
        "INSERT INTO file_attachments (message_id, filename, mime_type, size_bytes, s3_key, thumbnail_s3_key)
         VALUES ($1, 'note.txt', 'text/plain', 16, $2, $3) RETURNING id",
    )
    .bind(message)
    .bind(&original)
    .bind(&thumb)
    .fetch_one(&app.pool)
    .await
    .unwrap();
    (id, vec![original, thumb])
}

/// Status codes of download, thumbnail download and presigned URL.
async fn read_statuses(app: &TestApp, token: &str, attachment: Uuid) -> [u16; 3] {
    let mut out = [0u16; 3];
    let uris = [
        format!("/api/messages/attachments/{attachment}/download?token={token}"),
        format!("/api/messages/attachments/{attachment}/download?variant=thumbnail&token={token}"),
        format!("/api/messages/attachments/{attachment}/url"),
    ];
    for (i, uri) in uris.iter().enumerate() {
        let req = TestApp::request(Method::GET, uri)
            .header("Authorization", format!("Bearer {token}"))
            .body(Body::empty())
            .unwrap();
        out[i] = app.oneshot(req).await.status().as_u16();
    }
    out
}

/// A member who reads a private channel through a role loses download,
/// thumbnail and presigned URL of its attachments with the role.
#[sqlx::test]
async fn role_loss_blocks_download_variant_and_url(pool: PgPool) {
    if !rustfs_available().await {
        eprintln!("skipping: object storage not reachable");
        return;
    }
    let (app, _bucket) = fresh_test_app_with_s3_and_pool(pool.clone()).await;
    let (owner, _) = create_test_user(&pool).await;
    let (member, _) = create_test_user(&pool).await;
    let guild = create_guild_with_default_role(&pool, owner, member_perms()).await;
    add_guild_member(&pool, guild, member).await;
    let private = create_channel(&pool, guild, "private").await;
    let everyone = everyone_role(&pool, guild).await;
    let role = create_role(&pool, guild).await;
    set_override(
        &pool,
        private,
        everyone,
        GuildPermissions::empty(),
        GuildPermissions::VIEW_CHANNEL,
    )
    .await;
    set_override(
        &pool,
        private,
        role,
        GuildPermissions::VIEW_CHANNEL,
        GuildPermissions::empty(),
    )
    .await;
    assign_role(&pool, guild, member, role).await;

    let message = insert_message(&pool, private, owner, "file").await;
    let (attachment, _) = stored_attachment(&app, message).await;
    let token = generate_access_token(&app.config, member);

    assert_eq!(
        read_statuses(&app, &token, attachment).await,
        [200, 200, 200]
    );

    sqlx::query("DELETE FROM guild_member_roles WHERE user_id = $1 AND role_id = $2")
        .bind(member)
        .bind(role)
        .execute(&pool)
        .await
        .unwrap();

    assert_eq!(
        read_statuses(&app, &token, attachment).await,
        [403, 403, 403]
    );
}

/// A user outside a DM cannot read its attachments; both participants can.
#[sqlx::test]
async fn dm_non_participant_is_refused(pool: PgPool) {
    if !rustfs_available().await {
        eprintln!("skipping: object storage not reachable");
        return;
    }
    let (app, _bucket) = fresh_test_app_with_s3_and_pool(pool.clone()).await;
    let (a, _) = create_test_user(&pool).await;
    let (b, _) = create_test_user(&pool).await;
    let (outsider, _) = create_test_user(&pool).await;
    let dm = create_dm_channel(&pool, a, b).await;
    let message = insert_message(&pool, dm, a, "file").await;
    let (attachment, _) = stored_attachment(&app, message).await;

    let b_token = generate_access_token(&app.config, b);
    assert_eq!(
        read_statuses(&app, &b_token, attachment).await,
        [200, 200, 200]
    );

    let outsider_token = generate_access_token(&app.config, outsider);
    assert_eq!(
        read_statuses(&app, &outsider_token, attachment).await,
        [403, 403, 403]
    );
}

/// A participant who left a group DM loses its attachments; the others keep them.
#[sqlx::test]
async fn group_dm_leaver_is_refused(pool: PgPool) {
    if !rustfs_available().await {
        eprintln!("skipping: object storage not reachable");
        return;
    }
    let (app, _bucket) = fresh_test_app_with_s3_and_pool(pool.clone()).await;
    let (a, _) = create_test_user(&pool).await;
    let (b, _) = create_test_user(&pool).await;
    let (c, _) = create_test_user(&pool).await;
    let dm = create_dm_channel(&pool, a, b).await;
    sqlx::query("INSERT INTO dm_participants (channel_id, user_id) VALUES ($1, $2)")
        .bind(dm)
        .bind(c)
        .execute(&pool)
        .await
        .unwrap();
    let message = insert_message(&pool, dm, a, "file").await;
    let (attachment, _) = stored_attachment(&app, message).await;

    let c_token = generate_access_token(&app.config, c);
    assert_eq!(
        read_statuses(&app, &c_token, attachment).await,
        [200, 200, 200]
    );

    sqlx::query("DELETE FROM dm_participants WHERE channel_id = $1 AND user_id = $2")
        .bind(dm)
        .bind(c)
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(
        read_statuses(&app, &c_token, attachment).await,
        [403, 403, 403]
    );

    let b_token = generate_access_token(&app.config, b);
    assert_eq!(
        read_statuses(&app, &b_token, attachment).await,
        [200, 200, 200]
    );
}

/// The attachment of a soft-deleted message is refused, even to its author.
#[sqlx::test]
async fn deleted_message_attachment_is_refused(pool: PgPool) {
    if !rustfs_available().await {
        eprintln!("skipping: object storage not reachable");
        return;
    }
    let (app, _bucket) = fresh_test_app_with_s3_and_pool(pool.clone()).await;
    let (owner, _) = create_test_user(&pool).await;
    let guild = create_guild_with_default_role(&pool, owner, member_perms()).await;
    let channel = create_channel(&pool, guild, "general").await;
    let message = insert_message(&pool, channel, owner, "file").await;
    let (attachment, _) = stored_attachment(&app, message).await;
    let token = generate_access_token(&app.config, owner);
    assert_eq!(
        read_statuses(&app, &token, attachment).await,
        [200, 200, 200]
    );

    // Soft delete written directly (no attachment clean-up involved).
    sqlx::query("ALTER TABLE messages DISABLE TRIGGER USER")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("UPDATE messages SET deleted_at = NOW() WHERE id = $1")
        .bind(message)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("ALTER TABLE messages ENABLE TRIGGER USER")
        .execute(&pool)
        .await
        .unwrap();

    assert_eq!(
        read_statuses(&app, &token, attachment).await,
        [403, 403, 403]
    );
}

// ---------------------------------------------------------------------------
// Deleting a message deletes its files
// ---------------------------------------------------------------------------

async fn pending_keys(pool: &PgPool) -> Vec<String> {
    sqlx::query_scalar("SELECT s3_key FROM storage_pending_deletes ORDER BY s3_key")
        .fetch_all(pool)
        .await
        .unwrap()
}

async fn attachment_rows(pool: &PgPool, message: Uuid) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM file_attachments WHERE message_id = $1")
        .bind(message)
        .fetch_one(pool)
        .await
        .unwrap()
}

/// The author deletes a message: its attachment rows go in the same
/// transaction, the stored objects (original and thumbnail) are deleted and
/// the file cannot be opened any more.
#[sqlx::test]
async fn deleting_message_removes_rows_and_objects(pool: PgPool) {
    if !rustfs_available().await {
        eprintln!("skipping: object storage not reachable");
        return;
    }
    let (app, _bucket) = fresh_test_app_with_s3_and_pool(pool.clone()).await;
    let (owner, _) = create_test_user(&pool).await;
    let guild = create_guild_with_default_role(&pool, owner, member_perms()).await;
    let channel = create_channel(&pool, guild, "general").await;
    let message = insert_message(&pool, channel, owner, "file").await;
    let (attachment, keys) = stored_attachment(&app, message).await;
    let token = generate_access_token(&app.config, owner);

    let req = TestApp::request(Method::DELETE, &format!("/api/messages/{message}"))
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();
    assert_eq!(app.oneshot(req).await.status(), 204);

    assert_eq!(attachment_rows(&pool, message).await, 0, "rows removed");

    // The objects are deleted right after the commit (background task).
    let s3 = app.state.s3.as_ref().unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while !pending_keys(&pool).await.is_empty() {
        assert!(
            std::time::Instant::now() < deadline,
            "objects still queued: {:?}",
            pending_keys(&pool).await
        );
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    for key in &keys {
        assert!(
            s3.get_object_stream(key).await.is_err(),
            "object {key} still stored"
        );
    }

    assert_eq!(
        read_statuses(&app, &token, attachment).await,
        [403, 403, 403]
    );
}

/// Rows removed by a cascade (channel deletion) queue their objects too.
#[sqlx::test]
async fn channel_delete_queues_objects(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (owner, _) = create_test_user(&pool).await;
    let guild = create_guild_with_default_role(&pool, owner, member_perms()).await;
    let channel = create_channel(&pool, guild, "general").await;
    let message = insert_message(&pool, channel, owner, "file").await;
    sqlx::query(
        "INSERT INTO file_attachments (message_id, filename, mime_type, size_bytes, s3_key, thumbnail_s3_key, medium_s3_key)
         VALUES ($1, 'a.png', 'image/png', 1, 'k/a.png', 'k/a_thumb.webp', 'k/a_medium.webp')",
    )
    .bind(message)
    .execute(&pool)
    .await
    .unwrap();

    sqlx::query("DELETE FROM channels WHERE id = $1")
        .bind(channel)
        .execute(&app.pool)
        .await
        .unwrap();

    assert_eq!(
        pending_keys(&pool).await,
        vec!["k/a.png", "k/a_medium.webp", "k/a_thumb.webp"]
    );
}

/// A failed object delete is logged and stays queued for the next sweep.
#[sqlx::test]
async fn failed_object_delete_stays_queued(pool: PgPool) {
    let app = TestApp::with_pool_and_unreachable_s3(pool.clone()).await;
    let (owner, _) = create_test_user(&pool).await;
    let guild = create_guild_with_default_role(&pool, owner, member_perms()).await;
    let channel = create_channel(&pool, guild, "general").await;
    let message = insert_message(&pool, channel, owner, "file").await;
    super::helpers::insert_attachment(&pool, message).await;
    sqlx::query("UPDATE messages SET deleted_at = NOW() WHERE id = $1")
        .bind(message)
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(attachment_rows(&pool, message).await, 0);

    let deleted =
        vc_server::chat::storage_cleanup::drain_pending_deletes(&pool, app.state.s3.as_ref(), 10)
            .await
            .unwrap();
    assert_eq!(deleted, 0);
    let attempts: i32 =
        sqlx::query_scalar("SELECT attempts FROM storage_pending_deletes WHERE s3_key = $1")
            .bind("uploads/test.png")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(attempts, 1);
}

/// Account deletion removes the files of the (anonymized) messages.
#[sqlx::test]
async fn account_deletion_removes_files(pool: PgPool) {
    let _app = TestApp::with_pool(pool.clone()).await;
    let (owner, _) = create_test_user(&pool).await;
    let (leaver, _) = create_test_user(&pool).await;
    let guild = create_guild_with_default_role(&pool, owner, member_perms()).await;
    add_guild_member(&pool, guild, leaver).await;
    let channel = create_channel(&pool, guild, "general").await;
    let message = insert_message(&pool, channel, leaver, "file").await;
    super::helpers::insert_attachment(&pool, message).await;
    sqlx::query(
        "UPDATE users SET deletion_scheduled_at = NOW() - INTERVAL '1 minute' WHERE id = $1",
    )
    .bind(leaver)
    .execute(&pool)
    .await
    .unwrap();

    vc_server::governance::deletion::process_pending_deletions(&pool, &None)
        .await
        .unwrap();

    assert_eq!(attachment_rows(&pool, message).await, 0);
    assert_eq!(pending_keys(&pool).await, vec!["uploads/test.png"]);
}

// ---------------------------------------------------------------------------
// Download authentication: header, deprecated `?token=`
// ---------------------------------------------------------------------------

/// Collects every span field and event field the server records.
#[derive(Clone, Default)]
struct Capture(std::sync::Arc<std::sync::Mutex<String>>);

struct FieldWriter<'a>(&'a mut String);

impl tracing::field::Visit for FieldWriter<'_> {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        use std::fmt::Write;
        let _ = write!(self.0, " {}={:?}", field.name(), value);
    }
}

impl<S: tracing::Subscriber> tracing_subscriber::Layer<S> for Capture {
    fn on_new_span(
        &self,
        attrs: &tracing::span::Attributes<'_>,
        _id: &tracing::span::Id,
        _ctx: tracing_subscriber::layer::Context<'_, S>,
    ) {
        let mut out = self.0.lock().unwrap();
        out.push_str("\nspan ");
        out.push_str(attrs.metadata().name());
        attrs.record(&mut FieldWriter(&mut out));
    }

    fn on_record(
        &self,
        _id: &tracing::span::Id,
        values: &tracing::span::Record<'_>,
        _ctx: tracing_subscriber::layer::Context<'_, S>,
    ) {
        values.record(&mut FieldWriter(&mut self.0.lock().unwrap()));
    }

    fn on_event(
        &self,
        event: &tracing::Event<'_>,
        _ctx: tracing_subscriber::layer::Context<'_, S>,
    ) {
        let mut out = self.0.lock().unwrap();
        out.push_str("\nevent");
        event.record(&mut FieldWriter(&mut out));
    }
}

async fn download_fixture(pool: &PgPool) -> Option<(TestApp, Uuid, String)> {
    if !rustfs_available().await {
        eprintln!("skipping: object storage not reachable");
        return None;
    }
    let (app, _bucket) = fresh_test_app_with_s3_and_pool(pool.clone()).await;
    let (owner, _) = create_test_user(pool).await;
    let guild = create_guild_with_default_role(pool, owner, member_perms()).await;
    let channel = create_channel(pool, guild, "general").await;
    let message = insert_message(pool, channel, owner, "file").await;
    let (attachment, _) = stored_attachment(&app, message).await;
    let token = generate_access_token(&app.config, owner);
    Some((app, attachment, token))
}

#[sqlx::test]
async fn download_with_authorization_header(pool: PgPool) {
    let Some((app, attachment, token)) = download_fixture(&pool).await else {
        return;
    };
    let req = TestApp::request(
        Method::GET,
        &format!("/api/messages/attachments/{attachment}/download?variant=thumbnail"),
    )
    .header("Authorization", format!("Bearer {token}"))
    .body(Body::empty())
    .unwrap();
    assert_eq!(app.oneshot(req).await.status(), 200);

    // A bad header is refused, not ignored.
    let req = TestApp::request(
        Method::GET,
        &format!("/api/messages/attachments/{attachment}/download"),
    )
    .header("Authorization", "Bearer not-a-jwt")
    .body(Body::empty())
    .unwrap();
    assert_eq!(app.oneshot(req).await.status(), 401);

    // No credentials at all.
    let req = TestApp::request(
        Method::GET,
        &format!("/api/messages/attachments/{attachment}/download"),
    )
    .body(Body::empty())
    .unwrap();
    assert_eq!(app.oneshot(req).await.status(), 403);
}

/// `?token=` keeps working (deprecated), and the token never appears in a
/// span or log event the server records for the request.
#[sqlx::test]
async fn query_token_still_works_and_stays_out_of_traces(pool: PgPool) {
    use tracing_subscriber::layer::SubscriberExt;
    let Some((app, attachment, token)) = download_fixture(&pool).await else {
        return;
    };
    let capture = Capture::default();
    let subscriber = tracing_subscriber::registry().with(capture.clone());
    let guard = tracing::subscriber::set_default(subscriber);

    let req = TestApp::request(
        Method::GET,
        &format!("/api/messages/attachments/{attachment}/download?token={token}&variant=thumbnail"),
    )
    .body(Body::empty())
    .unwrap();
    let status = app.oneshot(req).await.status();
    drop(guard);
    assert_eq!(status, 200);

    let recorded = capture.0.lock().unwrap().clone();
    assert!(
        recorded.contains(&format!("/api/messages/attachments/{attachment}/download")),
        "the request span was not captured: {recorded}"
    );
    assert!(
        recorded.contains("variant=thumbnail"),
        "other query parameters stay visible: {recorded}"
    );
    let signature = token.rsplit('.').next().unwrap();
    assert!(
        !recorded.contains(signature),
        "the token reached a span or event: {recorded}"
    );
}
