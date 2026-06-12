package io.wolftown.kaiku.data.repository

import io.wolftown.kaiku.data.api.ChannelApi
import io.wolftown.kaiku.data.api.GuildApi
import io.wolftown.kaiku.domain.model.Channel
import io.wolftown.kaiku.domain.model.Guild
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import javax.inject.Inject
import javax.inject.Singleton

@Singleton
class GuildRepository @Inject constructor(
    private val guildApi: GuildApi,
    private val channelApi: ChannelApi
) {
    private val _guilds = MutableStateFlow<List<Guild>>(emptyList())
    val guilds: StateFlow<List<Guild>> = _guilds.asStateFlow()

    private val _selectedGuildId = MutableStateFlow<String?>(null)
    val selectedGuildId: StateFlow<String?> = _selectedGuildId.asStateFlow()

    private val _channels = MutableStateFlow<List<Channel>>(emptyList())
    val channels: StateFlow<List<Channel>> = _channels.asStateFlow()

    /** The channel the user currently has open; its messages count as read. */
    private val _activeChannelId = MutableStateFlow<String?>(null)

    suspend fun loadGuilds() {
        val guildList = guildApi.getGuilds()
        _guilds.value = guildList
    }

    fun selectGuild(guildId: String) {
        _selectedGuildId.value = guildId
    }

    suspend fun loadChannels(guildId: String) {
        val channelList = channelApi.getChannels(guildId)
        _channels.value = channelList
    }

    /**
     * Mark a channel as the active (open) one: clears its unread count and
     * records it so incoming messages there don't re-raise the badge.
     */
    fun setActiveChannel(channelId: String?) {
        _activeChannelId.value = channelId
        if (channelId != null) {
            _channels.value = clearUnread(_channels.value, channelId)
        }
    }

    /**
     * Called when a new message arrives (from the WS handler). Bumps the
     * channel's unread badge unless it's the channel the user is reading.
     */
    fun onMessageReceived(channelId: String) {
        if (channelId == _activeChannelId.value) return
        _channels.value = incrementUnread(_channels.value, channelId)
    }

    companion object {
        /** Increment unread for one channel in the list (pure, testable). */
        fun incrementUnread(channels: List<Channel>, channelId: String): List<Channel> =
            channels.map {
                if (it.id == channelId) it.copy(unreadCount = it.unreadCount + 1) else it
            }

        /** Reset unread for one channel in the list (pure, testable). */
        fun clearUnread(channels: List<Channel>, channelId: String): List<Channel> =
            channels.map { if (it.id == channelId) it.copy(unreadCount = 0) else it }
    }
}
