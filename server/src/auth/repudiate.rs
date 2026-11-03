//! "That wasn't me" (B.2 repudiation table; C28).
//!
//! The owner reports an event under the owner action rule: part 1 (a full
//! session and an `event.repudiate` grant made by a strong authenticator
//! older than the event and not its subject) or part 2 (no active strong
//! authenticator, a session older than the event and a fresh factor; part 2
//! never revokes). Effects run in one transaction under the account lock.

use axum::extract::{Path, State};
use axum::Json;
use serde::{Deserialize, Serialize};
use sqlx::PgConnection;
use uuid::Uuid;

use super::ctx::AuthCtx;
use super::error::{AuthError, AuthResult};
use super::factors::{self, Factor, FactorRef, StateCause};
use super::owner_rule::{authorize, OwnerAuthority, OwnerTarget};
use super::revocation::{end_sessions_tx, EndReason};
use super::step_up::{StepUpAction, StepUpTarget};
use super::strong::{AuthenticatorKind, AuthenticatorRef};
use super::AuthUser;
use crate::api::AppState;
use crate::security_events::queries::EventRow;
use crate::security_events::{record, spawn_delivery, Actor, EventType, NewSecurityEvent};

/// Event types "That wasn't me" applies to.
pub const REPUDIABLE: &[&str] = &[
    "sign_in",
    "authenticator_added",
    "password_set",
    "password_changed",
];

/// What a repudiation did.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct RepudiationOutcome {
    pub event: crate::security_events::SecurityEvent,
    pub ended_session_ids: Vec<Uuid>,
    pub revoked_authenticator_ids: Vec<Uuid>,
    /// `password` and/or `totp`.
    pub compromised: Vec<String>,
    pub voided_request_ids: Vec<Uuid>,
}

/// The effects of repudiating `ev` (B.2): which session ends, which
/// authenticator is revoked, which factor generations are compromised.
pub struct Effects {
    pub session: Option<Uuid>,
    pub revoke: Option<AuthenticatorRef>,
    pub compromise: Vec<FactorRef>,
}

fn authenticator_of(ev: &EventRow) -> Option<AuthenticatorRef> {
    let kind = AuthenticatorKind::parse(ev.authenticator_type.as_deref()?)?;
    Some(AuthenticatorRef {
        kind,
        id: ev.authenticator_id?,
    })
}

