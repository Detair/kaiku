package io.wolftown.kaiku.ui.channel

import androidx.lifecycle.SavedStateHandle
import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import dagger.hilt.android.lifecycle.HiltViewModel
import io.wolftown.kaiku.data.repository.ChatRepository
import io.wolftown.kaiku.data.repository.GuildRepository
import io.wolftown.kaiku.domain.model.Message
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharingStarted
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
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
    savedStateHandle: SavedStateHandle
) : ViewModel() {

    companion object {
        private val logger = Logger.getLogger("TextChannelViewModel")
    }

    private val channelId: String = savedStateHandle["channelId"]!!

    val channelName: StateFlow<String> = guildRepository.channels
        .map { channels -> channels.find { it.id == channelId }?.name ?: channelId }
        .stateIn(viewModelScope, SharingStarted.WhileSubscribed(5000), channelId)

    val messages: StateFlow<List<Message>> = chatRepository.getMessages(channelId)
    val typingUsers: StateFlow<Set<String>> = chatRepository.getTypingUsers(channelId)

    private val _isLoading = MutableStateFlow(false)
    val isLoading: StateFlow<Boolean> = _isLoading.asStateFlow()

    private val _messageInput = MutableStateFlow("")
    val messageInput: StateFlow<String> = _messageInput.asStateFlow()

    private val _error = MutableStateFlow<String?>(null)
    val error: StateFlow<String?> = _error.asStateFlow()

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
                // Restore input on failure so the user can retry
                _messageInput.value = content
                _error.value = "Failed to send message"
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
                _error.value = "Failed to edit message"
                logger.log(Level.WARNING, "Failed to edit message $messageId", e)
            }
        }
    }

    fun onDeleteMessage(messageId: String) {
        viewModelScope.launch {
            try {
                chatRepository.deleteMessage(channelId, messageId)
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                _error.value = "Failed to delete message"
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
                _error.value = "Failed to add reaction"
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
                _error.value = "Failed to remove reaction"
                logger.log(Level.WARNING, "Failed to remove reaction on $messageId", e)
            }
        }
    }

    fun onLoadMore() {
        val oldestMessage = messages.value.firstOrNull() ?: return
        viewModelScope.launch {
            try {
                chatRepository.loadMessages(channelId, before = oldestMessage.id)
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                _error.value = "Failed to load more messages"
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
                chatRepository.loadMessages(channelId)
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                _error.value = "Failed to load messages"
                logger.log(Level.WARNING, "Failed to load initial messages", e)
            } finally {
                _isLoading.value = false
            }
        }
    }
}
