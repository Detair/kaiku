//! Starting the rate limiter: when rate limiting is enabled, the limiter
//! must load, or the server does not start.
//!
//! Without its scripts the limiter cannot count anything: no request limit
//! per address, no counting of failed sign-ins per address and no address
//! blocks. Starting without them would leave every one of those off with
//! nothing but a log line to show for it.
//!
//! The server runs this before its database migrations, so a server that is
//! going to refuse to start has not changed its database yet.

use std::fmt;
use std::future::Future;

use fred::clients::Client;

use super::{RateLimitConfig, RateLimiter};

/// The setting that switches rate limiting off.
pub const ENABLED_VAR: &str = "RATE_LIMIT_ENABLED";

/// The spellings `RATE_LIMIT_ENABLED` accepts.
pub const ACCEPTED_VALUES: &str = "true, false";

/// How the value of `RATE_LIMIT_ENABLED` is read. Only `false` switches
/// rate limiting off; any value other than `true` and `false` keeps it on
/// (as it always has) and is warned about at start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EnabledValue {
    /// Not set: enabled.
    Unset,
    /// `true`.
    On,
    /// `false`.
    Off,
    /// Any other value, read as enabled.
    Unexpected(String),
}

impl EnabledValue {
    /// Classify a raw value (`None` = not set).
    #[must_use]
    pub fn read(raw: Option<&str>) -> Self {
        match raw {
            None => Self::Unset,
            Some("true") => Self::On,
            Some("false") => Self::Off,
            Some(other) => Self::Unexpected(other.to_string()),
        }
    }

    /// The value of `RATE_LIMIT_ENABLED` in this process's environment.
    #[must_use]
    pub fn from_env() -> Self {
        Self::read(std::env::var(ENABLED_VAR).ok().as_deref())
    }

    /// Whether rate limiting is on with this value.
    #[must_use]
    pub const fn is_enabled(&self) -> bool {
        !matches!(self, Self::Off)
    }
}

/// Rate limiting is enabled but the limiter could not be initialised.
#[derive(Debug)]
pub struct LimiterStartError {
    /// The Valkey error the script load failed with (without the script
    /// text a server may echo back).
    pub cause: String,
    /// The value of `RATE_LIMIT_ENABLED` when it was neither `true` nor
    /// `false`.
    pub unexpected_value: Option<String>,
}

impl fmt::Display for LimiterStartError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "rate limiting is enabled, but the rate limiter could not load its scripts into \
             Valkey: {}. Valkey must be reachable when the server starts and must allow \
             SCRIPT LOAD and EVALSHA for the server's user.",
            self.cause
        )?;
        if let Some(value) = &self.unexpected_value {
            write!(
                f,
                " {ENABLED_VAR} is \"{value}\", which is read as enabled (accepted values: \
                 {ACCEPTED_VALUES})."
            )?;
        }
        write!(
            f,
            " {ENABLED_VAR}=false switches rate limiting off; set it only for tests"
        )
    }
}

impl std::error::Error for LimiterStartError {}

/// The error text of a failed script load, cut before the arguments a
/// server repeats in an "unknown command" reply (the script itself).
fn cause_text(error: &fred::error::Error) -> String {
    let text = error.to_string();
    match text.find(", with args beginning with") {
        Some(at) => text[..at].to_string(),
        None => text,
    }
}

/// The rate limiter for this server, or `None` when rate limiting is
/// disabled by configuration (`RATE_LIMIT_ENABLED=false`).
///
/// `value` is how `RATE_LIMIT_ENABLED` was read ([`EnabledValue::from_env`]);
/// an unexpected spelling is logged once at WARN. A failure is logged at
/// ERROR before it is returned.
///
/// # Errors
/// [`LimiterStartError`] when rate limiting is enabled and the scripts do
/// not load (Valkey unreachable, or refusing script loading).
pub async fn start_rate_limiter(
    redis: Client,
    config: RateLimitConfig,
    value: &EnabledValue,
) -> Result<Option<RateLimiter>, LimiterStartError> {
    if let EnabledValue::Unexpected(raw) = value {
        warn_unexpected_value(raw);
    }
    let enabled = config.enabled;
    let result = decide(enabled, || async move {
        let mut limiter = RateLimiter::new(redis, config);
        limiter.init().await.map(|()| limiter)
    })
    .await
    .map_err(|mut e| {
        if let EnabledValue::Unexpected(raw) = value {
            e.unexpected_value = Some(raw.clone());
        }
        e
    });
    if let Err(e) = &result {
        log_start_error(e);
    }
    result
}

/// The one WARN line for an unexpected `RATE_LIMIT_ENABLED` value.
fn warn_unexpected_value(raw: &str) {
    tracing::warn!(
        setting = ENABLED_VAR,
        value = raw,
        accepted = ACCEPTED_VALUES,
        "{ENABLED_VAR} is \"{raw}\", which is not one of the accepted values \
         ({ACCEPTED_VALUES}); rate limiting stays enabled"
    );
}

