//! The object-cleanup worker: fair, bounded, retrying with backoff, safe
//! to run twice; the backfill of earlier deleted messages; the migration.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use sqlx::PgPool;
use uuid::Uuid;
use vc_server::chat::storage_cleanup::{
    backfill_batch, drain_pending_deletes, DrainLimits, ObjectStore,
};
use vc_server::permissions::GuildPermissions;

use super::helpers::{
    create_channel, create_guild_with_default_role, create_test_user,
    fresh_test_app_with_s3_and_pool, insert_message, rustfs_available, TestApp,
};

/// Object store fake: counts calls, fails keys starting with `fail_prefix`,
/// takes `delay` per delete (respecting the caller's limit like the S3
/// client does).
#[derive(Default)]
struct Fake {
    fail_prefix: Option<&'static str>,
    delay: Duration,
    calls: Mutex<HashMap<String, u32>>,
}

impl ObjectStore for Fake {
    async fn delete_object(&self, key: &str, limit: Duration) -> Result<(), String> {
        {
            let mut calls = self.calls.lock().unwrap();
            *calls.entry(key.to_string()).or_default() += 1;
        }
        if !self.delay.is_zero()
            && tokio::time::timeout(limit, tokio::time::sleep(self.delay))
                .await
                .is_err()
        {
            return Err("timed out".to_string());
        }
        match self.fail_prefix {
            Some(p) if key.starts_with(p) => Err("denied".to_string()),
            _ => Ok(()),
        }
    }
}

fn limits(max_rows: i64) -> DrainLimits {
    DrainLimits {
        max_rows,
        budget: Duration::from_secs(5),
        per_delete: Duration::from_secs(1),
    }
}

/// Queue `key` as if removed `age_secs` ago.
async fn queue(pool: &PgPool, key: &str, age_secs: f64) {
    sqlx::query(
        "INSERT INTO storage_pending_deletes (s3_key, created_at, next_attempt_at)
         VALUES ($1, NOW() - make_interval(secs => $2), NOW() - make_interval(secs => $2))",
    )
    .bind(key)
    .bind(age_secs)
    .execute(pool)
    .await
    .unwrap();
}

async fn queued(pool: &PgPool) -> Vec<String> {
    sqlx::query_scalar("SELECT s3_key FROM storage_pending_deletes ORDER BY s3_key")
        .fetch_all(pool)
        .await
        .unwrap()
}

async fn attempts(pool: &PgPool, key: &str) -> (i32, f64) {
    sqlx::query_as(
        "SELECT attempts,
                COALESCE(EXTRACT(EPOCH FROM next_attempt_at - last_attempt_at)::float8, 0)
         FROM storage_pending_deletes WHERE s3_key = $1",
    )
    .bind(key)
    .fetch_one(pool)
    .await
    .unwrap()
}

// ---------------------------------------------------------------------------
// Worker
// ---------------------------------------------------------------------------

/// Keys whose delete keeps failing neither block newer keys nor are tried
/// again before their backoff; newer keys go first.
#[sqlx::test]
async fn failing_keys_do_not_block_newer_keys(pool: PgPool) {
    let store = Fake {
        fail_prefix: Some("bad/"),
        ..Fake::default()
    };
    for i in 0..3 {
        queue(&pool, &format!("bad/{i}"), 3600.0).await;
    }
    queue(&pool, "good/1", 1.0).await;
    queue(&pool, "good/2", 0.0).await;

    // Two rows per drain: the newest keys win over the old backlog.
    let r = drain_pending_deletes(&pool, Some(&store), limits(2))
        .await
        .unwrap();
    assert_eq!(r.deleted, 2);
    assert_eq!(queued(&pool).await, ["bad/0", "bad/1", "bad/2"]);

    let r = drain_pending_deletes(&pool, Some(&store), limits(10))
        .await
        .unwrap();
    assert_eq!((r.deleted, r.failed), (0, 3));

    // The failed keys wait for their backoff; a new key goes through.
    queue(&pool, "good/3", 0.0).await;
    let r = drain_pending_deletes(&pool, Some(&store), limits(10))
        .await
        .unwrap();
    assert_eq!((r.deleted, r.failed), (1, 0));
    assert_eq!(queued(&pool).await, ["bad/0", "bad/1", "bad/2"]);
}

