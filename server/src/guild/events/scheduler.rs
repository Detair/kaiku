//! Background scheduler for guild events: status transitions + reminders.
//!
//! Ticks every 60s (the existing `tokio::interval` pattern). Idempotent via the
//! `reminder_sent` flag and status guards, so a restart never double-fires.

use std::time::Duration;

use chrono::Utc;
use fred::clients::Client;
use sqlx::PgPool;
use uuid::Uuid;

/// How long before `starts_at` a reminder fires.
const REMINDER_WINDOW_MINUTES: i64 = 15;
/// Default duration used to complete an event that has no explicit `ends_at`.
const DEFAULT_DURATION_HOURS: i64 = 2;

/// Spawn the periodic event scheduler. Call once at startup.
pub fn spawn(db: PgPool, redis: Client) {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(Duration::from_mins(1));
        loop {
            ticker.tick().await;
            if let Err(e) = run_tick(&db, &redis).await {
                tracing::warn!(error = %e, "guild-event scheduler tick failed");
            }
        }
    });
}

async fn run_tick(db: &PgPool, redis: &Client) -> Result<(), sqlx::Error> {
    let now = Utc::now();

    // scheduled → active once start time has passed.
    sqlx::query(
        "UPDATE guild_events SET status = 'active'
         WHERE status = 'scheduled' AND starts_at <= $1",
    )
    .bind(now)
    .execute(db)
    .await?;

    // active → completed at ends_at (or starts_at + default when no end).
    sqlx::query(
        "UPDATE guild_events SET status = 'completed'
         WHERE status = 'active'
           AND COALESCE(ends_at, starts_at + ($2 || ' hours')::interval) <= $1",
    )
    .bind(now)
    .bind(DEFAULT_DURATION_HOURS.to_string())
    .execute(db)
    .await?;

    // Fire reminders for events starting within the window (once).
    let due: Vec<(Uuid, Uuid, String)> = sqlx::query_as(
        "SELECT id, guild_id, name FROM guild_events
         WHERE status = 'scheduled' AND reminder_sent = false
           AND starts_at <= $1 + ($2 || ' minutes')::interval
           AND starts_at > $1",
    )
    .bind(now)
    .bind(REMINDER_WINDOW_MINUTES.to_string())
    .fetch_all(db)
    .await?;

    for (event_id, guild_id, name) in due {
        // Mark sent first (idempotent even if delivery fails).
        sqlx::query("UPDATE guild_events SET reminder_sent = true WHERE id = $1")
            .bind(event_id)
            .execute(db)
            .await?;

        // Notify each RSVP'd member (in-app; a push wake-signal once mobile push lands).
        let recipients: Vec<(Uuid,)> =
            sqlx::query_as("SELECT user_id FROM guild_event_rsvps WHERE event_id = $1")
                .bind(event_id)
                .fetch_all(db)
                .await?;
        for (user_id,) in recipients {
            let ev = crate::ws::ServerEvent::GuildEventReminder {
                guild_id,
                event_id,
                name: name.clone(),
            };
            if let Err(e) = crate::ws::broadcast_to_user(redis, user_id, &ev).await {
                tracing::warn!(error = %e, "failed to deliver event reminder");
            }
        }
    }

    Ok(())
}
