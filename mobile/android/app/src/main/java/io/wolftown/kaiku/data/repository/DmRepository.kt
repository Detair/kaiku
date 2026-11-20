package io.wolftown.kaiku.data.repository

import io.wolftown.kaiku.data.api.DmApi
import io.wolftown.kaiku.data.api.withRequestSession
import io.wolftown.kaiku.data.local.AuthState
import io.wolftown.kaiku.data.session.SessionGeneration
import io.wolftown.kaiku.data.ws.KaikuWebSocket
import io.wolftown.kaiku.data.ws.ServerEvent
import io.wolftown.kaiku.di.ChatCoroutineScope
import io.wolftown.kaiku.domain.model.DmConversation
import io.wolftown.kaiku.domain.parseServerTime
import io.wolftown.kaiku.domain.sortByActivity
import io.wolftown.kaiku.domain.model.LastMessagePreview
import io.wolftown.kaiku.domain.model.Message
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.CoroutineStart
import kotlinx.coroutines.Deferred
import kotlinx.coroutines.async
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
 * A `dm_created` WebSocket event (C3) adds a new conversation without a
 * reload. The list is always ordered by last activity (last message, else
 * creation), newest first, and re-sorted when a message arrives or is sent.
 *
 * **The list belongs to the signed-in session.** Every piece of work (a
 * list load, a "New message" request, a mark-read, a socket event) takes
 * the session's ticket ([SessionGeneration]) when it starts: its requests
 * are bound to that session, it runs in that session's scope and it
 * publishes only while the ticket is open. The session-end hook's advance
 * clears the list itself, and from the first step of a sign-out on the
 * repository takes no work for the session that is ending, although its
 * tokens are still stored until the server was told. So nothing of one
 * account's chats reaches the next account.
 */
