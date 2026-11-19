package io.wolftown.kaiku.data.voice

import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.awaitCancellation
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.launch
import kotlinx.coroutines.test.TestScope
import kotlinx.coroutines.test.advanceTimeBy
import kotlinx.coroutines.test.runCurrent
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * C32 "real elapsed time": the 60 s without a socket that carries the join
 * count time the device was suspended, which coroutine delays do not see.
 * The fake clock jumps like `elapsedRealtime` after a sleep.
 */
@OptIn(ExperimentalCoroutinesApi::class)
class VoiceSignallingClockTest {

    private class Rig(test: TestScope) {
        val socket = MutableStateFlow<Long?>(1L)
        var realMs = 0L
        val joinSocket = MutableStateFlow<Long?>(null)
        val gaveUp = CompletableDeferred<Unit>()
        val signalling = VoiceSignalling(socket, clock = { realMs })

        private val steps = object : VoiceSignalling.Steps {
            override val joinSocket: StateFlow<Long?> = this@Rig.joinSocket
            override suspend fun prepare() {}
            override suspend fun join(socket: Long): Boolean {
                this@Rig.joinSocket.value = socket
                return true
            }
            override suspend fun awaitConnected() {}

            override suspend fun awaitConnectionFailed() = awaitCancellation()
            override suspend fun rejoin(socket: Long): VoiceSignalling.Attempt = awaitCancellation()
            override suspend fun giveUp(why: VoiceSignalling.GiveUp) {
                gaveUp.complete(Unit)
            }
        }

        val owner = test.backgroundScope.launch { signalling.run(steps) }
    }

    @Test
    fun `a sleep longer than 60 s without a socket gives up at the first wake-up`() = runTest {
        val rig = Rig(this)
        runCurrent()
        rig.socket.value = null // the socket that carried the join is lost
        runCurrent()

        // The device sleeps: 70 s pass in real time, the coroutine clock moves 2 s.
        rig.realMs += 70_000
        advanceTimeBy(2_000); runCurrent()
        assertFalse(rig.gaveUp.isCompleted)
        // The next wake-up of the deadline (at most 5 s) compares the real clock.
        advanceTimeBy(VoiceSignalling.DEADLINE_CHECK_MS); runCurrent()
        assertTrue(rig.gaveUp.isCompleted)
    }

    @Test
    fun `without a sleep the deadline is the 60 s of the delays`() = runTest {
        val rig = Rig(this)
        runCurrent()
        rig.socket.value = null
        runCurrent()
        advanceTimeBy(59_999); runCurrent()
        assertFalse(rig.gaveUp.isCompleted)
        advanceTimeBy(1); runCurrent()
        assertTrue(rig.gaveUp.isCompleted)
    }

    @Test
    fun `a carried session is never ended by a clock jump`() = runTest {
        val rig = Rig(this)
        runCurrent()
        rig.realMs += 3_600_000
        advanceTimeBy(120_000); runCurrent()
        assertFalse(rig.gaveUp.isCompleted)
        assertEquals(VoiceLinkState.Stable, rig.signalling.state.value)
        rig.owner.cancel()
    }
}
