package io.wolftown.kaiku.data.voice

import io.wolftown.kaiku.data.voice.VoiceSignalling.Attempt
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.Job
import kotlinx.coroutines.NonCancellable
import kotlinx.coroutines.awaitCancellation
import kotlinx.coroutines.cancelAndJoin
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.launch
import kotlinx.coroutines.test.TestScope
import kotlinx.coroutines.test.advanceTimeBy
import kotlinx.coroutines.test.currentTime
import kotlinx.coroutines.test.runCurrent
import kotlinx.coroutines.test.runTest
import kotlinx.coroutines.withContext
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * The voice signalling state machine (C32 with its amendments) on virtual
 * time. No Android, no WebRTC: the socket is a state the test sets, the steps
 * are recorded.
 */
@OptIn(ExperimentalCoroutinesApi::class)
class VoiceSignallingTest {

    private companion object {
        val HANG = Any()
    }

    private class Harness(private val scope: TestScope, socket: Long?) {
        /** The id of the authenticated socket; the test sets it. */
        val socket = MutableStateFlow(socket)

        /** Whether the current connections reported ICE FAILED; a new attempt makes new ones. */
        val connectionFailed = MutableStateFlow(false)

        /** The socket that carried the last `voice_join` (first join or attempt), as the machine reads it. */
        val joinSocket = MutableStateFlow<Long?>(null)

        val joinSockets = mutableListOf<Long>()
        val attemptTimes = mutableListOf<Long>()
        val attemptSockets = mutableListOf<Long>()
        val statesAtGiveUp = mutableListOf<VoiceLinkState>()

        /** Result of each attempt in order ([Attempt], HANG, or a Throwable to throw); missing = Failed. */
        val results = ArrayDeque<Any>()

        /** Sockets on which the first join's send is refused (as if they were closing). */
        val refusing = mutableSetOf<Long>()
        var prepare: suspend () -> Unit = {}
        var beforeJoin: (Long) -> Unit = {}

        /** Runs right after an attempt's voice_join went out, before anything else can look. */
        var afterAttemptJoin: (Long) -> Unit = {}

        val signalling = VoiceSignalling(this.socket)
        val state get() = signalling.state.value
        lateinit var owner: Job

        /** What ended the machine, if a step's error did. */
        var failure: Exception? = null

        private val steps = object : VoiceSignalling.Steps {
            override val joinSocket get() = this@Harness.joinSocket

            override suspend fun prepare() = this@Harness.prepare()

            override suspend fun join(socket: Long): Boolean {
                beforeJoin(socket)
                // Like the real send: only on the socket that is authenticated now.
                if (this@Harness.socket.value != socket || socket in refusing) return false
                joinSockets += socket
                this@Harness.joinSocket.value = socket
                return true
            }

            override suspend fun awaitConnectionFailed() {
                connectionFailed.first { it }
            }

            override suspend fun rejoin(socket: Long): Attempt {
                connectionFailed.value = false
                attemptTimes += scope.currentTime
                attemptSockets += socket
                val result = results.removeFirstOrNull() ?: Attempt.Failed
                // An attempt that throws does so before its voice_join; one that
                // found its socket gone sent none. Every other one sent it.
                if (result !is Throwable && result != Attempt.SocketGone) {
                    this@Harness.joinSocket.value = socket
                    afterAttemptJoin(socket)
                }
                return when (result) {
                    HANG -> awaitCancellation()
                    is Throwable -> throw result
                    else -> result as Attempt
                }
            }

            override suspend fun giveUp() {
                statesAtGiveUp += signalling.state.value
            }
        }

        /** Starts the owner; with a socket there, the first join goes out at once. */
        fun start() {
            owner = scope.backgroundScope.launch {
                try {
                    signalling.run(steps)
                } catch (e: CancellationException) {
                    throw e
                } catch (e: Exception) {
                    failure = e
                }
            }
            scope.runCurrent()
        }

