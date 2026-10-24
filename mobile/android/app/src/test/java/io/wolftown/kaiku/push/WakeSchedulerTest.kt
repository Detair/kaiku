package io.wolftown.kaiku.push

import io.mockk.mockk
import io.mockk.verify
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class WakeSchedulerTest {

    private val enqueued = mutableListOf<WakeSignal>()
    private val sink: NotificationSink = mockk(relaxed = true)
    private val scheduler = WakeScheduler(WakeWorkEnqueuer { enqueued += it }, sink)

    @Test
    fun `dm wake enqueues work`() {
        val signal = WakeSignal(WakeKind.DM, "c1", 2)
        scheduler.onWake(signal)
        assertEquals(listOf(signal), enqueued)
        verify(exactly = 0) { sink.cancel(any()) }
    }

    @Test
    fun `mention wake enqueues work`() {
        scheduler.onWake(WakeSignal(WakeKind.MENTION, "c1", 1))
        assertEquals(1, enqueued.size)
    }

    @Test
    fun `read signal cancels notification without enqueuing work`() {
        scheduler.onWake(WakeSignal(WakeKind.READ, "c1", 0))
        assertTrue(enqueued.isEmpty())
        verify { sink.cancel("c1") }
    }

    @Test
    fun `call and unknown signals are ignored`() {
        scheduler.onWake(WakeSignal(WakeKind.CALL, "c1", 1))
        scheduler.onWake(WakeSignal(WakeKind.UNKNOWN, "c1", 1))
        assertTrue(enqueued.isEmpty())
        verify(exactly = 0) { sink.cancel(any()) }
    }
}
