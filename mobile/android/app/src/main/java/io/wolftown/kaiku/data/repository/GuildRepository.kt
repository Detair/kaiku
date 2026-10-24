package io.wolftown.kaiku.data.repository

import io.wolftown.kaiku.data.api.ChannelApi
import io.wolftown.kaiku.data.api.GuildApi
import io.wolftown.kaiku.di.ChatCoroutineScope
import io.wolftown.kaiku.domain.model.Category
import io.wolftown.kaiku.domain.model.Channel
import io.wolftown.kaiku.domain.model.Guild
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.getAndUpdate
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import java.util.logging.Logger
import javax.inject.Inject
import javax.inject.Singleton

@Singleton
class GuildRepository @Inject constructor(
    private val guildApi: GuildApi,
    private val channelApi: ChannelApi,
    @ChatCoroutineScope private val scope: CoroutineScope,
) {
    private val _guilds = MutableStateFlow<List<Guild>>(emptyList())
    val guilds: StateFlow<List<Guild>> = _guilds.asStateFlow()

    private val _selectedGuildId = MutableStateFlow<String?>(null)
    val selectedGuildId: StateFlow<String?> = _selectedGuildId.asStateFlow()

    private val _channels = MutableStateFlow<List<Channel>>(emptyList())
    val channels: StateFlow<List<Channel>> = _channels.asStateFlow()

    private val _categories = MutableStateFlow<List<Category>>(emptyList())
    val categories: StateFlow<List<Category>> = _categories.asStateFlow()

    /** The channel the user currently has open; its messages count as read. */
    private val _activeChannelId = MutableStateFlow<String?>(null)

    /**
     * Session epoch: [clearSession] bumps it, and a load publishes its result
     * only if the epoch is unchanged since the load started (checked and
     * published under [epochLock]), so a request still in flight at logout
     * cannot bring the old session's data back.
     */
    private val epochLock = Any()
    private var epoch = 0L

    private fun currentEpoch(): Long = synchronized(epochLock) { epoch }

    /** Runs [publish] only if [startEpoch] is still current; returns whether it ran. */
    private inline fun publishIfCurrent(startEpoch: Long, publish: () -> Unit): Boolean =
        synchronized(epochLock) {
            if (epoch != startEpoch) return false
            publish()
            true
        }

    /**
     * Session teardown: drops servers, channels, categories, the selection and
     * the open channel. A read mark still waiting for its debounce is dropped,
     * not sent: the session is ending. Loads still in flight publish nothing.
     */
    fun clearSession() {
        synchronized(readLock) {
            readJob?.cancel()
            readJob = null
            pending = null
        }
        synchronized(epochLock) {
            epoch++
            _activeChannelId.value = null
            _selectedGuildId.value = null
            _channels.value = emptyList()
            _categories.value = emptyList()
            _guilds.value = emptyList()
        }
    }

    suspend fun loadGuilds() {
        val startEpoch = currentEpoch()
        val guildList = guildApi.getGuilds()
        publishIfCurrent(startEpoch) { _guilds.value = guildList }
    }

    fun selectGuild(guildId: String) {
        _selectedGuildId.value = guildId
    }

    suspend fun loadChannels(guildId: String) {
        val startEpoch = currentEpoch()
        val channelList = channelApi.getChannels(guildId)
        // Category names are cosmetic: a failure leaves the channels ungrouped
        // instead of failing the whole load.
        val categoryList = try {
            guildApi.getCategories(guildId)
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            emptyList()
        }
        publishIfCurrent(startEpoch) {
            _categories.value = categoryList
            _channels.value = channelList
        }
    }

    /**
     * Reloads the channels (and their server-side unread counts) of the
     * selected server, e.g. after a reconnect. Does nothing when no server is
     * selected. The open channel stays marked read.
     */
    suspend fun reloadSelectedGuildChannels() {
        val guildId = _selectedGuildId.value ?: return
        loadChannels(guildId)
        _activeChannelId.value?.let { active -> _channels.update { clearUnread(it, active) } }
    }

    /**
     * Mark a channel as the active (open) one: clears its unread count,
     * records it so incoming messages there don't re-raise the badge, and
     * tells the server the channel is read up to its latest message.
     */
    fun setActiveChannel(channelId: String?) {
        val previous = _activeChannelId.getAndUpdate { channelId }
        if (previous != channelId) flushPendingRead()
        if (channelId != null) {
            val lastMessageId = _channels.value.find { it.id == channelId }?.lastMessageId
            _channels.update { clearUnread(it, channelId) }
            if (lastMessageId != null) scope.launch { sendRead(channelId, lastMessageId) }
        }
    }

    /**
     * The user left [channelId]. Resets the active channel only if it is still
     * the active one (a newer channel may already have taken over), so later
     * messages in [channelId] raise its unread badge again. A read still
     * waiting for the debounce is sent right away.
     */
    fun clearActiveChannel(channelId: String) {
        if (_activeChannelId.compareAndSet(channelId, null)) flushPendingRead()
    }

    /**
     * Called when a new message arrives (from the WS handler or a catch-up).
     * Remembers [messageId] as the channel's newest. Bumps the unread badge
     * unless the channel is open (then it is marked read, debounced) or
     * [countUnread] is false (the user's own message from another device).
     */
    fun onMessageReceived(channelId: String, messageId: String? = null, countUnread: Boolean = true) {
        val active = channelId == _activeChannelId.value
        _channels.update { list ->
            list.map {
                if (it.id != channelId) it
                else it.copy(
                    lastMessageId = messageId ?: it.lastMessageId,
                    unreadCount = if (!active && countUnread) it.unreadCount + 1 else it.unreadCount,
                )
            }
        }
        if (active && messageId != null) onActiveChannelMessage(channelId, messageId)
    }

    /**
     * [messageId] is the newest message of [channelId] the user can see. No-op
     * unless the channel is open. Marks it read on the server, at most once per
     * [READ_DEBOUNCE_MS], always with the newest id.
     */
    fun onActiveChannelMessage(channelId: String, messageId: String) {
        if (channelId != _activeChannelId.value) return
        synchronized(readLock) {
            if (pending != null && pending?.first != channelId) {
                // Switched channels: the old channel's read must not be lost.
                val old = pending!!
                pending = null
                scope.launch { sendRead(old.first, old.second) }
            }
            pending = channelId to messageId
            if (readJob != null) return
            readJob = scope.launch {
                while (true) {
                    delay(READ_DEBOUNCE_MS)
                    val next = synchronized(readLock) { pending.also { pending = null } }
                    if (next != null) sendRead(next.first, next.second)
                    val finished = synchronized(readLock) {
                        if (pending == null) {
                            readJob = null
                            true
                        } else {
                            false
                        }
                    }
                    if (finished) return@launch
                }
            }
        }
    }

    private val readLock = Any()
    private var readJob: Job? = null
    private var pending: Pair<String, String>? = null

    /** Sends the waiting read now instead of after the debounce. */
    private fun flushPendingRead() {
        val next = synchronized(readLock) { pending.also { pending = null } } ?: return
        scope.launch { sendRead(next.first, next.second) }
    }

    /** Best effort: unread is corrected by the next channel reload if this fails. */
    private suspend fun sendRead(channelId: String, lastMessageId: String) {
        try {
            channelApi.markAsRead(channelId, lastMessageId)
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            logger.warning("Failed to mark channel read (${e::class.simpleName})")
        }
    }

    companion object {
        private val logger = Logger.getLogger("GuildRepository")

        /** Minimum gap between read marks sent for the open channel. */
        const val READ_DEBOUNCE_MS = 1_000L

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
