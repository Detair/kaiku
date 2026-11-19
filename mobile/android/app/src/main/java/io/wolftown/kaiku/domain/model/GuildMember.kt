package io.wolftown.kaiku.domain.model

import kotlinx.serialization.Serializable

/** `GET /api/guilds/{id}/members` item (fields this app reads). */
@Serializable
data class GuildMember(
    val userId: String,
    val username: String,
    val displayName: String = "",
    val avatarUrl: String? = null,
    /** The member's nickname on this server, if set. */
    val nickname: String? = null,
)

/** `GET /api/friends/blocked` item: someone the signed-in user blocked. */
@Serializable
data class BlockedUser(val userId: String)