        /** The socket is lost and another one is authenticated. */
        fun reconnect(next: Long) {
            socket.value = null
            scope.runCurrent()
            socket.value = next
            scope.runCurrent()
        }
    }

    private fun TestScope.joined() = Harness(this, socket = 1L).also {
        it.start()
        assertEquals(listOf(1L), it.joinSockets)
        assertEquals(VoiceLinkState.Stable, it.state)
    }

    // -- First join -------------------------------------------------------------

    @Test
    fun `the first join is held while no socket is authenticated and goes out once on the first one`() = runTest {
        val h = Harness(this, socket = null)
        h.start()
        advanceTimeBy(30_000); runCurrent()
        assertTrue(h.joinSockets.isEmpty())
        assertEquals(VoiceLinkState.Interrupted, h.state)

        h.socket.value = 1L
        runCurrent()

        assertEquals(listOf(1L), h.joinSockets)
        assertEquals(VoiceLinkState.Stable, h.state)
        advanceTimeBy(600_000); runCurrent()
        assertEquals(listOf(1L), h.joinSockets)
        assertTrue(h.attemptTimes.isEmpty())
        assertTrue(h.statesAtGiveUp.isEmpty())
    }

    @Test
    fun `a socket replaced between the look and the send gets the join on the new socket at once`() = runTest {
        val h = Harness(this, socket = 1L)
        // Socket 1 is lost and socket 2 authenticated after the machine read "1".
        h.beforeJoin = { if (it == 1L) h.socket.value = 2L }
        h.start()

        assertEquals(listOf(2L), h.joinSockets)
        assertEquals(VoiceLinkState.Stable, h.state)
        advanceTimeBy(600_000); runCurrent()
        assertEquals(listOf(2L), h.joinSockets)
        assertTrue(h.attemptTimes.isEmpty())
    }

    @Test
    fun `a socket that refuses the join is not tried again and the next socket gets it`() = runTest {
        val h = Harness(this, socket = 1L)
        // Socket 1 is closing but its close has not been reported yet.
        h.refusing += 1L
        var asked = 0
        h.beforeJoin = { asked++ }
        h.start()
        advanceTimeBy(10_000); runCurrent()
        assertEquals(1, asked)
        assertTrue(h.joinSockets.isEmpty())

        h.socket.value = null
        runCurrent()
        h.socket.value = 2L
        runCurrent()

        assertEquals(2, asked)
        assertEquals(listOf(2L), h.joinSockets)
    }

    @Test
    fun `a failing set-up ends the machine with its error and without a give-up`() = runTest {
        val h = Harness(this, socket = 1L)
        h.prepare = { error("no ICE servers") }
        h.start()

        assertEquals("no ICE servers", h.failure?.message)
        assertTrue(h.owner.isCompleted)
        assertTrue(h.joinSockets.isEmpty())
        assertTrue(h.statesAtGiveUp.isEmpty())
        assertEquals(VoiceLinkState.Stable, h.state)
    }

    // -- Rejoin: schedule, budget, timeout --------------------------------------

    @Test
    fun `another socket rejoins after 1 s and ends stable on success`() = runTest {
        val h = joined()
        h.results += Attempt.Connected

        h.socket.value = null
        runCurrent()
        assertEquals(VoiceLinkState.Interrupted, h.state)
        h.socket.value = 2L
        runCurrent()
        assertEquals(VoiceLinkState.Rejoining(1), h.state)

        advanceTimeBy(999); runCurrent()
        assertTrue(h.attemptTimes.isEmpty())
        advanceTimeBy(1); runCurrent()

        assertEquals(listOf(1_000L), h.attemptTimes)
        assertEquals(listOf(2L), h.attemptSockets)
        assertEquals(VoiceLinkState.Stable, h.state)
        assertTrue(h.statesAtGiveUp.isEmpty())
    }

