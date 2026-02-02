//! Constants for information pages feature.

/// Maximum pages per scope (guild or platform).
pub const MAX_PAGES_PER_SCOPE: usize = 10;

/// Maximum content size in bytes (100KB).
pub const MAX_CONTENT_SIZE: usize = 102_400;

/// Maximum title length in characters.
pub const MAX_TITLE_LENGTH: usize = 100;

/// Maximum slug length in characters.
pub const MAX_SLUG_LENGTH: usize = 100;

/// Deleted slug cooldown period in days.
///
/// Prevents immediately reusing a slug that was recently deleted.
pub const DELETED_SLUG_COOLDOWN_DAYS: i64 = 7;

/// Reserved slugs that cannot be used for pages.
///
/// These are system-reserved paths that could conflict with API routes
/// or cause confusion in navigation.
pub const RESERVED_SLUGS: &[&str] = &[
    "admin", "api", "new", "edit", "delete", "settings", "create", "update", "list", "all", "me",
    "system",
];
