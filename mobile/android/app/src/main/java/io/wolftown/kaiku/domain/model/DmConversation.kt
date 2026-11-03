package io.wolftown.kaiku.domain.model

import kotlinx.serialization.Serializable

/**
 * A direct-message conversation as returned by `GET /api/dm`.
 *
 * The server flattens the underlying channel fields (id, name, channel_type,
 * icon_url, …) into this object alongside the DM-specific participants, last
 * message preview, and unread count. Unmodelled channel fields are dropped by
 * the JSON parser ([io.wolftown.kaiku.data.KaikuJson] has `ignoreUnknownKeys`).
 *
 * A DM *is* a channel, so its messages are loaded through the normal message
 * API using [id] as the channel id.
 */
@Serializable
data class DmConversation(
    val id: String,
    val name: String = "",
    val channelType: ChannelType = ChannelType.DM,
    val iconUrl: String? = null,
    val participants: List<DmParticipant> = emptyList(),
    val lastMessage: LastMessagePreview? = null,
    val unreadCount: Int = 0,
    /**
     * C28: the Kaiku DM, set only from the `is_system` flags of the item or its
     * participants (see `DmApiImpl.getDms`), never from an id or a name.
     */
    val isSystem: Boolean = false,
    /** C28 `read_only`: true only for the Kaiku DM. */
    val readOnly: Boolean = false,
)

@Serializable
data class DmParticipant(
    val userId: String,
    val username: String,
    val displayName: String,
    val avatarUrl: String? = null,
    val joinedAt: String = "",
    /** C28: the "Kaiku" system account. */
    val isSystem: Boolean = false,
)

@Serializable
data class LastMessagePreview(
    val id: String,
    val content: String,
    val userId: String? = null,
    val username: String? = null,
    val createdAt: String = "",
)
