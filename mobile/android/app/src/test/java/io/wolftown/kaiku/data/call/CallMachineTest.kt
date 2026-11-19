package io.wolftown.kaiku.data.call

import io.wolftown.kaiku.data.call.CallEffect.ArmEndedTimer
import io.wolftown.kaiku.data.call.CallEffect.ArmRingCap
import io.wolftown.kaiku.data.call.CallEffect.JoinVoice
import io.wolftown.kaiku.data.call.CallEffect.LeaveVoice
import io.wolftown.kaiku.data.call.CallEffect.Missed
import io.wolftown.kaiku.data.call.CallEffect.RestDecline
import io.wolftown.kaiku.data.call.CallEffect.RestJoin
import io.wolftown.kaiku.data.call.CallEffect.RestLeave
import io.wolftown.kaiku.data.call.CallEffect.RestStart
import io.wolftown.kaiku.data.call.CallEffect.Ring
import io.wolftown.kaiku.data.call.CallEffect.StopRing
import io.wolftown.kaiku.data.call.CallEvent.Accept
import io.wolftown.kaiku.data.call.CallEvent.Decline
import io.wolftown.kaiku.data.call.CallEvent.Declined
import io.wolftown.kaiku.data.call.CallEvent.JoinFailed
import io.wolftown.kaiku.data.call.CallEvent.JoinOk
import io.wolftown.kaiku.data.call.CallEvent.ParticipantJoined
import io.wolftown.kaiku.data.call.CallEvent.ParticipantLeft
import io.wolftown.kaiku.data.call.CallEvent.Rang
import io.wolftown.kaiku.data.call.CallEvent.Resynced
import io.wolftown.kaiku.data.call.CallEvent.RingCap
import io.wolftown.kaiku.data.call.CallEvent.ServerEnded
import io.wolftown.kaiku.data.call.CallEvent.Start
import io.wolftown.kaiku.data.call.CallEvent.StartFailed
import io.wolftown.kaiku.data.call.CallEvent.StartOk
import io.wolftown.kaiku.data.call.CallEvent.VoiceEnded
import io.wolftown.kaiku.data.call.CallEvent.VoiceJoinFailed
import io.wolftown.kaiku.data.call.CallEvent.VoiceJoined
import io.wolftown.kaiku.data.ws.VoiceRemovedReason
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/** Every transition of the plan's table, on the pure machine. */
class CallMachineTest {

    private val me = "me"
    private val dm = "dm-1"
    private var model = CallModel()
    private var effects: List<CallEffect> = emptyList()

    private fun send(event: CallEvent, user: String? = me): CallState {
        val step = CallMachine.reduce(model, event, user)
        model = step.model
        effects = step.effects
        return model.state
    }

    private fun incoming(callId: String = "c-1", at: Long = 0L) =
        send(Rang(dm, callId, "anna", "Anna", at))

    private fun outgoingWithId(callId: String = "c-1") {
        send(Start(dm, 0L))
        send(StartOk(dm, callId))
    }

    private fun connectedAsCallee() {
        incoming()
        send(Accept(dm))
        send(JoinOk(dm))
        send(VoiceJoined(dm, 1_000L))
    }

    // -- Outgoing ----------------------------------------------------------------

    @Test
    fun `start rings out and arms the cap`() {
        assertEquals(CallState.Outgoing(dm, null, 5L), send(Start(dm, 5L)))
        assertEquals(listOf(RestStart(dm), ArmRingCap(dm, 5L)), effects)
    }

    @Test
    fun `the server's answer joins voice`() {
        send(Start(dm, 0L))
        assertEquals(CallState.Outgoing(dm, "c-1", 0L), send(StartOk(dm, "c-1")))
        assertEquals(listOf(JoinVoice(dm)), effects)
    }

    @Test
    fun `glare goes back to idle and the other side's ring wins`() {
        send(Start(dm, 0L))
        assertEquals(CallState.Idle, send(StartFailed(dm, glare = true, blocked = false)))
        assertTrue(effects.isEmpty())
    }

