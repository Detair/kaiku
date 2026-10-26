package io.wolftown.kaiku.push

import io.mockk.every
import io.mockk.mockk
import io.mockk.verify
import io.wolftown.kaiku.data.local.TokenStorage
import io.wolftown.kaiku.testutil.InMemorySharedPreferences
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class WakeSchedulerTest {

    private val enqueuer = FakeWakeWorkEnqueuer()
    private val sink: NotificationSink = mockk(relaxed = true)
    private var signedIn: String? = "me"
    private val tokenStorage: TokenStorage = mockk {
        every { getUserId() } answers { signedIn }
    }
    private var clock = 1_000_000L
    private val callStore = PrefsCallNotificationStore(InMemorySharedPreferences()) { clock }
    private val intake = CallWakeIntake(tokenStorage, callStore).also { it.now = { clock } }
    private val scheduler = WakeScheduler(enqueuer, sink, intake)

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
    fun `call without call info and unknown signals are ignored`() {
        scheduler.onWake(WakeSignal(WakeKind.CALL, "c1", 1))
        scheduler.onWake(WakeSignal(WakeKind.UNKNOWN, "c1", 1))
        assertTrue(enqueuer.enqueued.isEmpty())
        assertTrue(enqueuer.calls.isEmpty())
        assertTrue(enqueuer.cancelled.isEmpty())
        verify(exactly = 0) { sink.cancel(any()) }
    }

    private val callId = "6f1c2a3b-4d5e-4f60-8a7b-9c0d1e2f3a4b"
    private val otherCallId = "1b2c3d4e-5f60-4a7b-8c9d-0e1f2a3b4c5d"
    private val callerId = "7a8b9c0d-1e2f-4a3b-8c4d-5e6f7a8b9c0d"

    private fun call(state: CallState, reason: CallEndReason? = null, id: String = callId) =
        WakeSignal(WakeKind.CALL, "c1", 1, CallInfo(id, callerId, state, reason))

    @Test
    fun `ringing call goes to the call path, never to a message fetch`() {
        val signal = call(CallState.RINGING)
        scheduler.onWake(signal)
        assertEquals(listOf(signal), enqueuer.calls)
        assertTrue(enqueuer.enqueued.isEmpty())
        assertTrue(enqueuer.cancelled.isEmpty())
        verify(exactly = 0) { sink.cancel(any()) }
    }

    @Test
    fun `ended for the ringing call goes to the call path`() {
        scheduler.onWake(call(CallState.RINGING))
        val ended = call(CallState.ENDED, CallEndReason.NO_ANSWER)
        scheduler.onWake(ended)
        assertEquals(ended, enqueuer.calls.last())
        assertTrue(enqueuer.enqueued.isEmpty())
    }

    @Test
    fun `ended with nothing stored goes to the call path`() {
        // The phone missed the ringing (offline, collapsed): the ended still shows a missed call.
        val ended = call(CallState.ENDED, CallEndReason.CANCELLED)
        scheduler.onWake(ended)
        assertEquals(listOf(ended), enqueuer.calls)
    }

    @Test
    fun `ended with unknown reason is not queued`() {
        scheduler.onWake(call(CallState.RINGING))
        scheduler.onWake(call(CallState.ENDED, CallEndReason.UNKNOWN))
        assertEquals(1, enqueuer.calls.size)
    }

    @Test
    fun `ended without reason is not queued`() {
        scheduler.onWake(call(CallState.RINGING))
        scheduler.onWake(call(CallState.ENDED, reason = null))
        assertEquals(1, enqueuer.calls.size)
    }

    @Test
    fun `ended for another call is not queued and cannot replace the ringing work`() {
        scheduler.onWake(call(CallState.RINGING, id = otherCallId))
        scheduler.onWake(call(CallState.ENDED, CallEndReason.NO_ANSWER, id = callId))
        assertEquals(listOf(otherCallId), enqueuer.calls.map { it.call?.callId })
    }

    @Test
    fun `call with unknown state is ignored`() {
        scheduler.onWake(call(CallState.UNKNOWN))
        assertTrue(enqueuer.calls.isEmpty())
        assertTrue(enqueuer.enqueued.isEmpty())
    }

    @Test
    fun `read wake does not touch call work`() {
        scheduler.onWake(WakeSignal(WakeKind.READ, "c1", 0))
        assertTrue(enqueuer.calls.isEmpty())
        assertEquals(listOf("c1"), enqueuer.cancelled)
    }

    @Test
    fun `accepted ringing call is recorded with its receive time before its work runs`() {
        scheduler.onWake(call(CallState.RINGING))
        assertEquals(ShownCall(callId, CallPhase.RINGING, clock), callStore.shown("me", "c1"))
    }

    @Test
    fun `repeated ringing keeps the first receive time`() {
        scheduler.onWake(call(CallState.RINGING))
        val first = clock
        clock += 5_000
        scheduler.onWake(call(CallState.RINGING))
        assertEquals(ShownCall(callId, CallPhase.RINGING, first), callStore.shown("me", "c1"))
        assertEquals(2, enqueuer.calls.size)
    }

    @Test
    fun `ringing for a call already missed is not queued`() {
        callStore.setShown("me", "c1", ShownCall(callId, CallPhase.MISSED, clock))
        scheduler.onWake(call(CallState.RINGING))
        assertEquals(CallPhase.MISSED, callStore.shown("me", "c1")?.phase)
        assertTrue(enqueuer.calls.isEmpty())
    }

    @Test
    fun `ringing for an ended call is not queued`() {
        callStore.setShown("me", "c1", ShownCall(callId, CallPhase.ENDED, clock))
        scheduler.onWake(call(CallState.RINGING))
        assertEquals(CallPhase.ENDED, callStore.shown("me", "c1")?.phase)
        assertTrue(enqueuer.calls.isEmpty())
    }

    @Test
    fun `duplicate ended after a missed call is not queued`() {
        callStore.setShown("me", "c1", ShownCall(callId, CallPhase.MISSED, clock))
        scheduler.onWake(call(CallState.ENDED, CallEndReason.NO_ANSWER))
        assertTrue(enqueuer.calls.isEmpty())
    }

    @Test
    fun `ended call records nothing at intake`() {
        scheduler.onWake(call(CallState.ENDED, CallEndReason.NO_ANSWER))
        assertNull(callStore.shown("me", "c1"))
    }

    @Test
    fun `ringing while signed out records and queues nothing`() {
        signedIn = null
        scheduler.onWake(call(CallState.RINGING))
        assertNull(callStore.shown("me", "c1"))
        assertTrue(enqueuer.calls.isEmpty())
    }
}
