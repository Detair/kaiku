//! Constants for information pages feature.

/// Default maximum pages per scope (guild or platform).
/// Can be overridden per-guild via `guilds.max_pages` or instance config.
pub const DEFAULT_MAX_PAGES_PER_SCOPE: i64 = 10;

/// Default maximum revisions per page.
/// Can be overridden per-guild via `guilds.max_revisions` or instance config.
pub const DEFAULT_MAX_REVISIONS_PER_PAGE: i64 = 25;

/// Maximum content size in bytes (100KB).
pub const MAX_CONTENT_SIZE: usize = 102_400;

/// Maximum title length in characters.
pub const MAX_TITLE_LENGTH: usize = 100;

/// Maximum slug length in characters.
pub const MAX_SLUG_LENGTH: usize = 100;

/// Maximum category name length in characters.
pub const MAX_CATEGORY_NAME_LENGTH: usize = 50;

/// Maximum categories per guild.
pub const MAX_CATEGORIES_PER_GUILD: i64 = 20;

/// Deleted slug cooldown period in days.
///
/// Prevents immediately reusing a slug that was recently deleted.
pub const DELETED_SLUG_COOLDOWN_DAYS: i64 = 7;

/// Reserved slugs that cannot be used for pages.
///
/// These are system-reserved paths that could conflict with API routes
/// or cause confusion in navigation.
pub const RESERVED_SLUGS: &[&str] = &[
    "admin",
    "api",
    "new",
    "edit",
    "delete",
    "settings",
    "create",
    "update",
    "list",
    "all",
    "me",
    "system",
    "library",
    "revisions",
    "categories",
];
