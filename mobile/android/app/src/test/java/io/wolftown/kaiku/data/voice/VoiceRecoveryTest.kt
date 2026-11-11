package io.wolftown.kaiku.data.voice

import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.delay
import kotlinx.coroutines.test.TestScope
import kotlinx.coroutines.test.advanceTimeBy
import kotlinx.coroutines.test.currentTime
import kotlinx.coroutines.test.runCurrent
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * C32 rejoin state machine on virtual time. No Android, no WebRTC.
 */
@OptIn(ExperimentalCoroutinesApi::class)
class VoiceRecoveryTest {

    companion object {
        private val HANG = Any()
    }

    private class Harness(scope: CoroutineScope, private val clock: () -> Long) {
        /** Virtual time of every rejoin attempt. */
        val attemptTimes = mutableListOf<Long>()
        val attemptChannels = mutableListOf<String>()
        val gaveUp = mutableListOf<String>()

        /** Result of each attempt in order; missing entries = false. HANG never answers. */
        val results = ArrayDeque<Any>()

        val recovery = VoiceRecovery(
            scope = scope,
            rejoin = { channel ->
                attemptTimes += clock()
                attemptChannels += channel
                val r = results.removeFirstOrNull() ?: false
                if (r == HANG) delay(Long.MAX_VALUE)
                r == true
            },
            onGaveUp = { gaveUp += it },
        )
    }

    private fun TestScope.harness() = Harness(backgroundScope) { currentTime }

    @Test
    fun `socket back rejoins after 1 s and ends stable on success`() = runTest {
        val h = harness()
        h.results += true
        h.recovery.begin("ch", socketUp = true)

        h.recovery.onSocketLost()
        assertEquals(VoiceLinkState.Interrupted, h.recovery.state.value)
        h.recovery.onSocketReady()
        assertEquals(VoiceLinkState.Rejoining(1), h.recovery.state.value)

        advanceTimeBy(999); runCurrent()
        assertTrue(h.attemptTimes.isEmpty())
        advanceTimeBy(1); runCurrent()

        assertEquals(listOf(1_000L), h.attemptTimes)
        assertEquals(listOf("ch"), h.attemptChannels)
        assertEquals(VoiceLinkState.Stable, h.recovery.state.value)
        assertTrue(h.gaveUp.isEmpty())
    }

    @Test
    fun `three failed attempts at 1 s, 3 s and 9 s then give up exactly once`() = runTest {
        val h = harness()
        h.recovery.begin("ch", socketUp = true)
        h.recovery.onSocketReady()

        advanceTimeBy(1_000); runCurrent()
        assertEquals(VoiceLinkState.Rejoining(2), h.recovery.state.value)
        advanceTimeBy(3_000); runCurrent()
        assertEquals(VoiceLinkState.Rejoining(3), h.recovery.state.value)
        assertTrue(h.gaveUp.isEmpty())
        advanceTimeBy(9_000); runCurrent()

        assertEquals(listOf(1_000L, 4_000L, 13_000L), h.attemptTimes)
        assertEquals(listOf("ch"), h.gaveUp)
        assertEquals(VoiceLinkState.Stable, h.recovery.state.value)

        advanceTimeBy(60_000); runCurrent()
        assertEquals(3, h.attemptTimes.size)
    }

    @Test
    fun `success on the second attempt stops the loop`() = runTest {
        val h = harness()
        h.results += false
        h.results += true
        h.recovery.begin("ch", socketUp = true)
        h.recovery.onSocketReady()

        advanceTimeBy(60_000); runCurrent()

        assertEquals(listOf(1_000L, 4_000L), h.attemptTimes)
        assertTrue(h.gaveUp.isEmpty())
        assertEquals(VoiceLinkState.Stable, h.recovery.state.value)
    }

    @Test
    fun `an attempt that never answers fails after the attempt timeout`() = runTest {
        val h = harness()
        h.results += HANG
        h.results += true
        h.recovery.begin("ch", socketUp = true)
        h.recovery.onSocketReady()

        advanceTimeBy(60_000); runCurrent()

        // 1 s delay + 10 s timeout, then the 3 s delay of attempt 2.
        assertEquals(listOf(1_000L, 14_000L), h.attemptTimes)
        assertEquals(VoiceLinkState.Stable, h.recovery.state.value)
    }

    @Test
    fun `own leave during the backoff cancels the pending rejoin`() = runTest {
        val h = harness()
        h.recovery.begin("ch", socketUp = true)
        h.recovery.onSocketReady()
        advanceTimeBy(500); runCurrent()

        h.recovery.end()
        advanceTimeBy(60_000); runCurrent()

        assertTrue(h.attemptTimes.isEmpty())
        assertTrue(h.gaveUp.isEmpty())
        assertEquals(VoiceLinkState.Stable, h.recovery.state.value)
    }

