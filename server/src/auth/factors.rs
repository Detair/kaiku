//! Credential states and factor generations (spec B.1 "Credential states",
//! B.7 rule 2; C27).
//!
//! A password or TOTP secret is `active`, `throttled` or `compromised`
//! (*disabled* = either of the last two). Each factor (password, TOTP
//! secret, recovery-code set) has a generation that changes when it is
//! replaced, removed or marked compromised; a throttle starts none. Ending a
//! generation runs `cascade::on_generation_ended` in the same transaction
//! (login attempts expire, account requests that used it become `void`).
//!
//! Every function runs inside the caller's transaction; callers hold the
//! account lock (B.7 rule 1).

use sqlx::PgConnection;
use uuid::Uuid;

use super::ctx::AuthCtx;
use super::error::AuthResult;
use crate::security_events::{record, Actor, EventType, NewSecurityEvent};

/// A knowledge or possession factor with a generation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Factor {
    Password,
    Totp,
    RecoveryCodes,
}

impl Factor {
    /// The wire and stored form.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Password => "password",
            Self::Totp => "totp",
            Self::RecoveryCodes => "recovery_codes",
        }
    }

    /// Parse the stored form.
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "password" => Some(Self::Password),
            "totp" => Some(Self::Totp),
            "recovery_codes" => Some(Self::RecoveryCodes),
            _ => None,
        }
    }
}

/// The state of a password or TOTP secret.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FactorState {
    Active,
    Throttled,
    Compromised,
}

impl FactorState {
    /// The stored and wire form.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Throttled => "throttled",
            Self::Compromised => "compromised",
        }
    }

    /// Parse the stored form; anything unknown is `compromised` (fail safe).
    #[must_use]
    pub fn parse(s: &str) -> Self {
        match s {
            "active" => Self::Active,
            "throttled" => Self::Throttled,
            _ => Self::Compromised,
        }
    }

    /// `throttled` or `compromised`.
    #[must_use]
    pub const fn is_disabled(self) -> bool {
        !matches!(self, Self::Active)
    }
}

/// Why a factor was disabled (C28 `details.cause`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StateCause {
    Guessing,
    Report,
    Repudiation,
    Cancel,
    Admin,
    Cli,
}

impl StateCause {
    /// The wire form (`cli` is appended to C28's list).
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Guessing => "guessing",
            Self::Report => "report",
            Self::Repudiation => "repudiation",
            Self::Cancel => "cancel",
            Self::Admin => "admin",
            Self::Cli => "cli",
        }
    }
}

/// A factor as it was used: its generation at the time.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FactorRef {
    pub factor: Factor,
    pub generation: i64,
}

/// One factor's current state and generation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FactorStatus {
    pub state: FactorState,
    pub generation: i64,
}

/// The account's factors as stored.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools)]
pub struct FactorStates {
    pub password: FactorStatus,
    pub totp: FactorStatus,
    /// A password is set.
    pub has_password: bool,
    /// TOTP is set up.
    pub has_totp: bool,
    pub recovery_codes_generation: i64,
    pub bootstrap_open: bool,
}

/// A way out of `throttled` or `compromised` (B.1 exit table).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExitPath {
    /// A successful sign-in of the account by another method.
    OtherMethodSignIn,
    /// A completed account request.
    CompletedRequest,
    /// An admin.
    Admin,
    /// The host CLI.
    Cli,
    /// Replacement or removal (password) / renewal (TOTP), from a full
    /// session with a step-up.
    Replacement,
}

/// Whether `why` leaves `state` of `f` (B.1 exit table; normative).
#[must_use]
pub const fn exits(f: Factor, state: FactorState, why: ExitPath) -> bool {
    use ExitPath::{Admin, Cli, CompletedRequest, OtherMethodSignIn, Replacement};
    match (f, state) {
        (_, FactorState::Active) | (Factor::RecoveryCodes, _) => false,
        // A password throttle ends with a sign-in by another method, a
        // completed request, an admin or the CLI (and its replacement).
        (Factor::Password, FactorState::Throttled) => matches!(
            why,
            OtherMethodSignIn | CompletedRequest | Admin | Cli | Replacement
        ),
        // A TOTP throttle never ends by another sign-in.
        (Factor::Totp, FactorState::Throttled) => {
            matches!(why, Replacement | CompletedRequest | Admin | Cli)
        }
        // Compromised: replacement or removal (full + step-up), a completed
        // request, the CLI; never an admin, never another sign-in.
        (_, FactorState::Compromised) => matches!(why, Replacement | CompletedRequest | Cli),
    }
}

