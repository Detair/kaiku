package io.wolftown.kaiku.domain.model

import kotlinx.serialization.Serializable

/**
 * `GET /api/me/unread`: the server's unread counts. Only channels with unread
 * messages are listed. Guild channel counts include the user's own messages;
 * DM counts only others' messages.
 */
@Serializable
data class UnreadAggregate(
    val guilds: List<GuildUnreadSummary> = emptyList(),
    val dms: List<ChannelUnread> = emptyList(),
    val total: Long = 0,
)

@Serializable
data class GuildUnreadSummary(
    val guildId: String,
    val guildName: String = "",
    val channels: List<ChannelUnread> = emptyList(),
    val totalUnread: Long = 0,
)

@Serializable
data class ChannelUnread(
    val channelId: String,
    val channelName: String = "",
    val unreadCount: Long = 0,
)
