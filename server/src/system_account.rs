//! The built-in "Kaiku" system account (spec Part C, contract C28).
//!
//! It sends security notices into one read-only DM per user. It has no
//! password, no session and no avatar; every auth path refuses it with that
//! path's normal unknown-user answer, and it is hidden from user lists,
//! search, presence, mentions, friend and bot paths. Clients recognise it
//! only by `is_system: true`.

use sqlx::PgConnection;
use uuid::{uuid, Uuid};

/// The fixed id of the system account (created by the migration).
pub const SYSTEM_USER_ID: Uuid = uuid!("6b61696b-7500-4000-8000-000000000001");

/// The reserved username of the system account.
pub const SYSTEM_USERNAME: &str = "kaiku";

/// Display name of the system account and of its DMs.
pub const SYSTEM_DISPLAY_NAME: &str = "Kaiku";

/// Whether `id` is the system account.
#[must_use]
pub fn is_system_user(id: Uuid) -> bool {
    id == SYSTEM_USER_ID
}

/// The user's Kaiku DM: get it, or create it (`read_only`).
///
/// Only this function creates DMs with the system account. It is serialised
/// per user with a transaction-scoped advisory lock, so concurrent notices
/// never create two DMs.
#[tracing::instrument(skip(conn))]
pub async fn kaiku_dm_tx(conn: &mut PgConnection, user_id: Uuid) -> sqlx::Result<Uuid> {
    Ok(kaiku_dm_created_tx(conn, user_id).await?.0)
}

/// [`kaiku_dm_tx`], also telling whether the DM was created just now.
pub async fn kaiku_dm_created_tx(
    conn: &mut PgConnection,
    user_id: Uuid,
) -> sqlx::Result<(Uuid, bool)> {
    // Per-user advisory key, namespaced by the "kaiku_dm:" prefix.
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended('kaiku_dm:' || $1::text, 0))")
        .bind(user_id)
        .execute(&mut *conn)
        .await?;
    if let Some(id) = sqlx::query_scalar::<_, Uuid>(
        "SELECT c.id FROM channels c
           JOIN dm_participants me ON me.channel_id = c.id AND me.user_id = $1
           JOIN dm_participants sys ON sys.channel_id = c.id AND sys.user_id = $2
          WHERE c.channel_type = 'dm' AND c.read_only
          ORDER BY c.created_at
          LIMIT 1",
    )
    .bind(user_id)
    .bind(SYSTEM_USER_ID)
    .fetch_optional(&mut *conn)
    .await?
    {
        return Ok((id, false));
    }
    let id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO channels (id, name, channel_type, guild_id, position, read_only)
         VALUES ($1, $2, 'dm', NULL, 0, true)",
    )
    .bind(id)
    .bind(SYSTEM_DISPLAY_NAME)
    .execute(&mut *conn)
    .await?;
    sqlx::query("INSERT INTO dm_participants (channel_id, user_id) VALUES ($1, $2), ($1, $3)")
        .bind(id)
        .bind(user_id)
        .bind(SYSTEM_USER_ID)
        .execute(&mut *conn)
        .await?;
    Ok((id, true))
}
