package io.wolftown.kaiku.domain

import io.wolftown.kaiku.data.repository.DmRepository
import io.wolftown.kaiku.domain.model.Channel
import io.wolftown.kaiku.domain.model.DmConversation

/** Display title of a channel; [isDm] tells the UI whether to skip the `#` prefix. */
data class ChannelTitle(val text: String, val isDm: Boolean)

/**
 * Resolves what to show as the title of [channelId]: the guild channel name,
 * or the DM title from the current user's perspective. Returns null when the
 * channel is unknown (yet); the UI then shows a neutral placeholder, never the id.
 */
fun resolveChannelTitle(
    channelId: String,
    channels: List<Channel>,
    dms: List<DmConversation>,
    currentUserId: String?,
): ChannelTitle? {
    channels.find { it.id == channelId }?.let { return ChannelTitle(it.name, isDm = false) }
    val dm = dms.find { it.id == channelId } ?: return null
    return ChannelTitle(DmRepository.conversationTitle(dm, currentUserId.orEmpty()), isDm = true)
}