    @Test
    fun `a socket replaced between two looks is seen as another socket`() = runTest {
        val h = joined()
        h.results += Attempt.Connected

        // Lost and replaced before anything could observe "no socket".
        h.socket.value = null
        h.socket.value = 2L
        runCurrent()
        assertEquals(VoiceLinkState.Rejoining(1), h.state)
        advanceTimeBy(1_000); runCurrent()

        assertEquals(listOf(2L), h.attemptSockets)
        assertEquals(VoiceLinkState.Stable, h.state)
    }

    @Test
    fun `three failed attempts at 1 s, 3 s and 9 s then give up exactly once`() = runTest {
        val h = joined()
        h.reconnect(2L)

        advanceTimeBy(1_000); runCurrent()
        assertEquals(VoiceLinkState.Rejoining(2), h.state)
        advanceTimeBy(3_000); runCurrent()
        assertEquals(VoiceLinkState.Rejoining(3), h.state)
        assertTrue(h.statesAtGiveUp.isEmpty())
        advanceTimeBy(9_000); runCurrent()

        assertEquals(listOf(1_000L, 4_000L, 13_000L), h.attemptTimes)
        // The state says "reconnecting" until the give-up teardown ran.
        assertEquals(listOf<VoiceLinkState>(VoiceLinkState.Rejoining(3)), h.statesAtGiveUp)
        assertEquals(VoiceLinkState.Stable, h.state)
        assertTrue(h.owner.isCompleted)

        // Nothing re-arms by itself.
        h.reconnect(3L)
        advanceTimeBy(60_000); runCurrent()
        assertEquals(3, h.attemptTimes.size)
        assertEquals(1, h.statesAtGiveUp.size)
    }

    @Test
    fun `success on the second attempt stops the attempts and the next trouble starts a fresh budget`() = runTest {
        val h = joined()
        h.results += Attempt.Failed
        h.results += Attempt.Connected
        h.reconnect(2L)
        advanceTimeBy(60_000); runCurrent()

        assertEquals(listOf(1_000L, 4_000L), h.attemptTimes)
        assertTrue(h.statesAtGiveUp.isEmpty())
        assertEquals(VoiceLinkState.Stable, h.state)

        // Another loss: attempt 1 again, after 1 s.
        h.results += Attempt.Connected
        h.reconnect(3L)
        assertEquals(VoiceLinkState.Rejoining(1), h.state)
        advanceTimeBy(1_000); runCurrent()
        assertEquals(listOf(1_000L, 4_000L, 61_000L), h.attemptTimes)
        assertEquals(VoiceLinkState.Stable, h.state)
    }

    @Test
    fun `an attempt that never answers fails after the 15 s attempt timeout`() = runTest {
        val h = joined()
        h.results += HANG
        h.results += Attempt.Connected
        h.reconnect(2L)

        advanceTimeBy(60_000); runCurrent()

        // 1 s delay + 15 s timeout, then the 3 s delay of attempt 2.
        assertEquals(listOf(1_000L, 19_000L), h.attemptTimes)
        assertEquals(VoiceLinkState.Stable, h.state)
    }

    @Test
    fun `the attempt timeout cancels the attempt at 16 s, before the next one starts`() = runTest {
        val cancelledAt = mutableListOf<Long>()
        val starts = mutableListOf<Long>()
        var calls = 0
        val socket = MutableStateFlow<Long?>(1L)
        val signalling = VoiceSignalling(socket)
        val steps = object : VoiceSignalling.Steps {
            override val joinSocket = MutableStateFlow<Long?>(null)
            override suspend fun prepare() {}
            override suspend fun join(socket: Long) = true.also { joinSocket.value = socket }
            override suspend fun awaitConnectionFailed() = awaitCancellation()
            override suspend fun rejoin(socket: Long): Attempt {
                joinSocket.value = socket
                starts += currentTime
                if (++calls == 1) {
                    try { awaitCancellation() } finally { cancelledAt += currentTime }
                }
                return Attempt.Connected
            }
            override suspend fun giveUp() {}
        }
        backgroundScope.launch { signalling.run(steps) }
        runCurrent()
        socket.value = 2L
        runCurrent()

        advanceTimeBy(60_000); runCurrent()

        assertEquals(listOf(16_000L), cancelledAt)
        assertEquals(listOf(1_000L, 19_000L), starts)
        assertEquals(VoiceLinkState.Stable, signalling.state.value)
    }

