package io.wolftown.kaiku.data.repository

import io.wolftown.kaiku.data.api.DmApi
import io.wolftown.kaiku.data.ws.KaikuWebSocket
import io.wolftown.kaiku.data.ws.ServerEvent
import io.wolftown.kaiku.di.ChatCoroutineScope
import io.wolftown.kaiku.domain.model.DmConversation
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.CoroutineStart
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.decodeFromJsonElement
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.atomic.AtomicLong
import java.util.logging.Logger
import javax.inject.Inject
import javax.inject.Singleton

/**
 * Holds the user's direct-message conversation list.
 *
 * The list is fetched from the REST API on demand; opening a DM marks it read
 * locally (and on the server) so its unread badge clears immediately.
 * A `dm_created` WebSocket event (C3) adds a new conversation at the top
 * without a reload.
 */
@Singleton
class DmRepository @Inject constructor(
    private val dmApi: DmApi,
    private val webSocket: KaikuWebSocket,
    private val json: Json,
    @ChatCoroutineScope private val scope: CoroutineScope,
) {
    private val _conversations = MutableStateFlow<List<DmConversation>>(emptyList())
    val conversations: StateFlow<List<DmConversation>> = _conversations.asStateFlow()

    init {
        // UNDISPATCHED: subscribe to the replay-0 SharedFlow before init returns.
        scope.launch(start = CoroutineStart.UNDISPATCHED) {
            webSocket.events.collect { event ->
                if (event is ServerEvent.DmCreated) onDmCreated(event)
            }
        }
    }

    /** Sequence of dm_created events, and the position each live DM arrived at. */
    private val liveSeq = AtomicLong(0)
    private val liveCreatedAt = ConcurrentHashMap<String, Long>()

    /**
     * Replaces the list with the server's, in server order. Conversations a
     * `dm_created` added while the request was in flight and that the
     * fetched list does not contain yet are kept, first. Anything else that
     * is missing from the server's list is dropped.
     */
    suspend fun loadDms() {
        val startedAt = liveSeq.get()
        val fetched = dmApi.getDms()
        val fetchedIds = fetched.mapTo(HashSet()) { it.id }
        _conversations.update { current ->
            val liveOnly = current.filter {
                it.id !in fetchedIds && (liveCreatedAt[it.id] ?: 0L) > startedAt
            }
            liveOnly + fetched
        }
        // The server knows these now; no need to remember them as live.
        fetchedIds.forEach { liveCreatedAt.remove(it) }
    }

    /**
     * Mark a DM read: clears its badge locally, then tells the server. The
     * local clear is optimistic so the UI updates even if the network is slow;
     * a server failure is surfaced to the caller.
     */
    suspend fun markAsRead(channelId: String) {
        val lastMessageId = _conversations.value.find { it.id == channelId }?.lastMessage?.id
        _conversations.value = clearUnread(_conversations.value, channelId)
        // The server marks read up to a message id; with no message there is nothing to send.
        if (lastMessageId != null) dmApi.markAsRead(channelId, lastMessageId)
    }

    private fun onDmCreated(event: ServerEvent.DmCreated) {
        val dm = try {
            json.decodeFromJsonElement<DmConversation>(event.channel)
        } catch (e: Exception) {
            logger.warning("Failed to decode dm_created payload (${e::class.simpleName})")
            return
        }
        liveCreatedAt[dm.id] = liveSeq.incrementAndGet()
        _conversations.update { upsertAtTop(it, dm) }
    }

    companion object {
        private val logger = Logger.getLogger("DmRepository")

        /** Put [dm] first, replacing any conversation with the same id (pure, testable). */
        fun upsertAtTop(conversations: List<DmConversation>, dm: DmConversation): List<DmConversation> =
            listOf(dm) + conversations.filter { it.id != dm.id }

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
