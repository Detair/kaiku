package io.wolftown.kaiku.domain.model

import kotlinx.serialization.Serializable

/**
 * A channel category of a server (`GET /api/guilds/{id}/categories`).
 * The server's `category_type` is ignored by this client.
 */
@Serializable
data class Category(
    val id: String,
    val name: String,
    val position: Int = 0,
    val parentId: String? = null,
)
