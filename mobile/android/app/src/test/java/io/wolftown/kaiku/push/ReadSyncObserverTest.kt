package io.wolftown.kaiku.push

import io.mockk.every
import io.mockk.mockk
import io.mockk.verify
import io.wolftown.kaiku.data.ws.KaikuWebSocket
import io.wolftown.kaiku.data.ws.ServerEvent
import io.wolftown.kaiku.testutil.InMemorySharedPreferences
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.test.runCurrent
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

@OptIn(ExperimentalCoroutinesApi::class)
class ReadSyncObserverTest {

    private val events = MutableSharedFlow<ServerEvent>(extraBufferCapacity = 64)
    private val webSocket: KaikuWebSocket = mockk(relaxed = true) {
        every { events } returns this@ReadSyncObserverTest.events
    }
    private val sink: NotificationSink = mockk(relaxed = true)
    private val ledger = PrefsNotificationLedger(InMemorySharedPreferences()) { "me" }

    @Test
    fun `channel_read event cancels notification and marks seen`() = runTest {
        ReadSyncObserver(webSocket, sink, ledger, backgroundScope).start()

        events.emit(ServerEvent.ChannelRead("c1", "m5"))
        runCurrent()

        verify { sink.cancel("c1") }
        assertEquals("m5", ledger.lastSeen("c1"))
    }

    @Test
    fun `dm_read without position only cancels`() = runTest {
        ReadSyncObserver(webSocket, sink, ledger, backgroundScope).start()

        events.emit(ServerEvent.DmRead("d1", null))
        runCurrent()

        verify { sink.cancel("d1") }
        assertNull(ledger.lastSeen("d1"))
    }
}
