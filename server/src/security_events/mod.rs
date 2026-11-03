//! Security events and notices (spec Part C, contract C28).
//!
//! **SECURITY CRITICAL.** Events are append-only (a trigger refuses every
//! change except setting `delivered_at` or `repudiated_at` once), readable by
//! the user and the admins, and never deletable from a session. Each event
//! has a `system_audit_log` row written in the same transaction.

pub mod record;
pub mod types;

pub use record::record;
pub use types::{Action, Actor, EventType, NewSecurityEvent, SecurityEvent, Severity};