/// The ERROR line for a refused start (standard error gets it from `main`).
fn log_start_error(error: &LimiterStartError) {
    tracing::error!(error = %error, "the server does not start: rate limiter not loaded");
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
    load().await.map(Some).map_err(|cause| LimiterStartError {
        cause: cause_text(&cause),
        unexpected_value: None,
    })
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
    async fn the_error_never_repeats_the_script() {
        let err = decide::<u8, _, _>(true, || async {
            Err(Error::new(
                ErrorKind::Unknown,
                "ERR unknown command 'SCRIPT', with args beginning with: 'LOAD' \
                 '-- Atomic rate limit check and increment -- KEYS[1]'",
            ))
        })
        .await
        .unwrap_err();
        let text = err.to_string();
        assert!(text.contains("unknown command 'SCRIPT'"), "{text}");
        assert!(!text.contains("KEYS[1]"), "{text}");
        assert!(!text.contains("Atomic rate limit"), "{text}");
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

    #[test]
    fn only_false_switches_rate_limiting_off() {
        for (raw, expect, enabled) in [
            (None, EnabledValue::Unset, true),
            (Some("true"), EnabledValue::On, true),
            (Some("false"), EnabledValue::Off, false),
            (Some("0"), EnabledValue::Unexpected("0".into()), true),
            (
                Some("False"),
                EnabledValue::Unexpected("False".into()),
                true,
            ),
            (Some("no"), EnabledValue::Unexpected("no".into()), true),
            (Some("off"), EnabledValue::Unexpected("off".into()), true),
            (Some("1"), EnabledValue::Unexpected("1".into()), true),
            (
                Some(" false"),
                EnabledValue::Unexpected(" false".into()),
                true,
            ),
            (Some(""), EnabledValue::Unexpected(String::new()), true),
        ] {
            let read = EnabledValue::read(raw);
            assert_eq!(read, expect, "{raw:?}");
            assert_eq!(read.is_enabled(), enabled, "{raw:?}");
            // The configuration reads the same value the same way.
            let vars: Vec<(&str, &str)> = raw.map(|v| vec![(ENABLED_VAR, v)]).unwrap_or_default();
            assert_eq!(
                RateLimitConfig::from_env_map(&vars).enabled,
                enabled,
                "config and start agree on {raw:?}"
            );
        }
    }

    #[tokio::test]
    async fn the_error_names_an_unexpected_value() {
        let mut err = decide::<u8, _, _>(true, || async {
            Err(Error::new(ErrorKind::IO, "connection refused"))
        })
        .await
        .unwrap_err();
        assert!(!err.to_string().contains("is read as enabled"));
        err.unexpected_value = Some("0".into());
        let text = err.to_string();
        assert!(
            text.contains("RATE_LIMIT_ENABLED is \"0\", which is read as enabled"),
            "{text}"
        );
        assert!(text.contains("accepted values: true, false"), "{text}");
    }

    /// Everything logged at WARN and above inside `f`, as JSON lines.
    fn logged(f: impl FnOnce()) -> String {
        use std::sync::{Arc, Mutex};
        #[derive(Clone)]
        struct Buf(Arc<Mutex<Vec<u8>>>);
        impl std::io::Write for Buf {
            fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
                self.0.lock().unwrap().extend_from_slice(b);
                Ok(b.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let buf = Buf(Arc::new(Mutex::new(Vec::new())));
        let writer = buf.clone();
        let subscriber = tracing_subscriber::fmt()
            .json()
            .with_max_level(tracing::Level::WARN)
            .with_writer(move || writer.clone())
            .finish();
        tracing::subscriber::with_default(subscriber, f);
        let bytes = buf.0.lock().unwrap().clone();
        String::from_utf8(bytes).unwrap()
    }

    #[test]
    fn an_unexpected_value_is_warned_about_once_with_value_and_spellings() {
        let out = logged(|| warn_unexpected_value("0"));
        assert_eq!(out.lines().count(), 1, "{out}");
        assert!(out.contains("\"level\":\"WARN\""), "{out}");
        assert!(out.contains("\"value\":\"0\""), "{out}");
        assert!(out.contains("\"accepted\":\"true, false\""), "{out}");
        assert!(out.contains("rate limiting stays enabled"), "{out}");
    }

    #[test]
    fn a_refused_start_is_an_error_line() {
        let err = LimiterStartError {
            cause: "NOPERM no permissions".into(),
            unexpected_value: None,
        };
        let out = logged(|| log_start_error(&err));
        assert_eq!(out.lines().count(), 1, "{out}");
        assert!(out.contains("\"level\":\"ERROR\""), "{out}");
        assert!(out.contains("NOPERM no permissions"), "{out}");
    }
}
