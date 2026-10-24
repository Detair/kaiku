package io.wolftown.kaiku.data.repository

import io.wolftown.kaiku.data.api.DmApi
import io.wolftown.kaiku.data.local.AuthState
import io.wolftown.kaiku.data.ws.KaikuWebSocket
import io.wolftown.kaiku.data.ws.ServerEvent
import io.wolftown.kaiku.di.ChatCoroutineScope
import io.wolftown.kaiku.domain.model.DmConversation
import io.wolftown.kaiku.domain.model.LastMessagePreview
import io.wolftown.kaiku.domain.model.Message
import kotlinx.coroutines.CancellationException
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
    private val authState: AuthState,
    @ChatCoroutineScope private val scope: CoroutineScope,
) {
    private val _conversations = MutableStateFlow<List<DmConversation>>(emptyList())
    val conversations: StateFlow<List<DmConversation>> = _conversations.asStateFlow()

    init {
        // UNDISPATCHED: subscribe to the replay-0 SharedFlow before init returns.
        scope.launch(start = CoroutineStart.UNDISPATCHED) {
            webSocket.events.collect { event ->
                when (event) {
                    is ServerEvent.DmCreated -> onDmCreated(event)
                    is ServerEvent.MessageNew -> onMessageNew(event)
                    else -> Unit
                }
            }
        }
    }

    /** The DM the user has open; its new messages count as read. */
    private val activeChannelId = MutableStateFlow<String?>(null)

    fun setActiveChannel(channelId: String) {
        activeChannelId.value = channelId
    }

    /** Resets the active DM only if it is still [channelId]. */
    fun clearActiveChannel(channelId: String) {
        activeChannelId.compareAndSet(channelId, null)
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
            val merged = liveOnly + fetched
            // The open DM is being read: the server's count does not apply.
            activeChannelId.value?.let { clearUnread(merged, it) } ?: merged
        }
        // The server knows these now; no need to remember them as live.
        fetchedIds.forEach { liveCreatedAt.remove(it) }
        activeChannelId.value?.let { markReadQuietly(it) }
    }

    /**
     * Mark a DM read: clears its badge locally, then tells the server. The
     * local clear is optimistic so the UI updates even if the network is slow;
     * a server failure is surfaced to the caller.
     */
    suspend fun markAsRead(channelId: String) {
        val lastMessageId = _conversations.value.find { it.id == channelId }?.lastMessage?.id
        _conversations.update { clearUnread(it, channelId) }
        // The server marks read up to a message id; with no message there is nothing to send.
        if (lastMessageId != null) dmApi.markAsRead(channelId, lastMessageId)
    }

    /**
     * A message in a known DM updates its preview and raises its unread badge,
     * unless that DM is open: then it is marked read instead. Messages for
     * channels that are not DMs in the list are ignored.
     */
    private fun onMessageNew(event: ServerEvent.MessageNew) {
        if (_conversations.value.none { it.id == event.channelId }) return
        val message = try {
            json.decodeFromJsonElement<Message>(event.message)
        } catch (e: Exception) {
            logger.warning("Failed to decode message_new payload (${e::class.simpleName})")
            return
        }
        val preview = LastMessagePreview(
            id = message.id,
            content = message.content,
            userId = message.author.id,
            username = message.author.username,
            createdAt = message.createdAt,
        )
        val active = event.channelId == activeChannelId.value
        val own = message.author.id == authState.currentUserId.value
        var duplicate = false
        _conversations.update { current ->
            current.map {
                when {
                    it.id != event.channelId -> it
                    it.lastMessage?.id == message.id -> {
                        duplicate = true
                        it
                    }
                    else -> it.copy(
                        lastMessage = preview,
                        unreadCount = when {
                            active -> 0
                            own -> it.unreadCount
                            else -> it.unreadCount + 1
                        },
                    )
                }
            }
        }
        if (active && !duplicate) markReadQuietly(event.channelId)
    }

    private fun markReadQuietly(channelId: String) {
        scope.launch {
            try {
                markAsRead(channelId)
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                logger.warning("Failed to mark DM read (${e::class.simpleName})")
            }
        }
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
