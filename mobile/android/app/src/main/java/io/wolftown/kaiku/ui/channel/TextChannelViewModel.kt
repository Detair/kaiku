package io.wolftown.kaiku.ui.channel

import androidx.lifecycle.SavedStateHandle
import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import dagger.hilt.android.lifecycle.HiltViewModel
import io.wolftown.kaiku.R
import io.wolftown.kaiku.data.local.AuthState
import io.wolftown.kaiku.data.repository.ChatRepository
import io.wolftown.kaiku.data.repository.DmRepository
import io.wolftown.kaiku.data.repository.GuildRepository
import io.wolftown.kaiku.data.ws.RealtimeConnectionManager
import io.wolftown.kaiku.data.ws.RealtimeStatus
import io.wolftown.kaiku.domain.ChannelTitle
import io.wolftown.kaiku.domain.model.Message
import io.wolftown.kaiku.domain.resolveChannelTitle
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharingStarted
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.combine
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
    authState: AuthState,
    realtimeConnectionManager: RealtimeConnectionManager,
    savedStateHandle: SavedStateHandle
) : ViewModel() {

    companion object {
        private val logger = Logger.getLogger("TextChannelViewModel")
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
    val typingUsers: StateFlow<Set<String>> = chatRepository.getTypingUsers(channelId)

    private val _isLoading = MutableStateFlow(false)
    val isLoading: StateFlow<Boolean> = _isLoading.asStateFlow()

    /** True while the server reports older history (`has_more`) for this channel. */
    private val _canLoadMore = MutableStateFlow(false)
    val canLoadMore: StateFlow<Boolean> = _canLoadMore.asStateFlow()

    private val _messageInput = MutableStateFlow("")
    val messageInput: StateFlow<String> = _messageInput.asStateFlow()

    /** `@StringRes` of the latest failed action, or null. Cleared by [clearError]. */
    private val _error = MutableStateFlow<Int?>(null)
    val error: StateFlow<Int?> = _error.asStateFlow()

    /** The message being edited; non-null while the edit dialog is shown. */
    private val _editingMessage = MutableStateFlow<Message?>(null)
    val editingMessage: StateFlow<Message?> = _editingMessage.asStateFlow()

    /** Realtime connection state, for the reconnect banner. */
    val connectionStatus: StateFlow<RealtimeStatus> = realtimeConnectionManager.status

    init {
        chatRepository.subscribeToChannel(channelId)
        loadInitialMessages()
    }

    fun onMessageInputChanged(text: String) {
        _messageInput.value = text
        if (text.isNotEmpty()) {
            chatRepository.sendTypingIndicator(channelId)
        }
    }

    fun onSendMessage() {
        val content = _messageInput.value.trim()
        if (content.isEmpty()) return

        _messageInput.value = ""
        viewModelScope.launch {
            try {
                chatRepository.sendMessage(channelId, content)
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                // Restore input on failure so the user can retry, unless a newer draft was typed
                if (_messageInput.value.isEmpty()) _messageInput.value = content
                _error.value = R.string.chat_send_failed
                logger.log(Level.WARNING, "Failed to send message", e)
            }
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

    fun onLoadMore() {
        if (!_canLoadMore.value) return
        val oldestMessage = messages.value.firstOrNull() ?: return
        viewModelScope.launch {
            try {
                _canLoadMore.value = chatRepository.loadMessages(channelId, before = oldestMessage.id)
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                _error.value = R.string.chat_load_failed
                logger.log(Level.WARNING, "Failed to load more messages", e)
            }
        }
    }

    fun clearError() {
        _error.value = null
    }

    public override fun onCleared() {
        chatRepository.unsubscribeFromChannel(channelId)
        super.onCleared()
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
