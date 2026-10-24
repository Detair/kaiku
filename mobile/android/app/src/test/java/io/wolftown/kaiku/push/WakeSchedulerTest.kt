package io.wolftown.kaiku.push

import io.mockk.every
import io.mockk.mockk
import io.mockk.verify
import io.wolftown.kaiku.data.local.TokenStorage
import io.wolftown.kaiku.testutil.InMemorySharedPreferences
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class WakeSchedulerTest {

    private val enqueuer = FakeWakeWorkEnqueuer()
    private val sink: NotificationSink = mockk(relaxed = true)
    private val ledger = PrefsNotificationLedger(InMemorySharedPreferences())
    private val tokenStorage: TokenStorage = mockk {
        every { getUserId() } returns "me"
    }
    private val scheduler = WakeScheduler(enqueuer, sink, ledger, tokenStorage)

    @Test
    fun `dm wake enqueues work`() {
        val signal = WakeSignal(WakeKind.DM, "c1", 2)
        scheduler.onWake(signal)
        assertEquals(listOf(signal), enqueuer.enqueued)
        verify(exactly = 0) { sink.cancel(any()) }
    }

    @Test
    fun `mention wake enqueues work`() {
        scheduler.onWake(WakeSignal(WakeKind.MENTION, "c1", 1))
        assertEquals(1, enqueuer.enqueued.size)
    }

    @Test
    fun `read signal cancels notification without enqueuing work`() {
        scheduler.onWake(WakeSignal(WakeKind.READ, "c1", 0))
        assertTrue(enqueuer.enqueued.isEmpty())
        verify { sink.cancel("c1") }
    }

    @Test
    fun `read signal cancels pending fetches of the channel`() {
        scheduler.onWake(WakeSignal(WakeKind.READ, "c1", 0))
        assertEquals(listOf("c1"), enqueuer.cancelled)
    }

    @Test
    fun `read signal records a read cursor at the receive time`() {
        scheduler.onWake(WakeSignal(WakeKind.READ, "c1", 0))

        val cursor = ledger.readCursor("me", "c1")
        assertNull(cursor.receivedId)
        assertNotNull(cursor.receivedAt)
    }

    @Test
    fun `call and unknown signals are ignored`() {
        scheduler.onWake(WakeSignal(WakeKind.CALL, "c1", 1))
        scheduler.onWake(WakeSignal(WakeKind.UNKNOWN, "c1", 1))
        assertTrue(enqueuer.enqueued.isEmpty())
        assertTrue(enqueuer.cancelled.isEmpty())
        verify(exactly = 0) { sink.cancel(any()) }
    }
}