    @Test
    fun `a ring for the same DM before our start was answered replaces it`() {
        send(Start(dm, 0L))
        assertEquals(CallState.Incoming(dm, "c-9", "anna", "Anna", 7L), send(Rang(dm, "c-9", "anna", "Anna", 7L)))
        assertEquals(listOf(Ring(dm, "c-9", "anna", "Anna"), ArmRingCap(dm, 7L)), effects)
        // Our start's late 409 changes nothing.
        send(StartFailed(dm, glare = true, blocked = false))
        assertTrue(model.state is CallState.Incoming)
    }

    @Test
    fun `a blocked or failed start ends with its reason`() {
        send(Start(dm, 0L))
        assertEquals(CallState.Ended(dm, CallEndReason.Blocked), send(StartFailed(dm, glare = false, blocked = true)))
        assertEquals(listOf(ArmEndedTimer), effects)
        assertEquals(CallState.Idle, send(CallEvent.EndedShown))

        send(Start(dm, 0L))
        assertEquals(CallState.Ended(dm, CallEndReason.StartFailed), send(StartFailed(dm, glare = false, blocked = false)))
    }

    @Test
    fun `the other side answering connects the caller`() {
        outgoingWithId()
        assertEquals(CallState.Connected(dm, "c-1", 9L, setOf("anna")), send(ParticipantJoined(dm, "c-1", "anna", 9L)))
    }

    @Test
    fun `an event of another call does not connect the caller`() {
        outgoingWithId()
        send(ParticipantJoined(dm, "c-old", "anna", 9L))
        assertTrue(model.state is CallState.Outgoing)
    }

    @Test
    fun `hang up while ringing out leaves voice and the call`() {
        outgoingWithId()
        assertEquals(CallState.Ended(dm, CallEndReason.Left), send(CallEvent.HangUp))
        assertEquals(listOf(LeaveVoice(dm), RestLeave(dm), ArmEndedTimer), effects)
    }

    @Test
    fun `the server ending the outgoing call ends it with the server's reason`() {
        outgoingWithId()
        assertEquals(CallState.Ended(dm, CallEndReason.Declined), send(ServerEnded(dm, "c-1", "all_declined")))
        assertEquals(listOf(LeaveVoice(dm), ArmEndedTimer), effects)
    }

    @Test
    fun `an outgoing ring ends locally after 100 s`() {
        send(Start(dm, 1_000L))
        send(StartOk(dm, "c-1"))
        // A cap of an older ring does nothing.
        send(RingCap(dm, 0L))
        assertTrue(model.state is CallState.Outgoing)
        assertEquals(CallState.Ended(dm, CallEndReason.NoAnswer), send(RingCap(dm, 1_000L)))
        assertEquals(listOf(LeaveVoice(dm), RestLeave(dm), ArmEndedTimer), effects)
    }

    @Test
    fun `a voice join that fails ends the outgoing call and leaves it on the server`() {
        outgoingWithId()
        assertEquals(CallState.Ended(dm, CallEndReason.Failed), send(VoiceJoinFailed(dm)))
        assertEquals(listOf(RestLeave(dm), ArmEndedTimer), effects)
    }

    // -- Incoming ----------------------------------------------------------------

    @Test
    fun `a ring rings and arms the cap`() {
        assertEquals(CallState.Incoming(dm, "c-1", "anna", "Anna", 3L), send(Rang(dm, "c-1", "anna", "Anna", 3L)))
        assertEquals(listOf(Ring(dm, "c-1", "anna", "Anna"), ArmRingCap(dm, 3L)), effects)
    }

    @Test
    fun `a ring from this user's own call is ignored`() {
        assertEquals(CallState.Idle, send(Rang(dm, "c-1", me, "me", 0L)))
        assertTrue(effects.isEmpty())
    }

    @Test
    fun `a repeated ring keeps the first and only learns a missing name`() {
        send(Rang(dm, "c-1", "anna", null, 0L))
        send(Rang(dm, "c-1", "anna", null, 5L))
        assertTrue(effects.isEmpty())
        assertEquals(CallState.Incoming(dm, "c-1", "anna", "Anna", 0L), send(Rang(dm, "c-1", "anna", "Anna", 6L)))
        assertEquals(listOf(Ring(dm, "c-1", "anna", "Anna")), effects)
    }

