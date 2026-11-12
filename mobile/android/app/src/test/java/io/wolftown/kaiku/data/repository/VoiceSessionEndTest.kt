package io.wolftown.kaiku.data.repository

import io.wolftown.kaiku.data.voice.VoiceEndNotice
import io.wolftown.kaiku.data.ws.VoiceRemovedReason
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.test.TestScope
import kotlinx.coroutines.test.advanceTimeBy
import kotlinx.coroutines.test.runCurrent
import org.junit.Assert.assertEquals
import org.junit.Test
import org.junit.runner.RunWith
import org.junit.runners.Parameterized

/**
 * Leave and `voice_removed` win at every point of the socket scenarios of
 * [VoiceSocketGatingTest] (contracts C31 and C32): after them nothing is sent
 * and no connection is created for that session, and everything it had is
 * closed. The only frame an own leave may still send is its `voice_leave`,
 * and only to a peer its `voice_join` created on a socket that is still up.
 */
@OptIn(ExperimentalCoroutinesApi::class)
@RunWith(Parameterized::class)
class VoiceSessionEndTest(
    private val scenario: Scenario,
    private val point: Point,
    private val end: End,
) {

    enum class End { Leave, Removed }

    /** Where the session ends: inside a step of the repository's work, or at a time. */
    sealed interface Point {
        data class Step(val name: String, val nth: Int, val what: String) : Point {
            override fun toString() = "$name#$nth ($what)"
        }

        data class Time(val millis: Long, val what: String) : Point {
            override fun toString() = "${millis}ms ($what)"
        }
    }

    enum class Scenario(val points: List<Point>, internal val run: suspend TestScope.(VoiceRig) -> Unit) {
        /** Socket 1 is lost at the join send; socket 2 is there before the retry looks. */
        LostAtTheSend(
            points = listOf(
                Point.Step("sub:create", 1, "setting up, no socket used yet"),
                Point.Step("join:check", 1, "socket 1 just dropped"),
                Point.Step("join:refused", 1, "socket 2 is up, the retry has not looked yet"),
                Point.Step("join:sent", 1, "voice_join went out on socket 2"),
                Point.Step("pub:create", 1, "creating the publisher"),
                Point.Time(1_000, "call is up"),
            ),
            run = { rig ->
                rig.socket.ready()
                rig.at("join:check") { rig.socket.drop() }
                rig.at("join:refused") { rig.socket.ready() }
                rig.repository.startJoin(rig.channel)
                runCurrent()
                advanceTimeBy(5_000); runCurrent()
            },
        ),

        /** Five sockets die at the send within 2 s while the first join is held; the sixth stays. */
        FlappingWhileHeld(
            points = listOf(
                Point.Step("join:check", 1, "socket 1 just dropped"),
                Point.Time(200, "held, no socket"),
                Point.Step("join:check", 3, "socket 3 just dropped"),
                Point.Time(1_800, "held, no socket, after five sockets"),
                Point.Step("socket:ready", 6, "socket 6 is up, nothing has looked yet"),
                Point.Step("join:sent", 1, "voice_join went out on socket 6"),
                Point.Step("pub:create", 1, "creating the publisher"),
                Point.Time(3_000, "call is up"),
            ),
            run = { rig ->
                rig.repository.startJoin(rig.channel)
                runCurrent()
                for (n in 1..5) rig.at("join:check", nth = n) { rig.socket.drop() }
                repeat(5) {
                    rig.socket.ready(); runCurrent()
                    advanceTimeBy(400); runCurrent()
                }
                rig.socket.ready(); runCurrent()
                advanceTimeBy(5_000); runCurrent()
            },
        ),

        /** A rejoin attempt is in flight on socket 2 when five flaps go by unobserved; socket 7 stays. */
        FlappingDuringRejoin(
            points = listOf(
                Point.Time(500, "waiting out the rejoin delay on socket 2"),
                Point.Step("close", 1, "the attempt discards the old connections"),
                Point.Step("sub:create", 2, "the attempt creates its subscriber"),
                Point.Step("join:sent", 2, "voice_join went out on socket 2, five flaps, socket 7 is up"),
                Point.Step("pub:create", 2, "the attempt creates its publisher, its socket is gone"),
                Point.Step("close", 2, "the cancelled attempt closes its connections"),
                Point.Time(1_500, "waiting out the rejoin delay on socket 7"),
                Point.Step("sub:create", 3, "the next attempt creates its subscriber"),
                Point.Step("join:sent", 3, "voice_join went out on socket 7"),
                Point.Step("pub:create", 3, "the next attempt creates its publisher"),
                Point.Time(4_000, "call is up again"),
            ),
            run = { rig ->
                rig.joinAndConnect()
                rig.socket.drop(); runCurrent()
                rig.socket.ready(); runCurrent()
                rig.at("join:sent", nth = 2) { repeat(5) { rig.socket.drop(); rig.socket.ready() } }
                advanceTimeBy(1_000); runCurrent()
                advanceTimeBy(1_000); runCurrent()
                advanceTimeBy(5_000); runCurrent()
            },
        ),
    }

    companion object {
        @JvmStatic
        @Parameterized.Parameters(name = "{0} at {1}: {2}")
        fun cases(): List<Array<Any>> = Scenario.entries.flatMap { scenario ->
            scenario.points.flatMap { point -> End.entries.map { end -> arrayOf(scenario, point, end) } }
        }
    }

    @Test
    fun `the session end wins and nothing follows it`() = rigTest { rig ->
        val endNow: () -> Unit = when (end) {
            End.Leave -> rig::leave
            End.Removed -> rig::removed
        }
        when (val p = point) {
            is Point.Time -> rig.atTime(p.millis, endNow)
            // After what the scenario itself does to the socket in that step.
            is Point.Step -> rig.atLast(p.name, p.nth, endNow)
        }

        scenario.run.invoke(this, rig)
        advanceTimeBy(180_000); runCurrent()

        val marker = if (end == End.Leave) "LEAVE" else "REMOVED"
        rig.assertEndedAt(marker, ownLeave = end == End.Leave)
        assertEquals(
            if (end == End.Removed) VoiceEndNotice.Removed(rig.channel, VoiceRemovedReason.Kicked) else null,
            rig.repository.endNotice.value,
        )
    }
}
