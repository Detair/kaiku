package io.wolftown.kaiku.ui.home

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import dagger.hilt.android.lifecycle.HiltViewModel
import io.wolftown.kaiku.data.local.TokenStorage
import io.wolftown.kaiku.data.repository.GuildRepository
import io.wolftown.kaiku.data.ws.KaikuWebSocket
import io.wolftown.kaiku.domain.model.Channel
import io.wolftown.kaiku.domain.model.ChannelType
import io.wolftown.kaiku.domain.model.Guild
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.flow.*
import kotlinx.coroutines.launch
import java.util.logging.Logger
import javax.inject.Inject

@HiltViewModel
class HomeViewModel @Inject constructor(
    private val guildRepository: GuildRepository,
    private val webSocket: KaikuWebSocket,
    private val tokenStorage: TokenStorage
) : ViewModel() {

    companion object {
        private val logger = Logger.getLogger("HomeViewModel")
    }

    val guilds: StateFlow<List<Guild>> = guildRepository.guilds

    val selectedGuild: StateFlow<Guild?> = combine(
        guildRepository.guilds,
        guildRepository.selectedGuildId
    ) { guilds, selectedId ->
        guilds.find { it.id == selectedId }
    }.stateIn(viewModelScope, SharingStarted.Eagerly, null)

    /** Channels for the selected guild, sorted by position. */
    val channels: StateFlow<List<Channel>> = guildRepository.channels
        .map { list -> list.sortedBy { it.position } }
        .stateIn(viewModelScope, SharingStarted.Eagerly, emptyList())

    private val _isLoading = MutableStateFlow(false)
    val isLoading: StateFlow<Boolean> = _isLoading.asStateFlow()

    private val _error = MutableStateFlow<String?>(null)
    val error: StateFlow<String?> = _error.asStateFlow()

    /**
     * One-shot navigation event for channel selection.
     * Bounded capacity (4) prevents buildup if a navigation collector is suspended.
     */
    private val _navigateToChannel = Channel<ChannelNavEvent>(capacity = 4)
    val navigateToChannel = _navigateToChannel.receiveAsFlow()

    init {
        connectWebSocket()
        loadGuilds()
    }

    fun onGuildSelected(guildId: String) {
        guildRepository.selectGuild(guildId)
        viewModelScope.launch {
            try {
                guildRepository.loadChannels(guildId)
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                _error.value = e.message ?: "Failed to load channels"
            }
        }
    }

    fun onChannelSelected(channelId: String, channelType: ChannelType) {
        _navigateToChannel.trySend(ChannelNavEvent(channelId, channelType))
    }

    fun refresh() {
        loadGuilds()
    }

    private fun connectWebSocket() {
        val serverUrl = tokenStorage.getServerUrl() ?: run {
            logger.warning("Server URL not configured, cannot connect WebSocket")
            return
        }
        webSocket.connect(serverUrl)
    }

    private fun loadGuilds() {
        _isLoading.value = true
        _error.value = null
        viewModelScope.launch {
            try {
                guildRepository.loadGuilds()
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                _error.value = e.message ?: "Failed to load guilds"
            } finally {
                _isLoading.value = false
            }
        }
    }
}

data class ChannelNavEvent(
    val channelId: String,
    val channelType: ChannelType
)
