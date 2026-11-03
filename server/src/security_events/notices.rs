//! Notice delivery: security events become coalesced messages in each
//! recipient's Kaiku DM, with a `t=security` push (contract C28, B.5).
//!
//! Delivery is an outbox (DC11): `security_events.delivered_at` is null
//! until the event's notices exist. [`spawn_delivery`] runs right after the
//! recording transaction commits; the minute sweep ([`deliver_pending`])
//! re-drives anything left over (a crash, a CLI action). Each event is
//! delivered under its own row lock (`FOR UPDATE SKIP LOCKED`), so a
//! concurrent sweep never delivers it twice.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::PgConnection;
use uuid::Uuid;

use super::push::SecurityPush;
use super::queries::{events_of_notices, EventRow};
use super::types::{EventType, SecurityEvent, Severity};
use crate::api::AppState;
use crate::system_account::{kaiku_dm_created_tx, SYSTEM_USER_ID};
use crate::ws::{broadcast_to_user, ServerEvent};

/// Events within this window of a notice's first event update that notice.
pub const COALESCE_WINDOW_SECS: i64 = 600;
/// At most one push per notice per this many seconds while it is updated.
pub const PUSH_INTERVAL_SECS: i64 = 60;
/// A notice carries at most this many events (newest first).
pub const MAX_NOTICE_EVENTS: usize = 50;

/// Who receives a notice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Recipient {
    pub user_id: Uuid,
    pub role: RecipientRole,
}

/// Why a recipient gets the notice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecipientRole {
    /// The account's own notice.
    Subject,
    /// A copy for an admin (high severity).
    Admin,
    /// A copy for the confirmed recovery contact (high severity, L1b).
    Contact,
}

/// `SecurityNotice.subject` (C28).
#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub struct NoticeSubject {
    pub user_id: Uuid,
    pub username: String,
    pub display_name: String,
}

/// `SecurityNotice` (C28): one coalesced notice as one reader sees it.
#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub struct SecurityNotice {
    pub notice_id: Uuid,
    pub subject: NoticeSubject,
    pub event_type: String,
    /// `high` or `normal`.
    pub severity: String,
    pub count: i32,
    pub first_at: DateTime<Utc>,
    pub last_at: DateTime<Utc>,
    /// Newest first, at most 50.
    pub events: Vec<SecurityEvent>,
    pub events_truncated: bool,
}

/// The recipients of an event on `subject_id`.
///
/// The subject's own copy; for `high` the confirmed recovery contact and
/// every admin other than the subject; the contact events also to the
/// contacts they name. Never the system account, never twice.
pub async fn recipients(
    conn: &mut PgConnection,
    subject_id: Uuid,
    severity: Severity,
    event_type: Option<EventType>,
    details: &serde_json::Value,
) -> sqlx::Result<Vec<Recipient>> {
    let mut out = vec![Recipient {
        user_id: subject_id,
        role: RecipientRole::Subject,
    }];
    // The confirmed recovery contact gets high notices (Part C); the
    // contact events also go to the contact they name (C28).
    let mut contacts: Vec<Uuid> = Vec::new();
    if severity == Severity::High {
        let confirmed: Option<Uuid> = sqlx::query_scalar(
            "SELECT recovery_contact_id FROM users
              WHERE id = $1 AND recovery_contact_confirmed_at IS NOT NULL",
        )
        .bind(subject_id)
        .fetch_optional(&mut *conn)
        .await?
        .flatten();
        contacts.extend(confirmed);
    }
    if matches!(
        event_type,
        Some(
            EventType::RecoveryContactNamed
                | EventType::RecoveryContactConfirmed
                | EventType::RecoveryContactChanged
        )
    ) {
        for key in ["contact_user_id", "previous_contact_user_id"] {
            if let Some(id) = details[key].as_str().and_then(|s| s.parse::<Uuid>().ok()) {
                contacts.push(id);
            }
        }
    }
    for c in contacts {
        if c != subject_id && !out.iter().any(|r| r.user_id == c) {
            out.push(Recipient {
                user_id: c,
                role: RecipientRole::Contact,
            });
        }
    }
    if severity == Severity::High {
        let admins: Vec<Uuid> = sqlx::query_scalar(
            "SELECT sa.user_id FROM system_admins sa
               JOIN users u ON u.id = sa.user_id
              WHERE sa.user_id <> $1 AND NOT u.is_system
              ORDER BY sa.user_id",
        )
        .bind(subject_id)
        .fetch_all(&mut *conn)
        .await?;
        for user_id in admins {
            if !out.iter().any(|r| r.user_id == user_id) {
                out.push(Recipient {
                    user_id,
                    role: RecipientRole::Admin,
                });
            }
        }
    }
    Ok(out)
}