    @Test
    fun `accept stops the ring and answers`() {
        incoming()
        assertEquals(CallState.Connecting(dm, "c-1", setOf("anna")), send(Accept(dm)))
        assertEquals(listOf(StopRing(dm, "c-1"), RestJoin(dm)), effects)
        send(JoinOk(dm))
        assertEquals(listOf(JoinVoice(dm)), effects)
        assertEquals(CallState.Connected(dm, "c-1", 1_000L, setOf("anna")), send(VoiceJoined(dm, 1_000L)))
    }

    @Test
    fun `this user's own join echo while connecting changes nothing`() {
        incoming()
        send(Accept(dm))
        send(ParticipantJoined(dm, "c-1", me, 0L))
        assertTrue(model.state is CallState.Connecting)
    }

    @Test
    fun `an answer the server refuses ends without leaving`() {
        incoming()
        send(Accept(dm))
        assertEquals(CallState.Ended(dm, CallEndReason.Gone), send(JoinFailed(dm, gone = true)))
        assertEquals(listOf(ArmEndedTimer), effects)
    }

    @Test
    fun `a voice join that fails after the answer leaves the call on the server`() {
        incoming()
        send(Accept(dm))
        send(JoinOk(dm))
        assertEquals(CallState.Ended(dm, CallEndReason.Failed), send(VoiceJoinFailed(dm)))
        assertEquals(listOf(RestLeave(dm), ArmEndedTimer), effects)
    }

    @Test
    fun `decline stops the ring and tells the server`() {
        incoming()
        assertEquals(CallState.Idle, send(Decline(dm)))
        assertEquals(listOf(StopRing(dm, "c-1"), RestDecline(dm)), effects)
        // The echo of our decline and a late ring of that call do nothing.
        send(Declined(dm, "c-1", me))
        send(Rang(dm, "c-1", "anna", "Anna", 0L))
        assertEquals(CallState.Idle, model.state)
        assertTrue(effects.isEmpty())
    }

    @Test
    fun `answered on another device of this user stops ringing here`() {
        incoming()
        assertEquals(CallState.Idle, send(ParticipantJoined(dm, "c-1", me, 0L)))
        assertEquals(listOf(StopRing(dm, "c-1")), effects)
    }

    @Test
    fun `declined on another device stops ringing here`() {
        incoming()
        assertEquals(CallState.Idle, send(Declined(dm, "c-1", me)))
        assertEquals(listOf(StopRing(dm, "c-1")), effects)
    }

    @Test
    fun `someone else answering a group call keeps it ringing here`() {
        incoming()
        send(ParticipantJoined(dm, "c-1", "bob", 0L))
        assertTrue(model.state is CallState.Incoming)
        assertTrue(effects.isEmpty())
    }

    @Test
    fun `an unanswered ring that the server ends is missed`() {
        incoming()
        assertEquals(CallState.Idle, send(ServerEnded(dm, "c-1", "no_answer")))
        assertEquals(listOf(StopRing(dm, "c-1"), Missed(dm, "c-1", "anna", "Anna")), effects)
        incoming("c-2")
        send(ServerEnded(dm, "c-2", "cancelled"))
        assertTrue(effects.contains(Missed(dm, "c-2", "anna", "Anna")))
    }

    @Test
    fun `a ring ended because everyone declined or left is not missed`() {
        incoming()
        send(ServerEnded(dm, "c-1", "all_declined"))
        assertEquals(listOf(StopRing(dm, "c-1")), effects)
    }

    @Test
    fun `the end of another call does not stop the ring`() {
        incoming("c-2")
        send(ServerEnded(dm, "c-1", "no_answer"))
        assertTrue(model.state is CallState.Incoming)
    }

    @Test
    fun `an incoming ring ends locally after 100 s as missed without telling the server`() {
        incoming(at = 2_000L)
        send(RingCap(dm, 1_999L))
        assertTrue(model.state is CallState.Incoming)
        assertEquals(CallState.Idle, send(RingCap(dm, 2_000L)))
        assertEquals(listOf(StopRing(dm, "c-1"), Missed(dm, "c-1", "anna", "Anna")), effects)
    }

    // -- Busy ----------------------------------------------------------------------