    @Test
    fun `an attempt that throws counts as a failed attempt`() = runTest {
        val h = joined()
        h.results += IllegalStateException("boom")
        // A step cancelled from inside, not by the machine: a failure too, not the end of the owner.
        h.results += CancellationException("stray")
        h.results += Attempt.Connected
        h.reconnect(2L)

        advanceTimeBy(60_000); runCurrent()

        assertEquals(listOf(1_000L, 4_000L, 13_000L), h.attemptTimes)
        assertEquals(VoiceLinkState.Stable, h.state)
        assertTrue(h.statesAtGiveUp.isEmpty())
        assertFalse(h.owner.isCompleted)
    }

    // -- Rejoin: socket loss never uses up an attempt -----------------------------

    @Test
    fun `a socket that drops during the delay makes the attempt wait without using it up`() = runTest {
        val h = joined()
        h.results += Attempt.Connected
        h.reconnect(2L)
        advanceTimeBy(100); runCurrent()

        h.socket.value = null
        runCurrent()
        advanceTimeBy(30_000); runCurrent()
        assertTrue(h.attemptTimes.isEmpty())
        assertTrue(h.statesAtGiveUp.isEmpty())
        assertEquals(VoiceLinkState.Rejoining(1), h.state)

        // The delay starts again with the new socket (time for the text re-subscription).
        h.socket.value = 3L
        runCurrent()
        advanceTimeBy(1_000); runCurrent()
        assertEquals(listOf(31_100L), h.attemptTimes)
        assertEquals(listOf(3L), h.attemptSockets)
        assertEquals(VoiceLinkState.Stable, h.state)
    }

    @Test
    fun `socket loss during attempt 3 cancels it without using it up`() = runTest {
        val h = joined()
        h.results += Attempt.Failed
        h.results += Attempt.Failed
        h.results += HANG
        h.results += Attempt.Connected
        h.reconnect(2L)
        advanceTimeBy(13_000); runCurrent()
        assertEquals(listOf(1_000L, 4_000L, 13_000L), h.attemptTimes)

        advanceTimeBy(1_000); runCurrent()
        h.socket.value = null
        runCurrent()
        // Attempt 3 would have timed out at 28 s; nothing gives up.
        advanceTimeBy(30_000); runCurrent()
        assertTrue(h.statesAtGiveUp.isEmpty())
        assertEquals(VoiceLinkState.Rejoining(3), h.state)

        h.socket.value = 3L // at 44 s
        runCurrent()
        advanceTimeBy(9_000); runCurrent()

        // Attempt 3 runs again with its own delay after the new socket.
        assertEquals(listOf(1_000L, 4_000L, 13_000L, 53_000L), h.attemptTimes)
        assertEquals(VoiceLinkState.Stable, h.state)
        assertTrue(h.statesAtGiveUp.isEmpty())
    }

    @Test
    fun `an attempt whose voice_join found its socket gone is not used up and waits for another socket`() = runTest {
        val h = joined()
        // The send was refused although the socket still looked authenticated.
        h.results += Attempt.SocketGone
        h.results += Attempt.Connected
        h.reconnect(2L)
        advanceTimeBy(1_000); runCurrent()
        assertEquals(listOf(2L), h.attemptSockets)
        assertEquals(VoiceLinkState.Rejoining(1), h.state)

        // Not again on socket 2, however long it still looks authenticated.
        advanceTimeBy(30_000); runCurrent()
        assertEquals(listOf(2L), h.attemptSockets)

        h.socket.value = 3L
        runCurrent()
        advanceTimeBy(1_000); runCurrent()
        assertEquals(listOf(2L, 3L), h.attemptSockets)
        assertEquals(VoiceLinkState.Stable, h.state)
    }

