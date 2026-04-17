package io.wolftown.kaiku.data.repository

import io.wolftown.kaiku.data.api.MessageApi
import io.wolftown.kaiku.data.ws.ClientEvent
import io.wolftown.kaiku.data.ws.KaikuWebSocket
import io.wolftown.kaiku.data.ws.ServerEvent
import io.wolftown.kaiku.di.ChatCoroutineScope
import io.wolftown.kaiku.domain.model.Message
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
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
 */
@Singleton
class ChatRepository @Inject constructor(
    private val messageApi: MessageApi,
    private val webSocket: KaikuWebSocket,
    private val json: Json,
    @ChatCoroutineScope private val scope: CoroutineScope,
) {
    companion object {
        private val logger = Logger.getLogger("ChatRepository")
        private const val TYPING_DEBOUNCE_MS = 3_000L
    }

    /** Per-channel message lists. */
    private val channelMessages = ConcurrentHashMap<String, MutableStateFlow<List<Message>>>()

    /** Per-channel typing user sets. */
    private val channelTypingUsers = ConcurrentHashMap<String, MutableStateFlow<Set<String>>>()

    /** Track last typing indicator send time per channel for debouncing. */
    private val lastTypingSent = ConcurrentHashMap<String, Long>()

    /** WebSocket event collection job. */
    private var eventCollectionJob: Job? = null

    init {
        startCollectingEvents()
    }

    // -- Public API ------------------------------------------------------------

    fun getMessages(channelId: String): StateFlow<List<Message>> {
        return getOrCreateMessagesFlow(channelId).asStateFlow()
    }

    suspend fun loadMessages(channelId: String, before: String? = null) {
        val messages = messageApi.getMessages(channelId, before)
        val flow = getOrCreateMessagesFlow(channelId)

        if (before != null) {
            // Pagination: prepend older messages
            val existing = flow.value
            val existingIds = existing.map { it.id }.toSet()
            val newMessages = messages.filter { it.id !in existingIds }
            flow.value = newMessages + existing
        } else {
            flow.value = messages
        }
    }

    suspend fun sendMessage(channelId: String, content: String) {
        val sentMessage = messageApi.sendMessage(channelId, content)

        // Optimistic update: add the message immediately
        val flow = getOrCreateMessagesFlow(channelId)
        val current = flow.value
        if (current.none { it.id == sentMessage.id }) {
            flow.value = current + sentMessage
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

    fun subscribeToChannel(channelId: String) {
        // Ensure flows exist
        getOrCreateMessagesFlow(channelId)
        getOrCreateTypingUsersFlow(channelId)

        webSocket.send(ClientEvent.Subscribe(channelId))
    }

    fun unsubscribeFromChannel(channelId: String) {
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
        eventCollectionJob = scope.launch {
            webSocket.events.collect { event ->
                handleServerEvent(event)
            }
        }
    }

    private fun handleServerEvent(event: ServerEvent) {
        when (event) {
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

    private fun handleMessageNew(event: ServerEvent.MessageNew) {
        try {
            val message = json.decodeFromJsonElement<Message>(event.message)
            val flow = getOrCreateMessagesFlow(event.channelId)
            val current = flow.value

            // Avoid duplicates (e.g., from optimistic send)
            if (current.none { it.id == message.id }) {
                flow.value = current + message
            }
        } catch (e: Exception) {
            logger.log(Level.WARNING, "Failed to deserialize MessageNew payload", e)
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
