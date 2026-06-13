package io.wolftown.kaiku.data.repository

import io.wolftown.kaiku.data.api.DmApi
import io.wolftown.kaiku.domain.model.DmConversation
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import javax.inject.Inject
import javax.inject.Singleton

/**
 * Holds the user's direct-message conversation list.
 *
 * The list is fetched from the REST API on demand; opening a DM marks it read
 * locally (and on the server) so its unread badge clears immediately.
 */
@Singleton
class DmRepository @Inject constructor(
    private val dmApi: DmApi,
) {
    private val _conversations = MutableStateFlow<List<DmConversation>>(emptyList())
    val conversations: StateFlow<List<DmConversation>> = _conversations.asStateFlow()

    suspend fun loadDms() {
        _conversations.value = dmApi.getDms()
    }

    /**
     * Mark a DM read: clears its badge locally, then tells the server. The
     * local clear is optimistic so the UI updates even if the network is slow;
     * a server failure is surfaced to the caller.
     */
    suspend fun markAsRead(channelId: String) {
        _conversations.value = clearUnread(_conversations.value, channelId)
        dmApi.markAsRead(channelId)
    }

    companion object {
        /** Reset unread for one conversation in the list (pure, testable). */
        fun clearUnread(conversations: List<DmConversation>, channelId: String): List<DmConversation> =
            conversations.map { if (it.id == channelId) it.copy(unreadCount = 0) else it }

        /**
         * Title to show for a conversation from the current user's perspective:
         * - 1:1 DM → the other participant's display name
         * - group DM → the explicit channel name, falling back to the joined
         *   names of the other participants
         */
        fun conversationTitle(dm: DmConversation, currentUserId: String): String {
            val others = dm.participants.filter { it.userId != currentUserId }
            return when {
                others.size > 1 -> dm.name.ifBlank { others.joinToString(", ") { it.displayName } }
                others.size == 1 -> others.first().displayName
                else -> dm.name.ifBlank { "Direct Message" }
            }
        }

        /**
         * Avatar to show for a conversation: the other participant's avatar for
         * a 1:1 DM, or the group icon for a multi-party DM. Null if neither is set.
         */
        fun conversationAvatarUrl(dm: DmConversation, currentUserId: String): String? {
            val others = dm.participants.filter { it.userId != currentUserId }
            return if (others.size == 1) others.first().avatarUrl else dm.iconUrl
        }
    }
}
