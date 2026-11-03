//! Recording an event: the event row and its audit row, in the caller's
//! transaction (spec B.2 "Data model": a `system_audit_log` row for the
//! admins in the same transaction).

use sqlx::PgConnection;
use uuid::Uuid;

use super::types::{Actor, NewSecurityEvent};
use crate::system_account::SYSTEM_USER_ID;

/// Insert the event and, in the same transaction, its `system_audit_log` row.
///
/// The audit row has `action` = `security.<event_type>` and `actor_id` = the
/// acting user, or the system account for system and CLI actions. Delivery
/// of the notice happens after commit (the outbox, Task 4).
#[tracing::instrument(skip(conn, ev), fields(event_type = ev.event_type.as_str(), subject_id = %ev.subject_id))]
pub async fn record(conn: &mut PgConnection, ev: NewSecurityEvent) -> sqlx::Result<Uuid> {
    let id = Uuid::now_v7();
    let severity = ev.event_type.severity(&ev.details);
    let (client_class, client_label) = match &ev.client {
        Some((class, label)) => (Some(class.as_str()), Some(label.as_str())),
        None => (None, None),
    };
    let ip = ev.ip.map(|ip| ip.to_string());
    sqlx::query(
        "INSERT INTO security_events
            (id, subject_id, event_type, severity, actor_kind, actor_user_id, client_class,
             client_label, ip, place, session_id, authenticator_type, authenticator_id,
             request_id, details)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9::inet, '', $10, $11, $12, $13, $14)",
    )
    .bind(id)
    .bind(ev.subject_id)
    .bind(ev.event_type.as_str())
    .bind(severity.as_str())
    .bind(ev.actor.kind())
    .bind(ev.actor.user_id())
    .bind(client_class)
    .bind(client_label)
    .bind(ip.as_deref())
    .bind(ev.session_id)
    .bind(ev.authenticator.map(|a| a.as_str()))
    .bind(ev.authenticator.map(|a| a.id))
    .bind(ev.request_id)
    .bind(&ev.details)
    .execute(&mut *conn)
    .await?;

    let audit_actor = match ev.actor {
        Actor::System | Actor::Cli => SYSTEM_USER_ID,
        other => other.user_id().unwrap_or(SYSTEM_USER_ID),
    };
    sqlx::query(
        "INSERT INTO system_audit_log (actor_id, action, target_type, target_id, details, ip_address)
         VALUES ($1, $2, 'user', $3, $4, $5::inet)",
    )
    .bind(audit_actor)
    .bind(format!("security.{}", ev.event_type.as_str()))
    .bind(ev.subject_id)
    .bind(serde_json::json!({
        "event_id": id,
        "severity": severity.as_str(),
        "actor_kind": ev.actor.kind(),
    }))
    .bind(ip.as_deref())
    .execute(&mut *conn)
    .await?;
    Ok(id)
}
