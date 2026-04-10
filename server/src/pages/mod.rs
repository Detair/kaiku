//! Information pages module.
//!
//! Provides platform-level and guild-level information pages for:
//! - Terms of Service, Privacy Policy (platform)
//! - Guild rules, FAQ, welcome pages (guild)
//!
//! Features:
//! - Markdown content with Mermaid diagram support
//! - Version tracking via content hashing
//! - Revision history with restore capability
//! - Guild-scoped page categories
//! - User acceptance tracking
//! - Audit logging

pub mod constants;
pub mod error;
pub mod handlers;
pub mod queries;
pub mod router;
pub mod types;

pub use constants::*;
pub use queries::*;
pub use router::{guild_page_categories_router, guild_pages_router, platform_pages_router};
pub use types::*;
