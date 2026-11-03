//! The per-account TOTP counter on every TOTP check (B.5; review round 1).
//!
//! [`pre_check`] answers 429 `TOO_MANY_ATTEMPTS` while the counter's delay
//! runs; [`after`] records the outcome: a success resets the counter, every
//! 20th failure records a coalesced `guessing {factor: totp}`, the 100th
//! throttles TOTP and compromises the password generation that came with it
//! (F4). The effects run in their own transaction, spawned, because the
//! caller may hold the account lock in a transaction that rolls back on the
//! refusal; the spawned one waits for that lock.

use uuid::Uuid;

use super::ctx::AuthCtx;
use super::error::{AuthError, AuthResult};
use super::factors::{Factor, FactorRef, StateCause};
use crate::ratelimit::account::{AccountLimiter, CounterKey, NOTICE_EVERY, THROTTLE_AT};

/// 429 while the account's TOTP delay runs (fail closed on Valkey errors).
pub async fn pre_check(ctx: &AuthCtx<'_>, user: Uuid) -> AuthResult<()> {
    match AccountLimiter::new(ctx.redis)
        .check(&CounterKey::Totp(user))
        .await
    {
        Ok(Ok(())) => Ok(()),
        Ok(Err(d)) => {
            tracing::warn!(%user, retry_after = d.retry_after, "TOTP check delayed (B.5)");
            Err(AuthError::TooManyAttempts {
                retry_after: d.retry_after,
            })
        }
        Err(_) => Err(AuthError::ServiceUnavailable),
    }
}

/// What a TOTP check used: the TOTP generation it checked against and the
/// password generation that came with the code (compromised at the 100th
/// failure). The effects re-check both under the account lock.
#[derive(Debug, Clone, Copy, Default)]
pub struct Used {
    pub totp_generation: Option<i64>,
    pub password: Option<FactorRef>,
}

/// Record the outcome of a TOTP check. With `state` the notices of the
/// effects are delivered at once (else by the minute sweep).
pub async fn after(
    ctx: &AuthCtx<'_>,
    state: Option<&crate::api::AppState>,
    user: Uuid,
    ok: bool,
    used: Used,
) {
    let limiter = AccountLimiter::new(ctx.redis);
    let key = CounterKey::Totp(user);
    if ok {
        let _ = limiter.record_success(&key).await;
        return;
    }
    let Ok(n) = limiter.record_failure(&key).await else {
        return;
    };
    if !n.is_multiple_of(NOTICE_EVERY) && n != THROTTLE_AT {
        return;
    }
    let db = ctx.db.clone();
    let redis = ctx.redis.clone();
    let config = ctx.config.clone();
    let state = state.cloned();
    tokio::spawn(async move {
        let ctx = AuthCtx {
            db: &db,
            redis: &redis,
            config: &config,
        };
        match effects(&ctx, user, n, used).await {
            Ok(()) => {
                if let Some(state) = state {
                    // Flash r2: the guessing / throttle notices now.
                    crate::security_events::notices::spawn_pending_delivery(&state);
                }
            }
            Err(e) => tracing::warn!(error = %e, %user, "TOTP guessing effects failed"),
        }
    });
}

async fn effects(ctx: &AuthCtx<'_>, user: Uuid, n: u32, used: Used) -> AuthResult<()> {
    use crate::security_events::{record, Actor, EventType, NewSecurityEvent};
    let mut tx = ctx.db.begin().await?;
    crate::approvals::transition::lock_accounts(&mut tx, &[user]).await?;
    // Codex r2: under the lock, the failures still count (no success reset
    // them since) and the TOTP they were made against is still current;
    // else nothing of this applies to the account's present TOTP.
    let still: Option<i64> = redis_count(ctx, user).await;
    if still.is_none_or(|c| c < i64::from(n)) {
        tx.commit().await?;
        return Ok(());
    }
    let st = super::factors::states(&mut tx, user).await?;
    if used
        .totp_generation
        .is_some_and(|g| g != st.totp.generation)
    {
        tx.commit().await?;
        return Ok(());
    }
    let password = used.password;
    if n.is_multiple_of(NOTICE_EVERY) {
        record(
            &mut tx,
            NewSecurityEvent::new(
                user,
                EventType::Guessing,
                Actor::System,
                serde_json::json!({"factor": "totp", "count": n}),
            ),
        )
        .await?;
    }
    if n == THROTTLE_AT {
        tracing::warn!(%user, "TOTP throttled after repeated failures (B.5)");
        super::factors::throttle_tx(&mut tx, ctx, user, Factor::Totp).await?;
        if let Some(p) = password.filter(|p| p.factor == Factor::Password) {
            super::factors::mark_compromised_tx(
                &mut tx,
                ctx,
                user,
                p,
                StateCause::Guessing,
                Actor::System,
            )
            .await?;
        }
    }
    tx.commit().await?;
    Ok(())
}

/// [`pre_check`], the replay-checked verification and [`after`] in one.
pub async fn verify(
    ctx: &AuthCtx<'_>,
    user: Uuid,
    encrypted_secret: &str,
    username: &str,
    code: &str,
    totp_generation: i64,
) -> AuthResult<bool> {
    pre_check(ctx, user).await?;
    let ok = super::reauth::accept_totp_ctx(ctx, user, encrypted_secret, username, code).await?;
    after(
        ctx,
        None,
        user,
        ok,
        Used {
            totp_generation: Some(totp_generation),
            password: None,
        },
    )
    .await;
    Ok(ok)
}

async fn redis_count(ctx: &AuthCtx<'_>, user: Uuid) -> Option<i64> {
    use fred::interfaces::HashesInterface;
    ctx.redis
        .hget::<Option<i64>, _, _>(format!("ratelimit:acct:totp:{user}"), "count")
        .await
        .ok()
        .flatten()
}
