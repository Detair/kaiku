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
    private var user: String? = "me"
    private val sink = CallWakeProcessorTest.FakeCallSink()
    private val store = PrefsCallNotificationStore(InMemorySharedPreferences()) { now }
    private val notifier = SocketCallNotifier(sink, store, { user }, { now })

    @Test
    fun `a ring posts once and a repeat keeps its start`() {
        notifier.ring("dm-1", "c-1", "anna", "Anna")
        now += 5_000
        notifier.ring("dm-1", "c-1", "anna", "Anna")

        assertEquals(2, sink.posts.size)
        assertEquals(1_000L, sink.posts.last().ringingSinceMs)
        assertEquals(CallKind.INCOMING, sink.posts.last().kind)
        assertEquals(ShownCall("c-1", CallPhase.RINGING, 1_000L), store.shown("me", "dm-1"))
    }

    @Test
    fun `a stopped ring leaves a tombstone so a late push does not ring again`() {
        notifier.ring("dm-1", "c-1", "anna", "Anna")
        notifier.stopRing("dm-1", "c-1")

        assertEquals(listOf("dm-1"), sink.cancelled)
        assertEquals(CallPhase.ENDED, store.shown("me", "dm-1")?.phase)
        val intake = CallWakeIntake(mockk<TokenStorage> { every { getUserId() } returns "me" }, store)
        val lateRing = WakeSignal(WakeKind.CALL, "dm-1", count = 0, call = CallInfo("c-1", "anna", CallState.RINGING))
        assertEquals(false, intake.accept(lateRing))
        notifier.ring("dm-1", "c-1", "anna", "Anna")
        assertEquals(1, sink.posts.size)
    }

    @Test
    fun `a missed call is posted once, also after a stop, and ignores the push's ended`() {
        notifier.ring("dm-1", "c-1", "anna", "Anna")
        notifier.stopRing("dm-1", "c-1")
        notifier.missed("dm-1", "c-1", "Anna")
        notifier.missed("dm-1", "c-1", "Anna")

        assertEquals(listOf(CallKind.INCOMING, CallKind.MISSED), sink.posts.map { it.kind })
        val ended = CallInfo("c-1", "anna", CallState.ENDED, CallEndReason.NO_ANSWER)
        assertEquals(false, CallRules.endedApplies(store.shown("me", "dm-1"), ended, now))
    }

    @Test
    fun `stopping an older call leaves a newer ring alone`() {
        notifier.ring("dm-1", "c-2", "anna", "Anna")
        notifier.stopRing("dm-1", "c-1")
        assertTrue(sink.cancelled.isEmpty())
    }

    @Test
    fun `nothing rings or is missed while nobody is signed in`() {
        user = null
        notifier.ring("dm-1", "c-1", "anna", "Anna")
        notifier.missed("dm-1", "c-1", "Anna")
        assertTrue(sink.posts.isEmpty())
    }
}
