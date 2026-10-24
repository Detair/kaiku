package io.wolftown.kaiku.push

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
 * Clears a channel's message notification when it was read in another
 * session of the same user (`channel_read` / `dm_read`) and records the read
 * position, so the next wake does not notify about those messages again.
 */
@Singleton
class ReadSyncObserver @Inject constructor(
    private val webSocket: KaikuWebSocket,
    private val sink: NotificationSink,
    private val ledger: NotificationLedger,
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
        sink.cancel(channelId)
        if (lastReadMessageId != null) ledger.markSeen(channelId, lastReadMessageId)
    }
}
