//! The owner action rule (spec B.2): who may act for the owner against an
//! event ("That wasn't me") or an account request (Cancel).
//!
//! - **Part 1:** the owner's own full session and a step-up grant for the action and target, made
//!   by a strong authenticator of the subject that is active, older than the event and not created
//!   by it.
//! - **Part 2:** only while the subject has no active strong authenticator: the owner's session
//!   (older than the event, for a repudiation) and a fresh factor — a TOTP code that is not
//!   `compromised` (replay store) or a recovery code (consumed).
//! - **Contact** (the confirmed recovery contact) and **Admin**: a full session and the grant.
//!
//! `may_approve` plays no part.

use chrono::{DateTime, Utc};
use sqlx::PgConnection;
use uuid::Uuid;

use super::ctx::AuthCtx;
use super::error::{AuthError, AuthResult};
use super::middleware::AuthUser;
use super::step_up::{self, ConsumedGrant, StepUpAction, StepUpTarget};
use super::strong::AuthenticatorRef;

/// A part-2 factor.
#[derive(Clone)]
pub enum Part2Factor {
    Totp(String),
    RecoveryCode(String),
}

impl std::fmt::Debug for Part2Factor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Totp(_) => "Totp(..)",
            Self::RecoveryCode(_) => "RecoveryCode(..)",
        })
    }
}

/// How the caller acts for the owner.
#[derive(Debug, Clone)]
pub enum OwnerAuthority {
    Part1 { grant: ConsumedGrant },
    Part2 { factor: &'static str },
    Contact { grant: ConsumedGrant },
    Admin { grant: ConsumedGrant },
}

impl OwnerAuthority {
    /// `cancelled_by.kind` / the event actor.
    #[must_use]
    pub const fn kind(&self) -> &'static str {
        match self {
            Self::Part1 { .. } | Self::Part2 { .. } => "self",
            Self::Contact { .. } => "recovery_contact",
            Self::Admin { .. } => "admin",
        }
    }
}

/// What is acted on.
#[derive(Debug, Clone)]
pub struct OwnerTarget {
    pub subject: Uuid,
    pub action: StepUpAction,
    pub target: StepUpTarget,
    /// Repudiation: the event's time (the authenticator and the session must
    /// be older).
    pub event_at: Option<DateTime<Utc>>,
    /// Authenticators the event created (never a part-1 authority).
    pub created_by_event: Vec<AuthenticatorRef>,
}

/// Decide the caller's authority under the subject's account lock (the
/// caller holds it). `None` part 2 needs the grant; refusals answer
/// `refused` (the caller's 403 code).
#[tracing::instrument(skip_all, fields(user_id = %auth.id, subject = %t.subject))]
pub async fn authorize(
    conn: &mut PgConnection,
    ctx: &AuthCtx<'_>,
    auth: &AuthUser,
    t: &OwnerTarget,
    part2: Option<Part2Factor>,
    refused: fn() -> AuthError,
) -> AuthResult<OwnerAuthority> {
    if let Some(factor) = part2 {
        if auth.id != t.subject {
            return Err(refused());
        }
        if super::strong::active_strong_count(conn, t.subject).await? > 0 {
            return Err(refused());
        }
        if let Some(at) = t.event_at {
            let older = auth.session.as_ref().is_some_and(|s| s.created_at < at);
            if !older {
                return Err(refused());
            }
        }
        return match factor {
            Part2Factor::Totp(code) => {
                let row: (Option<String>, String, String) = sqlx::query_as(
                    "SELECT mfa_secret, totp_state, username FROM users WHERE id = $1",
                )
                .bind(t.subject)
                .fetch_one(&mut *conn)
                .await?;
                let (Some(secret), state, username) = row else {
                    return Err(refused());
                };
                if state == "compromised"
                    || !super::reauth::accept_totp_ctx(ctx, t.subject, &secret, &username, &code)
                        .await?
                {
                    return Err(AuthError::InvalidMfaCode);
                }
                Ok(OwnerAuthority::Part2 { factor: "totp" })
            }
            Part2Factor::RecoveryCode(code) => {
                if super::recovery_codes::consume_tx(conn, t.subject, &code).await? {
                    Ok(OwnerAuthority::Part2 {
                        factor: "recovery_code",
                    })
                } else {
                    Err(AuthError::InvalidCredentials)
                }
            }
        };
    }
    super::guards::require_full(auth)?;
    let grant = step_up::consume_tx(conn, auth, t.action, &t.target).await?;
    if auth.id == t.subject {
        // Part 1: the subject's own authenticator, older than the event and
        // not created by it.
        if t.created_by_event.contains(&grant.by) {
            return Err(refused());
        }
        if let Some(at) = t.event_at {
            if !authenticator_older_than(conn, grant.by, at).await? {
                return Err(refused());
            }
        }
        return Ok(OwnerAuthority::Part1 { grant });
    }
    let contact: Option<Uuid> = sqlx::query_scalar(
        "SELECT recovery_contact_id FROM users
          WHERE id = $1 AND recovery_contact_confirmed_at IS NOT NULL",
    )
    .bind(t.subject)
    .fetch_optional(&mut *conn)
    .await?
    .flatten();
    if contact == Some(auth.id) {
        return Ok(OwnerAuthority::Contact { grant });
    }
    let admin: bool =
        sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM system_admins WHERE user_id = $1)")
            .bind(auth.id)
            .fetch_one(&mut *conn)
            .await?;
    if admin {
        return Ok(OwnerAuthority::Admin { grant });
    }
    Err(refused())
}

/// Whether `a` was created before `at`.
async fn authenticator_older_than(
    conn: &mut PgConnection,
    a: AuthenticatorRef,
    at: DateTime<Utc>,
) -> sqlx::Result<bool> {
    let sql = match a.kind {
        super::strong::AuthenticatorKind::KeyDevice => {
            "SELECT created_at < $2 FROM approval_devices WHERE id = $1"
        }
        super::strong::AuthenticatorKind::Passkey => {
            "SELECT created_at < $2 FROM passkeys WHERE id = $1"
        }
    };
    Ok(sqlx::query_scalar::<_, bool>(sql)
        .bind(a.id)
        .bind(at)
        .fetch_optional(&mut *conn)
        .await?
        .unwrap_or(false))
}