@Singleton
class DmRepository @Inject constructor(
    private val dmApi: DmApi,
    private val webSocket: KaikuWebSocket,
    private val json: Json,
    private val authState: AuthState,
    @ChatCoroutineScope private val scope: CoroutineScope,
    private val sessions: SessionGeneration = SessionGeneration(authState),
) {
    private val _conversations = MutableStateFlow<List<DmConversation>>(emptyList())
    val conversations: StateFlow<List<DmConversation>> = _conversations.asStateFlow()

    init {
        // Inside the session-end hook's advance: the list of the account that ends is gone
        // before the stored session changes.
        sessions.onAdvance { clearSession() }
        // UNDISPATCHED: subscribe to the replay-0 SharedFlow before init returns.
        scope.launch(start = CoroutineStart.UNDISPATCHED) {
            // The socket's events are those of the account it authenticated as (its Ready).
            var socketAccount: String? = null
            webSocket.events.collect { event ->
                if (event is ServerEvent.Ready) socketAccount = event.userId
                when (event) {
                    is ServerEvent.DmCreated -> forSocketAccount(socketAccount) { onDmCreated(event) }
                    is ServerEvent.MessageNew -> forSocketAccount(socketAccount) { onMessageNew(event, it) }
                    is ServerEvent.DmRead -> forSocketAccount(socketAccount) { onDmRead(event) }
                    else -> Unit
                }
            }
        }
    }

    /** Runs [block] for an event of [account]'s socket only while that account's session is open. */
    private inline fun forSocketAccount(account: String?, block: (SessionGeneration.Ticket) -> Unit) {
        val session = sessions.enter(account ?: return)?.takeIf { it.open } ?: return
        block(session)
    }

    /** The open session's ticket, or null: no work for nobody or for a session that is ending. */
    private fun openSession(): SessionGeneration.Ticket? = sessions.enter()?.takeIf { it.open }

    /** The scope of the signed-in session's background work (reloads, mark-read); ends with it. */
    private val workLock = Any()
    private var workSession: SessionGeneration.Ticket? = null
    private var workScope: CoroutineScope? = null

    private fun scopeOf(session: SessionGeneration.Ticket): CoroutineScope = synchronized(workLock) {
        if (workSession != session) {
            workSession = session
            workScope = session.scope(scope)
        }
        workScope!!
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

    /**
     * Session epoch: [clearSession] bumps it; [loadDms] publishes only if it
     * is unchanged since the load started (under [epochLock]), so a load in
     * flight at logout cannot bring the old list back.
     */
    private val epochLock = Any()
    private var epoch = 0L

    /** Loads started, and the newest one that published. Guarded by [epochLock]. */
    private var loadsStarted = 0L
    private var newestPublished = 0L

    /**
     * Session teardown: drops the conversation list, the open DM and live-DM
     * bookkeeping, and cancels every "New message" request of the session.
     * A load still in flight publishes nothing.
     */
    fun clearSession() {
        val cancelled = synchronized(epochLock) {
            epoch++
            activeChannelId.value = null
            liveCreatedAt.clear()
            reads.clear()
            _conversations.value = emptyList()
            opening.values.toList().also { opening.clear() }
        }
        cancelled.forEach { it.cancel() }
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
        // For the signed-in session of this moment; nothing for nobody or for a session that is ending.
        val session = openSession() ?: return
        val (startEpoch, load) = synchronized(epochLock) { epoch to ++loadsStarted }
        val startedAt = liveSeq.get()
        val fetched = withRequestSession(session.requestSession()) { dmApi.getDms() }
        val fetchedIds = fetched.mapTo(HashSet()) { it.id }
        synchronized(epochLock) {
            if (epoch != startEpoch || !session.open) return
            // An older load answering after a newer one has nothing newer to say.
            if (load < newestPublished) return
            newestPublished = load
            publishFetched(fetched, fetchedIds, startedAt)
        }
        activeChannelId.value?.let { markReadQuietly(it, session) }
    }

    private fun publishFetched(fetched: List<DmConversation>, fetchedIds: Set<String>, startedAt: Long) {
        _conversations.update { current ->
            val liveOnly = current.filter {
                it.id !in fetchedIds && (liveCreatedAt[it.id] ?: 0L) > startedAt
            }
            // A message that arrived after the server took its snapshot keeps its preview and count.
            val byId = current.associateBy { it.id }
            val kept = fetched.map { server -> byId[server.id]?.let { local -> newerOf(local, server) } ?: server }
            // A read that happened after this request was sent (here or on another device) is
            // newer than the answer's count, as long as it reached the answer's last message.
            val read = kept.map { dm ->
                val mark = reads[dm.id]
                if (mark != null && mark.seq > startedAt && dm.lastMessage?.id == mark.messageId) dm.copy(unreadCount = 0) else dm
            }
            val merged = sortByActivity(liveOnly + read)
            // The open DM is being read: the server's count does not apply.
            activeChannelId.value?.let { clearUnread(merged, it) } ?: merged
        }
        // The server knows these now; no need to remember them as live.
        fetchedIds.forEach { liveCreatedAt.remove(it) }
        // A request sent after a read has that read in its answer.
        reads.entries.removeIf { it.value.seq <= startedAt }
    }

    /** A read up to [messageId], at position [seq] of the live events. */
    private class ReadMark(val seq: Long, val messageId: String)

    /** The latest read per conversation that a list answer may not know yet. */
    private val reads = ConcurrentHashMap<String, ReadMark>()

    /** One "New message" request: of the session [epoch], to the person [userId]. */
    private data class OpenKey(val epoch: Long, val userId: String)

    /** "New message" requests in flight. Guarded by [epochLock], so they never outlive their session. */
    private val opening = HashMap<OpenKey, Deferred<Result<String>>>()

    /** The session ended while a request of it was waiting or running. */
    private class SessionEnded : CancellationException("session ended")

    /**
     * Returns the id of the 1:1 DM with [person], creating it if needed.
     * Only for someone a fresh look has just confirmed as writable
     * ([PeopleRepository.confirm]): a conversation already in the list is
     * then returned without a request. Otherwise `POST /api/dm`
     * (get-or-create on the server) runs once per person even when this is
     * called again before it answered (double tap, slow network): later
     * callers wait for the same answer. The request runs in the signed-in
     * session's scope, so leaving the screen does not cancel it, and its
     * answer is added to the list before this returns (unless the session
     * ended meanwhile). Throws what [DmApi.createDm] throws.
     */
    suspend fun openDirectMessage(person: EligiblePerson): String {
        val session = person.session
        val userId = person.userId
        var created: Deferred<Result<String>>? = null
        val (key, request) = synchronized(epochLock) {
            // Confirmed for a session that is over or ending: nothing is opened for it.
            if (!session.open) throw SessionEnded()
            // Looked up in the same step as the shared request: a request that just
            // published its answer and removed itself is found here, not sent again.
            existingDirectMessage(_conversations.value, userId, session.userId)?.let { return it.id }
            val key = OpenKey(epoch, userId)
            key to opening.getOrPut(key) {
                // Lazy: it cannot start before it is in the map, and sign-out can cancel it before it sends.
                scopeOf(session).async(start = CoroutineStart.LAZY) { create(key, session) }.also { created = it }
            }
        }
        created?.let { mine ->
            mine.invokeOnCompletion {
                synchronized(epochLock) { if (opening[key] === mine) opening.remove(key) }
            }
            mine.start()
        }
        val result = request.await()
        // An answer of an ended session never reaches the next one (or anyone).
        if (synchronized(epochLock) { epoch } != key.epoch || !session.open) throw SessionEnded()
        return result.getOrThrow()
    }

    /** The request of [key] for [session]: nothing goes out once that session ended. */
    private suspend fun create(key: OpenKey, session: SessionGeneration.Ticket): Result<String> {
        if (synchronized(epochLock) { epoch } != key.epoch || !session.open) throw SessionEnded()
        // The failure travels in the Result: a failed create must not fail the scope.
        return try {
            val dm = withRequestSession(session.requestSession()) { dmApi.createDm(key.userId) }
            var unknown = false
            synchronized(epochLock) {
                if (epoch != key.epoch || !session.open) throw SessionEnded()
                unknown = _conversations.value.none { it.id == dm.id }
                // Kept by a list load that started before and answers without it.
                liveCreatedAt[dm.id] = liveSeq.incrementAndGet()
                _conversations.update { sortByActivity(upsertKeepingState(it, dm)) }
            }
            // A conversation the cached list did not have: its preview and unread count come from the server.
            if (unknown) reloadQuietly(session)
            Result.success(dm.id)
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            Result.failure(e)
        }
    }

    private fun reloadQuietly(session: SessionGeneration.Ticket) {
        scopeOf(session).launch {
            try {
                loadDms()
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                logger.warning("Failed to reload direct messages (${e::class.simpleName})")
            }
        }
    }

    /**
     * Mark a DM read: clears its badge locally, then tells the server. The
     * local clear is optimistic so the UI updates even if the network is slow;
     * a server failure is surfaced to the caller.
     */
    suspend fun markAsRead(channelId: String) {
        val session = openSession() ?: return
        val lastMessageId = _conversations.value.find { it.id == channelId }?.lastMessage?.id
        _conversations.update { clearUnread(it, channelId) }
        // The server marks read up to a message id; with no message there is nothing to send.
        if (lastMessageId != null) {
            // Newer than a list answer whose request is already out.
            reads[channelId] = ReadMark(liveSeq.incrementAndGet(), lastMessageId)
            withRequestSession(session.requestSession()) { dmApi.markAsRead(channelId, lastMessageId) }
        }
    }

    /**
     * A message in a known DM updates its preview and raises its unread badge,
     * unless that DM is open: then it is marked read instead. Messages for
     * channels that are not DMs in the list are ignored.
     */
    private fun onMessageNew(event: ServerEvent.MessageNew, session: SessionGeneration.Ticket) {
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
            encrypted = message.encrypted,
        )
        val active = event.channelId == activeChannelId.value
        val own = message.author.id == session.userId
        var duplicate = false
        _conversations.update { current ->
            val updated = current.map {
                when {
                    it.id != event.channelId -> it
                    it.lastMessage?.id == message.id -> {
                        duplicate = true
                        it
                    }
                    else -> it.copy(
                        // A delayed older message counts, but the newer preview stays.
                        lastMessage = if (isOlder(preview, it.lastMessage)) it.lastMessage else preview,
                        unreadCount = when {
                            active -> 0
                            own -> it.unreadCount
                            else -> it.unreadCount + 1
                        },
                    )
                }
            }
            // The updated conversation goes first before the (stable) sort, so it wins a tie.
            val (moved, rest) = if (duplicate) emptyList<DmConversation>() to updated else updated.partition { it.id == event.channelId }
            sortByActivity(moved + rest)
        }
        if (active && !duplicate) markReadQuietly(event.channelId, session)
    }

    /** Marks [channelId] read in the signed-in session's scope (leaving the screen does not stop it); failures are logged. */
    fun markReadInBackground(channelId: String) {
        markReadQuietly(channelId, openSession() ?: return)
    }

    /**
     * Read on another device: the count goes to zero when the read reaches
     * the conversation's last message. A read up to an older message leaves
     * the count (the newer messages are still unread; their number is not known).
     */
    private fun onDmRead(event: ServerEvent.DmRead) {
        val readUpTo = event.lastReadMessageId ?: return
        // Kept for a list answer that is already on its way and still has the old count.
        reads[event.channelId] = ReadMark(liveSeq.incrementAndGet(), readUpTo)
        _conversations.update { current ->
            current.map { if (it.id == event.channelId && it.lastMessage?.id == readUpTo) it.copy(unreadCount = 0) else it }
        }
    }

    private fun markReadQuietly(channelId: String, session: SessionGeneration.Ticket) {
        scopeOf(session).launch {
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
        _conversations.update { sortByActivity(upsertAtTop(it, dm)) }
    }

    companion object {
        private val logger = Logger.getLogger("DmRepository")

        /** Whether [incoming] is older than [shown] (both with times; otherwise not older). */
        fun isOlder(incoming: LastMessagePreview, shown: LastMessagePreview?): Boolean {
            val a = parseServerTime(incoming.createdAt) ?: return false
            val b = parseServerTime(shown?.createdAt) ?: return false
            return a.isBefore(b)
        }

        /** The server's entry, or the local one where it already has a newer last message. */
        fun newerOf(local: DmConversation, server: DmConversation): DmConversation {
            val localLast = local.lastMessage ?: return server
            val serverLast = server.lastMessage
            val localNewer = serverLast == null || isOlder(serverLast, localLast)
            return if (localNewer) server.copy(lastMessage = localLast, unreadCount = local.unreadCount) else server
        }

        /** Put [dm] first, replacing any conversation with the same id (pure, testable). */
        fun upsertAtTop(conversations: List<DmConversation>, dm: DmConversation): List<DmConversation> =
            listOf(dm) + conversations.filter { it.id != dm.id }

        /**
         * The 1:1 DM between [me] and [userId] in [conversations], or null.
         * Group DMs and the Kaiku DM never count.
         */
        fun existingDirectMessage(conversations: List<DmConversation>, userId: String, me: String?): DmConversation? =
            conversations.firstOrNull { dm ->
                !dm.isSystem && dm.participants.filter { it.userId != me }.map { it.userId } == listOf(userId)
            }

        /**
         * Adds [dm] (an answer of `POST /api/dm`, without preview or unread
         * count) at the top, or keeps the known entry's preview and unread
         * count if it is already listed.
         */
        fun upsertKeepingState(conversations: List<DmConversation>, dm: DmConversation): List<DmConversation> {
            val known = conversations.find { it.id == dm.id } ?: return upsertAtTop(conversations, dm)
            return conversations.map { if (it.id == dm.id) known.copy(participants = dm.participants.ifEmpty { known.participants }) else it }
        }

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
