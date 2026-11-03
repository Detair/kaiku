//! Run migrations in two steps on an empty database, for backfill tests
//! (`#[sqlx::test(migrations = false)]`).

use sqlx::PgPool;

fn migrator() -> sqlx::migrate::Migrator {
    sqlx::migrate!("./migrations")
}

async fn apply(pool: &PgPool, keep: impl Fn(i64) -> bool) {
    for m in migrator().iter() {
        if m.migration_type.is_down_migration() || !keep(m.version) {
            continue;
        }
        sqlx::raw_sql(m.sql.clone())
            .execute(pool)
            .await
            .unwrap_or_else(|e| panic!("migration {} failed: {e}", m.version));
    }
}

/// Apply every migration with a version below `version`.
pub async fn apply_until(pool: &PgPool, version: i64) {
    apply(pool, |v| v < version).await;
}

/// Apply every migration from `version` on.
pub async fn apply_from(pool: &PgPool, version: i64) {
    apply(pool, |v| v >= version).await;
}