/// A failed attempt schedules the next one 1 min later, doubling per
/// failure; after many failures the key stays queued (escalation, no drop).
#[sqlx::test]
async fn failed_keys_back_off_and_are_never_dropped(pool: PgPool) {
    let store = Fake {
        fail_prefix: Some(""),
        ..Fake::default()
    };
    queue(&pool, "k/1", 0.0).await;
    drain_pending_deletes(&pool, Some(&store), limits(10))
        .await
        .unwrap();
    let (n, gap) = attempts(&pool, "k/1").await;
    assert_eq!(n, 1);
    assert!(
        (gap - 60.0).abs() < 1.0,
        "first retry after 1 min, got {gap}"
    );

    // Fourth failure: 8 min.
    sqlx::query("UPDATE storage_pending_deletes SET attempts = 3, next_attempt_at = NOW()")
        .execute(&pool)
        .await
        .unwrap();
    drain_pending_deletes(&pool, Some(&store), limits(10))
        .await
        .unwrap();
    let (n, gap) = attempts(&pool, "k/1").await;
    assert_eq!(n, 4);
    assert!(
        (gap - 480.0).abs() < 1.0,
        "fourth retry after 8 min, got {gap}"
    );

    // The 20th failure escalates (error log) and keeps the row; the delay
    // stays capped at 6 h.
    sqlx::query("UPDATE storage_pending_deletes SET attempts = 19, next_attempt_at = NOW()")
        .execute(&pool)
        .await
        .unwrap();
    drain_pending_deletes(&pool, Some(&store), limits(10))
        .await
        .unwrap();
    let (n, gap) = attempts(&pool, "k/1").await;
    assert_eq!(n, 20);
    assert!((gap - 6.0 * 3600.0).abs() < 1.0, "capped at 6 h, got {gap}");
    assert_eq!(queued(&pool).await, ["k/1"]);
}

/// Two drains at once (a second instance, an overlapping post-delete
/// drain) delete every key exactly once.
#[sqlx::test]
async fn concurrent_drains_delete_each_key_once(pool: PgPool) {
    let store = Fake {
        delay: Duration::from_millis(5),
        ..Fake::default()
    };
    for i in 0..60 {
        queue(&pool, &format!("k/{i:02}"), 0.0).await;
    }
    let (a, b) = tokio::join!(
        drain_pending_deletes(&pool, Some(&store), limits(60)),
        drain_pending_deletes(&pool, Some(&store), limits(60)),
    );
    let (a, b) = (a.unwrap(), b.unwrap());
    assert_eq!(a.deleted + b.deleted, 60);
    assert!(queued(&pool).await.is_empty());
    let calls = store.calls.lock().unwrap();
    assert_eq!(calls.len(), 60);
    assert!(calls.values().all(|n| *n == 1), "a key was deleted twice");
}

