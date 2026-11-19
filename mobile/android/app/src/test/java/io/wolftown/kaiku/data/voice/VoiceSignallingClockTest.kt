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

        /** What a proof after a gap answers, and on which sockets one was asked. */
        var proof = true
        val proofs = mutableListOf<Long>()
        var attempts = 0
        val signalling = VoiceSignalling(socket, clock = { realMs })

        private val steps = object : VoiceSignalling.Steps {
            override val joinSocket: StateFlow<Long?> = this@Rig.joinSocket
            override suspend fun prepare() {}
            override suspend fun join(socket: Long): Boolean {
                this@Rig.joinSocket.value = socket
                return true
            }
            override suspend fun awaitConnected() {}

            override suspend fun proveCarried(socket: Long): Boolean {
                proofs += socket
                return proof
            }

            override suspend fun awaitConnectionFailed() = awaitCancellation()
            override suspend fun rejoin(socket: Long): VoiceSignalling.Attempt {
                attempts++
                return awaitCancellation()
            }
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

    // -- C32 "proof of carried" ------------------------------------------------------

    @Test
    fun `after a gap a carried session proves it and stays up`() = runTest {
        val rig = Rig(this)
        runCurrent()
        rig.realMs += 30_000 // slept 30 s
        advanceTimeBy(VoiceSignalling.DEADLINE_CHECK_MS); runCurrent()
        assertEquals(listOf(1L), rig.proofs)
        assertEquals(VoiceLinkState.Stable, rig.signalling.state.value)
        advanceTimeBy(600_000); runCurrent()
        assertFalse(rig.gaveUp.isCompleted)
        rig.owner.cancel()
    }

    @Test
    fun `no gap, no proof`() = runTest {
        val rig = Rig(this)
        runCurrent()
        rig.realMs += 4_000
        advanceTimeBy(VoiceSignalling.DEADLINE_CHECK_MS); runCurrent()
        assertTrue(rig.proofs.isEmpty())
        rig.owner.cancel()
    }

    @Test
    fun `a failed proof after a sleep longer than 60 s gives up at once instead of rejoining`() = runTest {
        val rig = Rig(this)
        runCurrent()
        rig.proof = false
        rig.realMs += 70_000 // slept 70 s; the socket still looks authenticated
        advanceTimeBy(VoiceSignalling.DEADLINE_CHECK_MS); runCurrent()
        advanceTimeBy(VoiceSignalling.DEADLINE_CHECK_MS); runCurrent()
        assertTrue(rig.gaveUp.isCompleted)
        assertEquals(0, rig.attempts)
    }

    @Test
    fun `a failed proof after a short gap waits for another socket and counts 60 s from the last proof`() = runTest {
        val rig = Rig(this)
        runCurrent()
        rig.proof = false
        rig.realMs += 20_000 // slept 20 s
        advanceTimeBy(VoiceSignalling.DEADLINE_CHECK_MS); runCurrent()
        // Not carried since the last proof: "interrupted", no attempt on the unproven socket.
        assertEquals(VoiceLinkState.Interrupted, rig.signalling.state.value)
        assertEquals(0, rig.attempts)
        assertFalse(rig.gaveUp.isCompleted)

        // 60 s from the last proof: about 40 s more of waiting.
        rig.realMs += 35_000
        advanceTimeBy(35_000); runCurrent()
        assertFalse(rig.gaveUp.isCompleted)
        rig.realMs += 6_000
        advanceTimeBy(VoiceSignalling.DEADLINE_CHECK_MS); runCurrent()
        assertTrue(rig.gaveUp.isCompleted)
    }

    @Test
    fun `a failed proof rejoins on the next socket`() = runTest {
        val rig = Rig(this)
        runCurrent()
        rig.proof = false
        rig.realMs += 20_000
        advanceTimeBy(VoiceSignalling.DEADLINE_CHECK_MS); runCurrent()
        rig.socket.value = 2L // the connection manager replaced the dead socket
        runCurrent()
        advanceTimeBy(1_000); runCurrent()
        assertEquals(1, rig.attempts)
        rig.owner.cancel()
    }
}
