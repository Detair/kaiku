package io.wolftown.kaiku.data.call

import io.wolftown.kaiku.data.ws.ServerEvent
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.test.TestScope
import kotlinx.coroutines.test.advanceTimeBy
import kotlinx.coroutines.test.runCurrent
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * The call owner against the two things that come from outside its machine:
 *
 * - **The signed-in session changing at any step** (sign-out, then another
 *   account signs in). Whatever the old session still had coming (answers of
 *   requests that are out, timers, its voice's end, events on its socket,
 *   taps on its notifications) arrives afterwards: nothing of it runs,
 *   sends, publishes or posts.
 * - **A ring arriving by push, by socket or by both**, and its end the same
 *   ways: the same call state and the same notifications in every case.
 */
@OptIn(ExperimentalCoroutinesApi::class)
class CallSessionFuzzTest {

    private val dm = "dm-1"
    private val ref = CallRef("me", dm, "c-1")

    private class Step(val name: String, val run: suspend CallRig.() -> Unit)

    private fun step(name: String, run: suspend CallRig.() -> Unit) = Step(name, run)

    /** Call flows with every request held until a step lets its answer through. */
    private val scenarios: Map<String, List<Step>> = mapOf(
        "outgoing, answered, hung up" to listOf(
            step("tap Call") { controller.submit(CallInput.Start("me", dm)) },
            step("start answered") { api.release() },
            step("the other side answers") { server(ServerEvent.CallParticipantJoined(dm, "c-1", "anna", "anna")) },
            step("mute") { controller.submit(CallInput.SetMuted(ref, true)) },
            step("hang up") { controller.submit(CallInput.HangUp(ref)) },
            step("leave answered") { api.release() },
        ),
        "incoming, accepted, the other side leaves" to listOf(
            step("ring") { ring("c-1") },
            step("a second call rings meanwhile") { server(ServerEvent.IncomingCall("dm-2", "c-7", "bob", "bob")) },
            step("accept") { controller.submit(CallInput.Accept(ref)) },
            step("join answered") { api.release() },
            step("the other side leaves") { server(ServerEvent.CallParticipantLeft(dm, "c-1", "anna")) },
            step("leave answered") { api.release() },
        ),
        "incoming, declined from the notification" to listOf(
            step("ring by push") { pushRing("c-1") },
            step("decline") { controller.submit(CallInput.Decline(ref)) },
            step("decline answered") { api.release() },
        ),
        "a ring from a notification, accepted" to listOf(
            step("screen opened from the notification") {
                serverRings("c-1")
                ringNotification("c-1")
            },
            step("check answered") { api.release() },
            step("accept") { controller.submit(CallInput.Accept(ref)) },
            step("join answered") { api.release() },
            step("voice lost") { callEnds.tryEmit(CallVoiceEnd(tag, VoiceEnd.Lost)) },
        ),
        "a ring nobody answers" to listOf(
            step("ring") { ring("c-1") },
            step("100 s pass") { test.advanceTimeBy(CallMachine.RING_CAP_MS + CallController.CAP_CHECK_MS) },
        ),
        "outgoing, never answered" to listOf(
            step("tap Call") { controller.submit(CallInput.Start("me", dm)) },
            step("start answered") { api.release() },
            step("100 s pass") { test.advanceTimeBy(CallMachine.RING_CAP_MS + CallController.CAP_CHECK_MS) },
            step("leave answered") { api.release() },
        ),
    )

    private suspend fun TestScope.play(rig: CallRig, steps: List<Step>) {
        for (s in steps) {
            s.run(rig)
            runCurrent()
        }
    }

    @Test
    fun `the session changing at any step - nothing of the old session runs, sends, publishes or posts`() = runTest {
        var runs = 0
        for ((name, steps) in scenarios) for (cut in 0..steps.size) {
            val rig = CallRig(this, dm)
            rig.api.hold = true
            val old = rig.sessions.sessionId
            play(rig, steps.take(cut))

            // Sign-out, then another account signs in. Its socket is not up yet: what still
            // arrives on the line is the old account's.
            rig.signOut()
            rig.sessions.signIn("bert")
            val where = "$name, session changed after ${if (cut == 0) "nothing" else steps[cut - 1].name}"
            assertEquals("$where: still shown", CallState.Idle, rig.controller.state.value)
            val requests = rig.api.calls.toList()
            val posts = rig.notifier.log.toList()
            val voice = rig.voiceOps.toList()

            // Everything the old session still had coming.
            rig.api.releaseAll()
            runCurrent()
            play(rig, steps.drop(cut))
            rig.api.releaseAll()
            rig.callEnds.tryEmit(CallVoiceEnd(rig.tag, VoiceEnd.Left))
            advanceTimeBy(2 * CallMachine.RING_CAP_MS); runCurrent()
            rig.api.releaseAll()
            runCurrent()

            assertEquals("$where: a request went out", requests, rig.api.calls)
            assertTrue("$where: a request with the wrong session", rig.api.sessions.all { it == old })
            assertEquals("$where: something was posted or removed", posts, rig.notifier.log)
            assertEquals("$where: the voice layer was asked for something", voice, rig.voiceOps)
            assertEquals("$where: a call is shown", CallState.Idle, rig.controller.state.value)
            assertEquals("$where: a lane is still working", 0, rig.controller.busyLanes())
            runs++
        }
        assertEquals(scenarios.values.sumOf { it.size + 1 }, runs)
    }

