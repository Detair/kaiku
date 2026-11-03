//! `kaiku-admin` recovery commands (F6 item 4; L1b Task 28).
//!
//! Each runs as one B.7 transition with `Actor::Cli` under the account lock,
//! writes its `cli.*` audit row and records its C28 event (high events reach
//! the account's contact and every admin).
//! They work on admin accounts too.

use uuid::Uuid;

use super::{account, audit, deliver, CliCtx, CliIo};
use crate::approvals::transition::lock_accounts;
use crate::auth::factors::{self, ExitPath, Factor};
use crate::security_events::{record, Actor, EventType, NewSecurityEvent};

/// Clear a throttled or compromised password or TOTP (B.1 exit `cli`) and
/// its per-account counter.
pub async fn factor_reset(
    ctx: &CliCtx,
    username: &str,
    factor: Factor,
    io: &mut impl CliIo,
) -> anyhow::Result<()> {
    let mut tx = ctx.db.begin().await?;
    let user = account(&mut tx, username).await?;
    lock_accounts(&mut tx, &[user]).await?;
    let before = factors::states(&mut tx, user).await?;
    let was = match factor {
        Factor::Password => before.password.state,
        _ => before.totp.state,
    };
    let changed = factors::clear_throttle_tx(&mut tx, user, factor, ExitPath::Cli).await?;
    audit(
        &mut tx,
        "cli.factor.reset",
        user,
        serde_json::json!({ "factor": factor.as_str(), "changed": changed }),
    )
    .await?;
    let mut events = Vec::new();
    if changed {
        // C28: `factor_restored` (high) / `throttle_reset` (normal).
        let event_type = if was == factors::FactorState::Compromised {
            EventType::FactorRestored
        } else {
            EventType::ThrottleReset
        };
        events.push(
            record(
                &mut tx,
                NewSecurityEvent::new(
                    user,
                    event_type,
                    Actor::Cli,
                    serde_json::json!({ "factor": factor.as_str(), "by": "cli" }),
                ),
            )
            .await?,
        );
    }
    tx.commit().await?;
    deliver(ctx, events).await;
    reset_counters(ctx, username, user, Some(factor)).await;
    io.print(&if changed {
        format!("The {} of {username} is active again.", factor.as_str())
    } else {
        format!("The {} of {username} was not blocked.", factor.as_str())
    });
    Ok(())
}

/// Clear the throttles of password and TOTP and their counters.
pub async fn throttle_reset(
    ctx: &CliCtx,
    username: &str,
    io: &mut impl CliIo,
) -> anyhow::Result<()> {
    let mut tx = ctx.db.begin().await?;
    let user = account(&mut tx, username).await?;
    lock_accounts(&mut tx, &[user]).await?;
    let st = factors::states(&mut tx, user).await?;
    let mut cleared = Vec::new();
    for (f, state) in [
        (Factor::Password, st.password.state),
        (Factor::Totp, st.totp.state),
    ] {
        if state == factors::FactorState::Throttled
            && factors::clear_throttle_tx(&mut tx, user, f, ExitPath::Cli).await?
        {
            cleared.push(f.as_str());
        }
    }
    audit(
        &mut tx,
        "cli.throttle.reset",
        user,
        serde_json::json!({ "cleared": cleared }),
    )
    .await?;
    let mut events = Vec::new();
    for f in &cleared {
        events.push(
            record(
                &mut tx,
                NewSecurityEvent::new(
                    user,
                    EventType::ThrottleReset,
                    Actor::Cli,
                    serde_json::json!({ "factor": f, "by": "cli" }),
                ),
            )
            .await?,
        );
    }
    tx.commit().await?;
    deliver(ctx, events).await;
    reset_counters(ctx, username, user, None).await;
    io.print(&format!(
        "Cleared the sign-in delays of {username}{}.",
        if cleared.is_empty() {
            String::new()
        } else {
            format!(" ({})", cleared.join(", "))
        }
    ));
    Ok(())
}

async fn reset_counters(ctx: &CliCtx, username: &str, user: Uuid, only: Option<Factor>) {
    use crate::ratelimit::account::{AccountLimiter, CounterKey};
    let limiter = AccountLimiter::new(&ctx.redis);
    if only.is_none_or(|f| f == Factor::Password) {
        let _ = limiter
            .record_success(&CounterKey::password(username))
            .await;
    }
    if only.is_none_or(|f| f == Factor::Totp) {
        let _ = limiter.record_success(&CounterKey::Totp(user)).await;
    }
}

