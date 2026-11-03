//! Read-only channels: the Kaiku DM (C28, DC10).

use sqlx::PgExecutor;
use uuid::Uuid;

use super::ChatError;

/// Refuse every mutation of a read-only channel (send, edit, delete, react,
/// pin, typing, leave, rename, icon) with 403 `CHANNEL_READ_ONLY`. A missing
/// channel passes: the caller's own lookup answers it.
pub async fn ensure_writable<'e, E: PgExecutor<'e>>(
    ex: E,
    channel_id: Uuid,
) -> Result<(), ChatError> {
    if is_read_only(ex, channel_id).await? {
        return Err(ChatError::ChannelReadOnly);
    }
    Ok(())
}

/// Whether the channel is read-only.
pub async fn is_read_only<'e, E: PgExecutor<'e>>(ex: E, channel_id: Uuid) -> sqlx::Result<bool> {
    Ok(
        sqlx::query_scalar::<_, bool>("SELECT read_only FROM channels WHERE id = $1")
            .bind(channel_id)
            .fetch_optional(ex)
            .await?
            .unwrap_or(false),
    )
}