fn methods(ev: &EventRow) -> Vec<String> {
    ev.details["auth_methods"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|m| m.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

/// Decide the effects of `ev` (part 2 never revokes).
pub async fn effects(
    conn: &mut PgConnection,
    ev: &EventRow,
    may_revoke: bool,
) -> AuthResult<Effects> {
    let st = factors::states(conn, ev.subject_id).await?;
    let current = |f: Factor| FactorRef {
        factor: f,
        generation: match f {
            Factor::Password => st.password.generation,
            Factor::Totp => st.totp.generation,
            Factor::RecoveryCodes => st.recovery_codes_generation,
        },
    };
    let mut out = Effects {
        session: None,
        revoke: None,
        compromise: Vec::new(),
    };
    match ev.event_type.as_str() {
        "sign_in" => {
            out.session = ev.session_id;
            let m = methods(ev);
            // A passkey or key device that *signed in* is revoked; one that
            // only approved (a relayed QR) is not.
            if may_revoke && (m.iter().any(|x| x == "passkey" || x == "device_key")) {
                out.revoke = authenticator_of(ev);
            }
            if m.iter().any(|x| x == "password") {
                // The generation that produced it (recorded on the attempt).
                let g: Option<i64> = sqlx::query_scalar(
                    "SELECT first_factor_generation FROM login_attempts
                      WHERE session_id = $1 AND first_factor = 'password'",
                )
                .bind(ev.session_id)
                .fetch_optional(&mut *conn)
                .await?
                .flatten();
                out.compromise.push(g.map_or_else(
                    || current(Factor::Password),
                    |generation| FactorRef {
                        factor: Factor::Password,
                        generation,
                    },
                ));
            }
            if m.iter().any(|x| x == "totp") {
                out.compromise.push(current(Factor::Totp));
            }
        }
        "authenticator_added" => match ev.details["type"].as_str() {
            Some("totp") => out.compromise.push(current(Factor::Totp)),
            _ if may_revoke => out.revoke = authenticator_of(ev),
            _ => {}
        },
        "password_set" | "password_changed" => out.compromise.push(current(Factor::Password)),
        _ => return Err(AuthError::RepudiateNotAllowed),
    }
    Ok(out)
}

async fn event_row(
    conn: &mut PgConnection,
    id: Uuid,
    lock: bool,
) -> sqlx::Result<Option<EventRow>> {
    let sql = if lock {
        concat!(
            "SELECT ",
            crate::event_columns!(),
            " FROM security_events e LEFT JOIN users a ON a.id = e.actor_user_id
              WHERE e.id = $1 FOR UPDATE OF e"
        )
    } else {
        concat!(
            "SELECT ",
            crate::event_columns!(),
            " FROM security_events e LEFT JOIN users a ON a.id = e.actor_user_id
              WHERE e.id = $1"
        )
    };
    sqlx::query_as(sql)
        .bind(id)
        .fetch_optional(&mut *conn)
        .await
}

/// Repudiate `event_id` as `by` (the caller holds the subject's lock and
/// decided `by` in the same transaction).
pub async fn repudiate_tx(
    conn: &mut PgConnection,
    ctx: &AuthCtx<'_>,
    event_id: Uuid,
    by: &OwnerAuthority,
) -> AuthResult<(RepudiationOutcome, Vec<Uuid>)> {
    let ev = event_row(conn, event_id, true)
        .await?
        .ok_or_else(|| AuthError::NotFound("event".into()))?;
    if ev.repudiated_at.is_some() {
        return Err(AuthError::EventAlreadyRepudiated);
    }
    let may_revoke = !matches!(by, OwnerAuthority::Part2 { .. });
    let fx = effects(conn, &ev, may_revoke).await?;
    let open_before: Vec<Uuid> = sqlx::query_scalar(
        "SELECT id FROM account_requests WHERE user_id = $1 AND state IN ('waiting', 'ready')",
    )
    .bind(ev.subject_id)
    .fetch_all(&mut *conn)
    .await?;
    let mut events = Vec::new();
    let ended = match fx.session {
        Some(sid) => {
            end_sessions_tx(conn, ctx, ev.subject_id, &[sid], EndReason::Repudiated).await?
        }
        None => Vec::new(),
    };
    let mut revoked = Vec::new();
    if let Some(a) = fx.revoke {
        let done = match a.kind {
            AuthenticatorKind::KeyDevice => {
                match crate::approvals::devices::revoke_tx(
                    conn,
                    ctx,
                    a.id,
                    crate::approvals::devices::RevokeReason::Repudiated,
                    Actor::User(ev.subject_id),
                )
                .await
                {
                    Ok((_, ids)) => {
                        events.extend(ids);
                        true
                    }
                    Err(crate::approvals::error::ApprovalError::KeyDeviceRevoked) => false,
                    Err(crate::approvals::error::ApprovalError::Auth(e)) => return Err(e),
                    Err(crate::approvals::error::ApprovalError::Database(e)) => {
                        return Err(AuthError::Database(e))
                    }
                    Err(e) => return Err(AuthError::Internal(e.to_string())),
                }
            }
            AuthenticatorKind::Passkey => {
                let n = sqlx::query(
                    "UPDATE passkeys SET state = 'revoked', revoked_at = NOW(),
                            revoke_reason = 'repudiated'
                      WHERE id = $1 AND user_id = $2 AND state <> 'revoked'",
                )
                .bind(a.id)
                .bind(ev.subject_id)
                .execute(&mut *conn)
                .await?
                .rows_affected();
                if n > 0 {
                    crate::approvals::cascade::on_authenticator_revoked(
                        conn,
                        ctx,
                        ev.subject_id,
                        a,
                    )
                    .await?;
                    let mut removed = NewSecurityEvent::new(
                        ev.subject_id,
                        EventType::AuthenticatorRemoved,
                        Actor::User(ev.subject_id),
                        serde_json::json!({"type": "passkey", "by": "repudiated"}),
                    );
                    removed.authenticator = Some(a);
                    events.push(record(conn, removed).await?);
                }
                n > 0
            }
        };
        if done {
            revoked.push(a.id);
        }
    }
    let mut compromised = Vec::new();
    for f in fx.compromise {
        if factors::mark_compromised_tx(
            conn,
            ctx,
            ev.subject_id,
            f,
            StateCause::Repudiation,
            Actor::User(ev.subject_id),
        )
        .await?
        {
            compromised.push(f.factor.as_str().to_string());
        }
    }
    sqlx::query("UPDATE security_events SET repudiated_at = NOW() WHERE id = $1")
        .bind(ev.id)
        .execute(&mut *conn)
        .await?;
    let mut rec = NewSecurityEvent::new(
        ev.subject_id,
        EventType::Repudiated,
        Actor::User(ev.subject_id),
        serde_json::json!({
            "event_id": ev.id,
            "event_type": ev.event_type,
            "via": by.kind(),
        }),
    );
    rec.session_id = ev.session_id;
    events.push(record(conn, rec).await?);
    let voided: Vec<Uuid> =
        sqlx::query_scalar("SELECT id FROM account_requests WHERE id = ANY($1) AND state = 'void'")
            .bind(&open_before)
            .fetch_all(&mut *conn)
            .await?;
    let fresh = event_row(conn, ev.id, false)
        .await?
        .ok_or_else(|| AuthError::NotFound("event".into()))?;
    let label = crate::security_events::notices::authenticator_label(conn, &fresh).await?;
    Ok((
        RepudiationOutcome {
            event: fresh.to_wire(true, label),
            ended_session_ids: ended,
            revoked_authenticator_ids: revoked,
            compromised,
            voided_request_ids: voided,
        },
        events,
    ))
}

/// `POST …/repudiate` body: `part2` only while the account has no active
/// strong authenticator.
#[derive(Debug, Default, Deserialize, utoipa::ToSchema)]
pub struct RepudiateRequest {
    #[serde(default)]
    pub part2: Option<super::account_requests::types::Part2Body>,
}

/// `POST /api/me/security-events/{id}/repudiate {part2|null}` → 200
/// `RepudiationOutcome`.
#[utoipa::path(
    post,
    path = "/api/me/security-events/{id}/repudiate",
    tag = "auth",
    params(("id" = Uuid, Path, description = "Security event id")),
    request_body = RepudiateRequest,
    responses(
        (status = 200, body = RepudiationOutcome),
        (status = 403, description = "REPUDIATE_NOT_ALLOWED or STEP_UP_REQUIRED"),
        (status = 409, description = "EVENT_ALREADY_REPUDIATED"),
    ),
    security(("bearer_auth" = [])),
)]
#[tracing::instrument(skip_all, fields(user_id = %auth.id, event_id = %id))]
pub async fn repudiate(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
    body: Option<Json<RepudiateRequest>>,
) -> AuthResult<Json<RepudiationOutcome>> {
    let body = body.map(|Json(b)| b).unwrap_or_default();
    let ctx = AuthCtx::from(&state);
    let mut tx = state.db.begin().await?;
    crate::approvals::transition::lock_accounts(&mut tx, &[auth.id]).await?;
    let ev = event_row(&mut tx, id, false)
        .await?
        .filter(|e| e.subject_id == auth.id)
        .ok_or_else(|| AuthError::NotFound("event".into()))?;
    if !REPUDIABLE.contains(&ev.event_type.as_str()) {
        return Err(AuthError::RepudiateNotAllowed);
    }
    if ev.repudiated_at.is_some() {
        return Err(AuthError::EventAlreadyRepudiated);
    }
    let part2 = body
        .part2
        .map(super::account_requests::types::Part2Body::into_factor)
        .transpose()?;
    let target = OwnerTarget {
        subject: auth.id,
        action: StepUpAction::EventRepudiate,
        target: StepUpTarget::Id(id),
        event_at: Some(ev.created_at),
        // The event's own authenticator never vouches for its repudiation.
        created_by_event: authenticator_of(&ev).into_iter().collect(),
    };
    let by = authorize(&mut tx, &ctx, &auth, &target, part2, || {
        AuthError::RepudiateNotAllowed
    })
    .await?;
    if !matches!(
        by,
        OwnerAuthority::Part1 { .. } | OwnerAuthority::Part2 { .. }
    ) {
        return Err(AuthError::RepudiateNotAllowed);
    }
    let (out, events) = repudiate_tx(&mut tx, &ctx, id, &by).await?;
    tx.commit().await?;
    spawn_delivery(&state, events);
    crate::security_events::notices::spawn_pending_delivery(&state);
    Ok(Json(out))
}