    @Test
    fun `a second call while in a call is not rung and is missed if unanswered`() {
        connectedAsCallee()
        send(Rang("dm-2", "c-7", "bob", "Bob", 0L))
        assertTrue(model.state is CallState.Connected)
        assertTrue(effects.isEmpty())
        send(ServerEnded("dm-2", "c-7", "no_answer"))
        assertEquals(listOf(Missed("dm-2", "c-7", "bob", "Bob")), effects)
        assertTrue(model.state is CallState.Connected)
    }

    @Test
    fun `a busy ring answered on another device is not missed`() {
        connectedAsCallee()
        send(Rang("dm-2", "c-7", "bob", "Bob", 0L))
        send(ParticipantJoined("dm-2", "c-7", me, 0L))
        send(ServerEnded("dm-2", "c-7", "cancelled"))
        assertTrue(effects.isEmpty())
    }

    @Test
    fun `a ring while another ring is shown is busy too`() {
        incoming()
        send(Rang("dm-2", "c-7", "bob", "Bob", 0L))
        assertEquals("c-1", (model.state as CallState.Incoming).callId)
        assertEquals(BusyRing("c-7", "bob", "Bob"), model.busy["dm-2"])
    }

    // -- In the call ---------------------------------------------------------------

    @Test
    fun `hang up in the call leaves voice and the call`() {
        connectedAsCallee()
        assertEquals(CallState.Ended(dm, CallEndReason.Left), send(CallEvent.HangUp))
        assertEquals(listOf(LeaveVoice(dm), RestLeave(dm), ArmEndedTimer), effects)
    }

    @Test
    fun `the last other person leaving ends the call here`() {
        connectedAsCallee()
        send(ParticipantJoined(dm, "c-1", "bob", 0L))
        send(ParticipantLeft(dm, "c-1", "anna"))
        assertEquals(setOf("bob"), (model.state as CallState.Connected).others)
        assertEquals(CallState.Ended(dm, CallEndReason.Ended), send(ParticipantLeft(dm, "c-1", "bob")))
        assertEquals(listOf(LeaveVoice(dm), RestLeave(dm), ArmEndedTimer), effects)
    }

    @Test
    fun `the server ending the call ends it here`() {
        connectedAsCallee()
        assertEquals(CallState.Ended(dm, CallEndReason.Ended), send(ServerEnded(dm, "c-1", "last_left")))
        assertEquals(listOf(LeaveVoice(dm), ArmEndedTimer), effects)
    }

    @Test
    fun `voice_removed ends the call with its reason and no rejoin`() {
        connectedAsCallee()
        val s = send(VoiceEnded(dm, VoiceEnd.Removed(VoiceRemovedReason.SessionReplaced)))
        assertEquals(CallState.Ended(dm, CallEndReason.Removed, VoiceRemovedReason.SessionReplaced), s)
        assertEquals(listOf(RestLeave(dm), ArmEndedTimer), effects)
    }

    @Test
    fun `a voice session given up ends the call as cut off`() {
        connectedAsCallee()
        assertEquals(CallState.Ended(dm, CallEndReason.ConnectionLost), send(VoiceEnded(dm, VoiceEnd.Lost)))
        assertEquals(listOf(RestLeave(dm), ArmEndedTimer), effects)
    }

    @Test
    fun `leaving voice elsewhere hangs the call up`() {
        connectedAsCallee()
        assertEquals(CallState.Ended(dm, CallEndReason.Left), send(VoiceEnded(dm, VoiceEnd.Left)))
        assertEquals(listOf(RestLeave(dm), ArmEndedTimer), effects)
    }

    @Test
    fun `a refused rejoin clears the call silently`() {
        connectedAsCallee()
        assertEquals(CallState.Idle, send(VoiceEnded(dm, VoiceEnd.Refused)))
        assertTrue(effects.isEmpty())
    }

    @Test
    fun `the voice end of a call already hung up changes nothing`() {
        connectedAsCallee()
        send(CallEvent.HangUp)
        send(VoiceEnded(dm, VoiceEnd.Left))
        assertEquals(CallState.Ended(dm, CallEndReason.Left), model.state)
        assertTrue(effects.isEmpty())
    }