/// What one delivery did for one recipient (acted on after commit).
#[derive(Debug)]
struct Outcome {
    recipient: Uuid,
    channel_id: Uuid,
    message_id: Uuid,
    notice_id: Uuid,
    created_message: bool,
    created_dm: bool,
    push: Option<SecurityPush>,
}

#[derive(sqlx::FromRow)]
struct PendingEvent {
    id: Uuid,
    subject_id: Uuid,
    event_type: String,
    severity: String,
    actor_kind: String,
    actor_user_id: Option<Uuid>,
    created_at: DateTime<Utc>,
}

#[derive(sqlx::FromRow)]
struct NoticeRow {
    id: Uuid,
    message_id: Uuid,
    count: i32,
    last_pushed_at: Option<DateTime<Utc>>,
}

/// Deliver these events (idempotent: an event already delivered, or being
/// delivered by someone else, is skipped).
#[tracing::instrument(skip(state, event_ids), fields(count = event_ids.len()))]
pub async fn deliver(state: &AppState, event_ids: &[Uuid]) -> anyhow::Result<()> {
    let mut ordered: Vec<(DateTime<Utc>, Uuid)> =
        sqlx::query_as("SELECT created_at, id FROM security_events WHERE id = ANY($1)")
            .bind(event_ids)
            .fetch_all(&state.db)
            .await?;
    ordered.sort();
    for (_, id) in ordered {
        let outcomes = deliver_one(state, id).await?;
        after_commit(state, outcomes).await;
    }
    Ok(())
}

