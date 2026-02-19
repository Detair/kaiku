//! Webhooks & Bot Event System
//!
//! HTTP POST delivery of platform events to bot endpoints with HMAC signing,
//! retry logic, and dead-letter handling.

pub mod delivery;
pub mod dispatch;
pub mod events;
pub mod handlers;
pub mod queries;
pub mod signing;
pub mod ssrf;
pub mod types;
