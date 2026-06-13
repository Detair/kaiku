package io.wolftown.kaiku.ui.dm

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import dagger.hilt.android.lifecycle.HiltViewModel
import io.wolftown.kaiku.data.local.AuthState
import io.wolftown.kaiku.data.repository.DmRepository
import io.wolftown.kaiku.domain.model.DmConversation
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
class DmListViewModel @Inject constructor(
    private val dmRepository: DmRepository,
    authState: AuthState,
) : ViewModel() {

    companion object {
        private val logger = Logger.getLogger("DmListViewModel")
    }

    val conversations: StateFlow<List<DmConversation>> = dmRepository.conversations

    val currentUserId: StateFlow<String> = authState.currentUserId
        .map { it ?: "" }
        .stateIn(viewModelScope, SharingStarted.Eagerly, "")

    private val _isLoading = MutableStateFlow(false)
    val isLoading: StateFlow<Boolean> = _isLoading.asStateFlow()

    private val _error = MutableStateFlow<String?>(null)
    val error: StateFlow<String?> = _error.asStateFlow()

    init {
        refresh()
    }

    fun refresh() {
        _isLoading.value = true
        _error.value = null
        viewModelScope.launch {
            try {
                dmRepository.loadDms()
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                _error.value = "Failed to load direct messages"
                logger.log(Level.WARNING, "Failed to load DMs", e)
            } finally {
                _isLoading.value = false
            }
        }
    }

    /** Opening a DM clears its unread badge (best-effort; failures are logged). */
    fun onOpenConversation(channelId: String) {
        viewModelScope.launch {
            try {
                dmRepository.markAsRead(channelId)
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                logger.log(Level.WARNING, "Failed to mark DM $channelId as read", e)
            }
        }
    }
}