async fn deliver_one(state: &AppState, id: Uuid) -> anyhow::Result<Vec<Outcome>> {
    let mut tx = state.db.begin().await?;
    let Some(ev) = sqlx::query_as::<_, PendingEvent>(
        "SELECT id, subject_id, event_type, severity, actor_kind, actor_user_id, created_at
           FROM security_events
          WHERE id = $1 AND delivered_at IS NULL
          FOR UPDATE SKIP LOCKED",
    )
    .bind(id)
    .fetch_optional(&mut *tx)
    .await?
    else {
        tx.commit().await?;
        return Ok(Vec::new());
    };
    let severity = Severity::parse(&ev.severity).unwrap_or(Severity::High);
    let event_type = EventType::parse(&ev.event_type);
    let actor_id = match ev.actor_kind.as_str() {
        "admin" | "recovery_contact" => ev.actor_user_id,
        _ => None,
    };
    let mut outcomes = Vec::new();
    let details: serde_json::Value =
        sqlx::query_scalar("SELECT details FROM security_events WHERE id = $1")
            .bind(ev.id)
            .fetch_one(&mut *tx)
            .await?;
    for r in recipients(&mut tx, ev.subject_id, severity, event_type, &details).await? {
        let (channel_id, created_dm) = kaiku_dm_created_tx(&mut tx, r.user_id).await?;
        let existing = sqlx::query_as::<_, NoticeRow>(
            "SELECT id, message_id, count, last_pushed_at FROM security_notices
              WHERE recipient_id = $1 AND subject_id = $2 AND event_type = $3
                AND first_at > $4 - make_interval(secs => $5)
              ORDER BY first_at DESC
              LIMIT 1
              FOR UPDATE",
        )
        .bind(r.user_id)
        .bind(ev.subject_id)
        .bind(&ev.event_type)
        .bind(ev.created_at)
        .bind(COALESCE_WINDOW_SECS as f64)
        .fetch_optional(&mut *tx)
        .await?;
        let (notice_id, message_id, count, push_now, created_message) = if let Some(n) = existing {
            let push_now = n
                .last_pushed_at
                .is_none_or(|t| Utc::now() - t >= chrono::Duration::seconds(PUSH_INTERVAL_SECS));
            let count = n.count + 1;
            sqlx::query(
                "UPDATE security_notices
                    SET count = count + 1,
                        severity = CASE WHEN $2 = 'high' THEN 'high' ELSE severity END,
                        first_at = LEAST(first_at, $3),
                        last_at = GREATEST(last_at, $3),
                        last_pushed_at = CASE WHEN $4 THEN NOW() ELSE last_pushed_at END
                  WHERE id = $1",
            )
            .bind(n.id)
            .bind(severity.as_str())
            .bind(ev.created_at)
            .bind(push_now)
            .execute(&mut *tx)
            .await?;
            sqlx::query("UPDATE messages SET content = $2, edited_at = NOW() WHERE id = $1")
                .bind(n.message_id)
                .bind(fallback_content(&ev.event_type, count))
                .execute(&mut *tx)
                .await?;
            (n.id, n.message_id, count, push_now, false)
        } else {
            let notice_id = Uuid::now_v7();
            let message_id = Uuid::now_v7();
            sqlx::query(
                "INSERT INTO messages (id, channel_id, user_id, content, message_type)
                 VALUES ($1, $2, $3, $4, 'security')",
            )
            .bind(message_id)
            .bind(channel_id)
            .bind(SYSTEM_USER_ID)
            .bind(fallback_content(&ev.event_type, 1))
            .execute(&mut *tx)
            .await?;
            sqlx::query(
                "INSERT INTO security_notices
                    (id, recipient_id, subject_id, event_type, severity, channel_id, message_id,
                     count, first_at, last_at, last_pushed_at)
                 VALUES ($1, $2, $3, $4, $5, $6, $7, 1, $8, $8, NOW())",
            )
            .bind(notice_id)
            .bind(r.user_id)
            .bind(ev.subject_id)
            .bind(&ev.event_type)
            .bind(severity.as_str())
            .bind(channel_id)
            .bind(message_id)
            .bind(ev.created_at)
            .execute(&mut *tx)
            .await?;
            (notice_id, message_id, 1, true, true)
        };
        sqlx::query(
            "INSERT INTO security_notice_events (notice_id, event_id) VALUES ($1, $2)
             ON CONFLICT DO NOTHING",
        )
        .bind(notice_id)
        .bind(ev.id)
        .execute(&mut *tx)
        .await?;
        let push = match (push_now, event_type) {
            (true, Some(event_type)) => Some(SecurityPush {
                notice_id,
                event_type,
                severity,
                count: u32::try_from(count).unwrap_or(u32::MAX),
                subject_id: ev.subject_id,
                actor_id,
            }),
            _ => None,
        };
        outcomes.push(Outcome {
            recipient: r.user_id,
            channel_id,
            message_id,
            notice_id,
            created_message,
            created_dm,
            push,
        });
    }
    sqlx::query("UPDATE security_events SET delivered_at = NOW() WHERE id = $1")
        .bind(ev.id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(outcomes)
}

/// WebSocket hints and pushes, after the delivery committed. Failures are
/// logged; the notice itself is durable.
async fn after_commit(state: &AppState, outcomes: Vec<Outcome>) {
    for o in outcomes {
        if o.created_dm {
            match crate::chat::dm::dm_list_entry_by_id(&state.db, o.recipient, o.channel_id).await {
                Ok(Some(entry)) => {
                    let event = ServerEvent::DmCreated {
                        channel: serde_json::to_value(&entry).unwrap_or_default(),
                    };
                    if let Err(e) = broadcast_to_user(&state.redis, o.recipient, &event).await {
                        tracing::warn!(error = %e, "notice: DmCreated hint failed");
                    }
                }
                Ok(None) => {}
                Err(e) => tracing::warn!(error = %e, "notice: loading the Kaiku DM failed"),
            }
        }
        let event = if o.created_message {
            message_event(state, o.recipient, o.channel_id, o.message_id).await
        } else {
            notice_for_reader(state, o.recipient, o.notice_id)
                .await
                .map(|notice| ServerEvent::SecurityNoticeUpdated {
                    channel_id: o.channel_id,
                    message_id: o.message_id,
                    security_notice: serde_json::to_value(&notice).unwrap_or_default(),
                })
        };
        if let Some(event) = event {
            if let Err(e) = broadcast_to_user(&state.redis, o.recipient, &event).await {
                tracing::warn!(error = %e, "notice: WebSocket hint failed");
            }
        }
        if let Some(push) = o.push {
            if let Err(e) =
                crate::push::deliver_with(&state.db, &state.push, o.recipient, &push).await
            {
                tracing::warn!(error = %e, "notice: push failed");
            }
        }
    }
}

async fn message_event(
    state: &AppState,
    recipient: Uuid,
    channel_id: Uuid,
    message_id: Uuid,
) -> Option<ServerEvent> {
    let msg = crate::db::find_message_by_id(&state.db, message_id)
        .await
        .ok()??;
    let responses = crate::chat::messages::build_message_responses(&state.db, recipient, vec![msg])
        .await
        .ok()?;
    let message = serde_json::to_value(responses.into_iter().next()?).ok()?;
    Some(ServerEvent::MessageNew {
        channel_id,
        message,
    })
}

async fn notice_for_reader(
    state: &AppState,
    reader: Uuid,
    notice_id: Uuid,
) -> Option<SecurityNotice> {
    let mut conn = state.db.acquire().await.ok()?;
    let map = notices_by_message(&mut conn, reader, None, Some(&[notice_id]))
        .await
        .ok()?;
    map.into_values().next()
}

/// Run [`deliver`] in the background (after the recording transaction
/// committed). Errors are logged; the sweep retries.
pub fn spawn_delivery(state: &AppState, event_ids: Vec<Uuid>) {
    if event_ids.is_empty() {
        return;
    }
    let state = state.clone();
    tokio::spawn(async move {
        if let Err(e) = deliver(&state, &event_ids).await {
            tracing::warn!(error = %e, "notice delivery failed; the sweep retries");
        }
    });
}

/// Deliver every undelivered event in the background (after a transition
/// that recorded events without returning their ids, e.g. a cascade).
pub fn spawn_pending_delivery(state: &AppState) {
    let state = state.clone();
    tokio::spawn(async move {
        if let Err(e) = deliver_pending(&state).await {
            tracing::warn!(error = %e, "notice delivery failed; the sweep retries");
        }
    });
}

/// The sweep step: deliver every undelivered event (oldest first, bounded).
/// Returns how many events were picked up.
pub async fn deliver_pending(state: &AppState) -> anyhow::Result<usize> {
    let ids: Vec<Uuid> = sqlx::query_scalar(
        "SELECT id FROM security_events WHERE delivered_at IS NULL
          ORDER BY created_at LIMIT 500",
    )
    .fetch_all(&state.db)
    .await?;
    let n = ids.len();
    deliver(state, &ids).await?;
    Ok(n)
}

/// The notices of `reader`, keyed by message id.
///
/// Selected by message (`message_ids`) or by notice id (`notice_ids`). `ip`
/// and `place` are shown only on the reader's own notices (Part C: never in
/// a copy).
pub async fn notices_by_message(
    conn: &mut PgConnection,
    reader: Uuid,
    message_ids: Option<&[Uuid]>,
    notice_ids: Option<&[Uuid]>,
) -> sqlx::Result<HashMap<Uuid, SecurityNotice>> {
    #[derive(sqlx::FromRow)]
    struct Row {
        id: Uuid,
        message_id: Uuid,
        subject_id: Uuid,
        username: String,
        display_name: String,
        event_type: String,
        severity: String,
        count: i32,
        first_at: DateTime<Utc>,
        last_at: DateTime<Utc>,
    }
    let rows: Vec<Row> = sqlx::query_as(
        "SELECT n.id, n.message_id, n.subject_id, u.username, u.display_name, n.event_type,
                n.severity, n.count, n.first_at, n.last_at
           FROM security_notices n
           JOIN users u ON u.id = n.subject_id
          WHERE n.recipient_id = $1
            AND ($2::uuid[] IS NULL OR n.message_id = ANY($2))
            AND ($3::uuid[] IS NULL OR n.id = ANY($3))",
    )
    .bind(reader)
    .bind(message_ids)
    .bind(notice_ids)
    .fetch_all(&mut *conn)
    .await?;
    if rows.is_empty() {
        return Ok(HashMap::new());
    }
    let ids: Vec<Uuid> = rows.iter().map(|r| r.id).collect();
    let mut events: HashMap<Uuid, Vec<EventRow>> = HashMap::new();
    for (notice_id, ev) in events_of_notices(conn, &ids).await? {
        events.entry(notice_id).or_default().push(ev);
    }
    let mut out = HashMap::with_capacity(rows.len());
    for r in rows {
        let own = r.subject_id == reader;
        let evs = events.remove(&r.id).unwrap_or_default();
        let total = evs.len();
        let mut wire = Vec::with_capacity(total.min(MAX_NOTICE_EVENTS));
        for ev in evs.iter().take(MAX_NOTICE_EVENTS) {
            let label = authenticator_label(conn, ev).await?;
            wire.push(ev.to_wire(own, label));
        }
        out.insert(
            r.message_id,
            SecurityNotice {
                notice_id: r.id,
                subject: NoticeSubject {
                    user_id: r.subject_id,
                    username: r.username,
                    display_name: r.display_name,
                },
                event_type: r.event_type,
                severity: r.severity,
                count: r.count,
                first_at: r.first_at,
                last_at: r.last_at,
                events: wire,
                events_truncated: total > MAX_NOTICE_EVENTS || i64::from(r.count) > total as i64,
            },
        );
    }
    Ok(out)
}

/// The current display label of an event's authenticator, if any.
pub async fn authenticator_label(
    conn: &mut PgConnection,
    ev: &EventRow,
) -> sqlx::Result<Option<String>> {
    match crate::auth::strong::AuthenticatorRef::from_columns(
        ev.authenticator_type.as_deref(),
        ev.authenticator_id,
    ) {
        Some(a) => crate::auth::strong::authenticator_label(conn, a).await,
        None => Ok(None),
    }
}

/// English fallback text of a notice message (C16: clients render their
/// own text from `security_notice`; older clients show this).
fn fallback_content(event_type: &str, count: i32) -> String {
    let what = event_type.replace('_', " ");
    if count > 1 {
        format!("Security notice: {what} ({count})")
    } else {
        format!("Security notice: {what}")
    }
}
