//! Permission system types and utilities.
//!
//! Two-tier permission model:
//! - System permissions: Platform-level admin actions
//! - Guild permissions: Per-guild role-based access control

pub mod guild;
pub mod models;
pub mod queries;
pub mod resolver;
pub mod system;

pub use guild::GuildPermissions;
pub use models::*;
pub use queries::*;
pub use resolver::{can_manage_role, can_moderate_member, compute_guild_permissions, PermissionError};
pub use system::SystemPermission;