    @Test
    fun `sockets that change during a rejoin never run two attempts at once`() = runTest {
        val h = joined()
        h.results += HANG
        h.results += Attempt.Connected
        h.reconnect(2L)
        advanceTimeBy(1_000); runCurrent() // attempt 1 hangs on socket 2
        h.socket.value = 3L
        runCurrent()
        h.socket.value = 4L
        runCurrent()

        advanceTimeBy(60_000); runCurrent()

        // One on socket 2 (cancelled), then one on socket 4, 1 s after it came.
        assertEquals(listOf(2L, 4L), h.attemptSockets)
        assertEquals(listOf(1_000L, 2_000L), h.attemptTimes)
        assertEquals(VoiceLinkState.Stable, h.state)
    }

    // -- ICE ---------------------------------------------------------------------

    @Test
    fun `ice disconnected only shows reconnecting and does not rejoin`() = runTest {
        val h = joined()

        h.signalling.onIceDisconnected()
        assertEquals(VoiceLinkState.Interrupted, h.state)
        advanceTimeBy(60_000); runCurrent()
        assertTrue(h.attemptTimes.isEmpty())

        h.signalling.onIceConnected()
        assertEquals(VoiceLinkState.Stable, h.state)
    }

    @Test
    fun `failed connections on a live socket rejoin on that socket`() = runTest {
        val h = joined()
        h.results += Attempt.Connected

        h.connectionFailed.value = true
        runCurrent()
        assertEquals(VoiceLinkState.Rejoining(1), h.state)
        advanceTimeBy(1_000); runCurrent()

        assertEquals(listOf(1_000L), h.attemptTimes)
        assertEquals(listOf(1L), h.attemptSockets)
        assertEquals(VoiceLinkState.Stable, h.state)
    }

    @Test
    fun `failed connections while the socket is down wait for the socket`() = runTest {
        val h = joined()
        h.results += Attempt.Connected
        h.socket.value = null
        runCurrent()

        h.connectionFailed.value = true
        advanceTimeBy(30_000); runCurrent()
        assertTrue(h.attemptTimes.isEmpty())
        assertEquals(VoiceLinkState.Interrupted, h.state)

        h.socket.value = 2L
        runCurrent()
        advanceTimeBy(1_000); runCurrent()
        assertEquals(listOf(2L), h.attemptSockets)
    }

    // -- The 60 s without a socket that carries the join ---------------------------
    //
    // They run from the loss of the socket that carried the voice_join (for a
    // first join: from the start) until a voice_join went out on an
    // authenticated socket. A socket that only authenticates changes nothing.

    @Test
    fun `60 s without an authenticated socket gives up`() = runTest {
        val h = joined()
        h.socket.value = null
        runCurrent()

        advanceTimeBy(59_999); runCurrent()
        assertTrue(h.statesAtGiveUp.isEmpty())
        assertEquals(VoiceLinkState.Interrupted, h.state)
        advanceTimeBy(1); runCurrent()

        assertEquals(listOf<VoiceLinkState>(VoiceLinkState.Interrupted), h.statesAtGiveUp)
        assertEquals(VoiceLinkState.Stable, h.state)
        h.socket.value = 2L
        advanceTimeBy(60_000); runCurrent()
        assertTrue(h.attemptTimes.isEmpty())
    }

    @Test
    fun `a socket that only authenticates does not stop the 60 s`() = runTest {
        val h = joined()
        h.socket.value = null
        runCurrent()
        advanceTimeBy(59_000); runCurrent()
        // Up for half a second: too short for the 1 s before the rejoin, so
        // it never carries a voice_join.
        h.socket.value = 2L
        runCurrent()
        advanceTimeBy(500); runCurrent()
        h.socket.value = null
        runCurrent()

        advanceTimeBy(499); runCurrent() // 59 999 ms after the loss of socket 1
        assertTrue(h.statesAtGiveUp.isEmpty())
        advanceTimeBy(1); runCurrent()

        assertEquals(listOf<VoiceLinkState>(VoiceLinkState.Rejoining(1)), h.statesAtGiveUp)
        assertTrue(h.attemptTimes.isEmpty())
    }

