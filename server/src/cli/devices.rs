//! `kaiku-admin devices list|remove <username> …` (SM4).

use uuid::Uuid;

use super::{account, audit, CliCtx, CliIo};
use crate::approvals::devices::{after_revoke, find, revoke_tx, KeyDeviceRow, RevokeReason};
use crate::approvals::error::ApprovalError;
use crate::security_events::Actor;

fn when(t: Option<chrono::DateTime<chrono::Utc>>) -> String {
    t.map_or_else(
        || "never".to_string(),
        |t| t.format("%Y-%m-%d %H:%M UTC").to_string(),
    )
}

/// Print the account's key devices (every state).
pub async fn list(ctx: &CliCtx, username: &str, io: &mut impl CliIo) -> anyhow::Result<()> {
    let mut tx = ctx.db.begin().await?;
    let user_id = account(&mut tx, username).await?;
    let rows: Vec<KeyDeviceRow> =
        sqlx::query_as("SELECT * FROM approval_devices WHERE user_id = $1 ORDER BY created_at")
            .bind(user_id)
            .fetch_all(&mut *tx)
            .await?;
    audit(&mut tx, "cli.devices.list", user_id, serde_json::json!({})).await?;
    tx.commit().await?;
    if rows.is_empty() {
        io.print(&format!("{username} has no key phones."));
    }
    for d in rows {
        io.print(&format!(
            "{}  {:<8} '{}'  via {}  added {}  last used {}",
            d.id,
            d.state,
            d.label,
            d.enrolled_via,
            when(Some(d.created_at)),
            when(d.last_used_at),
        ));
    }
    Ok(())
}

/// Remove (revoke) a key device: its sessions end, its challenges and
/// grants are void, the account and all admins get a notice.
pub async fn remove(
    ctx: &CliCtx,
    username: &str,
    device: Uuid,
    io: &mut impl CliIo,
) -> anyhow::Result<()> {
    let auth = ctx.auth();
    let mut tx = ctx.db.begin().await?;
    let user_id = account(&mut tx, username).await?;
    // B.7: the account lock first, as every revoking route.
    crate::approvals::transition::lock_route_change(
        &mut tx,
        user_id,
        &[],
        &[crate::auth::strong::AuthenticatorRef::key_device(device)],
    )
    .await?;
    let row = find(&mut tx, device)
        .await?
        .filter(|d| d.user_id == user_id)
        .ok_or_else(|| anyhow::anyhow!("{username} has no key phone {device}."))?;
    let (revoked, event_ids) =
        match revoke_tx(&mut tx, &auth, row.id, RevokeReason::Cli, Actor::Cli).await {
            Ok(r) => r,
            Err(ApprovalError::KeyDeviceRevoked) => {
                anyhow::bail!("Key phone {device} was already removed.")
            }
            Err(e) => return Err(e.into()),
        };
    audit(
        &mut tx,
        "cli.devices.remove",
        user_id,
        serde_json::json!({ "key_device_id": revoked.id }),
    )
    .await?;
    // Built before the commit: a failure here must not skip the notice and
    // the push clean-up of a committed revocation.
    let state = ctx.app_state()?;
    tx.commit().await?;
    after_revoke(&state, user_id, row.device_id, event_ids).await;
    io.print(&format!(
        "Removed '{}' from {username}; its sessions have ended.",
        revoked.label
    ));
    Ok(())
}
