package io.wolftown.kaiku.push

import io.mockk.every
import io.mockk.mockk
import io.wolftown.kaiku.data.local.TokenStorage
import io.wolftown.kaiku.testutil.InMemorySharedPreferences
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

/** Socket rings and the push path share one store: one alert, one missed call per call. */
class SocketCallNotifierTest {
    private var now = 1_000L
    private val sessions = io.wolftown.kaiku.data.session.FakeSessions("me")
    private val me get() = sessions.generation.enter()!!
    private val sink = CallWakeProcessorTest.FakeCallSink()
    private val store = PrefsCallNotificationStore(InMemorySharedPreferences()) { now }
    private val notifier = SocketCallNotifier(sink, store) { now }

    @Test
    fun `a ring posts once and a repeat keeps its start`() {
        notifier.ring(me, "dm-1", "c-1", "anna", "Anna")
        now += 5_000
        notifier.ring(me, "dm-1", "c-1", "anna", "Anna")

        assertEquals(2, sink.posts.size)
        assertEquals(1_000L, sink.posts.last().ringingSinceMs)
        assertEquals(CallKind.INCOMING, sink.posts.last().kind)
        assertEquals(ShownCall("c-1", CallPhase.RINGING, 1_000L), store.shown("me", "dm-1"))
    }

    @Test
    fun `a stopped ring leaves a tombstone so a late push does not ring again`() {
        notifier.ring(me, "dm-1", "c-1", "anna", "Anna")
        notifier.stopRing(me, "dm-1", "c-1")

        assertEquals(listOf("dm-1"), sink.cancelled)
        assertEquals(CallPhase.ENDED, store.shown("me", "dm-1")?.phase)
        val intake = CallWakeIntake(mockk<TokenStorage> { every { getUserId() } returns "me" }, store)
        val lateRing = WakeSignal(WakeKind.CALL, "dm-1", count = 0, call = CallInfo("c-1", "anna", CallState.RINGING))
        assertEquals(false, intake.accept(lateRing))
        notifier.ring(me, "dm-1", "c-1", "anna", "Anna")
        assertEquals(1, sink.posts.size)
    }

    @Test
    fun `a missed call is posted once, also after a stop, and ignores the push's ended`() {
        notifier.ring(me, "dm-1", "c-1", "anna", "Anna")
        notifier.stopRing(me, "dm-1", "c-1")
        notifier.missed(me, "dm-1", "c-1", "Anna")
        notifier.missed(me, "dm-1", "c-1", "Anna")

        assertEquals(listOf(CallKind.INCOMING, CallKind.MISSED), sink.posts.map { it.kind })
        val ended = CallInfo("c-1", "anna", CallState.ENDED, CallEndReason.NO_ANSWER)
        assertEquals(false, CallRules.endedApplies(store.shown("me", "dm-1"), ended, now))
    }

    @Test
    fun `stopping an older call leaves a newer ring alone`() {
        notifier.ring(me, "dm-1", "c-2", "anna", "Anna")
        notifier.stopRing(me, "dm-1", "c-1")
        assertTrue(sink.cancelled.isEmpty())
    }

    @Test
    fun `a ring stopped by a session that ends at that moment removes nothing of the next one`() {
        val old = me
        notifier.ring(old, "dm-1", "c-1", "anna", "Anna")
        // The session ends while the stop is being carried out; the next account has its own
        // ring in a conversation it shares with the old one.
        sink.beforeCancel = {
            sink.beforeCancel = {}
            sessions.switchTo("bert")
            sink.shown["dm-1"] = CallNotification("bert", "dm-1", "c-9", CallKind.INCOMING, "Anna")
        }
        notifier.stopRing(old, "dm-1", "c-1")
        assertTrue("the ended session removed a notification: ${sink.cancelled}", sink.cancelled.isEmpty())
        assertEquals("c-9", sink.shown["dm-1"]?.callId)

        // The same for a ring that is only silenced.
        val bert = me
        notifier.ring(bert, "dm-2", "c-5", "anna", "Anna")
        sink.beforeCancel = { sink.beforeCancel = {}; sessions.switchTo("carl") }
        notifier.silenceRing(bert, "dm-2", "c-5")
        assertTrue(sink.cancelled.isEmpty())
    }

    @Test
    fun `nothing is posted for a session that is over, and what it posted is removed`() {
        val old = me
        notifier.ring(old, "dm-1", "c-1", "anna", "Anna")
        // The sign-out hook: the generation advances, then every ring goes.
        sessions.generation.advance()
        notifier.stopAll()
        assertEquals(1, sink.cancelledAll)

        notifier.ring(old, "dm-1", "c-2", "anna", "Anna")
        notifier.missed(old, "dm-1", "c-1", "Anna")
        notifier.missedIfRinging(old, "dm-1", "c-1", "Anna")
        notifier.stopRing(old, "dm-1", "c-1")
        assertEquals("only the ring from before the sign-out was ever posted", 1, sink.posts.size)
        assertTrue(sink.cancelled.isEmpty())
    }

    @Test
    fun `a ring whose notification timed out is missed only if nothing else was said about it`() {
        notifier.ring(me, "dm-1", "c-1", "anna", "Anna")
        notifier.missedIfRinging(me, "dm-1", "c-1", "Anna")
        assertEquals(listOf(CallKind.INCOMING, CallKind.MISSED), sink.posts.map { it.kind })

        // Declined (or ended) before its notification went away: not a missed call.
        notifier.ring(me, "dm-2", "c-7", "bob", "Bob")
        notifier.stopRing(me, "dm-2", "c-7")
        notifier.missedIfRinging(me, "dm-2", "c-7", "Bob")
        assertEquals(1, sink.posts.count { it.kind == CallKind.MISSED })
    }

    @Test
    fun `a ring that came by push keeps the time it was received`() {
        // The push intake recorded the ring when its wake arrived.
        store.setShown("me", "dm-1", ShownCall("c-1", CallPhase.RINGING, 400L))
        notifier.ring(me, "dm-1", "c-1", "anna", "Anna")
        assertEquals(400L, sink.posts.single().ringingSinceMs)
    }
}