/// A hanging store: the drain stops at its wall-time bound, keeps every
/// key, and the minute auth sweep does not touch the queue at all.
#[sqlx::test]
async fn hanging_store_keeps_the_tick_bounded(pool: PgPool) {
    let store = Fake {
        delay: Duration::from_secs(60),
        ..Fake::default()
    };
    for i in 0..10 {
        queue(&pool, &format!("k/{i}"), 0.0).await;
    }
    let started = Instant::now();
    let r = drain_pending_deletes(
        &pool,
        Some(&store),
        DrainLimits {
            max_rows: 10,
            budget: Duration::from_secs(1),
            per_delete: Duration::from_millis(300),
        },
    )
    .await
    .unwrap();
    let took = started.elapsed();
    assert!(took < Duration::from_secs(2), "tick took {took:?}");
    assert!(r.failed >= 3, "timed-out deletes count as failures: {r:?}");
    assert_eq!(r.failed + r.released, 10, "{r:?}");
    assert_eq!(queued(&pool).await.len(), 10);

    // The auth sweep no longer drains: a due key stays untouched.
    let app = TestApp::with_pool_and_unreachable_s3(pool.clone()).await;
    queue(&pool, "sweep/untouched", 0.0).await;
    let _ = vc_server::auth::timers::run_once(&app.state).await;
    assert_eq!(attempts(&pool, "sweep/untouched").await.0, 0);
}

/// Deleting an object that is already gone counts as success.
#[sqlx::test]
async fn missing_object_counts_as_deleted(pool: PgPool) {
    if !rustfs_available().await {
        return;
    }
    let (app, _bucket) = fresh_test_app_with_s3_and_pool(pool.clone()).await;
    queue(
        &pool,
        &format!("attachments/none/{}.txt", Uuid::now_v7()),
        0.0,
    )
    .await;
    let r = drain_pending_deletes(&pool, app.state.s3.as_ref(), DrainLimits::TICK)
        .await
        .unwrap();
    assert_eq!((r.deleted, r.failed), (1, 0));
    assert!(queued(&pool).await.is_empty());
}

// ---------------------------------------------------------------------------
// Backfill and migration
// ---------------------------------------------------------------------------

/// `n` messages with one attachment each, soft-deleted with triggers off
/// (as before the migration). Returns their attachment ids.
async fn deleted_before_migration(pool: &PgPool, channel: Uuid, user: Uuid, n: usize) -> Vec<Uuid> {
    let mut conn = pool.acquire().await.unwrap();
    sqlx::query("SET session_replication_role = replica")
        .execute(&mut *conn)
        .await
        .unwrap();
    let mut ids = Vec::new();
    for i in 0..n {
        let message = insert_message(pool, channel, user, "old").await;
        let id: Uuid = sqlx::query_scalar(
            "INSERT INTO file_attachments (message_id, filename, mime_type, size_bytes, s3_key)
             VALUES ($1, 'a.txt', 'text/plain', 1, $2) RETURNING id",
        )
        .bind(message)
        .bind(format!("attachments/old/{i}"))
        .fetch_one(&mut *conn)
        .await
        .unwrap();
        sqlx::query("UPDATE messages SET deleted_at = NOW() WHERE id = $1")
            .bind(message)
            .execute(&mut *conn)
            .await
            .unwrap();
        ids.push(id);
    }
    sqlx::query("SET session_replication_role = origin")
        .execute(&mut *conn)
        .await
        .unwrap();
    ids
}

async fn attachment_count(pool: &PgPool) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM file_attachments")
        .fetch_one(pool)
        .await
        .unwrap()
}

