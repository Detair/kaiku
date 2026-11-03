//! Rate limiting module for protecting against abuse.
//!
//! Provides Redis-based rate limiting for various request categories
//! including authentication, API calls, and WebSocket connections.

pub mod account;
pub mod config;
pub mod constants;
pub mod error;
pub mod ip;
pub mod limiter;
pub mod middleware;
pub mod types;

pub use config::*;
pub use constants::*;
pub use error::*;
pub use ip::*;
pub use limiter::*;
pub use middleware::{check_ip_not_blocked, rate_limit_by_ip, rate_limit_by_user, with_category};
pub use types::*;
