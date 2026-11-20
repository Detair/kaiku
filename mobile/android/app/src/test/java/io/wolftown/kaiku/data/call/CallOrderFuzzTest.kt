package io.wolftown.kaiku.data.call

import io.wolftown.kaiku.data.call.CallEvent.Accept
import io.wolftown.kaiku.data.call.CallEvent.JoinOk
import io.wolftown.kaiku.data.call.CallEvent.ParticipantJoined
import io.wolftown.kaiku.data.call.CallEvent.ParticipantLeft
import io.wolftown.kaiku.data.call.CallEvent.Rang
import io.wolftown.kaiku.data.call.CallEvent.Resynced
import io.wolftown.kaiku.data.call.CallEvent.ServerEnded
import io.wolftown.kaiku.data.call.CallEvent.Start
import io.wolftown.kaiku.data.call.CallEvent.StartFailed
import io.wolftown.kaiku.data.call.CallEvent.StartOk
import io.wolftown.kaiku.data.call.CallEvent.VoiceEnded
import io.wolftown.kaiku.data.call.CallEvent.VoiceJoined
import io.wolftown.kaiku.data.ws.VoiceRemovedReason
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Assert.fail
import org.junit.Test

/**
 * The same events and answers in every order the transport allows (an
 * answer never before its request, a voice result never before the voice
 * join was asked for): the end state must be an allowed one —
 * - in the call with its voice join asked for exactly once and not left, or
 * - ended (or not shown) with the server told about this user at most once,
 *   exactly once where this user was in the server's call, and every voice
 *   join that was asked for also left.
 * Never "ended here but voice joined", never "voice joined without a call",
 * never two leaves.
 */
class CallOrderFuzzTest {

    private val me = "me"
    private val dm = "dm-1"

    private class Step(val event: CallEvent, val after: Set<Int> = emptySet())

    /** What the effects did, applied as the controller would. */
    private class World {
        var voiceAsked = 0
        var voiceOn = false
        var leaves = 0
        val log = mutableListOf<String>()

        fun apply(effects: List<CallEffect>) {
            for (e in effects) when (e) {
                is CallEffect.JoinVoice -> {
                    voiceAsked++
                    voiceOn = true
                    log += "join-voice"
                }
                is CallEffect.LeaveVoice -> {
                    voiceOn = false
                    log += "leave-voice"
                }
                is CallEffect.RestLeave -> {
                    leaves++
                    log += "rest-leave"
                }
                else -> Unit
            }
        }
    }

    private fun <T> permutations(items: List<T>): List<List<T>> =
        if (items.size <= 1) listOf(items) else items.flatMap { x -> permutations(items - x).map { listOf(x) + it } }

    /**
     * Runs [prefix] then every allowed order of [steps]; checks each end with
     * [check]. Returns how many orders were run.
     */
    private fun fuzz(
        prefix: List<CallEvent>,
        steps: List<Step>,
        check: (CallModel, World, List<Int>) -> Unit,
    ): Int {
        var runs = 0
        for (order in permutations(steps.indices.toList())) {
            val position = order.withIndex().associate { (pos, i) -> i to pos }
            if (steps.indices.any { i -> steps[i].after.any { a -> position.getValue(a) > position.getValue(i) } }) continue
            var model = CallModel()
            val world = World()
            for (e in prefix + order.map { steps[it].event }) {
                val step = CallMachine.reduce(model, e, me)
                model = step.model
                world.apply(step.effects)
            }
            check(model, world, order)
            runs++
        }
        return runs
    }

    private fun ringingByMe(callId: String) = CallSnapshot(callId, ringing = true, active = false, startedBy = me, participants = emptySet(), declinedBy = emptySet())
    private fun ringingBy(callId: String, user: String) = CallSnapshot(callId, ringing = true, active = false, startedBy = user, participants = emptySet(), declinedBy = emptySet())

    private fun assertInCallWithVoice(model: CallModel, world: World, order: List<Int>) {
        val s = model.state
        assertTrue("not in the call after $order: $s ${world.log}", s is CallState.Connected || s is CallState.Outgoing || s is CallState.Connecting)
        assertEquals("voice asked for exactly once after $order: ${world.log}", 1, world.voiceAsked)
        assertTrue("voice left in a call after $order: ${world.log}", world.voiceOn)
        assertEquals("a leave in a running call after $order: ${world.log}", 0, world.leaves)
    }

    private fun assertEnded(model: CallModel, world: World, order: List<Int>, serverHasMe: Boolean) {
        val s = model.state
        assertTrue("still in a call after $order: $s", s is CallState.Ended || s is CallState.Idle || s is CallState.Incoming)
        assertTrue("ended here but voice joined after $order: ${world.log}", !world.voiceOn)
        assertEquals("server told ${world.leaves} times after $order: ${world.log}", if (serverHasMe) 1 else 0, world.leaves)
    }

    @Test
    fun `outgoing - start answer, the other side's answer and a resync in any order`() {
        val runs = fuzz(
            prefix = listOf(Start(dm, 0L)),
            steps = listOf(
                Step(StartOk(dm, "c-1", 0L)),
                Step(ParticipantJoined(dm, "c-1", "anna", 1L)),
                Step(Resynced(dm, ringingByMe("c-1"), 1L)),
            ),
        ) { model, world, order ->
            assertInCallWithVoice(model, world, order)
            assertTrue(model.state is CallState.Connected || (model.state as? CallState.Outgoing)?.callId == "c-1")
        }
        assertEquals(6, runs)
    }

