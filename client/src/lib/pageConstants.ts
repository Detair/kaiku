/**
 * Page-related constants.
 *
 * These values should match the server-side constants in:
 * - server/src/pages/handlers.rs (MAX_CONTENT_SIZE)
 * - server/src/pages/queries.rs (MAX_SLUG_LENGTH)
 */

/** Maximum content size in bytes (100KB). */
export const MAX_CONTENT_SIZE = 102_400;

/** Maximum slug length in characters. */
export const MAX_SLUG_LENGTH = 100;

/** Maximum pages per scope (platform or guild). Must match server constant. */
export const MAX_PAGES_PER_SCOPE = 10;

/** Scroll tolerance in pixels for "read to bottom" detection. */
export const SCROLL_TOLERANCE = 20;