    @Test
    fun `own leave during an attempt cancels it and nothing follows`() = runTest {
        val h = harness()
        h.results += HANG
        h.recovery.begin("ch", socketUp = true)
        h.recovery.onSocketReady()
        advanceTimeBy(1_000); runCurrent()
        assertEquals(1, h.attemptTimes.size)

        h.recovery.end()
        advanceTimeBy(60_000); runCurrent()

        assertEquals(1, h.attemptTimes.size)
        assertTrue(h.gaveUp.isEmpty())
    }

    @Test
    fun `no rejoin without a session or after the session ended`() = runTest {
        val h = harness()
        h.recovery.onSocketReady()
        h.recovery.onIceFailed()
        advanceTimeBy(60_000); runCurrent()
        assertTrue(h.attemptTimes.isEmpty())

        h.recovery.begin("ch", socketUp = true)
        h.recovery.end() // voice_removed, own leave
        h.recovery.onSocketLost()
        h.recovery.onSocketReady()
        h.recovery.onIceFailed()
        advanceTimeBy(60_000); runCurrent()
        assertTrue(h.attemptTimes.isEmpty())
    }

    @Test
    fun `ice disconnected only shows reconnecting and does not rejoin`() = runTest {
        val h = harness()
        h.recovery.begin("ch", socketUp = true)

        h.recovery.onIceDisconnected()
        assertEquals(VoiceLinkState.Interrupted, h.recovery.state.value)
        advanceTimeBy(60_000); runCurrent()
        assertTrue(h.attemptTimes.isEmpty())

        h.recovery.onIceConnected()
        assertEquals(VoiceLinkState.Stable, h.recovery.state.value)
    }

    @Test
    fun `ice failed on a live socket starts the same flow`() = runTest {
        val h = harness()
        h.results += true
        h.recovery.begin("ch", socketUp = true)

        h.recovery.onIceFailed()
        assertEquals(VoiceLinkState.Rejoining(1), h.recovery.state.value)
        advanceTimeBy(1_000); runCurrent()

        assertEquals(listOf(1_000L), h.attemptTimes)
        assertEquals(VoiceLinkState.Stable, h.recovery.state.value)
    }

    @Test
    fun `ice failed while the socket is down waits for the socket`() = runTest {
        val h = harness()
        h.results += true
        h.recovery.begin("ch", socketUp = true)
        h.recovery.onSocketLost()

        h.recovery.onIceFailed()
        advanceTimeBy(60_000); runCurrent()
        assertTrue(h.attemptTimes.isEmpty())
        assertEquals(VoiceLinkState.Interrupted, h.recovery.state.value)

        h.recovery.onSocketReady()
        advanceTimeBy(1_000); runCurrent()
        assertEquals(1, h.attemptTimes.size)
    }

    @Test
    fun `a socket that drops during the backoff makes the attempt wait without using it up`() = runTest {
        val h = harness()
        h.results += true
        h.recovery.begin("ch", socketUp = true)
        h.recovery.onSocketReady()
        advanceTimeBy(100); runCurrent()

        h.recovery.onSocketLost()
        advanceTimeBy(30_000); runCurrent()
        assertTrue(h.attemptTimes.isEmpty())
        assertTrue(h.gaveUp.isEmpty())

        h.recovery.onSocketReady()
        runCurrent()
        assertEquals(listOf(30_100L), h.attemptTimes)
        assertEquals(VoiceLinkState.Stable, h.recovery.state.value)
    }

    @Test
    fun `a second Ready while rejoining does not start a second loop`() = runTest {
        val h = harness()
        h.recovery.begin("ch", socketUp = true)
        h.recovery.onSocketReady()
        h.recovery.onSocketReady()
        h.recovery.onIceFailed()

        advanceTimeBy(60_000); runCurrent()

        assertEquals(3, h.attemptTimes.size)
        assertEquals(1, h.gaveUp.size)
    }

    @Test
    fun `a failing rejoin that throws counts as a failed attempt`() = runTest {
        var calls = 0
        val gaveUp = mutableListOf<String>()
        val recovery = VoiceRecovery(
            scope = backgroundScope,
            rejoin = { calls++; error("boom") },
            onGaveUp = { gaveUp += it },
        )
        recovery.begin("ch", socketUp = true)
        recovery.onSocketReady()

        advanceTimeBy(60_000); runCurrent()

        assertEquals(3, calls)
        assertEquals(listOf("ch"), gaveUp)
    }
}