    @Test
    fun `outgoing - hang up at any point tells the server exactly once`() {
        val runs = fuzz(
            prefix = listOf(Start(dm, 0L)),
            steps = listOf(
                Step(StartOk(dm, "c-1", 0L)),
                Step(ParticipantJoined(dm, "c-1", "anna", 1L)),
                Step(Resynced(dm, ringingByMe("c-1"), 1L)),
                Step(CallEvent.HangUp),
            ),
        ) { model, world, order -> assertEnded(model, world, order, serverHasMe = true) }
        assertEquals(24, runs)
    }

    @Test
    fun `incoming - accept and its answers in every allowed order`() {
        val runs = fuzz(
            prefix = listOf(Rang(dm, "c-1", "anna", "Anna", 0L)),
            steps = listOf(
                Step(Accept(dm)),
                Step(JoinOk(dm, "c-1", setOf("anna", me)), after = setOf(0)),
                Step(ParticipantJoined(dm, "c-1", me, 1L), after = setOf(0)), // the echo of this device's join
                Step(VoiceJoined(dm, 2L, "c-1"), after = setOf(1)),
            ),
        ) { model, world, order ->
            assertInCallWithVoice(model, world, order)
            assertTrue(model.state is CallState.Connected)
        }
        assertTrue(runs > 1)
    }

    @Test
    fun `incoming - hang up at any point after accepting tells the server exactly once`() {
        val runs = fuzz(
            prefix = listOf(Rang(dm, "c-1", "anna", "Anna", 0L)),
            steps = listOf(
                Step(Accept(dm)),
                Step(JoinOk(dm, "c-1", setOf("anna", me)), after = setOf(0)),
                Step(CallEvent.HangUp, after = setOf(0)),
                Step(VoiceJoined(dm, 2L, "c-1"), after = setOf(1)),
            ),
        ) { model, world, order -> assertEnded(model, world, order, serverHasMe = true) }
        assertTrue(runs > 1)
    }

    @Test
    fun `glare - both call at once, the other side's ring wins and nothing is joined or left`() {
        fuzz(
            prefix = listOf(Start(dm, 0L)),
            steps = listOf(
                Step(Rang(dm, "c-9", "anna", "Anna", 1L)),
                Step(StartFailed(dm, glare = true, blocked = false, startedAtMs = 0L)),
                Step(Resynced(dm, ringingBy("c-9", "anna"), 1L)),
            ),
        ) { model, world, order ->
            assertEquals("after $order", "c-9", (model.state as? CallState.Incoming)?.callId)
            assertEquals(0, world.voiceAsked)
            assertEquals(0, world.leaves)
        }
    }

    @Test
    fun `group call with a late answerer - the caller leaving does not end it`() {
        fuzz(
            prefix = listOf(Rang(dm, "c-1", "anna", "Anna", 0L)),
            steps = listOf(
                Step(ParticipantJoined(dm, "c-1", "carl", 1L)),
                Step(Accept(dm), after = setOf(0)),
                Step(JoinOk(dm, "c-1", setOf("anna", "carl", me)), after = setOf(1)),
                Step(ParticipantLeft(dm, "c-1", "anna"), after = setOf(2)),
                Step(VoiceJoined(dm, 2L, "c-1"), after = setOf(2)),
            ),
        ) { model, world, order ->
            assertInCallWithVoice(model, world, order)
            assertEquals("after $order", setOf("carl"), (model.state as CallState.Connected).others)
        }
    }

    @Test
    fun `takeover on another device - ended here without telling the server`() {
        val connected = listOf(
            Rang(dm, "c-1", "anna", "Anna", 0L),
            Accept(dm),
            JoinOk(dm, "c-1", setOf("anna", me)),
            VoiceJoined(dm, 1L, "c-1"),
        )
        fuzz(
            prefix = connected,
            steps = listOf(
                Step(VoiceEnded(dm, VoiceEnd.Removed(VoiceRemovedReason.SessionReplaced))),
                Step(ParticipantJoined(dm, "c-1", me, 2L)), // the other device's join
            ),
        ) { model, world, order ->
            assertEquals("after $order", CallState.Ended(dm, CallEndReason.Removed, VoiceRemovedReason.SessionReplaced), model.state)
            assertEquals(0, world.leaves)
        }
    }

    @Test
    fun `the server ending the call races the hang-up - at most one leave`() {
        fuzz(
            prefix = listOf(Rang(dm, "c-1", "anna", "Anna", 0L), Accept(dm), JoinOk(dm, "c-1", setOf("anna", me)), VoiceJoined(dm, 1L, "c-1")),
            steps = listOf(Step(CallEvent.HangUp), Step(ServerEnded(dm, "c-1", "last_left"))),
        ) { model, world, order ->
            assertTrue(!world.voiceOn)
            if (world.leaves > 1) fail("two leaves after $order: ${world.log}")
        }
    }
}