/// The backfill removes old rows in batches, queues their objects, skips
/// rows other transactions hold (without waiting for them), only finishes
/// when nothing is left, and is idle afterwards.
#[sqlx::test]
async fn backfill_runs_in_batches_beside_live_writes(pool: PgPool) {
    let (user, _) = create_test_user(&pool).await;
    let guild = create_guild_with_default_role(&pool, user, GuildPermissions::VIEW_CHANNEL).await;
    let channel = create_channel(&pool, guild, "general").await;
    let old = deleted_before_migration(&pool, channel, user, 5).await;
    assert_eq!(attachment_count(&pool).await, 5);

    // First batch: two rows. State lives only in the database, so every
    // later call is the same as one after a restart.
    assert_eq!(backfill_batch(&pool, 2).await.unwrap(), Some(2));

    // A live transaction holds one old row and writes a new message with a
    // file; the backfill neither waits for it nor finishes early.
    let mut live = pool.begin().await.unwrap();
    let held: Uuid = sqlx::query_scalar(
        "SELECT id FROM file_attachments WHERE id = ANY($1) ORDER BY id LIMIT 1 FOR UPDATE",
    )
    .bind(&old)
    .fetch_one(&mut *live)
    .await
    .unwrap();
    let fresh: Uuid = sqlx::query_scalar(
        "INSERT INTO messages (id, channel_id, user_id, content) VALUES ($1, $2, $3, 'new')
         RETURNING id",
    )
    .bind(Uuid::now_v7())
    .bind(channel)
    .bind(user)
    .fetch_one(&mut *live)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO file_attachments (message_id, filename, mime_type, size_bytes, s3_key)
         VALUES ($1, 'b.txt', 'text/plain', 1, 'attachments/live/1')",
    )
    .bind(fresh)
    .execute(&mut *live)
    .await
    .unwrap();

    let batch = tokio::time::timeout(Duration::from_secs(3), backfill_batch(&pool, 10))
        .await
        .expect("backfill waited for a live transaction")
        .unwrap();
    assert_eq!(batch, Some(2), "the held row is skipped");
    let batch = tokio::time::timeout(Duration::from_secs(3), backfill_batch(&pool, 10))
        .await
        .expect("backfill waited for a live transaction")
        .unwrap();
    assert_eq!(batch, Some(0), "not finished while a row is held");
    live.commit().await.unwrap();

    assert_eq!(backfill_batch(&pool, 10).await.unwrap(), Some(1));
    assert_eq!(backfill_batch(&pool, 10).await.unwrap(), None);
    assert_eq!(backfill_batch(&pool, 10).await.unwrap(), None, "idle");

    let remaining: Vec<Uuid> = sqlx::query_scalar("SELECT id FROM file_attachments")
        .fetch_all(&pool)
        .await
        .unwrap();
    assert_eq!(remaining.len(), 1, "only the live file is left");
    assert!(!remaining.contains(&held));
    let keys = queued(&pool).await;
    assert_eq!(keys.len(), 5);
    assert!(keys.iter().all(|k| k.starts_with("attachments/old/")));
}

/// The migration only creates tables, indexes and triggers: on a database
/// with old deleted messages it changes no attachment row.
#[sqlx::test]
async fn migration_changes_no_existing_rows(pool: PgPool) {
    let (user, _) = create_test_user(&pool).await;
    let guild = create_guild_with_default_role(&pool, user, GuildPermissions::VIEW_CHANNEL).await;
    let channel = create_channel(&pool, guild, "general").await;

    // Back to the state before the migration, then add old rows.
    sqlx::raw_sql(
        "DROP TRIGGER messages_soft_delete_attachments ON messages;
         DROP TRIGGER file_attachments_queue_objects ON file_attachments;
         DROP FUNCTION delete_attachments_of_deleted_message();
         DROP FUNCTION queue_attachment_objects();
         DROP TABLE storage_cleanup_backfill;
         DROP TABLE storage_pending_deletes;",
    )
    .execute(&pool)
    .await
    .unwrap();
    deleted_before_migration(&pool, channel, user, 3).await;
    insert_message(&pool, channel, user, "kept").await;

    let started = Instant::now();
    let mut tx = pool.begin().await.unwrap();
    sqlx::raw_sql(include_str!(
        "../../migrations/20261123000000_attachment_object_cleanup.sql"
    ))
    .execute(&mut *tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();
    assert!(started.elapsed() < Duration::from_secs(5));

    assert_eq!(
        attachment_count(&pool).await,
        3,
        "no attachment row removed"
    );
    assert!(
        queued(&pool).await.is_empty(),
        "nothing queued by the migration"
    );
    let finished: Option<chrono::DateTime<chrono::Utc>> =
        sqlx::query_scalar("SELECT finished_at FROM storage_cleanup_backfill")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(
        finished.is_none(),
        "the worker still has to run the backfill"
    );
}
