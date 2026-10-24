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
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.decodeFromJsonElement
import java.util.concurrent.ConcurrentHashMap
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

    /**
     * Newest message id per channel as it was when the last `Ready` arrived,
     * i.e. before any event of the new socket. [catchUp] pages from here, so a
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
     * except for messages newer than the page's newest item: those arrived
     * live while the request was in flight and are kept. For an empty page,
     * messages that arrived during the request are kept.
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
            val latest = MessageOrdering.merge(emptyList(), page.items)
            val newest = latest.lastOrNull()
            flow.update { existing ->
                val live = existing.filter { message ->
                    if (newest != null) {
                        MessageOrdering.isNewer(message, newest)
                    } else {
                        message.id !in idsBeforeRequest
                    }
                }
                MessageOrdering.merge(latest, live)
            }
        }
        return page.hasMore
    }

    /**
     * Fetches the messages of [channelId] that are newer than the newest one
     * in the local list and merges them in (deduplicated by id). This is the
     * reconnect catch-up and the Phase 2 wake-fetch seam.
     *
     * Pages forwards with `after = <newest id>` and `limit = `[CATCH_UP_PAGE_SIZE]
     * while the server reports `has_more`, up to [CATCH_UP_MAX_PAGES] pages. If
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
        val anchor = catchUpAnchors.remove(channelId)
        var after = anchor ?: flow.value.lastOrNull()?.id ?: return

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
            val known = flow.value.mapTo(HashSet()) { it.id }
            flow.update { current -> MessageOrdering.merge(current, page.items) }
            val added = page.items.mapTo(HashSet()) { it.id }.count { it !in known }
            repeat(added) { guildRepository.onMessageReceived(channelId) }

            // Next cursor: the newest message of this page, not of the local
            // list, which may already hold newer live messages.
            val newest = MessageOrdering.merge(emptyList(), page.items).lastOrNull()
            if (!page.hasMore || newest == null) return
            after = newest.id
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
     * no screen has open. Returns the ids a `subscribe` was sent for.
     */
    fun setDmChannels(channelIds: Collection<String>): Set<String> {
        val next = channelIds.toSet()
        val added = next - dmChannels
        val removed = dmChannels - next
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
        dmChannels.add(channelId)
    }

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
        catchUpAnchors.clear()
        channelMessages.forEach { (channelId, flow) ->
            flow.value.lastOrNull()?.let { catchUpAnchors[channelId] = it.id }
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
