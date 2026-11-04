//! `kaiku-admin sessions list|end <username> …` (B.4).

use chrono::{DateTime, Utc};
use uuid::Uuid;

use super::{account, audit, CliCtx, CliIo};
use crate::auth::revocation::{end_all_sessions_tx, end_sessions_tx, EndReason};

/// Print the account's live sessions, most recently used first.
pub async fn list(ctx: &CliCtx, username: &str, io: &mut impl CliIo) -> anyhow::Result<()> {
    type Row = (
        Uuid,
        String,
        String,
        Vec<String>,
        DateTime<Utc>,
        DateTime<Utc>,
        Option<String>,
    );
    let mut tx = ctx.db.begin().await?;
    let user_id = account(&mut tx, username).await?;
    let rows: Vec<Row> = sqlx::query_as(
        "SELECT id, client_class, assurance, auth_methods, created_at, last_used_at, user_agent
           FROM sessions
          WHERE user_id = $1 AND ended_at IS NULL
            AND expires_at > NOW() AND absolute_expires_at > NOW()
          ORDER BY last_used_at DESC",
    )
    .bind(user_id)
    .fetch_all(&mut *tx)
    .await?;
    audit(&mut tx, "cli.sessions.list", user_id, serde_json::json!({})).await?;
    tx.commit().await?;
    if rows.is_empty() {
        io.print(&format!("{username} has no live sessions."));
    }
    for (id, class, assurance, methods, created, used, ua) in rows {
        let device = ua.map_or_else(
            || "unknown device".to_string(),
            |ua| crate::auth::ua_parser::parse_device_name(&ua),
        );
        io.print(&format!(
            "{id}  {class:<7} {assurance:<9} {}  {device}  since {}  last used {}",
            methods.join("+"),
            created.format("%Y-%m-%d %H:%M UTC"),
            used.format("%Y-%m-%d %H:%M UTC"),
        ));
    }
    Ok(())
}

/// End one session, or with `None` every session of the account. The
/// running server closes their sockets (`NOTIFY`, DC5).
pub async fn end(
    ctx: &CliCtx,
    username: &str,
    session: Option<Uuid>,
    io: &mut impl CliIo,
) -> anyhow::Result<()> {
    let auth = ctx.auth();
    let mut tx = ctx.db.begin().await?;
    let user_id = account(&mut tx, username).await?;
    let ended = match session {
        Some(sid) => end_sessions_tx(&mut tx, &auth, user_id, &[sid], EndReason::Cli).await?,
        None => end_all_sessions_tx(&mut tx, &auth, user_id, None, EndReason::Cli).await?,
    };
    if session.is_some() && ended.is_empty() {
        anyhow::bail!(
            "{username} has no live session {}.",
            session.unwrap_or_default()
        );
    }
    audit(
        &mut tx,
        "cli.sessions.end",
        user_id,
        serde_json::json!({ "session_ids": ended }),
    )
    .await?;
    let mut events = Vec::new();
    if !ended.is_empty() {
        events.push(
            crate::security_events::record(
                &mut tx,
                crate::security_events::NewSecurityEvent::new(
                    user_id,
                    crate::security_events::EventType::SessionsEnded,
                    crate::security_events::Actor::Cli,
                    serde_json::json!({ "count": ended.len(), "by": "cli" }),
                ),
            )
            .await?,
        );
    }
    tx.commit().await?;
    super::deliver(ctx, events).await;
    io.print(&format!("Ended {} session(s) of {username}.", ended.len()));
    Ok(())
}