#[derive(sqlx::FromRow)]
struct StatesRow {
    password_state: String,
    totp_state: String,
    password_generation: i64,
    totp_generation: i64,
    recovery_codes_generation: i64,
    bootstrap_open: bool,
    has_password: bool,
    has_totp: bool,
}

/// The account's factor states.
pub async fn states(conn: &mut PgConnection, user_id: Uuid) -> sqlx::Result<FactorStates> {
    let r: StatesRow = sqlx::query_as(
        "SELECT password_state, totp_state, password_generation, totp_generation,
                recovery_codes_generation, bootstrap_open,
                password_hash IS NOT NULL AS has_password, mfa_secret IS NOT NULL AS has_totp
           FROM users WHERE id = $1",
    )
    .bind(user_id)
    .fetch_one(&mut *conn)
    .await?;
    Ok(FactorStates {
        password: FactorStatus {
            state: FactorState::parse(&r.password_state),
            generation: r.password_generation,
        },
        totp: FactorStatus {
            state: FactorState::parse(&r.totp_state),
            generation: r.totp_generation,
        },
        has_password: r.has_password,
        has_totp: r.has_totp,
        recovery_codes_generation: r.recovery_codes_generation,
        bootstrap_open: r.bootstrap_open,
    })
}

/// The current generation of `f`, under the row lock.
async fn locked_generation(conn: &mut PgConnection, user_id: Uuid, f: Factor) -> sqlx::Result<i64> {
    let sql = match f {
        Factor::Password => "SELECT password_generation FROM users WHERE id = $1 FOR UPDATE",
        Factor::Totp => "SELECT totp_generation FROM users WHERE id = $1 FOR UPDATE",
        Factor::RecoveryCodes => {
            "SELECT recovery_codes_generation FROM users WHERE id = $1 FOR UPDATE"
        }
    };
    sqlx::query_scalar(sql)
        .bind(user_id)
        .fetch_one(&mut *conn)
        .await
}

/// Start a new generation of `f` with `state` (and `compromised_at` when
/// compromised). Returns the new generation.
async fn new_generation(
    conn: &mut PgConnection,
    user_id: Uuid,
    f: Factor,
    state: FactorState,
) -> sqlx::Result<i64> {
    let sql = match f {
        Factor::Password => {
            "UPDATE users SET password_generation = password_generation + 1,
                    password_state = $2,
                    password_compromised_at = CASE WHEN $2 = 'compromised' THEN NOW() END
              WHERE id = $1 RETURNING password_generation"
        }
        Factor::Totp => {
            "UPDATE users SET totp_generation = totp_generation + 1,
                    totp_state = $2,
                    totp_compromised_at = CASE WHEN $2 = 'compromised' THEN NOW() END
              WHERE id = $1 RETURNING totp_generation"
        }
        Factor::RecoveryCodes => {
            return sqlx::query_scalar(
                "UPDATE users SET recovery_codes_generation = recovery_codes_generation + 1
                  WHERE id = $1 RETURNING recovery_codes_generation",
            )
            .bind(user_id)
            .fetch_one(&mut *conn)
            .await;
        }
    };
    sqlx::query_scalar(sql)
        .bind(user_id)
        .bind(state.as_str())
        .fetch_one(&mut *conn)
        .await
}

/// Mark `used` compromised if its generation is still current.
///
/// A new generation, the rule-2 cascade and the `*_compromised` event.
/// Returns whether anything changed. For the recovery-code set this
/// invalidates the set (there is no state or event for it).
#[tracing::instrument(skip(conn, ctx))]
pub async fn mark_compromised_tx(
    conn: &mut PgConnection,
    ctx: &AuthCtx<'_>,
    user_id: Uuid,
    used: FactorRef,
    cause: StateCause,
    actor: Actor,
) -> AuthResult<bool> {
    let current = locked_generation(conn, user_id, used.factor).await?;
    if current != used.generation {
        return Ok(false);
    }
    new_generation(conn, user_id, used.factor, FactorState::Compromised).await?;
    if used.factor == Factor::RecoveryCodes {
        sqlx::query(
            "UPDATE recovery_codes SET used_at = NOW() WHERE user_id = $1 AND used_at IS NULL",
        )
        .bind(user_id)
        .execute(&mut *conn)
        .await?;
    }
    crate::approvals::cascade::on_generation_ended(conn, ctx, user_id, used).await?;
    let event_type = match used.factor {
        Factor::Password => Some(EventType::PasswordCompromised),
        Factor::Totp => Some(EventType::TotpCompromised),
        Factor::RecoveryCodes => None,
    };
    if let Some(event_type) = event_type {
        record(
            conn,
            NewSecurityEvent::new(
                user_id,
                event_type,
                actor,
                serde_json::json!({ "cause": cause.as_str() }),
            ),
        )
        .await?;
    }
    Ok(true)
}

