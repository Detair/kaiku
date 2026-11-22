//! Starting the rate limiter: when rate limiting is enabled, the limiter
//! must load, or the server does not start.
//!
//! Without its scripts the limiter cannot count anything: no request limit
//! per address, no counting of failed sign-ins per address and no address
//! blocks. Starting without them would leave every one of those off with
//! nothing but a log line to show for it.

use std::future::Future;

use fred::clients::Client;

use super::{RateLimitConfig, RateLimiter};

/// Rate limiting is enabled but the limiter could not be initialised.
#[derive(Debug, thiserror::Error)]
#[error(
    "rate limiting is enabled, but the rate limiter could not load its scripts into Valkey: \
     {cause}. Check that Valkey is reachable at start and allows SCRIPT LOAD. \
     RATE_LIMIT_ENABLED=false switches rate limiting off; set it only for tests"
)]
pub struct LimiterStartError {
    /// The Valkey error the script load failed with.
    pub cause: fred::error::Error,
}

/// The rate limiter for this server, or `None` when rate limiting is
/// disabled by configuration (`RATE_LIMIT_ENABLED=false`).
///
/// # Errors
/// [`LimiterStartError`] when rate limiting is enabled and the scripts do
/// not load (Valkey unreachable, or refusing `SCRIPT LOAD`).
pub async fn start_rate_limiter(
    redis: Client,
    config: RateLimitConfig,
) -> Result<Option<RateLimiter>, LimiterStartError> {
    let enabled = config.enabled;
    decide(enabled, || async move {
        let mut limiter = RateLimiter::new(redis, config);
        limiter.init().await.map(|()| limiter)
    })
    .await
}

/// The start decision: disabled → `None` without loading anything; enabled
/// → the loaded limiter, or an error when it does not load.
async fn decide<T, F, Fut>(enabled: bool, load: F) -> Result<Option<T>, LimiterStartError>
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = Result<T, fred::error::Error>>,
{
    if !enabled {
        return Ok(None);
    }
    load()
        .await
        .map(Some)
        .map_err(|cause| LimiterStartError { cause })
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, Ordering};

    use fred::error::{Error, ErrorKind};

    use super::*;

    #[tokio::test]
    async fn enabled_and_not_loaded_is_an_error_naming_cause_and_setting() {
        let err = decide::<u8, _, _>(true, || async {
            Err(Error::new(ErrorKind::IO, "connection refused"))
        })
        .await
        .unwrap_err();
        let text = err.to_string();
        assert!(text.contains("connection refused"), "{text}");
        assert!(text.contains("RATE_LIMIT_ENABLED=false"), "{text}");
        assert!(text.contains("only for tests"), "{text}");
    }

    #[tokio::test]
    async fn enabled_and_loaded_is_the_limiter() {
        let got = decide(true, || async { Ok::<_, Error>(7u8) })
            .await
            .unwrap();
        assert_eq!(got, Some(7));
    }

    #[tokio::test]
    async fn disabled_is_no_limiter_and_loads_nothing() {
        let called = AtomicBool::new(false);
        let got = decide::<u8, _, _>(false, || async {
            called.store(true, Ordering::SeqCst);
            Err(Error::new(ErrorKind::IO, "connection refused"))
        })
        .await
        .unwrap();
        assert_eq!(got, None);
        assert!(!called.load(Ordering::SeqCst), "nothing is loaded");
    }
}
