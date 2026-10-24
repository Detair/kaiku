package io.wolftown.kaiku.push

import io.mockk.every
import io.mockk.mockk
import io.mockk.verify
import io.wolftown.kaiku.data.local.TokenStorage
import io.wolftown.kaiku.data.repository.ChatRepository
import io.wolftown.kaiku.data.ws.KaikuWebSocket
import io.wolftown.kaiku.data.ws.ServerEvent
import io.wolftown.kaiku.testutil.InMemorySharedPreferences
import kotlinx.coroutines.CoroutineScope
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
    private val enqueuer = FakeWakeWorkEnqueuer()
    private val ledger = PrefsNotificationLedger(InMemorySharedPreferences())
    private val chat: ChatRepository = mockk {
        every { cachedCreatedAt(any(), any()) } returns null
    }
    private val tokenStorage: TokenStorage = mockk {
        every { getUserId() } returns "me"
    }

    private fun observer(scope: CoroutineScope) =
        ReadSyncObserver(webSocket, sink, enqueuer, ledger, chat, tokenStorage, scope).also { it.start() }

    @Test
    fun `channel_read event cancels notification and marks seen`() = runTest {
        every { chat.cachedCreatedAt("c1", "m5") } returns "2026-10-24T10:05:00Z"
        observer(backgroundScope)

        events.emit(ServerEvent.ChannelRead("c1", "m5"))
        runCurrent()

        verify { sink.cancel("c1") }
        assertEquals(listOf("c1"), enqueuer.cancelled)
        assertEquals("m5", ledger.lastSeen("me", "c1"))
        assertEquals("2026-10-24T10:05:00Z", ledger.lastSeenCreatedAt("me", "c1"))
    }

    @Test
    fun `dm_read without position only cancels`() = runTest {
        observer(backgroundScope)

        events.emit(ServerEvent.DmRead("d1", null))
        runCurrent()

        verify { sink.cancel("d1") }
        assertEquals(listOf("d1"), enqueuer.cancelled)
        assertNull(ledger.lastSeen("me", "d1"))
    }

    @Test
    fun `read of an uncached message never moves a known position backwards`() = runTest {
        ledger.markSeen("me", "c1", "f0000000-0000-4000-8000-000000000009", "2026-10-24T10:09:00Z")
        observer(backgroundScope)

        events.emit(ServerEvent.ChannelRead("c1", "00000000-0000-4000-8000-000000000001"))
        runCurrent()

        assertEquals("f0000000-0000-4000-8000-000000000009", ledger.lastSeen("me", "c1"))
    }
}
