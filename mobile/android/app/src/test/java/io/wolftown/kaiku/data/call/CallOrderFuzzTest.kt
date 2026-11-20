package io.wolftown.kaiku.data.call

import io.wolftown.kaiku.data.call.CallEvent.Accept
import io.wolftown.kaiku.data.call.CallEvent.JoinFailed
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
 * join was asked for; socket events at any point, also while a request is
 * out): the end state must be an allowed one —
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

        /** The microphone service is up for a call (started at the tap, given up with the call's voice). */
        var serviceHeld = false
        val log = mutableListOf<String>()

        fun apply(effects: List<CallEffect>) {
            for (e in effects) when (e) {
                is CallEffect.HoldVoice -> {
                    serviceHeld = true
                    log += "hold"
                }
                is CallEffect.JoinVoice -> {
                    voiceAsked++
                    voiceOn = true
                    log += "join-voice"
                }
                is CallEffect.LeaveVoice -> {
                    voiceOn = false
                    serviceHeld = false
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
                val before = model.state
                val step = CallMachine.reduce(model, e, me)
                model = step.model
                world.apply(step.effects)
                // As the controller: leaving a call without a LeaveVoice effect still ends its voice.
                val had = voiceIdOf(before)
                if (had != null && voiceIdOf(model.state) != had && step.effects.none { it is CallEffect.LeaveVoice }) {
                    world.apply(listOf(CallEffect.LeaveVoice(before.channelId!!, had)))
                }
            }
            check(model, world, order)
            runs++
        }
        return runs
    }

    private fun voiceIdOf(state: CallState): Long? = when (state) {
        is CallState.Outgoing -> state.voiceId
        is CallState.Connecting -> state.voiceId
        is CallState.Connected -> state.voiceId
        else -> null
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
        assertTrue("ended here but the microphone service is still up after $order: ${world.log}", !world.serviceHeld)
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
                Step(CallEvent.HangUp(dm, null)),
            ),
        ) { model, world, order -> assertEnded(model, world, order, serverHasMe = true) }
        assertEquals(24, runs)
    }

    @Test
    fun `incoming - accept and its answers in every allowed order`() {
        val runs = fuzz(
            prefix = listOf(Rang(dm, "c-1", "anna", "Anna", 0L)),
            steps = listOf(
                Step(Accept(dm, "c-1")),
                Step(JoinOk(dm, "c-1", setOf("anna", me)), after = setOf(0)),
                Step(ParticipantJoined(dm, "c-1", me, 1L), after = setOf(0)), // the echo of this device's join
                Step(VoiceJoined(dm, 2L, 1L), after = setOf(1)),
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
                Step(Accept(dm, "c-1")),
                Step(JoinOk(dm, "c-1", setOf("anna", me)), after = setOf(0)),
                Step(CallEvent.HangUp(dm, null), after = setOf(0)),
                Step(VoiceJoined(dm, 2L, 1L), after = setOf(1)),
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
                Step(Accept(dm, "c-1"), after = setOf(0)),
                Step(JoinOk(dm, "c-1", setOf("anna", "carl", me)), after = setOf(1)),
                // The caller may leave at any point after this phone answered, also while the
                // answer is still on its way to the server and back.
                Step(ParticipantLeft(dm, "c-1", "anna"), after = setOf(1)),
                Step(VoiceJoined(dm, 2L, 1L), after = setOf(2)),
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
            Accept(dm, "c-1"),
            JoinOk(dm, "c-1", setOf("anna", me)),
            VoiceJoined(dm, 1L, 1L),
        )
        fuzz(
            prefix = connected,
            steps = listOf(
                Step(VoiceEnded(1L, VoiceEnd.Removed(VoiceRemovedReason.SessionReplaced))),
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
            prefix = listOf(Rang(dm, "c-1", "anna", "Anna", 0L), Accept(dm, "c-1"), JoinOk(dm, "c-1", setOf("anna", me)), VoiceJoined(dm, 1L, 1L)),
            steps = listOf(Step(CallEvent.HangUp(dm, null)), Step(ServerEnded(dm, "c-1", "last_left"))),
        ) { model, world, order ->
            assertTrue(!world.voiceOn)
            if (world.leaves > 1) fail("two leaves after $order: ${world.log}")
        }
    }

    // -- Orders the transport allows while a request is out ------------------------------------

    @Test
    fun `one-to-one - the other side leaving at any point after the answer ends the call once`() {
        val runs = fuzz(
            prefix = listOf(Rang(dm, "c-1", "anna", "Anna", 0L)),
            steps = listOf(
                Step(Accept(dm, "c-1")),
                Step(JoinOk(dm, "c-1", setOf("anna", me)), after = setOf(0)), // the list as the server had it when it answered
                Step(ParticipantLeft(dm, "c-1", "anna"), after = setOf(0)),
                Step(VoiceJoined(dm, 2L, 1L), after = setOf(1)),
            ),
        ) { model, world, order ->
            // Never "in the call alone": ended here, voice off, the server told once.
            assertEnded(model, world, order, serverHasMe = true)
        }
        assertEquals(3, runs)
    }

    @Test
    fun `group - someone joining at any point after the answer is in the call`() {
        fuzz(
            prefix = listOf(Rang(dm, "c-1", "anna", "Anna", 0L)),
            steps = listOf(
                Step(Accept(dm, "c-1")),
                Step(JoinOk(dm, "c-1", setOf("anna", me)), after = setOf(0)),
                Step(ParticipantJoined(dm, "c-1", "carl", 1L), after = setOf(0)),
                Step(VoiceJoined(dm, 2L, 1L), after = setOf(1)),
            ),
        ) { model, world, order ->
            assertInCallWithVoice(model, world, order)
            assertEquals("after $order", setOf("anna", "carl"), (model.state as CallState.Connected).others)
        }
    }

    @Test
    fun `outgoing - the call's end and the start's answer in either order never join a call that is over`() {
        val runs = fuzz(
            prefix = listOf(Start(dm, 0L)),
            steps = listOf(
                Step(StartOk(dm, "c-1", 0L)),
                Step(ServerEnded(dm, "c-1", "all_declined")),
            ),
        ) { model, world, order ->
            assertEquals("after $order", CallState.Ended(dm, CallEndReason.Declined), model.state)
            // The server ended it: nothing to tell, and no voice.
            assertEnded(model, world, order, serverHasMe = false)
        }
        assertEquals(2, runs)
    }

    @Test
    fun `outgoing - end, answer, the other side's answer and a hang-up in any order end with voice off`() {
        fuzz(
            prefix = listOf(Start(dm, 0L)),
            steps = listOf(
                Step(StartOk(dm, "c-1", 0L)),
                Step(ServerEnded(dm, "c-1", "cancelled")),
                Step(CallEvent.HangUp(dm, null)),
                Step(Resynced(dm, ringingByMe("c-1"), 1L)),
            ),
        ) { model, world, order ->
            assertTrue("after $order: ${model.state}", model.state is CallState.Ended)
            assertTrue("voice on after $order: ${world.log}", !world.voiceOn && !world.serviceHeld)
            if (world.leaves > 1) fail("two leaves after $order: ${world.log}")
        }
    }

    @Test
    fun `incoming - the microphone service is held from the answer and given up on every way out`() {
        fuzz(
            prefix = listOf(Rang(dm, "c-1", "anna", "Anna", 0L)),
            steps = listOf(
                Step(Accept(dm, "c-1")),
                Step(JoinFailed(dm, "c-1", gone = true), after = setOf(0)),
                Step(ServerEnded(dm, "c-1", "cancelled")),
                Step(CallEvent.HangUp(dm, "c-1"), after = setOf(0)),
            ),
        ) { model, world, order ->
            assertTrue("after $order: ${model.state}", model.state is CallState.Ended || model.state is CallState.Idle)
            assertTrue("the microphone service stayed up after $order: ${world.log}", !world.serviceHeld && !world.voiceOn)
            assertEquals("a leave for an answer the server never accepted after $order: ${world.log}", 0, world.leaves)
        }
    }
}
