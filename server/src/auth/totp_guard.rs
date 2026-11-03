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

/// Record the outcome of a TOTP check; `password` is the password
/// generation that came with the code (compromised at the 100th failure).
pub async fn after(ctx: &AuthCtx<'_>, user: Uuid, ok: bool, password: Option<FactorRef>) {
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
    tokio::spawn(async move {
        let ctx = AuthCtx {
            db: &db,
            redis: &redis,
            config: &config,
        };
        if let Err(e) = effects(&ctx, user, n, password).await {
            tracing::warn!(error = %e, %user, "TOTP guessing effects failed");
        }
    });
}

async fn effects(
    ctx: &AuthCtx<'_>,
    user: Uuid,
    n: u32,
    password: Option<FactorRef>,
) -> AuthResult<()> {
    use crate::security_events::{record, Actor, EventType, NewSecurityEvent};
    let mut tx = ctx.db.begin().await?;
    crate::approvals::transition::lock_accounts(&mut tx, &[user]).await?;
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
    password: Option<FactorRef>,
) -> AuthResult<bool> {
    pre_check(ctx, user).await?;
    let ok = super::reauth::accept_totp_ctx(ctx, user, encrypted_secret, username, code).await?;
    after(ctx, user, ok, password).await;
    Ok(ok)
}
