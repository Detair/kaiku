//! Recovery codes (spec B.1 "Recovery codes"; C27 `POST /auth/recovery-codes`).
//!
//! 10 single-use codes of 13 RFC 4648 base32 characters (65 bits from the
//! OS CSPRNG each), shown as `ABCDE-FGHIJ-KLM` and stored only as Argon2id
//! hashes of the current `recovery_codes_generation`. Their only use is one
//! factor of an account request (or part 2 of the owner action rule); they
//! never sign anyone in.

use axum::extract::State;
use axum::Json;
use sqlx::{PgConnection, PgPool};
use uuid::Uuid;

use super::ctx::AuthCtx;
use super::error::{AuthError, AuthResult};
use super::middleware::AuthUser;
use super::step_up::{self, StepUpAction, StepUpTarget};
use super::types::{RecoveryCodesCountResponse, RecoveryCodesResponse};
use crate::api::AppState;
use crate::security_events::{record, spawn_delivery, Actor, EventType, NewSecurityEvent};

/// Codes per set.
pub const CODE_COUNT: usize = 10;
/// Base32 characters per code (13 × 5 = 65 bits).
pub const CODE_CHARS: usize = 13;

/// RFC 4648 base32 alphabet.
const ALPHABET: &[u8; 32] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";

/// One raw code: 13 characters, 5 bits each from the OS CSPRNG.
fn raw_code() -> String {
    use rand::RngCore;
    let mut bytes = [0u8; CODE_CHARS];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    bytes
        .iter()
        .map(|b| char::from(ALPHABET[usize::from(b & 0x1f)]))
        .collect()
}

/// `ABCDEFGHIJKLM` → `ABCDE-FGHIJ-KLM`.
fn display(raw: &str) -> String {
    format!("{}-{}-{}", &raw[..5], &raw[5..10], &raw[10..])
}

/// Generate a set: the display forms and their Argon2id hashes.
///
/// # Panics
/// If Argon2 hashing fails (it does not for these inputs).
#[must_use]
pub fn generate() -> (Vec<String>, Vec<String>) {
    let mut shown = Vec::with_capacity(CODE_COUNT);
    let mut hashes = Vec::with_capacity(CODE_COUNT);
    while shown.len() < CODE_COUNT {
        let raw = raw_code();
        let d = display(&raw);
        if shown.contains(&d) {
            continue;
        }
        hashes.push(super::hash_password(&raw).expect("argon2 hash of a recovery code"));
        shown.push(d);
    }
    (shown, hashes)
}

/// Replace the account's set with a new generation (B.7 rule 2) and
/// return the new codes once. Hashing runs off the async runtime.
#[tracing::instrument(skip(conn, ctx))]
pub async fn replace_tx(
    conn: &mut PgConnection,
    ctx: &AuthCtx<'_>,
    user_id: Uuid,
) -> AuthResult<Vec<String>> {
    let (shown, hashes) = tokio::task::spawn_blocking(generate)
        .await
        .map_err(|e| AuthError::Internal(format!("recovery code generation failed: {e}")))?;
    let generation =
        super::factors::replace_tx(conn, ctx, user_id, super::factors::Factor::RecoveryCodes)
            .await?;
    sqlx::query("DELETE FROM recovery_codes WHERE user_id = $1")
        .bind(user_id)
        .execute(&mut *conn)
        .await?;
    for hash in &hashes {
        sqlx::query(
            "INSERT INTO recovery_codes (id, user_id, generation, code_hash) VALUES ($1, $2, $3, $4)",
        )
        .bind(Uuid::new_v4())
        .bind(user_id)
        .bind(generation)
        .bind(hash)
        .execute(&mut *conn)
        .await?;
    }
    Ok(shown)
}

/// Consume one unused code of the current generation; `false` if none
/// matches. Lower case, spaces and dashes are accepted. The rows are locked,
/// so a code is consumed at most once.
#[tracing::instrument(skip(conn, code))]
pub async fn consume_tx(conn: &mut PgConnection, user_id: Uuid, code: &str) -> AuthResult<bool> {
    let raw = normalise(code);
    let rows: Vec<(Uuid, String)> = sqlx::query_as(
        "SELECT rc.id, rc.code_hash FROM recovery_codes rc
           JOIN users u ON u.id = rc.user_id
          WHERE rc.user_id = $1 AND rc.generation = u.recovery_codes_generation
            AND rc.used_at IS NULL
          ORDER BY rc.created_at, rc.id
          FOR UPDATE OF rc",
    )
    .bind(user_id)
    .fetch_all(&mut *conn)
    .await?;
    // Always exactly CODE_COUNT Argon2 checks (padded with the dummy hash):
    // the time reveals neither the shape, nor how many codes are left, nor
    // whether the account exists (B.5; `dummy_check` does the same work).
    let dummy = super::password::DUMMY_PASSWORD_HASH.clone();
    if raw.len() != CODE_CHARS || !raw.bytes().all(|b| ALPHABET.contains(&b)) {
        let _ = matching(&raw, vec![dummy; CODE_COUNT]).await?;
        return Ok(false);
    }
    let mut hashes: Vec<String> = rows.iter().map(|(_, h)| h.clone()).collect();
    let real = hashes.len();
    hashes.resize(CODE_COUNT.max(real), dummy);
    let Some(i) = matching(&raw, hashes).await?.filter(|i| *i < real) else {
        return Ok(false);
    };
    sqlx::query("UPDATE recovery_codes SET used_at = NOW() WHERE id = $1")
        .bind(rows[i].0)
        .execute(&mut *conn)
        .await?;
    Ok(true)
}