    // -- Resync ------------------------------------------------------------------

    private fun snapshot(
        callId: String = "c-1",
        ringing: Boolean = false,
        active: Boolean = true,
        startedBy: String? = null,
        participants: Set<String> = setOf("anna", me),
        declinedBy: Set<String> = emptySet(),
    ) = CallSnapshot(callId, ringing, active, startedBy, participants, declinedBy)

    @Test
    fun `a ring that vanished while offline is missed`() {
        incoming()
        assertEquals(CallState.Idle, send(Resynced(dm, null, 0L)))
        assertEquals(listOf(StopRing(dm, "c-1"), Missed(dm, "c-1", "anna", "Anna")), effects)
    }

    @Test
    fun `a ring answered elsewhere while offline stops without a missed call`() {
        incoming()
        send(Resynced(dm, snapshot(), 0L))
        assertEquals(CallState.Idle, model.state)
        assertEquals(listOf(StopRing(dm, "c-1")), effects)
    }

    @Test
    fun `a ring that still rings keeps ringing`() {
        incoming()
        send(Resynced(dm, snapshot(ringing = true, active = false, startedBy = "anna", participants = emptySet()), 0L))
        assertTrue(model.state is CallState.Incoming)
        assertTrue(effects.isEmpty())
    }

    @Test
    fun `no call clears an outgoing or connected call silently`() {
        outgoingWithId()
        assertEquals(CallState.Idle, send(Resynced(dm, null, 0L)))
        assertEquals(listOf(LeaveVoice(dm)), effects)

        model = CallModel()
        connectedAsCallee()
        assertEquals(CallState.Idle, send(Resynced(dm, snapshot(callId = "c-other"), 0L)))
        assertEquals(listOf(LeaveVoice(dm)), effects)
    }

    @Test
    fun `an outgoing call answered while offline is connected`() {
        outgoingWithId()
        assertEquals(CallState.Connected(dm, "c-1", 4L, setOf("anna")), send(Resynced(dm, snapshot(), 4L)))
    }

    @Test
    fun `a connected call this user is no longer in clears silently`() {
        connectedAsCallee()
        assertEquals(CallState.Idle, send(Resynced(dm, snapshot(participants = setOf("anna")), 0L)))
        assertEquals(listOf(LeaveVoice(dm)), effects)
    }

    // -- Rejoin gate (DM calls) --------------------------------------------------

    @Test
    fun `a call may rejoin only while the server still has this user in it`() {
        val connected = CallState.Connected(dm, "c-1", 0L)
        assertTrue(CallMachine.mayRejoin(connected, snapshot(), me))
        assertFalse(CallMachine.mayRejoin(connected, null, me))
        assertFalse(CallMachine.mayRejoin(connected, snapshot(callId = "c-2"), me))
        assertFalse(CallMachine.mayRejoin(connected, snapshot(participants = setOf("anna")), me))
        val ringingOut = CallState.Outgoing(dm, "c-1", 0L)
        assertTrue(CallMachine.mayRejoin(ringingOut, snapshot(ringing = true, active = false, startedBy = me), me))
        assertFalse(CallMachine.mayRejoin(CallState.Idle, snapshot(), me))
    }

    // -- Signed-in user ------------------------------------------------------------------

    @Test
    fun `sign-out forgets everything and stops every ring`() {
        incoming()
        send(Rang("dm-2", "c-7", "bob", "Bob", 0L))
        assertEquals(CallState.Idle, send(CallEvent.SignedOut))
        assertEquals(listOf(CallEffect.StopAllRings), effects)
        assertEquals(CallModel(), model)
    }

    @Test
    fun `nothing happens while nobody is signed in`() {
        assertEquals(CallState.Idle, send(Rang(dm, "c-1", "anna", "Anna", 0L), user = null))
        assertTrue(effects.isEmpty())
        assertEquals(CallState.Idle, send(Start(dm, 0L), user = null))
    }

    @Test
    fun `at most five ended call ids are remembered per DM`() {
        repeat(7) { i ->
            incoming("c-$i")
            send(Decline(dm))
        }
        assertEquals(listOf("c-2", "c-3", "c-4", "c-5", "c-6"), model.ended[dm])
    }
}