/// Turn phone confirmations (`may_approve`) on or off.
pub async fn may_approve(
    ctx: &CliCtx,
    username: &str,
    on: bool,
    io: &mut impl CliIo,
) -> anyhow::Result<()> {
    let mut tx = ctx.db.begin().await?;
    let user = account(&mut tx, username).await?;
    lock_accounts(&mut tx, &[user]).await?;
    sqlx::query("UPDATE users SET may_approve = $2 WHERE id = $1")
        .bind(user)
        .bind(on)
        .execute(&mut *tx)
        .await?;
    audit(
        &mut tx,
        "cli.may_approve",
        user,
        serde_json::json!({ "may_approve": on }),
    )
    .await?;
    let event = record(
        &mut tx,
        NewSecurityEvent::new(
            user,
            EventType::MayApproveChanged,
            Actor::Cli,
            serde_json::json!({ "may_approve": on, "by": "cli" }),
        ),
    )
    .await?;
    tx.commit().await?;
    deliver(ctx, vec![event]).await;
    io.print(&format!(
        "Phone confirmations for {username} are {}.",
        if on { "on" } else { "off" }
    ));
    Ok(())
}

/// Name (or with `None` remove) the account's recovery contact.
pub async fn recovery_contact(
    ctx: &CliCtx,
    username: &str,
    contact: Option<&str>,
    io: &mut impl CliIo,
) -> anyhow::Result<()> {
    let auth = ctx.auth();
    let mut tx = ctx.db.begin().await?;
    let user = account(&mut tx, username).await?;
    let contact_id = match contact {
        Some(c) => Some(account(&mut tx, c).await?),
        None => None,
    };
    let (_, events) = crate::auth::recovery_contact::set_tx(
        &mut tx,
        &auth,
        user,
        contact_id,
        crate::auth::recovery_contact::ContactActor::Cli,
    )
    .await
    .map_err(|e| anyhow::anyhow!("{e}"))?;
    audit(
        &mut tx,
        "cli.recovery_contact",
        user,
        serde_json::json!({ "contact_user_id": contact_id }),
    )
    .await?;
    tx.commit().await?;
    deliver(ctx, events).await;
    io.print(&match contact {
        Some(c) => {
            format!("{c} is now the recovery contact of {username}; they confirm it in the app.")
        }
        None => format!("{username} has no recovery contact now."),
    });
    Ok(())
}

/// Remove the system admin role (and its open `admin` requests).
pub async fn admin_demote(ctx: &CliCtx, username: &str, io: &mut impl CliIo) -> anyhow::Result<()> {
    let auth = ctx.auth();
    let mut tx = ctx.db.begin().await?;
    let user = account(&mut tx, username).await?;
    lock_accounts(&mut tx, &[user]).await?;
    let n = sqlx::query("DELETE FROM system_admins WHERE user_id = $1")
        .bind(user)
        .execute(&mut *tx)
        .await?
        .rows_affected();
    if n == 0 {
        anyhow::bail!("{username} is no admin.");
    }
    sqlx::query("DELETE FROM elevated_sessions WHERE user_id = $1")
        .bind(user)
        .execute(&mut *tx)
        .await?;
    crate::auth::account_requests::core::on_admin_demoted(&mut tx, &auth, user)
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    let event = record(
        &mut tx,
        NewSecurityEvent::new(
            user,
            EventType::AdminRoleRemoved,
            Actor::Cli,
            serde_json::json!({ "by_user_id": null }),
        ),
    )
    .await?;
    audit(&mut tx, "cli.admin.demote", user, serde_json::json!({})).await?;
    tx.commit().await?;
    deliver(ctx, vec![event]).await;
    io.print(&format!("{username} is no longer an admin."));
    Ok(())
}

/// Cancel an open account request (as the CLI; a `self` request's factor
/// generations are compromised as on any cancel).
pub async fn request_cancel(
    ctx: &CliCtx,
    request: Uuid,
    io: &mut impl CliIo,
) -> anyhow::Result<()> {
    let auth = ctx.auth();
    let mut tx = ctx.db.begin().await?;
    let row = crate::auth::account_requests::core::lock_by_id(&mut tx, request)
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))?
        .ok_or_else(|| anyhow::anyhow!("No request {request}."))?;
    let events = crate::auth::account_requests::core::cancel_locked_tx(
        &mut tx,
        &auth,
        &row,
        &crate::auth::owner_rule::OwnerAuthority::Cli,
        crate::system_account::SYSTEM_USER_ID,
        Some("kaiku-admin"),
    )
    .await
    .map_err(|e| anyhow::anyhow!("{e}"))?;
    audit(
        &mut tx,
        "cli.request.cancel",
        row.user_id,
        serde_json::json!({ "request_id": request }),
    )
    .await?;
    tx.commit().await?;
    deliver(ctx, events).await;
    io.print(&format!("Cancelled request {request}."));
    Ok(())
}
