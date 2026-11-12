package io.wolftown.kaiku.data.voice

import io.wolftown.kaiku.data.voice.VoiceSignalling.Attempt
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.awaitCancellation
import kotlinx.coroutines.cancelAndJoin
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.launch
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.withTimeoutOrNull
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import java.util.Collections
import java.util.concurrent.atomic.AtomicInteger
import java.util.concurrent.atomic.AtomicLong

/**
 * The signalling machine on real threads: sockets come and go as fast as a
 * thread can set them while the owner runs on a thread pool.
 *
 * Unlike [VoiceSignallingTest] this is not a fixed interleaving; it checks
 * what must hold in every interleaving. A wake-up lost anywhere shows as the
 * machine never arriving on the socket that stays.
 */
class VoiceSignallingThreadsTest {

    private class Run {
        val socket = MutableStateFlow<Long?>(null)

        /** The socket of the last `voice_join` that went out (first join or rejoin attempt). */
        val joinedOn = AtomicLong(0)
        val joins: MutableList<Long> = Collections.synchronizedList(mutableListOf())
        val firstJoins = AtomicInteger(0)
        val gaveUp = AtomicInteger(0)
        private val stepsRunning = AtomicInteger(0)
        val mostStepsAtOnce = AtomicInteger(0)

        // Delays of 1 ms so the machine keeps up with the flapping; the cap is far away.
        val signalling = VoiceSignalling(
            socket = socket,
            delaysMs = listOf(1L, 1L, 1L),
            attemptTimeoutMs = 30_000L,
            offlineCapMs = 120_000L,
        )

        private fun <T> step(block: () -> T): T {
            mostStepsAtOnce.accumulateAndGet(stepsRunning.incrementAndGet(), ::maxOf)
            try {
                return block()
            } finally {
                stepsRunning.decrementAndGet()
            }
        }

        /** Like the real send: the frame goes out only if [id] is the authenticated socket right now. */
        private fun sendJoin(id: Long): Boolean {
            if (socket.value != id) return false
            joins += id
            joinedOn.set(id)
            return true
        }

        val steps = object : VoiceSignalling.Steps {
            override suspend fun prepare() {}

            override suspend fun join(socket: Long): Boolean = step {
                sendJoin(socket).also { if (it) firstJoins.incrementAndGet() }
            }

            // The connections never fail by themselves: every rejoin is a socket change.
            override suspend fun awaitConnectionFailed() = awaitCancellation()

            override suspend fun rejoin(socket: Long): Attempt = step {
                if (sendJoin(socket)) Attempt.Connected else Attempt.SocketGone
            }

            override suspend fun giveUp() {
                gaveUp.incrementAndGet()
            }
        }
    }

    @Test
    fun `whatever the sockets do, the session ends up joined on the socket that stays`() = runBlocking {
        val scope = CoroutineScope(SupervisorJob() + Dispatchers.Default)
        try {
            repeat(40) { round ->
                val run = Run()
                val owner = scope.launch { run.signalling.run(run.steps) }

                // Sockets come and go as fast as this thread can set them, sometimes
                // with a pause that lets the machine get somewhere in between.
                var id = 0L
                repeat(500) { n ->
                    run.socket.value = ++id
                    if ((n + round) % 7 == 0) Thread.sleep(0, 200_000)
                    if (n % 3 != 0) run.socket.value = null
                    if ((n + round) % 11 == 0) Thread.yield()
                }
                // The end: a socket the machine can settle on, then replaced by the
                // socket that stays without any "no socket" in between.
                run.socket.value = ++id
                Thread.sleep(2)
                val last = ++id
                run.socket.value = last

                val arrived = withTimeoutOrNull(30_000) {
                    while (run.joinedOn.get() != last || run.signalling.state.value != VoiceLinkState.Stable) delay(1)
                }
                assertTrue("round $round: stuck on socket ${run.joinedOn.get()}, the socket that stays is $last", arrived != null)

                owner.cancelAndJoin()
                // One step at a time, the first join exactly once, never back to an older
                // socket, never two joins on one socket, and nothing was given up.
                assertEquals(1, run.mostStepsAtOnce.get())
                assertEquals(1, run.firstJoins.get())
                assertEquals("round $round", run.joins.sorted(), run.joins.toList())
                assertEquals("round $round", run.joins.distinct(), run.joins.toList())
                assertEquals(0, run.gaveUp.get())
                assertEquals(VoiceLinkState.Stable, run.signalling.state.value)
            }
        } finally {
            scope.coroutineContext[kotlinx.coroutines.Job]!!.cancelAndJoin()
        }
    }
}
