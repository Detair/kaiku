package io.wolftown.kaiku.ui.channel

import android.net.Uri
import androidx.lifecycle.SavedStateHandle
import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import dagger.hilt.android.lifecycle.HiltViewModel
import io.wolftown.kaiku.R
import io.wolftown.kaiku.data.local.AuthState
import io.wolftown.kaiku.data.repository.ChatRepository
import io.wolftown.kaiku.data.repository.DmRepository
import io.wolftown.kaiku.data.repository.GuildRepository
import io.wolftown.kaiku.data.repository.OutgoingMessages
import io.wolftown.kaiku.data.repository.PendingMessage
import io.wolftown.kaiku.data.ws.RealtimeConnectionManager
import io.wolftown.kaiku.data.ws.RealtimeStatus
import io.wolftown.kaiku.domain.ChannelTitle
import io.wolftown.kaiku.domain.model.Message
import io.wolftown.kaiku.domain.resolveChannelTitle
import io.wolftown.kaiku.push.ActiveChannelTracker
import io.wolftown.kaiku.push.NotificationLedger
import io.wolftown.kaiku.push.NotificationSink
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharingStarted
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.combine
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.flow.stateIn
import kotlinx.coroutines.launch
import java.util.logging.Level
import java.util.logging.Logger
import kotlin.coroutines.cancellation.CancellationException
import javax.inject.Inject

