//! Reading security events (the log and the notice payloads).

use chrono::{DateTime, Utc};
use sqlx::{PgConnection, PgExecutor};
use uuid::Uuid;

use super::types::{EventActor, EventAuthenticator, EventClient, SecurityEvent};

/// One stored event with the actor's username.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct EventRow {
    pub id: Uuid,
    pub subject_id: Uuid,
    pub event_type: String,
    pub severity: String,
    pub actor_kind: String,
    pub actor_user_id: Option<Uuid>,
    pub actor_username: Option<String>,
    pub client_class: Option<String>,
    pub client_label: Option<String>,
    pub ip: Option<String>,
    pub place: Option<String>,
    pub session_id: Option<Uuid>,
    pub authenticator_type: Option<String>,
    pub authenticator_id: Option<Uuid>,
    pub request_id: Option<Uuid>,
    pub details: serde_json::Value,
    pub repudiated_at: Option<DateTime<Utc>>,
    pub delivered_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

/// The column list every event read uses (`e` = `security_events`,
/// `a` = the actor's `users` row). A macro so the SQL stays `&'static str`.
macro_rules! event_columns {
    () => {
        "e.id, e.subject_id, e.event_type, e.severity, e.actor_kind,
         e.actor_user_id, a.username AS actor_username, e.client_class, e.client_label,
         host(e.ip) AS ip, e.place, e.session_id, e.authenticator_type, e.authenticator_id,
         e.request_id, e.details, e.repudiated_at, e.delivered_at, e.created_at"
    };
}

impl EventRow {
    /// The C28 wire form. `reveal_network` adds `ip` and `place` (only the
    /// owner's own log and notices, and the admin log, Part C); `label` is
    /// the authenticator's current display label.
    #[must_use]
    pub fn to_wire(&self, reveal_network: bool, label: Option<String>) -> SecurityEvent {
        let client = match (&self.client_class, &self.client_label) {
            (Some(class), label) => Some(EventClient {
                class: class.clone(),
                label: label.clone().unwrap_or_default(),
            }),
            (None, _) => None,
        };
        let authenticator = match (&self.authenticator_type, self.authenticator_id) {
            (Some(kind), Some(id)) => Some(EventAuthenticator {
                id,
                kind: kind.clone(),
                label,
            }),
            _ => None,
        };
        let place = self.place.clone().filter(|p| !p.is_empty());
        SecurityEvent {
            id: self.id,
            event_type: self.event_type.clone(),
            severity: self.severity.clone(),
            created_at: self.created_at,
            subject_id: self.subject_id,
            actor: EventActor {
                kind: self.actor_kind.clone(),
                user_id: self.actor_user_id,
                username: self.actor_username.clone(),
            },
            client,
            place: if reveal_network { place } else { None },
            ip: if reveal_network {
                self.ip.clone()
            } else {
                None
            },
            session_id: self.session_id,
            authenticator,
            request_id: self.request_id,
            details: self.details.clone(),
            actions: Vec::new(),
            repudiated_at: self.repudiated_at,
        }
    }
}

/// One event by id.
pub async fn event_by_id<'e, E: PgExecutor<'e>>(ex: E, id: Uuid) -> sqlx::Result<Option<EventRow>> {
    sqlx::query_as::<_, EventRow>(concat!(
        "SELECT ",
        event_columns!(),
        " FROM security_events e
           LEFT JOIN users a ON a.id = e.actor_user_id
          WHERE e.id = $1"
    ))
    .bind(id)
    .fetch_optional(ex)
    .await
}

/// A page of the log, newest first.
///
/// `subject` = `None` lists every account (admin log). `before` is the id of
/// the last event of the previous page. Returns at most `limit + 1` rows, so
/// the caller can tell `has_more`.
pub async fn page(
    conn: &mut PgConnection,
    subject: Option<Uuid>,
    before: Option<Uuid>,
    limit: i64,
) -> sqlx::Result<Vec<EventRow>> {
    sqlx::query_as::<_, EventRow>(concat!(
        "SELECT ",
        event_columns!(),
        " FROM security_events e
           LEFT JOIN users a ON a.id = e.actor_user_id
          WHERE ($1::uuid IS NULL OR e.subject_id = $1)
            AND ($2::uuid IS NULL OR (e.created_at, e.id) < (
                    SELECT c.created_at, c.id FROM security_events c
                     WHERE c.id = $2 AND ($1::uuid IS NULL OR c.subject_id = $1)))
          ORDER BY e.created_at DESC, e.id DESC
          LIMIT $3"
    ))
    .bind(subject)
    .bind(before)
    .bind(limit + 1)
    .fetch_all(&mut *conn)
    .await
}

/// The newest `per_notice` events of each notice, newest first (a
/// coalesced notice can hold many more than it shows).
pub async fn events_of_notices(
    conn: &mut PgConnection,
    notice_ids: &[Uuid],
    per_notice: i64,
) -> sqlx::Result<Vec<(Uuid, EventRow)>> {
    #[derive(sqlx::FromRow)]
    struct Row {
        notice_id: Uuid,
        #[sqlx(flatten)]
        event: EventRow,
    }
    let rows = sqlx::query_as::<_, Row>(concat!(
        "SELECT n.id AS notice_id, ",
        event_columns!(),
        "
           FROM unnest($1::uuid[]) AS n(id)
          CROSS JOIN LATERAL (
                SELECT ne.event_id FROM security_notice_events ne
                  JOIN security_events le ON le.id = ne.event_id
                 WHERE ne.notice_id = n.id
                 ORDER BY le.created_at DESC, le.id DESC
                 LIMIT $2) newest
           JOIN security_events e ON e.id = newest.event_id
           LEFT JOIN users a ON a.id = e.actor_user_id
          ORDER BY e.created_at DESC, e.id DESC"
    ))
    .bind(notice_ids)
    .bind(per_notice)
    .fetch_all(&mut *conn)
    .await?;
    Ok(rows.into_iter().map(|r| (r.notice_id, r.event)).collect())
}