    @Test
    fun `the next account's own call works right after the change`() = runTest {
        for ((name, steps) in scenarios) for (cut in 0..steps.size) {
            val rig = CallRig(this, dm)
            rig.api.hold = true
            play(rig, steps.take(cut))
            rig.signOut()
            rig.sessions.signIn("bert")
            rig.socketOf("bert")
            rig.api.releaseAll()
            runCurrent()
            rig.api.hold = false
            val before = rig.api.calls.size

            // Bert is called and answers.
            rig.api.joinAnswers = "c-9"
            rig.server(ServerEvent.IncomingCall(dm, "c-9", "anna", "anna"))
            rig.controller.submit(CallInput.Accept(CallRef("bert", dm, "c-9")))
            runCurrent()

            val where = "$name, session changed after step $cut"
            assertTrue("$where: ${rig.controller.state.value}", rig.controller.state.value is CallState.Connected)
            assertEquals("$where", listOf("hold", "join"), rig.api.calls.drop(before))
            assertEquals("$where: bert's requests carry bert's session", rig.sessions.sessionId, rig.api.sessions.last())
        }
    }

    // -- One ring, whichever way it arrives -----------------------------------------------------

    private enum class Way { Socket, Push, SocketThenPush, PushThenSocket }

    private suspend fun CallRig.ringBy(way: Way) {
        when (way) {
            Way.Socket -> ring("c-1")
            Way.Push -> pushRing("c-1")
            Way.SocketThenPush -> {
                ring("c-1")
                pushRing("c-1")
            }
            Way.PushThenSocket -> {
                pushRing("c-1")
                ring("c-1")
            }
        }
    }

    private suspend fun CallRig.endedBy(way: Way, reason: String) {
        val socket = ServerEvent.CallEnded(dm, "c-1", reason)
        when (way) {
            Way.Socket -> server(socket)
            Way.Push -> pushEnded("c-1", reason)
            Way.SocketThenPush -> {
                server(socket)
                pushEnded("c-1", reason)
            }
            Way.PushThenSocket -> {
                pushEnded("c-1", reason)
                server(socket)
            }
        }
    }

    /** What the user ends up with: the call state and everything posted or removed. */
    private fun CallRig.outcome() = controller.state.value to notifier.log.toList()

    @Test
    fun `a ring by push, by socket or by both ends up the same - unanswered`() = runTest {
        val outcomes = Way.entries.associateWith { way ->
            val rig = CallRig(this, dm)
            rig.ringBy(way)
            runCurrent()
            assertTrue("$way: ${rig.controller.state.value}", rig.controller.state.value is CallState.Incoming)
            advanceTimeBy(CallMachine.RING_CAP_MS + CallController.CAP_CHECK_MS); runCurrent()
            rig.outcome()
        }
        assertEquals(CallState.Idle to listOf("ring:c-1:Anna", "stop:c-1", "missed:c-1"), outcomes.getValue(Way.Socket))
        assertEquals("the same whichever way the ring came: $outcomes", 1, outcomes.values.toSet().size)
    }

    @Test
    fun `a ring and its end by push, by socket or by both end up the same`() = runTest {
        for (reason in listOf("no_answer", "cancelled", "all_declined", "last_left")) {
            val outcomes = Way.entries.flatMap { ringWay -> Way.entries.map { ringWay to it } }.associateWith { (ringWay, endWay) ->
                val rig = CallRig(this, dm)
                rig.ringBy(ringWay)
                rig.endedBy(endWay, reason)
                runCurrent()
                // Nothing more comes later either.
                advanceTimeBy(2 * CallMachine.RING_CAP_MS); runCurrent()
                rig.outcome()
            }
            assertEquals("$reason: the same whichever way it came: $outcomes", 1, outcomes.values.toSet().size)
            val (state, log) = outcomes.values.first()
            assertEquals(CallState.Idle, state)
            val missed = reason == "no_answer" || reason == "cancelled"
            assertEquals(reason, listOfNotNull("ring:c-1:Anna", "stop:c-1", "missed:c-1".takeIf { missed }), log)
        }
    }

    @Test
    fun `a ring while a call is on is not rung and missed alike, by push, by socket or by both`() = runTest {
        val outcomes = Way.entries.associateWith { way ->
            val rig = CallRig(this, "dm-0")
            rig.ring("c-0")
            rig.accept()
            runCurrent()
            val before = rig.notifier.log.size
            // Another conversation's call rings, this way.
            when (way) {
                Way.Socket -> rig.server(ServerEvent.IncomingCall(dm, "c-1", "anna", "anna"))
                Way.Push -> rig.pushRing("c-1", channel = dm)
                Way.SocketThenPush -> {
                    rig.server(ServerEvent.IncomingCall(dm, "c-1", "anna", "anna"))
                    rig.pushRing("c-1", channel = dm)
                }
                Way.PushThenSocket -> {
                    rig.pushRing("c-1", channel = dm)
                    rig.server(ServerEvent.IncomingCall(dm, "c-1", "anna", "anna"))
                }
            }
            runCurrent()
            assertTrue("$way: rang over the call that is on", rig.notifier.log.drop(before).isEmpty())
            advanceTimeBy(CallMachine.RING_CAP_MS + CallController.CAP_CHECK_MS); runCurrent()
            (rig.controller.state.value is CallState.Connected) to rig.notifier.log.drop(before)
        }
        assertEquals(true to listOf("missed:c-1"), outcomes.getValue(Way.Socket))
        assertEquals("$outcomes", 1, outcomes.values.toSet().size)
    }
}