/// The work of checking `code` against a full set, for an account that has
/// none or is unknown (B.5: the same answer and work).
pub async fn dummy_check(code: &str) -> AuthResult<()> {
    let raw = normalise(code);
    let _ = matching(
        &raw,
        vec![super::password::DUMMY_PASSWORD_HASH.clone(); CODE_COUNT],
    )
    .await?;
    Ok(())
}

/// Index of the hash `raw` matches (every hash is checked: no early exit).
async fn matching(raw: &str, hashes: Vec<String>) -> AuthResult<Option<usize>> {
    let raw = raw.to_string();
    tokio::task::spawn_blocking(move || {
        let mut found = None;
        for (i, h) in hashes.iter().enumerate() {
            if super::verify_password(&raw, h).unwrap_or(false) && found.is_none() {
                found = Some(i);
            }
        }
        found
    })
    .await
    .map_err(|e| AuthError::Internal(format!("recovery code check failed: {e}")))
}

/// Unused codes of the current generation.
pub async fn left(pool: &PgPool, user_id: Uuid) -> sqlx::Result<i64> {
    left_conn(&mut *pool.acquire().await?, user_id).await
}

/// [`left`] on a connection (inside a transaction).
pub async fn left_conn(conn: &mut PgConnection, user_id: Uuid) -> sqlx::Result<i64> {
    sqlx::query_scalar(
        "SELECT COUNT(*) FROM recovery_codes rc JOIN users u ON u.id = rc.user_id
          WHERE rc.user_id = $1 AND rc.generation = u.recovery_codes_generation
            AND rc.used_at IS NULL",
    )
    .bind(user_id)
    .fetch_one(&mut *conn)
    .await
}

/// A new set from a full session with the step-up `recovery_codes.create`
/// (C27); event `recovery_codes_created`.
///
/// POST /auth/recovery-codes
#[utoipa::path(
    post,
    path = "/auth/recovery-codes",
    tag = "auth",
    responses(
        (status = 200, description = "New recovery codes (shown once)", body = RecoveryCodesResponse),
        (status = 403, description = "FULL_SESSION_REQUIRED or STEP_UP_REQUIRED"),
    ),
    security(("bearer_auth" = [])),
)]
#[tracing::instrument(skip(state, auth), fields(user_id = %auth.id))]
pub async fn create(
    State(state): State<AppState>,
    auth: AuthUser,
) -> AuthResult<Json<RecoveryCodesResponse>> {
    super::guards::require_full(&auth)?;
    let ctx = AuthCtx::from(&state);
    let mut tx = state.db.begin().await?;
    crate::approvals::transition::lock_accounts(&mut tx, &[auth.id]).await?;
    let grant = step_up::consume_tx(
        &mut tx,
        &auth,
        StepUpAction::RecoveryCodesCreate,
        &StepUpTarget::None,
    )
    .await?;
    let codes = replace_tx(&mut tx, &ctx, auth.id).await?;
    let mut ev = NewSecurityEvent::new(
        auth.id,
        EventType::RecoveryCodesCreated,
        Actor::User(auth.id),
        serde_json::json!({}),
    );
    ev.session_id = auth.session_id;
    ev.authenticator = Some(grant.by);
    let event_id = record(&mut tx, ev).await?;
    tx.commit().await?;
    spawn_delivery(&state, vec![event_id]);
    Ok(Json(RecoveryCodesResponse {
        recovery_codes: codes,
    }))
}

/// Unused recovery codes (any session, C27).
///
/// GET /auth/recovery-codes/count
#[utoipa::path(
    get,
    path = "/auth/recovery-codes/count",
    tag = "auth",
    responses((status = 200, description = "Unused codes", body = RecoveryCodesCountResponse)),
    security(("bearer_auth" = [])),
)]
#[tracing::instrument(skip(state, auth), fields(user_id = %auth.id))]
pub async fn count(
    State(state): State<AppState>,
    auth: AuthUser,
) -> AuthResult<Json<RecoveryCodesCountResponse>> {
    Ok(Json(RecoveryCodesCountResponse {
        left: left(&state.db, auth.id).await?,
    }))
}

/// Upper case without spaces and dashes.
#[must_use]
pub fn normalise(code: &str) -> String {
    code.chars()
        .filter(|c| !c.is_whitespace() && *c != '-')
        .map(|c| c.to_ascii_uppercase())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_codes_have_65_bits_in_5_5_3_groups() {
        let (codes, hashes) = generate();
        assert_eq!(codes.len(), CODE_COUNT);
        assert_eq!(hashes.len(), CODE_COUNT);
        for (c, h) in codes.iter().zip(&hashes) {
            let raw: String = c.chars().filter(|ch| *ch != '-').collect();
            assert_eq!(raw.len(), CODE_CHARS, "{c}");
            assert_eq!(&c[5..6], "-");
            assert_eq!(&c[11..12], "-");
            assert!(raw
                .chars()
                .all(|ch| ch.is_ascii_uppercase() || ('2'..='7').contains(&ch)));
            assert!(h.starts_with("$argon2id$"));
            assert!(crate::auth::verify_password(&raw, h).unwrap());
        }
    }

    #[test]
    fn normalise_accepts_lower_case_spaces_and_dashes() {
        assert_eq!(normalise(" abcde-fghij klm "), "ABCDEFGHIJKLM");
        assert_eq!(normalise("ABCDEFGHIJKLM"), "ABCDEFGHIJKLM");
    }
}
