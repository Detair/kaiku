package io.wolftown.kaiku.domain.model

import kotlinx.serialization.Serializable

@Serializable
data class Message(
    val id: String,
    val channelId: String,
    val author: User,
    val content: String,
    val encrypted: Boolean = false,
    val attachments: List<Attachment> = emptyList(),
    val replyTo: String? = null,
    val editedAt: String? = null,
    val createdAt: String = "",
    val reactions: List<Reaction>? = null
)
