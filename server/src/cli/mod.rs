//! `kaiku-admin`, the host CLI (spec F6 item 4; plan Task 13).
//!
//! Runs on the server host with the server's own environment
//! (`docker compose exec server kaiku-admin …`): whoever can run it already
//! controls the server, so it needs no sign-in. It sets up a key phone for
//! any account (admins included) and removes devices or ends sessions.
//!
//! Every command writes a `system_audit_log` row (`actor_id` = the system
//! account); changes also record their security event with actor `cli`, so
//! the account (and for high events every admin) gets a notice. Notices go
//! through the outbox (DC11): delivered right after commit like the HTTP
//! routes, else by the server's minute sweep. Session ends go through
//! `NOTIFY` (DC5), so the running server closes the sockets.
//!
//! The CLI prints no token other than the setup link it issues.

pub mod devices;
pub mod enroll;
pub mod recovery;
pub mod sessions;

use std::future::Future;

use sqlx::PgConnection;
use uuid::Uuid;

use crate::api::AppState;
use crate::auth::ctx::AuthCtx;
use crate::config::Config;
use crate::system_account::SYSTEM_USER_ID;

/// Usage text (printed with exit code 2).
pub const USAGE: &str = "Usage:
  kaiku-admin enroll <username>
  kaiku-admin devices list <username>
  kaiku-admin devices remove <username> <device-id>
  kaiku-admin sessions list <username>
  kaiku-admin sessions end <username> (<session-id> | --all)
  kaiku-admin factor reset <username> (password | totp)
  kaiku-admin throttle reset <username>
  kaiku-admin may-approve <username> (on | off)
  kaiku-admin recovery-contact <username> (<contact-username> | --none)
  kaiku-admin admin demote <username>
  kaiku-admin request cancel <request-id>";

/// A CLI command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// Set up a key phone for the account (active at once).
    Enroll {
        username: String,
    },
    DevicesList {
        username: String,
    },
    /// `device` is the key device's id from `devices list`.
    DevicesRemove {
        username: String,
        device: Uuid,
    },
    SessionsList {
        username: String,
    },
    /// `None` ends every session of the account.
    SessionsEnd {
        username: String,
        session: Option<Uuid>,
    },
    /// Clear a throttled or compromised password or TOTP.
    FactorReset {
        username: String,
        factor: crate::auth::factors::Factor,
    },
    ThrottleReset {
        username: String,
    },
    MayApprove {
        username: String,
        on: bool,
    },
    /// `None` removes the contact.
    RecoveryContact {
        username: String,
        contact: Option<String>,
    },
    AdminDemote {
        username: String,
    },
    RequestCancel {
        request: Uuid,
    },
}

/// Bad command line (exit code 2).
#[derive(Debug, thiserror::Error)]
#[error("{0}\n{USAGE}")]
pub struct UsageError(pub String);

fn uuid_arg(s: &str, what: &str) -> Result<Uuid, UsageError> {
    Uuid::parse_str(s).map_err(|_| UsageError(format!("{what} must be a UUID")))
}

impl Command {
    /// Parse the arguments after the program name.
    pub fn parse(args: &[String]) -> Result<Self, UsageError> {
        let a: Vec<&str> = args.iter().map(String::as_str).collect();
        let username = |u: &str| u.to_string();
        match a.as_slice() {
            ["enroll", u] => Ok(Self::Enroll {
                username: username(u),
            }),
            ["devices", "list", u] => Ok(Self::DevicesList {
                username: username(u),
            }),
            ["devices", "remove", u, d] => Ok(Self::DevicesRemove {
                username: username(u),
                device: uuid_arg(d, "<device-id>")?,
            }),
            ["sessions", "list", u] => Ok(Self::SessionsList {
                username: username(u),
            }),
            ["sessions", "end", u, "--all"] => Ok(Self::SessionsEnd {
                username: username(u),
                session: None,
            }),
            ["sessions", "end", u, s] => Ok(Self::SessionsEnd {
                username: username(u),
                session: Some(uuid_arg(s, "<session-id>")?),
            }),
            ["factor", "reset", u, "password"] => Ok(Self::FactorReset {
                username: username(u),
                factor: crate::auth::factors::Factor::Password,
            }),
            ["factor", "reset", u, "totp"] => Ok(Self::FactorReset {
                username: username(u),
                factor: crate::auth::factors::Factor::Totp,
            }),
            ["throttle", "reset", u] => Ok(Self::ThrottleReset {
                username: username(u),
            }),
            ["may-approve", u, on @ ("on" | "off")] => Ok(Self::MayApprove {
                username: username(u),
                on: *on == "on",
            }),
            ["recovery-contact", u, "--none"] => Ok(Self::RecoveryContact {
                username: username(u),
                contact: None,
            }),
            ["recovery-contact", u, c] => Ok(Self::RecoveryContact {
                username: username(u),
                contact: Some(username(c)),
            }),
            ["admin", "demote", u] => Ok(Self::AdminDemote {
                username: username(u),
            }),
            ["request", "cancel", r] => Ok(Self::RequestCancel {
                request: uuid_arg(r, "request-id")?,
            }),
            [] => Err(UsageError("No command given.".into())),
            _ => Err(UsageError("Unknown command or wrong arguments.".into())),
        }
    }
}

