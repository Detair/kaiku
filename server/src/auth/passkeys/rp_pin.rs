//! The RP-ID pin (DC5): `system_settings` `passkeys.rp_id`.
//!
//! Passkeys are bound to the RP ID (the `PUBLIC_URL` host). A changed host
//! with live passkeys refuses startup instead of silently breaking every
//! passkey, unless `PASSKEY_ALLOW_RP_ID_CHANGE=<new host>` revokes them.

use crate::auth::ctx::AuthCtx;

pub const RP_ID_SETTING: &str = "passkeys.rp_id";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RpPinOutcome {
    Pinned,
    Unchanged,
    Moved { from: String, revoked: u64 },
}

/// Check (and pin) the RP ID at startup, after the migrations.
pub async fn check_rp_id(
    ctx: &AuthCtx<'_>,
    rp_id: &str,
    allow_change_to: Option<&str>,
) -> anyhow::Result<RpPinOutcome> {
    let mut tx = ctx.db.begin().await?;
    let inserted_now = sqlx::query(
        "INSERT INTO system_settings (key, value) VALUES ($1, to_jsonb($2::text))
         ON CONFLICT (key) DO NOTHING",
    )
    .bind(RP_ID_SETTING)
    .bind(rp_id)
    .execute(&mut *tx)
    .await?
    .rows_affected()
        == 1;
    if inserted_now {
        tx.commit().await?;
        tracing::info!(rp_id, "Passkey RP ID pinned");
        return Ok(RpPinOutcome::Pinned);
    }
    let pinned: serde_json::Value =
        sqlx::query_scalar("SELECT value FROM system_settings WHERE key = $1 FOR UPDATE")
            .bind(RP_ID_SETTING)
            .fetch_one(&mut *tx)
            .await?;
    let old = pinned.as_str().unwrap_or_default().to_string();
    if old == rp_id {
        tx.commit().await?;
        return Ok(RpPinOutcome::Unchanged);
    }
    let live: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM passkeys WHERE state <> 'revoked'")
        .fetch_one(&mut *tx)
        .await?;
    let revoked = if live == 0 {
        0
    } else if allow_change_to == Some(rp_id) {
        super::queries::revoke_all_live(&mut tx, ctx, "rp_id_changed")
            .await
            .map_err(|e| anyhow::anyhow!("revoking passkeys for the RP ID change failed: {e}"))?
    } else {
        anyhow::bail!(
            "PUBLIC_URL host changed from {old} to {rp_id}; {live} passkeys are bound to {old} \
             and would stop working. Restore PUBLIC_URL, or set \
             PASSKEY_ALLOW_RP_ID_CHANGE={rp_id} to revoke them and start."
        );
    };
    sqlx::query(
        "UPDATE system_settings SET value = to_jsonb($2::text), updated_at = NOW() WHERE key = $1",
    )
    .bind(RP_ID_SETTING)
    .bind(rp_id)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    tracing::warn!(from = %old, to = rp_id, revoked, "Passkey RP ID moved");
    Ok(RpPinOutcome::Moved { from: old, revoked })
}