    @Test
    fun `sockets that come and go for a minute never stop the 60 s`() = runTest {
        val h = joined()
        h.socket.value = null
        runCurrent()
        // A new socket every 1.5 s, each up for half a second.
        var id = 1L
        repeat(39) {
            advanceTimeBy(1_000); runCurrent()
            h.socket.value = ++id
            runCurrent()
            advanceTimeBy(500); runCurrent()
            h.socket.value = null
            runCurrent()
        }

        advanceTimeBy(1_499); runCurrent()
        assertTrue(h.statesAtGiveUp.isEmpty())
        advanceTimeBy(1); runCurrent()

        assertEquals(1, h.statesAtGiveUp.size)
        assertTrue(h.attemptTimes.isEmpty())
        assertTrue(h.owner.isCompleted)
    }

    @Test
    fun `a voice_join carried within the 60 s ends them`() = runTest {
        val h = joined()
        h.results += Attempt.Connected
        h.socket.value = null
        runCurrent()
        advanceTimeBy(58_000); runCurrent()
        h.socket.value = 2L
        runCurrent()
        advanceTimeBy(1_000); runCurrent() // the attempt's voice_join goes out at 59 s

        assertEquals(listOf(59_000L), h.attemptTimes)
        advanceTimeBy(600_000); runCurrent()
        assertTrue(h.statesAtGiveUp.isEmpty())
        assertEquals(VoiceLinkState.Stable, h.state)
    }

    @Test
    fun `a socket that authenticates too late for its voice_join does not save the session`() = runTest {
        val h = joined()
        h.results += Attempt.Connected
        h.socket.value = null
        runCurrent()
        advanceTimeBy(59_500); runCurrent()
        // Its voice_join would be due at 60.5 s: the 1 s before an attempt still applies.
        h.socket.value = 2L
        runCurrent()

        advanceTimeBy(499); runCurrent()
        assertTrue(h.statesAtGiveUp.isEmpty())
        advanceTimeBy(1); runCurrent()

        assertEquals(listOf<VoiceLinkState>(VoiceLinkState.Rejoining(1)), h.statesAtGiveUp)
        assertTrue(h.attemptTimes.isEmpty())
    }

    @Test
    fun `once the join is carried a slow attempt is a matter of its 15 s, not of the 60 s`() = runTest {
        val h = joined()
        h.results += HANG
        h.results += Attempt.Connected
        h.socket.value = null
        runCurrent()
        advanceTimeBy(50_000); runCurrent()
        h.socket.value = 2L
        runCurrent()

        // Attempt 1 sends its voice_join at 51 s and hangs until 66 s, past the 60 s.
        advanceTimeBy(120_000); runCurrent()

        assertEquals(listOf(51_000L, 69_000L), h.attemptTimes)
        assertTrue(h.statesAtGiveUp.isEmpty())
        assertEquals(VoiceLinkState.Stable, h.state)
    }

    @Test
    fun `a carried voice_join whose socket is lost again starts new 60 s from that loss`() = runTest {
        val h = joined()
        h.results += HANG
        h.socket.value = null
        runCurrent() // first loss at 0 s
        advanceTimeBy(9_000); runCurrent()
        h.socket.value = 2L
        runCurrent()
        advanceTimeBy(1_000); runCurrent() // voice_join carried at 10 s
        assertEquals(listOf(10_000L), h.attemptTimes)
        advanceTimeBy(10_000); runCurrent()
        h.socket.value = null
        runCurrent() // the carrying socket is lost at 20 s

        // Sockets that only authenticate, at 30 s and at 70 s.
        advanceTimeBy(10_000); runCurrent()
        h.socket.value = 3L
        runCurrent()
        advanceTimeBy(500); runCurrent()
        h.socket.value = null
        runCurrent()
        advanceTimeBy(39_500); runCurrent()
        h.socket.value = 4L
        runCurrent()
        advanceTimeBy(500); runCurrent()
        h.socket.value = null
        runCurrent()

        advanceTimeBy(9_499); runCurrent() // 79 999 ms
        assertTrue(h.statesAtGiveUp.isEmpty())
        advanceTimeBy(1); runCurrent() // 80 s = 20 s + 60 s

        assertEquals(1, h.statesAtGiveUp.size)
        assertEquals(listOf(10_000L), h.attemptTimes)
    }

