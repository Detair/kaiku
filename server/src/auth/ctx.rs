//! Borrowed handles every auth and approvals core function takes, so the
//! same code runs from HTTP handlers, the timers and the host CLI.

use sqlx::PgPool;

use crate::api::AppState;
use crate::config::Config;

/// Database, Valkey and configuration, borrowed.
#[derive(Clone, Copy)]
pub struct AuthCtx<'a> {
    pub db: &'a PgPool,
    pub redis: &'a fred::clients::Client,
    pub config: &'a Config,
}

impl<'a> From<&'a AppState> for AuthCtx<'a> {
    fn from(state: &'a AppState) -> Self {
        Self {
            db: &state.db,
            redis: &state.redis,
            config: &state.config,
        }
    }
}

impl AuthCtx<'_> {
    /// The signature origin (`PUBLIC_URL`'s ASCII origin, C26).
    #[must_use]
    pub fn origin(&self) -> &str {
        &self.config.public_origin.origin
    }

    /// The server key for keyed hashes of pre-session secrets (DC1).
    #[must_use]
    pub const fn hmac_key(&self) -> &[u8; 32] {
        self.config.auth_hmac_key.as_bytes()
    }
}