/// The terminal the CLI talks to (a script in the tests).
pub trait CliIo {
    fn print(&mut self, s: &str);
    fn read_line(&mut self, prompt: &str) -> impl Future<Output = std::io::Result<String>> + Send;
}

/// Stdout and stdin.
pub struct StdIo;

impl CliIo for StdIo {
    fn print(&mut self, s: &str) {
        println!("{s}");
    }

    async fn read_line(&mut self, prompt: &str) -> std::io::Result<String> {
        use std::io::Write;
        print!("{prompt}");
        std::io::stdout().flush()?;
        tokio::task::spawn_blocking(|| {
            let mut line = String::new();
            let n = std::io::stdin().read_line(&mut line)?;
            if n == 0 {
                return Err(std::io::ErrorKind::UnexpectedEof.into());
            }
            Ok(line)
        })
        .await
        .map_err(std::io::Error::other)?
    }
}

/// The server's database, Valkey and configuration.
pub struct CliCtx {
    pub db: sqlx::PgPool,
    pub redis: fred::clients::Client,
    pub config: Config,
}

impl CliCtx {
    /// From the server's environment (`.env` is read as by the server).
    pub async fn from_env() -> anyhow::Result<Self> {
        dotenvy::dotenv().ok();
        let config = Config::from_env()?;
        let db = sqlx::postgres::PgPoolOptions::new()
            .max_connections(2)
            .acquire_timeout(std::time::Duration::from_secs(10))
            .connect(&config.database_url)
            .await?;
        let redis = crate::db::create_redis_client_with_timeout(
            &config.redis_url,
            config.valkey_command_timeout,
        )
        .await?;
        Ok(Self { db, redis, config })
    }

    /// The handles the auth and approvals cores take.
    #[must_use]
    pub const fn auth(&self) -> AuthCtx<'_> {
        AuthCtx {
            db: &self.db,
            redis: &self.redis,
            config: &self.config,
        }
    }

    /// A server state for the after-commit steps the HTTP routes run too
    /// (notice delivery, WS hints, push clean-up). No listener, no limiter.
    pub fn app_state(&self) -> anyhow::Result<AppState> {
        let sfu = crate::voice::sfu::SfuServer::new(std::sync::Arc::new(self.config.clone()), None)
            .map_err(|e| anyhow::anyhow!("voice setup: {e}"))?;
        Ok(AppState::new(crate::api::AppStateConfig {
            db: self.db.clone(),
            redis: self.redis.clone(),
            config: self.config.clone(),
            s3: None,
            sfu,
            rate_limiter: None,
            screen_share_limiter: None,
            email: None,
            oidc_manager: None,
            http_client: reqwest::Client::new(),
        }))
    }
}

/// The account named `username` (never the system account).
pub(crate) async fn account(conn: &mut PgConnection, username: &str) -> anyhow::Result<Uuid> {
    sqlx::query_scalar("SELECT id FROM users WHERE username = $1 AND NOT is_system")
        .bind(username)
        .fetch_optional(&mut *conn)
        .await?
        .ok_or_else(|| anyhow::anyhow!("No account named '{username}'."))
}

/// The CLI's own audit row: `cli.<command>` by the system account.
pub(crate) async fn audit(
    conn: &mut PgConnection,
    action: &str,
    user_id: Uuid,
    details: serde_json::Value,
) -> sqlx::Result<()> {
    sqlx::query(
        "INSERT INTO system_audit_log (actor_id, action, target_type, target_id, details)
         VALUES ($1, $2, 'user', $3, $4)",
    )
    .bind(SYSTEM_USER_ID)
    .bind(action)
    .bind(user_id)
    .bind(details)
    .execute(&mut *conn)
    .await?;
    Ok(())
}

/// Run one command.
pub async fn run(ctx: &CliCtx, cmd: Command, io: &mut impl CliIo) -> anyhow::Result<()> {
    match cmd {
        Command::Enroll { username } => enroll::run(ctx, &username, io).await,
        Command::DevicesList { username } => devices::list(ctx, &username, io).await,
        Command::DevicesRemove { username, device } => {
            devices::remove(ctx, &username, device, io).await
        }
        Command::SessionsList { username } => sessions::list(ctx, &username, io).await,
        Command::SessionsEnd { username, session } => {
            sessions::end(ctx, &username, session, io).await
        }
        Command::FactorReset { username, factor } => {
            recovery::factor_reset(ctx, &username, factor, io).await
        }
        Command::ThrottleReset { username } => recovery::throttle_reset(ctx, &username, io).await,
        Command::MayApprove { username, on } => recovery::may_approve(ctx, &username, on, io).await,
        Command::RecoveryContact { username, contact } => {
            recovery::recovery_contact(ctx, &username, contact.as_deref(), io).await
        }
        Command::AdminDemote { username } => recovery::admin_demote(ctx, &username, io).await,
        Command::RequestCancel { request } => recovery::request_cancel(ctx, request, io).await,
    }
}

/// Deliver notices right after a commit (the minute sweep retries).
pub(crate) async fn deliver(ctx: &CliCtx, events: Vec<Uuid>) {
    if events.is_empty() {
        return;
    }
    match ctx.app_state() {
        Ok(state) => {
            if let Err(e) = crate::security_events::notices::deliver(&state, &events).await {
                tracing::warn!(error = %e, "notice delivery failed; the sweep retries");
            }
        }
        Err(e) => tracing::warn!(error = %e, "no server state for notices; the sweep delivers"),
    }
}