@HiltViewModel
class TextChannelViewModel @Inject constructor(
    private val chatRepository: ChatRepository,
    private val guildRepository: GuildRepository,
    private val dmRepository: DmRepository,
    private val authState: AuthState,
    realtimeConnectionManager: RealtimeConnectionManager,
    private val activeChannelTracker: ActiveChannelTracker,
    private val notificationSink: NotificationSink,
    private val notificationLedger: NotificationLedger,
    private val outgoing: OutgoingMessages,
    private val savedStateHandle: SavedStateHandle
) : ViewModel() {

    companion object {
        private val logger = Logger.getLogger("TextChannelViewModel")
        private const val KEY_DRAFT = "draft"

        /**
         * Longer drafts are kept only in memory, not in the saved state: the
         * saved state goes through a ~1 MB binder transaction (AND-B-19 / K6).
         */
        const val MAX_SAVED_DRAFT_CHARS = 16_000
    }

    private val channelId: String = savedStateHandle["channelId"]!!

    /** Guild channel name or DM participant name; null while unknown (never the raw id). */
    val title: StateFlow<ChannelTitle?> = combine(
        guildRepository.channels,
        dmRepository.conversations,
        authState.currentUserId,
    ) { channels, dms, userId ->
        resolveChannelTitle(channelId, channels, dms, userId)
    }.stateIn(viewModelScope, SharingStarted.WhileSubscribed(5000), null)

    val messages: StateFlow<List<Message>> = chatRepository.getMessages(channelId)

    /** Ids of the other users typing right now (entries expire in the repository). */
    val typingUserIds: StateFlow<Set<String>> = combine(
        chatRepository.getTypingUsers(channelId),
        authState.currentUserId,
    ) { typing, me ->
        typing.keys.filterTo(LinkedHashSet()) { it != me }
    }.stateIn(viewModelScope, SharingStarted.WhileSubscribed(5000), emptySet())

    /** User id to display name for everyone who wrote in the loaded messages. */
    val memberNames: StateFlow<Map<String, String>> = messages
        .map { list -> list.associate { it.author.id to it.author.displayName } }
        .stateIn(viewModelScope, SharingStarted.WhileSubscribed(5000), emptyMap())

    private val _isLoading = MutableStateFlow(false)
    val isLoading: StateFlow<Boolean> = _isLoading.asStateFlow()

    /** True while the server reports older history (`has_more`) for this channel. */
    private val _canLoadMore = MutableStateFlow(false)
    val canLoadMore: StateFlow<Boolean> = _canLoadMore.asStateFlow()

    // AND-B-19: the draft survives leaving the chat (process memory) and process death (saved state).
    private val _messageInput = MutableStateFlow(
        savedStateHandle.get<String>(KEY_DRAFT)?.takeIf { it.isNotEmpty() } ?: outgoing.draft(channelId)
    )
    val messageInput: StateFlow<String> = _messageInput.asStateFlow()

    /**
     * AND-B-3: messages sent from this chat that the server has not confirmed yet,
     * and failed ones (they stay until retried or discarded, also after leaving the chat).
     */
    val pendingMessages: StateFlow<List<PendingMessage>> = outgoing.pending(channelId)
        .stateIn(viewModelScope, SharingStarted.Eagerly, outgoing.pendingNow(channelId))

    /** `@StringRes` of the latest failed action, or null. Cleared by [clearError]. */
    private val _error = MutableStateFlow<Int?>(null)
    val error: StateFlow<Int?> = _error.asStateFlow()

    private val _failureNotice = MutableStateFlow<String?>(null)

    /**
     * Local id of the failed message the "not sent" snackbar is about, or null
     * once that message is no longer failed (deleted, retried or sent). The
     * screen dismisses the snackbar when this stops naming its message.
     */
    val failureNotice: StateFlow<String?> = _failureNotice.asStateFlow()

    /** True while a picked photo or video of this chat is being uploaded. */
    val isUploading: StateFlow<Boolean> = pendingMessages
        .map { list -> list.any { it.attachment != null && it.state == PendingMessage.State.SENDING } }
        .stateIn(viewModelScope, SharingStarted.Eagerly, false)

    /** The message being edited; non-null while the edit dialog is shown. */
    private val _editingMessage = MutableStateFlow<Message?>(null)
    val editingMessage: StateFlow<Message?> = _editingMessage.asStateFlow()

    /** Realtime connection state, for the reconnect banner. */
    val connectionStatus: StateFlow<RealtimeStatus> = realtimeConnectionManager.status

    /**
     * C28: the read-only Kaiku DM, recognised only by `is_system` + `read_only`
     * (never an id or a name). The screen hides the composer and call buttons.
     */
    val isKaikuDm: StateFlow<Boolean> = dmRepository.conversations
        .map { dms -> dms.any { it.id == channelId && it.isSystem && it.readOnly } }
        .stateIn(viewModelScope, SharingStarted.Eagerly, false)

    /** True while the screen is resumed (between [onVisible] and [onHidden]). */
    private var visible = false

    init {
        chatRepository.subscribeToChannel(channelId)
        loadInitialMessages()
        // While visible, every message the user can see counts as seen for notifications.
        viewModelScope.launch {
            messages.collect { list -> if (visible) markNewestSeen(list) }
        }
        // A send that fails while this chat is open is also reported in a snackbar.
        viewModelScope.launch {
            outgoing.failures.collect { failed ->
                if (failed.channelId == channelId) {
                    // Set before the error, so the screen sees the id when it shows the snackbar.
                    _failureNotice.value = failed.localId
                    _error.value = pendingFailureRes(failed.failure)
                }
            }
        }
        // The snapshot, not the emitted list: the failure may be newer than the list.
        viewModelScope.launch {
            outgoing.pending(channelId).collect {
                val id = _failureNotice.value ?: return@collect
                val stillFailed = outgoing.pendingNow(channelId)
                    .any { it.localId == id && it.state == PendingMessage.State.FAILED }
                if (!stillFailed) _failureNotice.compareAndSet(id, null)
            }
        }
        // C28: coalescing edits older notices, so the Kaiku DM reloads its latest page on every reconnect.
        viewModelScope.launch {
            var wasConnected = connectionStatus.value == RealtimeStatus.Connected
            connectionStatus.collect { status ->
                val connected = status == RealtimeStatus.Connected
                if (connected && !wasConnected && isKaikuDm.value) reloadLatest()
                wasConnected = connected
            }
        }
    }

    /** The screen is resumed: no notifications for this channel, and clear the shown one. */
    fun onVisible() {
        visible = true
        activeChannelTracker.onVisible(channelId)
        notificationSink.cancel(channelId)
        markNewestSeen(messages.value)
    }

    /** The screen is paused or left. */
    fun onHidden() {
        visible = false
        activeChannelTracker.onHidden(channelId)
    }

    private fun markNewestSeen(list: List<Message>) {
        val userId = authState.currentUserId.value ?: return
        val newest = list.lastOrNull() ?: return
        notificationLedger.markSeen(userId, channelId, newest.id, newest.createdAt.ifEmpty { null })
    }

    fun onMessageInputChanged(text: String) {
        setInput(text)
        if (text.isNotEmpty()) {
            chatRepository.sendTypingIndicator(channelId)
        }
    }

    private fun setInput(text: String) {
        _messageInput.value = text
        if (text.length <= MAX_SAVED_DRAFT_CHARS) savedStateHandle[KEY_DRAFT] = text
        else savedStateHandle.remove<String>(KEY_DRAFT)
        outgoing.setDraft(channelId, text)
    }

    /**
     * Hands the text to [OutgoingMessages], which sends it in the app's scope:
     * leaving the chat right away does not cancel it (AND-B-3). The text shows
     * as "Sending…" until the server confirms it; a failure keeps it in the chat
     * as "Not sent" with Retry and Delete.
     */
    fun onSendMessage() {
        val content = _messageInput.value.trim()
        if (content.isEmpty()) return
        setInput("")
        outgoing.send(channelId, content)
    }

    /**
     * Uploads the picked photo/video in the app's scope; the current draft
     * becomes its caption. Picks while an upload of this chat runs are ignored.
     */
    fun onAttachmentPicked(uri: Uri) {
        // The snapshot, not [isUploading]: that flow lags behind a double tap.
        val uploading = outgoing.pendingNow(channelId)
            .any { it.attachment != null && it.state == PendingMessage.State.SENDING }
        if (uploading) return
        val caption = _messageInput.value.trim()
        setInput("")
        outgoing.sendAttachment(channelId, uri, caption.ifEmpty { null })
    }

    /** "Retry" on a message that was not sent. */
    fun onRetryPending(localId: String) = outgoing.retry(localId)

    /** "Retry" in the snackbar: every message of this chat that was not sent. */
    fun onRetryFailed() = outgoing.retryAll(channelId)

    /**
     * "Delete" on a message that was not sent. The caption of a photo or video
     * that could not go out (too large, refused type) comes back into an empty
     * field, so it can still be sent as text.
     */
    fun onDiscardPending(localId: String) {
        val dropped = outgoing.discard(localId) ?: return
        if (dropped.attachment != null && dropped.text.isNotBlank() && _messageInput.value.isEmpty()) {
            setInput(dropped.text)
        }
    }

    fun onEditMessage(messageId: String, content: String) {
        viewModelScope.launch {
            try {
                chatRepository.editMessage(messageId, content)
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                _error.value = R.string.chat_edit_failed
                logger.log(Level.WARNING, "Failed to edit message $messageId", e)
            }
        }
    }

    fun onStartEdit(messageId: String) {
        _editingMessage.value = messages.value.firstOrNull { it.id == messageId }
    }

    fun onConfirmEdit(content: String) {
        val message = _editingMessage.value ?: return
        val trimmed = content.trim()
        if (trimmed.isEmpty()) return
        _editingMessage.value = null
        if (trimmed == message.content) return
        onEditMessage(message.id, trimmed)
    }

    fun onCancelEdit() {
        _editingMessage.value = null
    }

    fun onDeleteMessage(messageId: String) {
        viewModelScope.launch {
            try {
                chatRepository.deleteMessage(channelId, messageId)
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                _error.value = R.string.chat_delete_failed
                logger.log(Level.WARNING, "Failed to delete message $messageId", e)
            }
        }
    }

    fun onAddReaction(messageId: String, emoji: String) {
        viewModelScope.launch {
            try {
                chatRepository.addReaction(channelId, messageId, emoji)
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                _error.value = R.string.chat_reaction_failed
                logger.log(Level.WARNING, "Failed to add reaction on $messageId", e)
            }
        }
    }

    fun onRemoveReaction(messageId: String, emoji: String) {
        viewModelScope.launch {
            try {
                chatRepository.removeReaction(channelId, messageId, emoji)
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                _error.value = R.string.chat_reaction_failed
                logger.log(Level.WARNING, "Failed to remove reaction on $messageId", e)
            }
        }
    }

    private val _isLoadingMore = MutableStateFlow(false)

    /** True while an older page is loading; reaching the top again does not start another. */
    val isLoadingMore: StateFlow<Boolean> = _isLoadingMore.asStateFlow()

    /** Loads the page before the oldest message; returns whether a load started. */
    fun onLoadMore(): Boolean {
        if (!_canLoadMore.value || _isLoadingMore.value) return false
        val oldestMessage = messages.value.firstOrNull() ?: return false
        _isLoadingMore.value = true
        viewModelScope.launch {
            try {
                _canLoadMore.value = chatRepository.loadMessages(channelId, before = oldestMessage.id)
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                _error.value = R.string.chat_load_failed
                logger.log(Level.WARNING, "Failed to load more messages", e)
            } finally {
                _isLoadingMore.value = false
            }
        }
        return true
    }

    fun clearError() {
        _error.value = null
    }

    public override fun onCleared() {
        activeChannelTracker.onHidden(channelId)
        chatRepository.unsubscribeFromChannel(channelId)
        guildRepository.clearActiveChannel(channelId)
        dmRepository.clearActiveChannel(channelId)
        super.onCleared()
    }

    private fun reloadLatest() {
        viewModelScope.launch {
            try {
                _canLoadMore.value = chatRepository.loadMessages(channelId)
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                logger.log(Level.WARNING, "Failed to reload the Kaiku DM (${e::class.simpleName})")
            }
        }
    }

    private fun loadInitialMessages() {
        _isLoading.value = true
        viewModelScope.launch {
            try {
                _canLoadMore.value = chatRepository.loadMessages(channelId)
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                _error.value = R.string.chat_load_failed
                logger.log(Level.WARNING, "Failed to load initial messages", e)
            } finally {
                _isLoading.value = false
            }
        }
    }
}
