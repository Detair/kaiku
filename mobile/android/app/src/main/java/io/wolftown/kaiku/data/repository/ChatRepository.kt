package io.wolftown.kaiku.data.repository

import android.net.Uri
import io.ktor.http.HttpStatusCode
import io.wolftown.kaiku.data.local.AuthSession
import io.wolftown.kaiku.data.local.AuthState
import io.wolftown.kaiku.data.api.ApiException
import io.wolftown.kaiku.data.api.MessageApi
import io.wolftown.kaiku.data.api.UploadLimits
import io.wolftown.kaiku.data.ws.ClientEvent
import io.wolftown.kaiku.data.ws.KaikuWebSocket
import io.wolftown.kaiku.data.ws.ServerEvent
import io.wolftown.kaiku.data.ws.WallClock
import io.wolftown.kaiku.di.ChatCoroutineScope
import io.wolftown.kaiku.domain.MessageOrdering
import io.wolftown.kaiku.domain.withReactionAdded
import io.wolftown.kaiku.domain.withReactionRemoved
import io.wolftown.kaiku.domain.model.Message
import io.wolftown.kaiku.domain.model.Reaction
import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import kotlinx.coroutines.CoroutineStart
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.currentCoroutineContext
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.getAndUpdate
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.decodeFromJsonElement
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.atomic.AtomicLong
import java.util.logging.Level
import java.util.logging.Logger
import javax.inject.Inject
import javax.inject.Singleton

/**
 * Manages text message state for all subscribed channels.
 *
 * Responsibilities:
 * - Exposes per-channel message lists as [StateFlow]s
 * - Loads message history from the REST API
 * - Processes real-time WebSocket events (new/edit/delete, typing)
 * - Keeps each message's reactions up to date from reaction events
 * - Sends client events (subscribe, unsubscribe, typing) via WebSocket
 * - Typing indicator debouncing (max once per 3 seconds)
 * - Remembers which channels need a server-side subscription, so they can be
 *   re-subscribed after a reconnect ([resubscribeAll]), and fetches messages
 *   missed while offline ([catchUp])
 */
