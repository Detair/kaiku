package io.wolftown.kaiku.domain.model

import kotlinx.serialization.Serializable

/**
 * Channel kinds the client understands. Any other server value (forum,
 * announcement, ...) decodes to [UNKNOWN] and is hidden from the channel list.
 */
@Serializable(with = ChannelTypeSerializer::class)
enum class ChannelType {
    TEXT,
    VOICE,
    DM,
    UNKNOWN;
}

object ChannelTypeSerializer : LenientEnumSerializer<ChannelType>(
    serialName = "ChannelType",
    values = ChannelType.values(),
    unknown = ChannelType.UNKNOWN,
    wireName = { type ->
        when (type) {
            ChannelType.TEXT -> "text"
            ChannelType.VOICE -> "voice"
            ChannelType.DM -> "dm"
            ChannelType.UNKNOWN -> "unknown"
        }
    },
)

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
