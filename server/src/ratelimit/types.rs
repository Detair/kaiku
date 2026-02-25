//! Rate limiting types.

use serde::Serialize;

/// Categories for rate limiting with different thresholds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RateLimitCategory {
    /// Login attempts
    AuthLogin,
    /// Registration attempts
    AuthRegister,
    /// Password reset requests
    AuthPasswordReset,
    /// Other auth operations (token refresh, etc.)
    AuthOther,
    /// Write operations (create/update/delete)
    Write,
    /// Social operations (friend requests, invites)
    Social,
    /// Read operations (fetch data)
    Read,
    /// WebSocket connection attempts
    WsConnect,
    /// WebSocket message rate
    WsMessage,
    /// Failed authentication tracking (for IP blocking)
    FailedAuth,
    /// Voice channel join attempts
    VoiceJoin,
    /// Search operations
    Search,
    /// Data governance operations (export, deletion)
    DataGovernance,
}

impl RateLimitCategory {
    /// Returns the string identifier for this category (used in Redis keys).
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::AuthLogin => "auth_login",
            Self::AuthRegister => "auth_register",
            Self::AuthPasswordReset => "auth_pwd_reset",
            Self::AuthOther => "auth_other",
            Self::Write => "write",
            Self::Social => "social",
            Self::Read => "read",
            Self::WsConnect => "ws_connect",
            Self::WsMessage => "ws_message",
            Self::FailedAuth => "failed_auth",
            Self::VoiceJoin => "voice_join",
            Self::Search => "search",
            Self::DataGovernance => "data_governance",
        }
    }

    /// Returns all categories except `FailedAuth` (which is handled separately).
    pub const fn all() -> &'static [Self] {
        &[
            Self::AuthLogin,
            Self::AuthRegister,
            Self::AuthPasswordReset,
            Self::AuthOther,
            Self::Write,
            Self::Social,
            Self::Read,
            Self::WsConnect,
            Self::WsMessage,
            Self::VoiceJoin,
            Self::Search,
            Self::DataGovernance,
        ]
    }
}

/// Result of a rate limit check.
#[derive(Debug, Clone)]
pub struct RateLimitResult {
    /// Whether the request is allowed
    pub allowed: bool,
    /// Maximum requests allowed in the window
    pub limit: u32,
    /// Remaining requests in the current window
    pub remaining: u32,
    /// Unix timestamp when the window resets
    pub reset_at: u64,
    /// Seconds to wait before retrying (0 if allowed)
    pub retry_after: u64,
}

/// Information about a blocked IP address.
#[derive(Debug, Clone, Serialize)]
pub struct BlockedIpInfo {
    /// The blocked IP address (normalized)
    pub ip: String,
    /// Number of failed authentication attempts
    pub failed_attempts: u32,
    /// Unix timestamp when the block expires
    pub blocked_until: u64,
}

/// Normalized IP address stored in request extensions.
///
/// IPv4 addresses are stored as-is.
/// IPv6 addresses are normalized to /64 prefix for rate limiting.
#[derive(Debug, Clone)]
pub struct NormalizedIp(pub String);