    @Test
    fun `a carried voice_join whose socket is lost before anyone looked still starts new 60 s`() = runTest {
        val h = joined()
        h.results += HANG
        // The attempt's voice_join goes out and its socket is lost in the same breath.
        h.afterAttemptJoin = { h.socket.value = null }
        h.socket.value = null
        runCurrent() // first loss at 0 s
        advanceTimeBy(30_000); runCurrent()
        h.socket.value = 2L
        runCurrent()
        advanceTimeBy(1_000); runCurrent() // carried and lost at 31 s
        assertEquals(listOf(31_000L), h.attemptTimes)

        advanceTimeBy(59_999); runCurrent() // 90 999 ms: the first 60 s are long over
        assertTrue(h.statesAtGiveUp.isEmpty())
        advanceTimeBy(1); runCurrent() // 91 s = 31 s + 60 s

        assertEquals(1, h.statesAtGiveUp.size)
    }

    @Test
    fun `a rejoin still waiting 60 s after the loss is given up once`() = runTest {
        val h = joined()
        h.reconnect(2L)
        advanceTimeBy(500); runCurrent()
        h.socket.value = null
        runCurrent()

        advanceTimeBy(59_499); runCurrent() // 59 999 ms after the loss of socket 1
        assertTrue(h.statesAtGiveUp.isEmpty())
        advanceTimeBy(1); runCurrent()

        assertEquals(listOf<VoiceLinkState>(VoiceLinkState.Rejoining(1)), h.statesAtGiveUp)
        assertTrue(h.attemptTimes.isEmpty())
        advanceTimeBy(120_000); runCurrent()
        assertEquals(1, h.statesAtGiveUp.size)
    }

    @Test
    fun `a socket that refuses the join does not stop the 60 s`() = runTest {
        val h = Harness(this, socket = 1L)
        // Socket 1 is closing; its close is never reported.
        h.refusing += 1L
        h.start()
        assertEquals(VoiceLinkState.Interrupted, h.state)

        advanceTimeBy(59_999); runCurrent()
        assertTrue(h.statesAtGiveUp.isEmpty())
        advanceTimeBy(1); runCurrent()

        assertEquals(1, h.statesAtGiveUp.size)
        assertTrue(h.joinSockets.isEmpty())
    }

    @Test
    fun `a first join held for 60 s gives up without a join and one held for 59 s goes out`() = runTest {
        val gaveUp = Harness(this, socket = null)
        gaveUp.start()
        advanceTimeBy(59_999); runCurrent()
        assertTrue(gaveUp.statesAtGiveUp.isEmpty())
        advanceTimeBy(1); runCurrent()
        assertEquals(listOf<VoiceLinkState>(VoiceLinkState.Interrupted), gaveUp.statesAtGiveUp)
        assertTrue(gaveUp.joinSockets.isEmpty())
        assertEquals(VoiceLinkState.Stable, gaveUp.state)

        val joined = Harness(this, socket = null)
        joined.start()
        advanceTimeBy(59_000); runCurrent()
        joined.socket.value = 1L
        advanceTimeBy(600_000); runCurrent()
        assertEquals(listOf(1L), joined.joinSockets)
        assertTrue(joined.statesAtGiveUp.isEmpty())
    }