@Singleton
class ChatRepository internal constructor(
    private val messageApi: MessageApi,
    private val webSocket: KaikuWebSocket,
    private val json: Json,
    private val guildRepository: GuildRepository,
    private val authState: AuthState,
    private val scope: CoroutineScope,
    /** Time source for typing expiry; tests pass virtual time. */
    private val clock: WallClock,
    private val attachments: AttachmentReader? = null,
    /** Dispatcher for blocking file access; tests pass a test dispatcher. */
    private val ioDispatcher: CoroutineDispatcher = Dispatchers.IO,
) {

    @Inject
    constructor(
        messageApi: MessageApi,
        webSocket: KaikuWebSocket,
        json: Json,
        guildRepository: GuildRepository,
        authState: AuthState,
        @ChatCoroutineScope scope: CoroutineScope,
        attachments: AttachmentReader,
    ) : this(
        messageApi, webSocket, json, guildRepository, authState, scope,
        // Monotonic: typing expiry only compares differences.
        WallClock { System.nanoTime() / 1_000_000L },
        attachments,
        Dispatchers.IO,
    )

    companion object {
        private val logger = Logger.getLogger("ChatRepository")
        private const val TYPING_DEBOUNCE_MS = 3_000L

        /** Photo/video types the server accepts from the picker (its default allowlist). */
        private val UPLOAD_MIME_TYPES = setOf(
            "image/jpeg", "image/png", "image/gif", "image/webp", "video/mp4", "video/webm",
        )

        /** A typing indicator disappears this long after the user's last typing event. */
        const val TYPING_EXPIRY_MS = 8_000L

        /** Page size for [catchUp] (`limit`). */
        const val CATCH_UP_PAGE_SIZE = 100

        /** [catchUp] fetches at most this many pages, then reloads the latest page instead. */
        const val CATCH_UP_MAX_PAGES = 5
    }

    /** Per-channel message lists. */
    private val channelMessages = ConcurrentHashMap<String, MutableStateFlow<List<Message>>>()

    /** Per-channel typing users: user id to the time ([WallClock]) their indicator expires. */
    private val channelTypingUsers = ConcurrentHashMap<String, MutableStateFlow<Map<String, Long>>>()

    /** Track last typing indicator send time per channel for debouncing. */
    private val lastTypingSent = ConcurrentHashMap<String, Long>()

    /** WebSocket event collection job. */
    private var eventCollectionJob: Job? = null

    /** Channels a screen has subscribed to (open chat screens). */
    private val subscribedChannels: MutableSet<String> = ConcurrentHashMap.newKeySet()

    /**
     * DM channels that stay subscribed in the background, independent of any
     * open screen, so new DM messages arrive live (badges, DM list).
     *
     * Kept as a separate set rather than a reference count: a channel needs a
     * server subscription while it is in either set, and
     * [unsubscribeFromChannel] only sends `unsubscribe` when the channel is in
     * neither. Closing an open DM screen therefore never drops the DM's
     * background subscription.
     */
    private val dmChannels: MutableSet<String> = ConcurrentHashMap.newKeySet()

    /** Sequence of [trackDmChannel] calls, and the position each DM was tracked at. */
    private val dmTrackSeq = AtomicLong(0)
    private val dmTrackedAt = ConcurrentHashMap<String, Long>()

    /**
     * Newest message id per channel as it was when the first `Ready` since
     * the channel's last completed catch-up arrived, i.e. before any event of
     * that socket. An unconsumed anchor survives further reconnects. [catchUp] pages from here, so a
     * live message that lands before the catch-up starts cannot hide the gap.
     */
    private val catchUpAnchors = ConcurrentHashMap<String, String>()

    private val _lastServerTime = MutableStateFlow<String?>(null)

    /**
     * `server_time` of the latest `Ready` (C2 watermark). Phase 1 only stores
     * it; null until a Ready arrives or on servers that do not send it.
     */
    val lastServerTime: StateFlow<String?> = _lastServerTime.asStateFlow()

    /**
     * Session epoch: [clearSession] bumps it. A load ([loadMessages],
     * [catchUp]) or send captures it at start and publishes its result only if
     * it is unchanged, checked under [epochLock] together with the write.
     */
    private val epochLock = Any()
    @Volatile
    private var epoch = 0L

    /** Jobs of uploads in flight; [clearSession] cancels them. */
    private val uploadJobs: MutableSet<Job> = ConcurrentHashMap.newKeySet()

    /** Runs [publish] only if [startEpoch] is still current; returns whether it ran. */
    private inline fun publishIfCurrent(startEpoch: Long, publish: () -> Unit): Boolean =
        synchronized(epochLock) {
            if (epoch != startEpoch) return false
            publish()
            true
        }

    init {
        startCollectingEvents()
    }

    // -- Public API ------------------------------------------------------------

    /**
     * Session teardown: empties every message and typing list (screens still
     * holding a flow see it empty), forgets subscriptions, DM tracking,
     * catch-up anchors, the server-time watermark and the upload limits. The
     * WebSocket event collector keeps running for the next session. Loads
     * still in flight publish nothing (session epoch).
     */
    fun clearSession() = synchronized(epochLock) {
        epoch++
        // An upload of the ended session must not go out later (with whatever
        // token is stored then): cancel every one in flight.
        uploadJobs.forEach { it.cancel() }
        uploadJobs.clear()
        channelMessages.values.forEach { it.value = emptyList() }
        channelMessages.clear()
        channelTypingUsers.values.forEach { it.value = emptyMap() }
        channelTypingUsers.clear()
        lastTypingSent.clear()
        subscribedChannels.clear()
        dmChannels.clear()
        dmTrackedAt.clear()
        catchUpAnchors.clear()
        _lastServerTime.value = null
        cachedLimits = null
    }

    /** `created_at` of a message in the local cache, or null when it is not cached. Creates nothing. */
    fun cachedCreatedAt(channelId: String, messageId: String): String? =
        channelMessages[channelId]?.value?.firstOrNull { it.id == messageId }?.createdAt?.ifEmpty { null }

    fun getMessages(channelId: String): StateFlow<List<Message>> {
        return getOrCreateMessagesFlow(channelId).asStateFlow()
    }

    /**
     * Loads one page of history for [channelId] and returns whether older
     * messages exist on the server (`has_more`).
     *
     * Without [before] the page replaces the current list (initial load),
     * except for messages that arrived live while the request was in flight
     * and are not in the page: those are kept. The fresh page also supersedes
     * any unconsumed catch-up anchor of the channel.
     * With [before] the older page is merged into the current list.
     * Either way the list ends up oldest-to-newest (see [MessageOrdering]).
     */
    suspend fun loadMessages(channelId: String, before: String? = null): Boolean {
        val startEpoch = epoch
        val flow = getOrCreateMessagesFlow(channelId)
        val idsBeforeRequest = flow.value.mapTo(HashSet()) { it.id }
        val page = messageApi.getMessages(channelId, before = before)

        if (before != null) {
            publishIfCurrent(startEpoch) {
                flow.update { existing -> MessageOrdering.merge(existing, page.items) }
            }
        } else {
            val pageIds = page.items.mapTo(HashSet()) { it.id }
            val published = publishIfCurrent(startEpoch) {
                flow.update { existing ->
                    // Arrived during the request; compared by id, not by time, so
                    // a live message with the same timestamp as the page's newest
                    // item is not lost.
                    val live = existing.filter { it.id !in idsBeforeRequest && it.id !in pageIds }
                    MessageOrdering.merge(page.items, live)
                }
                catchUpAnchors.remove(channelId)
            }
            // Opened channel: everything just loaded has been seen.
            if (published) flow.value.lastOrNull()?.let { guildRepository.onActiveChannelMessage(channelId, it.id) }
        }
        return page.hasMore
    }

    /**
     * Fetches the messages of [channelId] that are newer than the newest one
     * in the local list and merges them in (deduplicated by id). This is the
     * reconnect catch-up and the Phase 2 wake-fetch seam.
     *
     * Pages forwards with `after = <newest id>`, then `after = next_cursor`,
     * and `limit = `[CATCH_UP_PAGE_SIZE] while the server reports `has_more`
     * with a `next_cursor`, up to [CATCH_UP_MAX_PAGES] pages. If
     * more remain after that, the list is replaced with the latest page
     * ([loadMessages]) instead of fetching everything. Every newly added
     * message is reported to [GuildRepository.onMessageReceived] (unread badge
     * for non-active channels).
     *
     * Does nothing for a channel whose list was never loaded (empty): its
     * screen does a full load when it opens. To be gap-free, call this only
     * after the server acknowledged the channel's `subscribe` (see
     * `server/src/ws/AGENTS.md`).
     */
    suspend fun catchUp(channelId: String) {
        val startEpoch = epoch
        val flow = channelMessages[channelId] ?: return
        // The anchor is only removed once the catch-up completed: if it fails
        // or the socket drops first, the next reconnect starts from it again.
        val anchor = catchUpAnchors[channelId]
        val start = anchor ?: flow.value.lastOrNull()?.id ?: return
        if (!catchUpFrom(channelId, flow, start, startEpoch)) return
        if (anchor != null) publishIfCurrent(startEpoch) { catchUpAnchors.remove(channelId, anchor) }
    }

    /** Returns false if the session ended (epoch changed) and the catch-up was dropped. */
    private suspend fun catchUpFrom(
        channelId: String,
        flow: MutableStateFlow<List<Message>>,
        start: String,
        startEpoch: Long,
    ): Boolean {
        var after = start

        repeat(CATCH_UP_MAX_PAGES) { pageIndex ->
            val page = try {
                messageApi.getMessages(channelId, after = after, limit = CATCH_UP_PAGE_SIZE)
            } catch (e: ApiException) {
                // 400 "Unknown cursor": the anchor message no longer exists in
                // this channel. Fall back to the latest page.
                if (pageIndex == 0 && e.status == HttpStatusCode.BadRequest) {
                    logger.info("catch-up cursor rejected, reloading latest page")
                    loadMessages(channelId)
                    return true
                }
                throw e
            }
            var previous: List<Message> = emptyList()
            val published = publishIfCurrent(startEpoch) {
                previous = flow.getAndUpdate { current -> MessageOrdering.merge(current, page.items) }
            }
            if (!published) return false
            val known = previous.mapTo(HashSet()) { it.id }
            val me = authState.currentUserId.value
            page.items.filter { it.id !in known }.distinctBy { it.id }.forEach {
                guildRepository.onMessageReceived(channelId, it.id, countUnread = it.author.id != me)
            }

            // Follow the server's cursor, not the newest returned item: the
            // server computes `next_cursor` before filtering out messages of
            // blocked users, so a page can be empty while more messages follow.
            val next = page.nextCursor
            if (!page.hasMore || next == null) return true
            after = next
        }

        logger.info("catch-up for a channel exceeded $CATCH_UP_MAX_PAGES pages, reloading latest page")
        loadMessages(channelId)
        return true
    }

    /**
     * Sends `subscribe` for every channel that needs a server subscription
     * (open screens and background DM channels) and returns their ids. Called
     * on every `Ready`, because a new socket starts with no channel
     * subscriptions.
     */
    fun resubscribeAll(): Set<String> {
        val ids = subscribedChannels + dmChannels
        ids.forEach { webSocket.send(ClientEvent.Subscribe(it)) }
        return ids
    }

    /**
     * Makes [channelIds] the background DM subscriptions. Sends `subscribe`
     * for ids that were not tracked yet and `unsubscribe` for dropped ids that
     * no screen has open, except ids [trackDmChannel] added after the
     * [dmTrackingMark] passed as [keepTrackedAfter]. Returns the ids a
     * `subscribe` was sent for.
     */
    fun setDmChannels(channelIds: Collection<String>, keepTrackedAfter: Long = Long.MAX_VALUE): Set<String> {
        val next = channelIds.toSet()
        val added = next - dmChannels
        // A DM tracked from dm_created after [keepTrackedAfter] may simply be
        // missing from a DM list fetched before it existed: keep it.
        val removed = (dmChannels - next).filterNot { (dmTrackedAt[it] ?: 0L) > keepTrackedAfter }.toSet()
        removed.forEach { dmTrackedAt.remove(it) }
        dmChannels.addAll(added)
        dmChannels.removeAll(removed)
        added.forEach { webSocket.send(ClientEvent.Subscribe(it)) }
        removed.filterNot { it in subscribedChannels }
            .forEach { webSocket.send(ClientEvent.Unsubscribe(it)) }
        return added
    }

    /**
     * Tracks a newly created DM as a background subscription without sending
     * anything: the server already subscribed every open socket (C3).
     */
    fun trackDmChannel(channelId: String) {
        dmTrackedAt[channelId] = dmTrackSeq.incrementAndGet()
        dmChannels.add(channelId)
    }

    /** Current position of [trackDmChannel] calls; pass it to [setDmChannels] later. */
    fun dmTrackingMark(): Long = dmTrackSeq.get()

    /** Whether [channelId] is tracked as a background DM subscription. */
    fun isDmChannelTracked(channelId: String): Boolean = channelId in dmChannels

    suspend fun sendMessage(channelId: String, content: String) {
        val startEpoch = epoch
        val sentMessage = messageApi.sendMessage(channelId, content)

        // Optimistic update: add the message immediately. A WebSocket echo
        // with the same id is deduplicated by MessageOrdering.merge.
        publishIfCurrent(startEpoch) {
            getOrCreateMessagesFlow(channelId).update { current ->
                MessageOrdering.merge(current, listOf(sentMessage))
            }
        }
    }

    /** Why [sendAttachment] failed; the UI maps it to a string resource. */
    sealed class AttachmentException(cause: Throwable? = null) : Exception(cause) {
        class TooLarge : AttachmentException()
        class UnsupportedType : AttachmentException()
        class Failed(cause: Throwable? = null) : AttachmentException(cause)
    }

    /** Thrown by [LimitedInputStream] when an unsized file turns out to exceed the limit. */
    private class StreamTooLargeException : java.io.IOException("attachment exceeds upload limit")

    /** Passes bytes through and fails once more than [limit] were read. */
    private class LimitedInputStream(
        private val delegate: java.io.InputStream,
        private val limit: Long,
    ) : java.io.InputStream() {
        private var total = 0L

        override fun read(): Int {
            val b = delegate.read()
            if (b >= 0 && ++total > limit) throw StreamTooLargeException()
            return b
        }

        override fun read(buffer: ByteArray, off: Int, len: Int): Int {
            val n = delegate.read(buffer, off, len)
            if (n > 0) {
                total += n
                if (total > limit) throw StreamTooLargeException()
            }
            return n
        }

        override fun close() = delegate.close()
    }

    /**
     * Upload limits of the current session, keyed by the signed-in user id
     * (logout and login as someone else miss the cache); also dropped after a 413.
     */
    @Volatile
    private var cachedLimits: Pair<String?, UploadLimits>? = null

    private suspend fun uploadLimits(): UploadLimits {
        val startEpoch = epoch
        val me = authState.currentUserId.value
        cachedLimits?.takeIf { it.first == me }?.let { return it.second }
        // A fetch that outlives a session clear is used once but not cached.
        return messageApi.getUploadLimits().also { publishIfCurrent(startEpoch) { cachedLimits = me to it } }
    }

    /**
     * Sends the picked photo/video [uri] to [channelId] with an optional
     * [caption]. The file is streamed, never held in memory, and all file
     * access runs on the IO dispatcher. Files above the server's attachment
     * limit are rejected before the file is opened when the size is known,
     * otherwise while streaming ([AttachmentException.TooLarge]); types the
     * server would refuse are rejected early ([AttachmentException.UnsupportedType]);
     * every other failure is [AttachmentException.Failed]. The created message
     * is merged into the channel list; the WebSocket echo is deduplicated by id.
     *
     * The upload is bound to the session it starts in. When that session
     * ends ([clearSession]), the coroutine calling this is cancelled (its
     * [Job] is registered for the duration of the upload), so callers see a
     * [kotlinx.coroutines.CancellationException], and nothing is sent or
     * published in another session.
     */
    suspend fun sendAttachment(channelId: String, uri: Uri, caption: String?): Result<Message> {
        val reader = attachments ?: return Result.failure(AttachmentException.Failed())
        val startEpoch = epoch
        // Bound to the session it starts in: never sent in another one.
        val sessionId = (authState.session.value as? AuthSession.LoggedIn)?.sessionId
        val uploadJob = currentCoroutineContext()[Job]
        if (uploadJob != null) {
            val registered = publishIfCurrent(startEpoch) { uploadJobs += uploadJob }
            if (!registered) return Result.failure(AttachmentException.Failed())
        }
        return try {
            val limit = uploadLimits().maxUploadSize
            val message = withContext(ioDispatcher) {
                val declared = reader.size(uri)
                if (declared != null && declared > limit) {
                    return@withContext Result.failure<Message>(AttachmentException.TooLarge())
                }
                val mime = reader.mimeType(uri)?.lowercase() ?: "application/octet-stream"
                if (mime !in UPLOAD_MIME_TYPES) {
                    return@withContext Result.failure<Message>(AttachmentException.UnsupportedType())
                }
                val name = reader.displayName(uri)?.takeIf { it.isNotBlank() } ?: "upload"
                // Fail before the request if the file cannot be opened at all.
                val probe = reader.open(uri)
                    ?: return@withContext Result.failure<Message>(AttachmentException.Failed())
                // Every stream opened here is closed on success, failure and cancellation.
                val opened = mutableListOf(probe)
                var probeUsed = false
                try {
                    // The session may have ended while the limits or the file were read.
                    if (epoch != startEpoch) {
                        return@withContext Result.failure<Message>(AttachmentException.Failed())
                    }
                    val sent = messageApi.uploadMessageWithFile(
                        channelId = channelId,
                        fileName = name,
                        mimeType = mime,
                        size = declared,
                        sessionId = sessionId,
                        open = {
                            val stream = if (!probeUsed) probe.also { probeUsed = true }
                            else (reader.open(uri) ?: throw java.io.IOException("cannot open attachment"))
                                .also { opened += it }
                            LimitedInputStream(stream, limit)
                        },
                        content = caption?.takeIf { it.isNotBlank() },
                    )
                    Result.success(sent)
                } finally {
                    opened.forEach { runCatching { it.close() } }
                }
            }
            message.onSuccess { sent ->
                // Not after a session clear: the old session's message stays off screen.
                publishIfCurrent(startEpoch) {
                    getOrCreateMessagesFlow(channelId).update { current ->
                        MessageOrdering.merge(current, listOf(sent))
                    }
                }
            }
        } catch (e: kotlin.coroutines.cancellation.CancellationException) {
            throw e
        } catch (e: OutOfMemoryError) {
            logger.warning("attachment upload ran out of memory")
            Result.failure(AttachmentException.Failed())
        } catch (e: Exception) {
            val status = (e as? ApiException)?.status
            val tooLarge = status == HttpStatusCode.PayloadTooLarge ||
                generateSequence<Throwable>(e) { it.cause }.any { it is StreamTooLargeException }
            if (tooLarge) publishIfCurrent(startEpoch) { cachedLimits = null }
            if (e is ApiException) logger.info("attachment upload rejected: status=${e.status.value}")
            else logger.log(Level.WARNING, "attachment upload failed (${e::class.simpleName})")
            Result.failure(
                when {
                    tooLarge -> AttachmentException.TooLarge()
                    status == HttpStatusCode.UnsupportedMediaType || (e as? ApiException)?.normalizedCode == "INVALID_MIME_TYPE" ->
                        AttachmentException.UnsupportedType()
                    else -> AttachmentException.Failed(e)
                }
            )
        } finally {
            if (uploadJob != null) uploadJobs -= uploadJob
        }
    }

    suspend fun editMessage(messageId: String, content: String) {
        messageApi.editMessage(messageId, content)
    }

    suspend fun deleteMessage(channelId: String, messageId: String) {
        messageApi.deleteMessage(messageId)

        // Optimistic removal
        val flow = getOrCreateMessagesFlow(channelId)
        flow.value = flow.value.filter { it.id != messageId }
    }

    suspend fun addReaction(channelId: String, messageId: String, emoji: String) {
        messageApi.addReaction(channelId, messageId, emoji)
        // The echo event is idempotent with this, so it works with or without one.
        authState.currentUserId.value?.let { me ->
            updateReactions(channelId, messageId) { it.withReactionAdded(emoji, me, isMe = true) }
        }
    }

    suspend fun removeReaction(channelId: String, messageId: String, emoji: String) {
        messageApi.removeReaction(channelId, messageId, emoji)
        authState.currentUserId.value?.let { me ->
            updateReactions(channelId, messageId) { it.withReactionRemoved(emoji, me, isMe = true) }
        }
    }

    /**
     * Subscribes a screen to [channelId]. Sends `subscribe` right away; if the
     * socket is not connected yet, the resend on the next `Ready`
     * ([resubscribeAll]) covers it.
     */
    fun subscribeToChannel(channelId: String) {
        // Ensure flows exist
        getOrCreateMessagesFlow(channelId)
        getOrCreateTypingUsersFlow(channelId)

        subscribedChannels.add(channelId)
        webSocket.send(ClientEvent.Subscribe(channelId))
    }

    /**
     * The screen for [channelId] closed. The server subscription is dropped
     * unless the channel is a background DM subscription.
     */
    fun unsubscribeFromChannel(channelId: String) {
        subscribedChannels.remove(channelId)
        if (channelId in dmChannels) return
        webSocket.send(ClientEvent.Unsubscribe(channelId))
    }

    /** Users typing in [channelId] (id to expiry time); entries disappear [TYPING_EXPIRY_MS] after the last event. */
    fun getTypingUsers(channelId: String): StateFlow<Map<String, Long>> {
        return getOrCreateTypingUsersFlow(channelId).asStateFlow()
    }

    fun sendTypingIndicator(channelId: String) {
        val now = System.currentTimeMillis()
        val lastSent = lastTypingSent[channelId] ?: 0L

        if (now - lastSent >= TYPING_DEBOUNCE_MS) {
            lastTypingSent[channelId] = now
            webSocket.send(ClientEvent.Typing(channelId))
        }
    }

    // -- Internal -------------------------------------------------------------

    private fun getOrCreateMessagesFlow(channelId: String): MutableStateFlow<List<Message>> {
        return channelMessages.getOrPut(channelId) { MutableStateFlow(emptyList()) }
    }

    private fun getOrCreateTypingUsersFlow(channelId: String): MutableStateFlow<Map<String, Long>> {
        return channelTypingUsers.getOrPut(channelId) { MutableStateFlow(emptyMap()) }
    }

    private fun startCollectingEvents() {
        eventCollectionJob?.cancel()
        // UNDISPATCHED: subscribe to the replay-0 SharedFlow before init returns.
        eventCollectionJob = scope.launch(start = CoroutineStart.UNDISPATCHED) {
            webSocket.events.collect { event ->
                handleServerEvent(event)
            }
        }
    }

    private fun handleServerEvent(event: ServerEvent) {
        when (event) {
            is ServerEvent.Ready -> handleReady(event)
            is ServerEvent.MessageNew -> handleMessageNew(event)
            is ServerEvent.MessageEdit -> handleMessageEdit(event)
            is ServerEvent.MessageDelete -> handleMessageDelete(event)
            is ServerEvent.ReactionAdd -> handleReactionAdd(event)
            is ServerEvent.ReactionRemove -> handleReactionRemove(event)
            is ServerEvent.TypingStart -> handleTypingStart(event)
            is ServerEvent.TypingStop -> handleTypingStop(event)
            is ServerEvent.Error -> {
                logger.warning("Server error: code=${event.code} message=${event.message}")
            }
            else -> { /* Ignored — other events are handled elsewhere */ }
        }
    }

    /**
     * Runs before any later event of the new socket (one collector, in order):
     * stores the watermark and snapshots each channel's newest id as the
     * catch-up anchor.
     */
    private fun handleReady(event: ServerEvent.Ready) {
        event.serverTime?.let { _lastServerTime.value = it }
        channelMessages.forEach { (channelId, flow) ->
            // Keep an anchor whose catch-up has not completed yet: re-anchoring
            // at a live message of the dropped socket would skip the gap.
            flow.value.lastOrNull()?.let { catchUpAnchors.putIfAbsent(channelId, it.id) }
        }
    }

    private fun handleMessageNew(event: ServerEvent.MessageNew) {
        try {
            val message = json.decodeFromJsonElement<Message>(event.message)
            val flow = getOrCreateMessagesFlow(event.channelId)
            // Avoid double-counting unread for duplicates (e.g., from optimistic send)
            val isNew = flow.value.none { it.id == message.id }

            flow.update { current -> MessageOrdering.merge(current, listOf(message)) }

            if (isNew) {
                // Raise the unread badge for non-active channels.
                guildRepository.onMessageReceived(
                    event.channelId,
                    message.id,
                    countUnread = message.author.id != authState.currentUserId.value,
                )
            }
        } catch (e: Exception) {
            logger.log(Level.WARNING, "Failed to deserialize MessageNew payload (${e::class.simpleName})")
        }
    }

    private fun handleMessageEdit(event: ServerEvent.MessageEdit) {
        val flow = channelMessages[event.channelId] ?: return
        flow.value = flow.value.map { msg ->
            if (msg.id == event.messageId) {
                msg.copy(content = event.content, editedAt = event.editedAt)
            } else {
                msg
            }
        }
    }

    private fun handleMessageDelete(event: ServerEvent.MessageDelete) {
        val flow = channelMessages[event.channelId] ?: return
        flow.value = flow.value.filter { it.id != event.messageId }
    }

    private fun handleReactionAdd(event: ServerEvent.ReactionAdd) {
        val isMe = event.userId == authState.currentUserId.value
        updateReactions(event.channelId, event.messageId) { it.withReactionAdded(event.emoji, event.userId, isMe) }
    }

    private fun handleReactionRemove(event: ServerEvent.ReactionRemove) {
        val isMe = event.userId == authState.currentUserId.value
        updateReactions(event.channelId, event.messageId) { it.withReactionRemoved(event.emoji, event.userId, isMe) }
    }

    private fun updateReactions(
        channelId: String,
        messageId: String,
        change: (List<Reaction>?) -> List<Reaction>,
    ) {
        val flow = channelMessages[channelId] ?: return
        flow.update { current ->
            current.map { msg -> if (msg.id == messageId) msg.copy(reactions = change(msg.reactions)) else msg }
        }
    }

    private fun handleTypingStart(event: ServerEvent.TypingStart) {
        val flow = getOrCreateTypingUsersFlow(event.channelId)
        val expiresAt = clock.nowMillis() + TYPING_EXPIRY_MS
        flow.update { it + (event.userId to expiresAt) }
        scope.launch {
            delay(TYPING_EXPIRY_MS)
            // A newer typing event moved the expiry; its own timer removes the entry.
            flow.update { current ->
                if (current[event.userId] == expiresAt) current - event.userId else current
            }
        }
    }

    private fun handleTypingStop(event: ServerEvent.TypingStop) {
        val flow = getOrCreateTypingUsersFlow(event.channelId)
        flow.update { it - event.userId }
    }
}
