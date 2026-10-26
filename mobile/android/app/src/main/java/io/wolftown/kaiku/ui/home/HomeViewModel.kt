package io.wolftown.kaiku.ui.home

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import dagger.hilt.android.lifecycle.HiltViewModel
import io.wolftown.kaiku.R
import io.wolftown.kaiku.data.repository.GuildRepository
import io.wolftown.kaiku.data.ws.RealtimeConnectionManager
import io.wolftown.kaiku.data.ws.RealtimeStatus
import io.wolftown.kaiku.domain.ChannelSection
import io.wolftown.kaiku.domain.groupChannels
import io.wolftown.kaiku.domain.model.Channel
import io.wolftown.kaiku.domain.model.ChannelType
import io.wolftown.kaiku.domain.model.Guild
import io.wolftown.kaiku.push.PushRegistrar
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.channels.Channel as CoroutineChannel
import kotlinx.coroutines.flow.*
import kotlinx.coroutines.launch
import java.util.logging.Logger
import javax.inject.Inject

@HiltViewModel
class HomeViewModel @Inject constructor(
    private val guildRepository: GuildRepository,
    realtimeConnectionManager: RealtimeConnectionManager,
    private val pushRegistrar: PushRegistrar
) : ViewModel() {

    companion object {
        private val logger = Logger.getLogger("HomeViewModel")
    }

    val guilds: StateFlow<List<Guild>> = guildRepository.guilds

    /** Realtime connection state, for the reconnect banner. */
    val connectionStatus: StateFlow<RealtimeStatus> = realtimeConnectionManager.status

    val selectedGuild: StateFlow<Guild?> = combine(
        guildRepository.guilds,
        guildRepository.selectedGuildId
    ) { guilds, selectedId ->
        guilds.find { it.id == selectedId }
    }.stateIn(viewModelScope, SharingStarted.Eagerly, null)

    /**
     * Channels for the selected server, sorted by position. Channel types this
     * client does not know (forum, announcement, ...) and DMs are dropped here,
     * so every consumer sees only channels it can open.
     */
    val channels: StateFlow<List<Channel>> = guildRepository.channels
        .map { list ->
            list
                .filter { it.channelType == ChannelType.TEXT || it.channelType == ChannelType.VOICE }
                .sortedBy { it.position }
        }
        .stateIn(viewModelScope, SharingStarted.Eagerly, emptyList())

    /** [channels] grouped under their category headers. */
    val sections: StateFlow<List<ChannelSection>> = combine(channels, guildRepository.categories) { list, categories ->
        groupChannels(list, categories)
    }.stateIn(viewModelScope, SharingStarted.Eagerly, emptyList())

    private val _isLoading = MutableStateFlow(false)
    val isLoading: StateFlow<Boolean> = _isLoading.asStateFlow()

    /** `@StringRes` of the current load error, or null. */
    private val _error = MutableStateFlow<Int?>(null)
    val error: StateFlow<Int?> = _error.asStateFlow()

    /**
     * One-shot navigation event for channel selection.
     * Bounded capacity (4) prevents buildup if a navigation collector is suspended.
     */
    private val _navigateToChannel = CoroutineChannel<ChannelNavEvent>(capacity = 4)
    val navigateToChannel = _navigateToChannel.receiveAsFlow()

    init {
        // The realtime connection is owned by RealtimeConnectionManager, started by KaikuApplication.
        loadGuilds()
    }

    fun onGuildSelected(guildId: String) {
        guildRepository.selectGuild(guildId)
        loadChannels(guildId)
    }

    /** Retries whatever failed: the selected server's channels, else the server list. */
    fun retry() {
        val guildId = guildRepository.selectedGuildId.value
        if (guildId != null) loadChannels(guildId) else loadGuilds()
    }

    private fun loadChannels(guildId: String) {
        _error.value = null
        viewModelScope.launch {
            try {
                guildRepository.loadChannels(guildId)
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                logger.warning("Failed to load channels: ${e.javaClass.simpleName}")
                _error.value = R.string.home_channels_load_failed
            }
        }
    }

    fun onChannelSelected(channelId: String, channelType: ChannelType) {
        // Opening a text channel marks it read and clears its unread badge.
        if (channelType == ChannelType.TEXT) {
            guildRepository.setActiveChannel(channelId)
        }
        val result = _navigateToChannel.trySend(ChannelNavEvent(channelId, channelType))
        if (result.isFailure) {
            logger.warning("navigateToChannel dropped (collector suspended or buffer full): $channelId")
        }
    }

    /** Notifications are (or became) allowed: register this device for push. */
    fun onNotificationsAllowed() {
        pushRegistrar.onPermissionChanged()
    }

    fun refresh() {
        loadGuilds()
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
                logger.warning("Failed to load servers: ${e.javaClass.simpleName}")
                _error.value = R.string.home_servers_load_failed
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
