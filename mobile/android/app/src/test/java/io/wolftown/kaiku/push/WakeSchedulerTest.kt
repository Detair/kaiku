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
    private val callStore = PrefsCallNotificationStore(InMemorySharedPreferences())
    private val scheduler = WakeScheduler(enqueuer, sink, CallWakeIntake(tokenStorage, callStore))

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
    private val callerId = "7a8b9c0d-1e2f-4a3b-8c4d-5e6f7a8b9c0d"

    private fun call(state: CallState, reason: CallEndReason? = null) =
        WakeSignal(WakeKind.CALL, "c1", 1, CallInfo(callId, callerId, state, reason))

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
    fun `ended call goes to the call path`() {
        val signal = call(CallState.ENDED, CallEndReason.NO_ANSWER)
        scheduler.onWake(signal)
        assertEquals(listOf(signal), enqueuer.calls)
        assertTrue(enqueuer.enqueued.isEmpty())
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
    fun `accepted ringing call is recorded before its work runs`() {
        scheduler.onWake(call(CallState.RINGING))
        assertEquals(ShownCall(callId, missed = false), callStore.shown("me", "c1"))
    }

    @Test
    fun `ringing does not downgrade a call already shown as missed`() {
        callStore.setShown("me", "c1", ShownCall(callId, missed = true))
        scheduler.onWake(call(CallState.RINGING))
        assertEquals(ShownCall(callId, missed = true), callStore.shown("me", "c1"))
    }

    @Test
    fun `ended call records nothing`() {
        scheduler.onWake(call(CallState.ENDED, CallEndReason.NO_ANSWER))
        assertNull(callStore.shown("me", "c1"))
    }

    @Test
    fun `ringing while signed out records nothing`() {
        signedIn = null
        scheduler.onWake(call(CallState.RINGING))
        assertNull(callStore.shown("me", "c1"))
    }
}