/// Whether "That wasn't me" is usable for `reader` on `ev`, and how
/// (`step_up` / `part2`), with the B.6.6 warning.
pub async fn repudiate_action(
    conn: &mut PgConnection,
    reader: &AuthUser,
    ev: &crate::security_events::SecurityEvent,
) -> sqlx::Result<Option<(String, Option<String>)>> {
    if ev.subject_id != reader.id
        || ev.repudiated_at.is_some()
        || !REPUDIABLE.contains(&ev.event_type.as_str())
    {
        return Ok(None);
    }
    let event_auth = ev.authenticator.as_ref().map(|a| a.id);
    // Part 1: an active strong authenticator older than the event that is
    // not its subject.
    let older: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM approval_devices
                         WHERE user_id = $1 AND state = 'active' AND created_at < $2
                           AND id IS DISTINCT FROM $3)
             OR EXISTS (SELECT 1 FROM passkeys
                         WHERE user_id = $1 AND state = 'active' AND created_at < $2
                           AND id IS DISTINCT FROM $3)",
    )
    .bind(reader.id)
    .bind(ev.created_at)
    .bind(event_auth)
    .fetch_one(&mut *conn)
    .await?;
    let via = if older {
        "step_up"
    } else {
        let any_strong = super::strong::active_strong_count(conn, reader.id)
            .await
            .unwrap_or(1);
        let session_older = reader
            .session
            .as_ref()
            .is_some_and(|s| s.created_at < ev.created_at);
        if any_strong == 0 && session_older {
            "part2"
        } else {
            return Ok(None);
        }
    };
    // B.6.6: an open request of the account that used a factor this would
    // mark compromised.
    let compromises: Vec<&str> = match ev.event_type.as_str() {
        "sign_in" => {
            let m: Vec<&str> = ev.details["auth_methods"]
                .as_array()
                .map(|a| a.iter().filter_map(|v| v.as_str()).collect())
                .unwrap_or_default();
            m.into_iter()
                .filter(|x| *x == "password" || *x == "totp")
                .collect()
        }
        "authenticator_added" if ev.details["type"] == "totp" => vec!["totp"],
        "password_set" | "password_changed" => vec!["password"],
        _ => Vec::new(),
    };
    let mut warning = None;
    if !compromises.is_empty() {
        let open: Option<(String, serde_json::Value)> = sqlx::query_as(
            "SELECT purpose, factors FROM account_requests
              WHERE user_id = $1 AND state IN ('waiting', 'ready')",
        )
        .bind(reader.id)
        .fetch_optional(&mut *conn)
        .await?;
        if let Some((purpose, f)) = open {
            let used = f.as_array().is_some_and(|a| {
                a.iter().any(|x| {
                    x["factor"]
                        .as_str()
                        .is_some_and(|n| compromises.contains(&n))
                })
            });
            if used {
                warning = Some(if purpose == "bootstrap" {
                    "voids_own_bootstrap".to_string()
                } else {
                    "voids_own_request".to_string()
                });
            }
        }
    }
    Ok(Some((via.to_string(), warning)))
}
