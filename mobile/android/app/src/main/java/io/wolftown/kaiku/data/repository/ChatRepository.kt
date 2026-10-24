package io.wolftown.kaiku.data.repository

import io.ktor.http.HttpStatusCode
import io.wolftown.kaiku.data.api.ApiException
import io.wolftown.kaiku.data.api.MessageApi
import io.wolftown.kaiku.data.ws.ClientEvent
import io.wolftown.kaiku.data.ws.KaikuWebSocket
import io.wolftown.kaiku.data.ws.ServerEvent
import io.wolftown.kaiku.di.ChatCoroutineScope
import io.wolftown.kaiku.domain.MessageOrdering
import io.wolftown.kaiku.domain.model.Message
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.CoroutineStart
import kotlinx.coroutines.Job
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
 * - Reactions are received but not yet reflected in the UI
 * - Sends client events (subscribe, unsubscribe, typing) via WebSocket
 * - Typing indicator debouncing (max once per 3 seconds)
 * - Remembers which channels need a server-side subscription, so they can be
 *   re-subscribed after a reconnect ([resubscribeAll]), and fetches messages
 *   missed while offline ([catchUp])
 */
@Singleton
class ChatRepository @Inject constructor(
    private val messageApi: MessageApi,
    private val webSocket: KaikuWebSocket,
    private val json: Json,
    private val guildRepository: GuildRepository,
    @ChatCoroutineScope private val scope: CoroutineScope,
) {
    companion object {
        private val logger = Logger.getLogger("ChatRepository")
        private const val TYPING_DEBOUNCE_MS = 3_000L

        /** Page size for [catchUp] (`limit`). */
        const val CATCH_UP_PAGE_SIZE = 100

        /** [catchUp] fetches at most this many pages, then reloads the latest page instead. */
        const val CATCH_UP_MAX_PAGES = 5
    }

    /** Per-channel message lists. */
    private val channelMessages = ConcurrentHashMap<String, MutableStateFlow<List<Message>>>()

    /** Per-channel typing user sets. */
    private val channelTypingUsers = ConcurrentHashMap<String, MutableStateFlow<Set<String>>>()

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

    init {
        startCollectingEvents()
    }

    // -- Public API ------------------------------------------------------------

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
        val flow = getOrCreateMessagesFlow(channelId)
        val idsBeforeRequest = flow.value.mapTo(HashSet()) { it.id }
        val page = messageApi.getMessages(channelId, before = before)

        if (before != null) {
            flow.update { existing -> MessageOrdering.merge(existing, page.items) }
        } else {
            val pageIds = page.items.mapTo(HashSet()) { it.id }
            flow.update { existing ->
                // Arrived during the request; compared by id, not by time, so
                // a live message with the same timestamp as the page's newest
                // item is not lost.
                val live = existing.filter { it.id !in idsBeforeRequest && it.id !in pageIds }
                MessageOrdering.merge(page.items, live)
            }
            catchUpAnchors.remove(channelId)
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
        val flow = channelMessages[channelId] ?: return
        // The anchor is only removed once the catch-up completed: if it fails
        // or the socket drops first, the next reconnect starts from it again.
        val anchor = catchUpAnchors[channelId]
        val start = anchor ?: flow.value.lastOrNull()?.id ?: return
        catchUpFrom(channelId, flow, start)
        if (anchor != null) catchUpAnchors.remove(channelId, anchor)
    }

    private suspend fun catchUpFrom(channelId: String, flow: MutableStateFlow<List<Message>>, start: String) {
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
                    return
                }
                throw e
            }
            val previous = flow.getAndUpdate { current -> MessageOrdering.merge(current, page.items) }
            val known = previous.mapTo(HashSet()) { it.id }
            val added = page.items.mapTo(HashSet()) { it.id }.count { it !in known }
            repeat(added) { guildRepository.onMessageReceived(channelId) }

            // Follow the server's cursor, not the newest returned item: the
            // server computes `next_cursor` before filtering out messages of
            // blocked users, so a page can be empty while more messages follow.
            val next = page.nextCursor
            if (!page.hasMore || next == null) return
            after = next
        }

        logger.info("catch-up for a channel exceeded $CATCH_UP_MAX_PAGES pages, reloading latest page")
        loadMessages(channelId)
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
        val sentMessage = messageApi.sendMessage(channelId, content)

        // Optimistic update: add the message immediately. A WebSocket echo
        // with the same id is deduplicated by MessageOrdering.merge.
        getOrCreateMessagesFlow(channelId).update { current ->
            MessageOrdering.merge(current, listOf(sentMessage))
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
    }

    suspend fun removeReaction(channelId: String, messageId: String, emoji: String) {
        messageApi.removeReaction(channelId, messageId, emoji)
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

    fun getTypingUsers(channelId: String): StateFlow<Set<String>> {
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

    private fun getOrCreateTypingUsersFlow(channelId: String): MutableStateFlow<Set<String>> {
        return channelTypingUsers.getOrPut(channelId) { MutableStateFlow(emptySet()) }
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
                guildRepository.onMessageReceived(event.channelId)
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
        // Reaction handling is a no-op for now since the Message model
        // does not include a reactions field. This will be extended when
        // a Reaction data class is added to the domain model.
        logger.fine("ReactionAdd: ${event.emoji} on ${event.messageId} by ${event.userId}")
    }

    private fun handleReactionRemove(event: ServerEvent.ReactionRemove) {
        logger.fine("ReactionRemove: ${event.emoji} on ${event.messageId} by ${event.userId}")
    }

    private fun handleTypingStart(event: ServerEvent.TypingStart) {
        val flow = getOrCreateTypingUsersFlow(event.channelId)
        flow.value = flow.value + event.userId
    }

    private fun handleTypingStop(event: ServerEvent.TypingStop) {
        val flow = getOrCreateTypingUsersFlow(event.channelId)
        flow.value = flow.value - event.userId
    }
}