    @Test
    fun `a first join that no socket ever carries gives up 60 s after it started`() = runTest {
        val h = Harness(this, socket = null)
        // Every socket is lost between the machine's look and the send.
        h.beforeJoin = { h.socket.value = null }
        h.start()
        var id = 0L
        repeat(39) {
            advanceTimeBy(1_500); runCurrent()
            h.socket.value = ++id
            runCurrent()
        }

        advanceTimeBy(1_499); runCurrent() // 59 999 ms after the start
        assertTrue(h.statesAtGiveUp.isEmpty())
        advanceTimeBy(1); runCurrent()

        assertEquals(1, h.statesAtGiveUp.size)
        assertTrue(h.joinSockets.isEmpty())
    }

    @Test
    fun `the 60 s also run while the first join is still setting up`() = runTest {
        val h = Harness(this, socket = null)
        var cancelledAt = -1L
        h.prepare = {
            try { awaitCancellation() } finally { cancelledAt = currentTime }
        }
        h.start()
        // Sockets that come and go meanwhile change nothing.
        advanceTimeBy(30_000); runCurrent()
        h.socket.value = 1L
        runCurrent()
        advanceTimeBy(20_000); runCurrent()
        h.socket.value = null
        runCurrent()

        advanceTimeBy(10_000); runCurrent()

        assertEquals(60_000L, cancelledAt)
        assertEquals(1, h.statesAtGiveUp.size)
        assertTrue(h.joinSockets.isEmpty())
    }

    // -- The owner's end -------------------------------------------------------------

    @Test
    fun `cancelling the owner during the delay cancels the pending rejoin`() = runTest {
        val h = joined()
        h.reconnect(2L)
        advanceTimeBy(500); runCurrent()

        h.owner.cancel()
        runCurrent()
        assertEquals(VoiceLinkState.Stable, h.state)
        advanceTimeBy(60_000); runCurrent()

        assertTrue(h.attemptTimes.isEmpty())
        assertTrue(h.statesAtGiveUp.isEmpty())
    }

    @Test
    fun `cancelling the owner during an attempt cancels it and nothing follows whatever the socket does`() = runTest {
        val h = joined()
        h.results += HANG
        h.reconnect(2L)
        advanceTimeBy(1_000); runCurrent()
        assertEquals(1, h.attemptTimes.size)

        h.owner.cancel()
        runCurrent()
        h.reconnect(3L)
        h.connectionFailed.value = true
        advanceTimeBy(120_000); runCurrent()

        assertEquals(1, h.attemptTimes.size)
        assertTrue(h.statesAtGiveUp.isEmpty())
        assertEquals(VoiceLinkState.Stable, h.state)
    }

    @Test
    fun `cancelAndJoin of the owner returns only after the running attempt finished cancelling`() = runTest {
        var cleanedAt = -1L
        val socket = MutableStateFlow<Long?>(1L)
        val signalling = VoiceSignalling(socket)
        val steps = object : VoiceSignalling.Steps {
            override val joinSocket = MutableStateFlow<Long?>(null)
            override suspend fun prepare() {}
            override suspend fun join(socket: Long) = true.also { joinSocket.value = socket }
            override suspend fun awaitConnectionFailed() = awaitCancellation()
            override suspend fun rejoin(socket: Long): Attempt {
                joinSocket.value = socket
                try {
                    awaitCancellation()
                } finally {
                    // Clean-up that takes time even when cancelled (closing connections).
                    withContext(NonCancellable) { delay(500) }
                    cleanedAt = currentTime
                }
            }
            override suspend fun giveUp() {}
        }
        val owner = backgroundScope.launch { signalling.run(steps) }
        runCurrent()
        socket.value = 2L
        runCurrent()
        advanceTimeBy(2_000); runCurrent()

        var endedAt = -1L
        backgroundScope.launch { owner.cancelAndJoin(); endedAt = currentTime }
        runCurrent()
        assertEquals(-1L, endedAt)
        advanceTimeBy(500); runCurrent()

        assertEquals(2_500L, cleanedAt)
        assertEquals(2_500L, endedAt)
        assertEquals(VoiceLinkState.Stable, signalling.state.value)
    }
}