/// Throttle a password or TOTP secret (B.5): no new generation. A factor
/// that is already disabled stays as it is.
#[tracing::instrument(skip(conn, _ctx))]
pub async fn throttle_tx(
    conn: &mut PgConnection,
    _ctx: &AuthCtx<'_>,
    user_id: Uuid,
    f: Factor,
) -> AuthResult<()> {
    let (sql, event_type) = match f {
        Factor::Password => (
            "UPDATE users SET password_state = 'throttled' WHERE id = $1 AND password_state = 'active'",
            EventType::PasswordThrottled,
        ),
        Factor::Totp => (
            "UPDATE users SET totp_state = 'throttled' WHERE id = $1 AND totp_state = 'active'",
            EventType::TotpThrottled,
        ),
        Factor::RecoveryCodes => return Ok(()),
    };
    let changed = sqlx::query(sql)
        .bind(user_id)
        .execute(&mut *conn)
        .await?
        .rows_affected();
    if changed > 0 {
        record(
            conn,
            NewSecurityEvent::new(
                user_id,
                event_type,
                Actor::System,
                serde_json::json!({ "cause": StateCause::Guessing.as_str() }),
            ),
        )
        .await?;
    }
    Ok(())
}

/// Replace (or remove) a factor: a new generation, state `active`, and the
/// rule-2 cascade for the old generation. Returns the new generation. The
/// caller stores the new value in the same transaction.
#[tracing::instrument(skip(conn, ctx))]
pub async fn replace_tx(
    conn: &mut PgConnection,
    ctx: &AuthCtx<'_>,
    user_id: Uuid,
    f: Factor,
) -> AuthResult<i64> {
    let old = locked_generation(conn, user_id, f).await?;
    let new = new_generation(conn, user_id, f, FactorState::Active).await?;
    crate::approvals::cascade::on_generation_ended(
        conn,
        ctx,
        user_id,
        FactorRef {
            factor: f,
            generation: old,
        },
    )
    .await?;
    Ok(new)
}

/// Leave `throttled` or `compromised` by `why` where the B.1 exit table
/// allows it (no new generation). Returns whether the state changed.
#[tracing::instrument(skip(conn))]
pub async fn clear_throttle_tx(
    conn: &mut PgConnection,
    user_id: Uuid,
    f: Factor,
    why: ExitPath,
) -> AuthResult<bool> {
    let s = states(conn, user_id).await?;
    let (state, sql) = match f {
        Factor::Password => (
            s.password.state,
            "UPDATE users SET password_state = 'active', password_compromised_at = NULL WHERE id = $1",
        ),
        Factor::Totp => (
            s.totp.state,
            "UPDATE users SET totp_state = 'active', totp_compromised_at = NULL WHERE id = $1",
        ),
        Factor::RecoveryCodes => return Ok(false),
    };
    if !exits(f, state, why) {
        return Ok(false);
    }
    sqlx::query(sql).bind(user_id).execute(&mut *conn).await?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The B.1 exit table, every row.
    #[test]
    fn exit_rules_follow_b1() {
        use ExitPath::{Admin, Cli, CompletedRequest, OtherMethodSignIn, Replacement};
        use Factor::{Password, Totp};
        use FactorState::{Active, Compromised, Throttled};
        let all = [OtherMethodSignIn, CompletedRequest, Admin, Cli, Replacement];
        let table: [(Factor, FactorState, &[ExitPath]); 4] = [
            (
                Password,
                Throttled,
                &[OtherMethodSignIn, CompletedRequest, Admin, Cli, Replacement],
            ),
            (
                Totp,
                Throttled,
                &[Replacement, CompletedRequest, Admin, Cli],
            ),
            (Password, Compromised, &[Replacement, CompletedRequest, Cli]),
            (Totp, Compromised, &[Replacement, CompletedRequest, Cli]),
        ];
        for (f, state, allowed) in table {
            for why in all {
                assert_eq!(
                    exits(f, state, why),
                    allowed.contains(&why),
                    "{f:?} {state:?} via {why:?}"
                );
            }
        }
        for why in all {
            assert!(!exits(Password, Active, why));
            assert!(!exits(Totp, Active, why));
        }
    }
}
