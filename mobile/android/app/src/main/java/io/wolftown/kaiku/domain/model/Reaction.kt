package io.wolftown.kaiku.domain.model

import kotlinx.serialization.Serializable

/** One emoji reaction on a message, as the server sends it (`ReactionInfo`). */
@Serializable
data class Reaction(
    val emoji: String,
    val count: Int = 0,
    val me: Boolean = false,
    val users: List<String> = emptyList()
)
