//! Information pages module.
//!
//! Provides platform-level and guild-level information pages for:
//! - Terms of Service, Privacy Policy (platform)
//! - Guild rules, FAQ, welcome pages (guild)
//!
//! Features:
//! - Markdown content with Mermaid diagram support
//! - Version tracking via content hashing
//! - User acceptance tracking
//! - Audit logging
//! - Role-based visibility (guild pages)

pub mod constants;
pub mod handlers;
pub mod queries;
pub mod router;
pub mod types;

pub use constants::*;
pub use queries::*;
pub use router::{guild_pages_router, platform_pages_router};
pub use types::*;
