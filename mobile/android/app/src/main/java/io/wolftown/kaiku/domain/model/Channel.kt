package io.wolftown.kaiku.domain.model

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable

@Serializable
enum class ChannelType {
    @SerialName("text") TEXT,
    @SerialName("voice") VOICE,
    @SerialName("dm") DM;
}

@Serializable
data class Channel(
    val id: String,
    val name: String,
    val channelType: ChannelType,
    val categoryId: String? = null,
    val topic: String? = null,
    val userLimit: Int? = null,
    val position: Int = 0,
    val createdAt: String = "",
    // Unread tracking (server sends these on the guild channels endpoint;
    // defaulted so other channel responses that omit them still parse).
    val unreadCount: Int = 0,
    val lastMessageId: String? = null,
    val lastReadMessageId: String? = null,
)
