package io.wolftown.kaiku.push

import io.wolftown.kaiku.data.local.TokenStorage
import io.wolftown.kaiku.data.repository.ChatRepository
import io.wolftown.kaiku.data.ws.KaikuWebSocket
import io.wolftown.kaiku.data.ws.ServerEvent
import io.wolftown.kaiku.di.ChatCoroutineScope
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.CoroutineStart
import kotlinx.coroutines.launch
import java.util.concurrent.atomic.AtomicBoolean
import javax.inject.Inject
import javax.inject.Singleton

/**
 * Handles `channel_read` / `dm_read` (the channel was read in another session
 * of this user): clears the channel's notification, stops its pending wake
 * fetches and records the read position, so later wakes do not notify about
 * those messages again.
 */
@Singleton
class ReadSyncObserver @Inject constructor(
    private val webSocket: KaikuWebSocket,
    private val sink: NotificationSink,
    private val enqueuer: WakeWorkEnqueuer,
    private val ledger: NotificationLedger,
    private val chat: ChatRepository,
    private val tokenStorage: TokenStorage,
    @ChatCoroutineScope private val scope: CoroutineScope,
) {
    private val started = AtomicBoolean(false)

    fun start() {
        if (!started.compareAndSet(false, true)) return
        // UNDISPATCHED: subscribe to the replay-0 SharedFlow before start() returns.
        scope.launch(start = CoroutineStart.UNDISPATCHED) {
            webSocket.events.collect { event ->
                when (event) {
                    is ServerEvent.ChannelRead -> onRead(event.channelId, event.lastReadMessageId)
                    is ServerEvent.DmRead -> onRead(event.channelId, event.lastReadMessageId)
                    else -> Unit
                }
            }
        }
    }

    private fun onRead(channelId: String, lastReadMessageId: String?) {
        // Ledger first, then cancel: a post racing with this read either sees
        // the read cursor or is cancelled right after it was posted.
        val userId = tokenStorage.getUserId()
        if (userId != null && lastReadMessageId != null) {
            // Exact when the message is cached; otherwise kept by id only, as a
            // boundary for the next fetch.
            val createdAt = chat.cachedCreatedAt(channelId, lastReadMessageId)
            ledger.markRead(userId, channelId, lastReadMessageId, createdAt)
            if (createdAt != null) ledger.markSeen(userId, channelId, lastReadMessageId, createdAt)
        }
        enqueuer.cancel(channelId)
        sink.cancel(channelId)
    }
}
