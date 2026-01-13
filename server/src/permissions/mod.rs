//! Permission system for the VoiceChat platform.
//!
//! This module provides types and utilities for managing permissions:
//! - Guild-level permissions using bitflags for efficient storage and operations
//! - System-level permissions for administrative actions
//!
//! Permissions are stored as BIGINT in PostgreSQL and support efficient bitwise operations.

mod guild;
mod system;

pub use guild::GuildPermissions;
pub use system::SystemPermission;
