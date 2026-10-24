package io.wolftown.kaiku.domain.model

import kotlinx.serialization.Serializable

/**
 * Cursor-paginated response wrapper (server `CursorPaginatedResponse<T>`).
 *
 * Wire format (snake_case via `KaikuJson`):
 * `{"items": [...], "has_more": true, "next_cursor": "<uuid>"}`.
 * `next_cursor` is omitted by the server when there are no more items.
 */
@Serializable
data class CursorPage<T>(
    val items: List<T>,
    val hasMore: Boolean = false,
    val nextCursor: String? = null
)
