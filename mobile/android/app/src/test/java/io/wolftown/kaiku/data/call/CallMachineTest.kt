package io.wolftown.kaiku.data.call

import io.wolftown.kaiku.data.call.CallEffect.ArmEndedTimer
import io.wolftown.kaiku.data.call.CallEffect.AskRing
import io.wolftown.kaiku.data.call.CallEffect.SilenceRing
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
import io.wolftown.kaiku.data.call.CallEvent.RingChecked
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
        return model.state.plain()
    }

    /** The state without its voice id (each test has one call; [voice] is its voice id). */
    private fun CallState.plain(): CallState = when (this) {
        is CallState.Outgoing -> copy(voiceId = 0L)
        is CallState.Connecting -> copy(voiceId = 0L)
        is CallState.Connected -> copy(voiceId = 0L)
        else -> this
    }

    /** The voice id of the call started or answered last. */
    private val voice: Long get() = model.voiceSeq

    /** The shown call's id, as a screen or a notification would name it. */
    private val shownId: String?
        get() = when (val s = model.state) {
            is CallState.Incoming -> s.callId
            is CallState.Outgoing -> s.callId
            is CallState.Connecting -> s.callId
            is CallState.Connected -> s.callId
            else -> null
        }

    private fun accept() = Accept(dm, shownId.orEmpty())
    private fun decline(channel: String = dm) = Decline(channel, shownId.orEmpty())
    private fun hangUp() = CallEvent.HangUp(model.state.channelId.orEmpty(), shownId)

    private fun incoming(callId: String = "c-1", at: Long = 0L) =
        send(Rang(dm, callId, "anna", "Anna", at))

    private fun outgoingWithId(callId: String = "c-1") {
        send(Start(dm, 0L))
        send(StartOk(dm, callId, 0L))
    }

    private fun connectedAsCallee() {
        incoming()
        send(accept())
        send(JoinOk(dm, "c-1"))
        send(VoiceJoined(dm, 1_000L, voice))
    }

    // -- Outgoing ----------------------------------------------------------------

    @Test
    fun `start rings out and arms the cap`() {
        assertEquals(CallState.Outgoing(dm, null, 5L), send(Start(dm, 5L)))
        // The microphone service first (the app is in front now), then the request.
        assertEquals(listOf(CallEffect.HoldVoice(dm, voice), RestStart(dm, 5L), ArmRingCap(dm, 5L)), effects)
    }

    @Test
    fun `the server's answer joins voice`() {
        send(Start(dm, 0L))
        assertEquals(CallState.Outgoing(dm, "c-1", 0L, voice = true), send(StartOk(dm, "c-1", 0L)))
        assertEquals(listOf(JoinVoice(dm, "c-1", voice)), effects)
    }

    @Test
    fun `glare goes back to idle and the other side's ring wins`() {
        send(Start(dm, 0L))
        assertEquals(CallState.Idle, send(StartFailed(dm, glare = true, blocked = false, startedAtMs = 0L)))
        assertTrue(effects.isEmpty())
    }

    @Test
    fun `a ring for the same DM before our start was answered replaces it`() {
        send(Start(dm, 0L))
        assertEquals(CallState.Incoming(dm, "c-9", "anna", "Anna", 7L), send(Rang(dm, "c-9", "anna", "Anna", 7L)))
        assertEquals(listOf(Ring(dm, "c-9", "anna", "Anna"), ArmRingCap(dm, 7L)), effects)
        // Our start's late 409 changes nothing.
        send(StartFailed(dm, glare = true, blocked = false, startedAtMs = 0L))
        assertTrue(model.state is CallState.Incoming)
    }

    @Test
    fun `a blocked or failed start ends with its reason`() {
        send(Start(dm, 0L))
        assertEquals(CallState.Ended(dm, CallEndReason.Blocked), send(StartFailed(dm, glare = false, blocked = true, startedAtMs = 0L)))
        assertEquals(listOf(ArmEndedTimer), effects)
        assertEquals(CallState.Idle, send(CallEvent.EndedShown()))

        send(Start(dm, 0L))
        assertEquals(CallState.Ended(dm, CallEndReason.StartFailed), send(StartFailed(dm, glare = false, blocked = false, startedAtMs = 0L)))
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
        assertEquals(CallState.Ended(dm, CallEndReason.Left), send(hangUp()))
        assertEquals(listOf(LeaveVoice(dm, voice), RestLeave(dm), ArmEndedTimer), effects)
    }

    @Test
    fun `the server ending the outgoing call ends it with the server's reason`() {
        outgoingWithId()
        assertEquals(CallState.Ended(dm, CallEndReason.Declined), send(ServerEnded(dm, "c-1", "all_declined")))
        assertEquals(listOf(LeaveVoice(dm, voice), ArmEndedTimer), effects)
    }

    @Test
    fun `an outgoing ring ends locally after 100 s`() {
        send(Start(dm, 1_000L))
        send(StartOk(dm, "c-1", 1_000L))
        // A cap of an older ring does nothing.
        send(RingCap(dm, 0L))
        assertTrue(model.state is CallState.Outgoing)
        assertEquals(CallState.Ended(dm, CallEndReason.NoAnswer), send(RingCap(dm, 1_000L)))
        assertEquals(listOf(LeaveVoice(dm, voice), RestLeave(dm), ArmEndedTimer), effects)
    }

    @Test
    fun `a voice join that fails ends the outgoing call and leaves it on the server`() {
        outgoingWithId()
        assertEquals(CallState.Ended(dm, CallEndReason.Failed), send(VoiceJoinFailed(dm, voice)))
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
        assertEquals(CallState.Connecting(dm, "c-1", setOf("anna")), send(accept()))
        assertEquals(listOf(StopRing(dm, "c-1"), CallEffect.HoldVoice(dm, voice), RestJoin(dm, "c-1")), effects)
        send(JoinOk(dm, "c-1"))
        assertEquals(listOf(JoinVoice(dm, "c-1", voice)), effects)
        assertEquals(CallState.Connected(dm, "c-1", 1_000L, setOf("anna")), send(VoiceJoined(dm, 1_000L, voice)))
    }

    @Test
    fun `this user's own join echo while connecting changes nothing`() {
        incoming()
        send(accept())
        send(ParticipantJoined(dm, "c-1", me, 0L))
        assertTrue(model.state is CallState.Connecting)
    }

    @Test
    fun `a status answer without this user while this user's answer is on its way changes nothing`() {
        incoming()
        send(accept())
        // A group call someone else answered: the server's look, taken before this user's own
        // join counted, shows it active without this user.
        send(Resynced(dm, snapshot(participants = setOf("anna", "bob")), 0L, model.state))
        assertTrue("an answer on its way was undone: ${model.state}", model.state is CallState.Connecting)
        assertTrue(effects.none { it is LeaveVoice || it is RestLeave })
        // The answer comes: the voice joins.
        send(JoinOk(dm, "c-1"))
        assertEquals(listOf(JoinVoice(dm, "c-1", voice)), effects)
    }

    @Test
    fun `a status answer of no call or another call still ends an answer on its way`() {
        for (said in listOf(null, snapshot(callId = "c-2", participants = setOf("anna", "bob")))) {
            model = CallModel()
            incoming()
            send(accept())
            assertEquals("$said", CallState.Idle, send(Resynced(dm, said, 0L, model.state)))
        }
    }

    @Test
    fun `an answer the server refuses ends without leaving`() {
        incoming()
        send(accept())
        assertEquals(CallState.Ended(dm, CallEndReason.Gone), send(JoinFailed(dm, "c-1", gone = true)))
        assertEquals(listOf(ArmEndedTimer), effects)
    }

    @Test
    fun `a voice join that fails after the answer leaves the call on the server`() {
        incoming()
        send(accept())
        send(JoinOk(dm, "c-1"))
        assertEquals(CallState.Ended(dm, CallEndReason.Failed), send(VoiceJoinFailed(dm, voice)))
        assertEquals(listOf(RestLeave(dm), ArmEndedTimer), effects)
    }

    @Test
    fun `decline stops the ring and tells the server`() {
        incoming()
        assertEquals(CallState.Idle, send(decline()))
        assertEquals(listOf(StopRing(dm, "c-1"), RestDecline(dm, "c-1")), effects)
        // The echo of our decline and a late ring of that call do nothing.
        send(Declined(dm, "c-1", me))
        send(Rang(dm, "c-1", "anna", "Anna", 0L))
        assertEquals(CallState.Idle, model.state.plain())
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
    fun `an incoming ring ends locally after 100 s, asks the server and tells it nothing`() {
        incoming(at = 2_000L)
        send(RingCap(dm, 1_999L))
        assertTrue(model.state is CallState.Incoming)
        assertEquals(CallState.Idle, send(RingCap(dm, 2_000L)))
        // Nothing is claimed yet: the server is asked how the call ended for this user.
        assertEquals(listOf(SilenceRing(dm, "c-1"), AskRing(dm, "c-1")), effects)
        assertTrue(effects.none { it is RestDecline || it is RestLeave })
        // Gone: no end reason, no claim.
        send(RingChecked(dm, "c-1", answer = null, asked = true))
        assertEquals(emptyList<CallEffect>(), effects)
    }

    // -- Busy ----------------------------------------------------------------------

    @Test
    fun `a second call while in a call is not rung and is missed if unanswered`() {
        connectedAsCallee()
        send(Rang("dm-2", "c-7", "bob", "Bob", 0L))
        assertTrue(model.state is CallState.Connected)
        assertEquals(listOf(CallEffect.ArmBusyCap("dm-2", "c-7", 0L)), effects)
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
        assertEquals(CallState.Ended(dm, CallEndReason.Left), send(hangUp()))
        assertEquals(listOf(LeaveVoice(dm, voice), RestLeave(dm), ArmEndedTimer), effects)
    }

    @Test
    fun `the last other person leaving ends the call here`() {
        connectedAsCallee()
        send(ParticipantJoined(dm, "c-1", "bob", 0L))
        send(ParticipantLeft(dm, "c-1", "anna"))
        assertEquals(setOf("bob"), (model.state as CallState.Connected).others)
        assertEquals(CallState.Ended(dm, CallEndReason.Ended), send(ParticipantLeft(dm, "c-1", "bob")))
        assertEquals(listOf(LeaveVoice(dm, voice), RestLeave(dm), ArmEndedTimer), effects)
    }

    @Test
    fun `the server ending the call ends it here`() {
        connectedAsCallee()
        assertEquals(CallState.Ended(dm, CallEndReason.Ended), send(ServerEnded(dm, "c-1", "last_left")))
        assertEquals(listOf(LeaveVoice(dm, voice), ArmEndedTimer), effects)
    }

    @Test
    fun `voice_removed ends the call with its reason and no rejoin`() {
        connectedAsCallee()
        val s = send(VoiceEnded(voice, VoiceEnd.Removed(VoiceRemovedReason.SessionReplaced)))
        assertEquals(CallState.Ended(dm, CallEndReason.Removed, VoiceRemovedReason.SessionReplaced), s)
        // No leave: it would remove the user from the call another device took over.
        assertEquals(listOf(ArmEndedTimer), effects)
    }

    @Test
    fun `a voice session given up ends the call as cut off`() {
        connectedAsCallee()
        assertEquals(CallState.Ended(dm, CallEndReason.ConnectionLost), send(VoiceEnded(voice, VoiceEnd.Lost)))
        assertEquals(listOf(RestLeave(dm), ArmEndedTimer), effects)
    }

    @Test
    fun `leaving voice elsewhere hangs the call up`() {
        connectedAsCallee()
        assertEquals(CallState.Ended(dm, CallEndReason.Left), send(VoiceEnded(voice, VoiceEnd.Left)))
        assertEquals(listOf(RestLeave(dm), ArmEndedTimer), effects)
    }

    @Test
    fun `a refused rejoin clears the call silently`() {
        connectedAsCallee()
        assertEquals(CallState.Idle, send(VoiceEnded(voice, VoiceEnd.Refused)))
        assertTrue(effects.isEmpty())
    }

    @Test
    fun `the voice end of a call already hung up changes nothing`() {
        connectedAsCallee()
        send(hangUp())
        send(VoiceEnded(voice, VoiceEnd.Left))
        assertEquals(CallState.Ended(dm, CallEndReason.Left), model.state.plain())
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
    fun `a ring that vanished while offline claims nothing until its end says`() {
        incoming()
        assertEquals(CallState.Idle, send(Resynced(dm, null, 0L)))
        assertEquals(listOf(SilenceRing(dm, "c-1")), effects)
        send(ServerEnded(dm, "c-1", "cancelled", rungHere = true, callerId = "anna"))
        assertEquals(listOf(Missed(dm, "c-1", "anna", "Anna")), effects)
    }

    @Test
    fun `a ring answered elsewhere while offline stops without a missed call`() {
        incoming()
        send(Resynced(dm, snapshot(), 0L))
        assertEquals(CallState.Idle, model.state.plain())
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
        assertEquals(listOf(LeaveVoice(dm, voice)), effects)

        model = CallModel()
        connectedAsCallee()
        assertEquals(CallState.Idle, send(Resynced(dm, snapshot(callId = "c-other"), 0L)))
        assertEquals(listOf(LeaveVoice(dm, voice)), effects)
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
        assertEquals(listOf(LeaveVoice(dm, voice)), effects)
    }

    // -- Rejoin gate (DM calls) --------------------------------------------------

    @Test
    fun `a call may rejoin only while the server still has this user in it`() {
        val connected = CallState.Connected(dm, "c-1", 0L)
        assertTrue(CallMachine.mayRejoin(connected, dm, snapshot(), me))
        assertFalse(CallMachine.mayRejoin(connected, dm, null, me))
        assertFalse(CallMachine.mayRejoin(connected, dm, snapshot(callId = "c-2"), me))
        assertFalse(CallMachine.mayRejoin(connected, dm, snapshot(participants = setOf("anna")), me))
        val ringingOut = CallState.Outgoing(dm, "c-1", 0L)
        assertTrue(CallMachine.mayRejoin(ringingOut, dm, snapshot(ringing = true, active = false, startedBy = me), me))
        assertFalse(CallMachine.mayRejoin(CallState.Idle, dm, snapshot(), me))
    }

    // -- Signed-in user ------------------------------------------------------------------

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
            send(decline())
        }
        assertEquals(listOf("c-2", "c-3", "c-4", "c-5", "c-6"), model.ended[dm])
    }

    // -- Fix round: late answers, removals, group calls, busy rings --------------------

    @Test
    fun `hang up before the start answered leaves nothing ringing on the server`() {
        send(Start(dm, 0L))
        assertEquals(CallState.Ended(dm, CallEndReason.Left), send(hangUp()))
        // No leave yet: it would reach the server before the start and miss the call.
        assertTrue(effects.none { it is RestLeave })
        // The start's answer comes after the hang-up: that call is left at once.
        send(StartOk(dm, "c-1", 0L))
        assertEquals(listOf(RestLeave(dm)), effects)
        assertEquals(CallState.Ended(dm, CallEndReason.Left), model.state.plain())
    }

    @Test
    fun `a start answered for an older start is left`() {
        send(Start(dm, 0L))
        send(hangUp())
        send(CallEvent.EndedShown())
        send(Start(dm, 5_000L)) // a new call to the same DM
        send(StartOk(dm, "c-old", 0L))
        assertEquals(listOf(RestLeave(dm)), effects)
        assertEquals(CallState.Outgoing(dm, null, 5_000L), model.state.plain())
    }

    @Test
    fun `the start's answer after a resync already learned the call is not left`() {
        send(Start(dm, 0L))
        val asked = model.state
        send(Resynced(dm, snapshot(ringing = true, active = false, startedBy = me, participants = emptySet()), 0L, asked))
        send(StartOk(dm, "c-1", 0L))
        assertTrue(effects.none { it is RestLeave })
    }

    @Test
    fun `no call from a resync while the start is still in flight changes nothing`() {
        send(Start(dm, 0L))
        val asked = model.state
        assertEquals(CallState.Outgoing(dm, null, 0L), send(Resynced(dm, null, 0L, asked)))
        assertTrue(effects.isEmpty())
    }

    @Test
    fun `a forced removal ends the call without leaving it on the server`() {
        connectedAsCallee()
        send(VoiceEnded(voice, VoiceEnd.Removed(VoiceRemovedReason.SessionReplaced)))
        assertTrue(effects.none { it is RestLeave })
        model = CallModel()
        connectedAsCallee()
        send(VoiceEnded(voice, VoiceEnd.Removed(VoiceRemovedReason.Kicked)))
        assertTrue(effects.none { it is RestLeave })
    }

    @Test
    fun `a resync answer for a state that has changed since is dropped`() {
        incoming()
        val asked = model.state
        send(accept())
        // Asked while ringing: "ringing, not you" must not end the answered call.
        send(Resynced(dm, snapshot(ringing = true, active = false, startedBy = "anna", participants = emptySet()), 0L, asked))
        assertTrue(model.state is CallState.Connecting)
        assertTrue(effects.isEmpty())
    }

    @Test
    fun `a ringing answer while connecting never ends the call`() {
        incoming()
        send(accept())
        val asked = model.state
        send(Resynced(dm, snapshot(ringing = true, active = false, startedBy = "anna", participants = emptySet()), 0L, asked))
        assertTrue(model.state is CallState.Connecting)
    }

    @Test
    fun `an answer for an older call does not make the newer ring missed`() {
        incoming("c-1")
        val asked = model.state
        send(ServerEnded(dm, "c-1", "cancelled"))
        incoming("c-2")
        send(Resynced(dm, null, 0L, asked))
        assertEquals("c-2", (model.state as CallState.Incoming).callId)
        assertTrue(effects.isEmpty())
    }

    @Test
    fun `a new socket asks about the shown call through the machine`() {
        incoming()
        send(CallEvent.SocketReady)
        assertEquals(listOf(CallEffect.Resync(dm, model.state)), effects)
        model = CallModel()
        send(CallEvent.SocketReady)
        assertTrue(effects.isEmpty())
    }

    @Test
    fun `someone who answered a group call while it rang here is known after answering`() {
        incoming()
        send(ParticipantJoined(dm, "c-1", "carl", 0L))
        send(accept())
        send(JoinOk(dm, "c-1"))
        send(VoiceJoined(dm, 1_000L, voice))
        // The caller leaves first: Carl is still in the call.
        send(ParticipantLeft(dm, "c-1", "anna"))
        assertEquals(setOf("carl"), (model.state as CallState.Connected).others)
    }

    @Test
    fun `the server's participants after answering are known`() {
        incoming()
        send(accept())
        send(JoinOk(dm, "c-1", setOf("anna", "carl", me)))
        send(VoiceJoined(dm, 1_000L, voice))
        assertEquals(setOf("anna", "carl"), (model.state as CallState.Connected).others)
    }

    @Test
    fun `a voice result of another call is ignored`() {
        outgoingWithId("c-2")
        send(VoiceJoinFailed(dm, voice + 7))
        assertTrue(model.state is CallState.Outgoing)
        incoming("c-9")
        model = CallModel()
        incoming("c-3")
        send(accept())
        send(VoiceJoined(dm, 0L, voice + 7))
        assertTrue(model.state is CallState.Connecting)
    }

    @Test
    fun `a busy ring gets its own cap, after which its end decides`() {
        connectedAsCallee()
        send(Rang("dm-2", "c-7", "bob", "Bob", 4L))
        assertEquals(listOf(CallEffect.ArmBusyCap("dm-2", "c-7", 4L)), effects)
        send(CallEvent.BusyCap("dm-2", "c-7"))
        assertEquals(emptyList<CallEffect>(), effects)
        assertTrue(model.busy.isEmpty())
        assertEquals(1, model.openRings.size)
        // A cap of a ring that already ended does nothing.
        send(CallEvent.BusyCap("dm-2", "c-7"))
        assertTrue(effects.isEmpty())
    }

    // -- Order and identity (answers arriving in any order) --------------------------

    @Test
    fun `the other side answering before the start's answer still joins voice once`() {
        send(Start(dm, 0L))
        assertEquals(CallState.Connected(dm, "c-1", 3L, setOf("anna")), send(ParticipantJoined(dm, "c-1", "anna", 3L)))
        assertEquals(listOf(JoinVoice(dm, "c-1", voice)), effects)
        send(StartOk(dm, "c-1", 0L))
        assertTrue(effects.isEmpty())
    }

    @Test
    fun `a resync that learns the id before the start's answer joins voice once`() {
        send(Start(dm, 0L))
        val asked = model.state
        send(Resynced(dm, snapshot(ringing = true, active = false, startedBy = me, participants = emptySet()), 0L, asked))
        assertEquals(listOf(JoinVoice(dm, "c-1", voice)), effects)
        send(StartOk(dm, "c-1", 0L))
        assertTrue(effects.isEmpty())
    }

    @Test
    fun `a resync during the start never adopts someone else's call`() {
        send(Start(dm, 0L))
        val asked = model.state
        // The other side called first.
        send(Resynced(dm, snapshot(callId = "c-9", ringing = true, active = false, startedBy = "anna", participants = emptySet()), 0L, asked))
        assertEquals(CallState.Outgoing(dm, null, 0L), model.state.plain())
        // A group call already running without this user.
        send(Resynced(dm, snapshot(callId = "c-8", participants = setOf("anna", "bob")), 0L, model.state))
        assertEquals(CallState.Outgoing(dm, null, 0L), model.state.plain())
        assertEquals(CallState.Idle, send(StartFailed(dm, glare = true, blocked = false, startedAtMs = 0L)))
    }

    @Test
    fun `a voice join that fails after the caller's call connected ends it and leaves`() {
        send(Start(dm, 0L))
        send(StartOk(dm, "c-1", 0L))
        send(ParticipantJoined(dm, "c-1", "anna", 1L))
        assertEquals(CallState.Ended(dm, CallEndReason.Failed), send(VoiceJoinFailed(dm, voice)))
        assertEquals(listOf(RestLeave(dm), ArmEndedTimer), effects)
    }

    @Test
    fun `hang up before the answer was accepted leaves only after the late acceptance, once`() {
        incoming()
        send(accept())
        assertEquals(CallState.Ended(dm, CallEndReason.Left), send(hangUp()))
        assertTrue(effects.none { it is RestLeave })
        send(JoinOk(dm, "c-1"))
        assertEquals(listOf(RestLeave(dm)), effects)
        send(JoinOk(dm, "c-1"))
        assertTrue(effects.isEmpty())
    }

    @Test
    fun `an answer result of another call is ignored`() {
        incoming("c-2")
        send(accept())
        send(JoinFailed(dm, "c-1", gone = true))
        assertTrue(model.state is CallState.Connecting)
        send(JoinOk(dm, "c-1"))
        assertTrue(effects.none { it is RestLeave || it is JoinVoice })
    }

    @Test
    fun `someone leaving while this phone joins is gone after the join, and alone means over`() {
        incoming()
        send(accept())
        send(JoinOk(dm, "c-1", setOf("anna", me)))
        send(ParticipantLeft(dm, "c-1", "anna"))
        assertEquals(CallState.Ended(dm, CallEndReason.Ended), send(VoiceJoined(dm, 1L, voice)))
        assertTrue(effects.contains(RestLeave(dm)))
    }

    @Test
    fun `a caller who left a group call while it rang here is not counted after answering`() {
        incoming()
        send(ParticipantJoined(dm, "c-1", "carl", 0L))
        send(ParticipantLeft(dm, "c-1", "anna"))
        assertEquals(setOf("carl"), (send(accept()) as CallState.Connecting).others)
    }

    @Test
    fun `the server's participants replace what was known while ringing`() {
        incoming()
        send(ParticipantJoined(dm, "c-1", "carl", 0L))
        send(accept())
        send(JoinOk(dm, "c-1", setOf("bob", me)))
        assertEquals(setOf("bob"), (model.state as CallState.Connecting).others)
    }

    @Test
    fun `the rejoin permission is only for the conversation shown`() {
        val connected = CallState.Connected(dm, "c-1", 0L)
        assertFalse(CallMachine.mayRejoin(connected, "dm-2", snapshot(), me))
        assertFalse(CallMachine.mayRejoin(CallState.Outgoing("dm-2", null, 0L), dm, snapshot(ringing = true, active = false, startedBy = me), me))
    }

    @Test
    fun `a call that ended while another was on never rings later`() {
        connectedAsCallee()
        send(ServerEnded("dm-2", "c-7", "no_answer"))
        send(hangUp())
        send(CallEvent.EndedShown())
        assertEquals(CallState.Idle, send(Rang("dm-2", "c-7", "bob", "Bob", 0L)))
    }

    @Test
    fun `a busy ring that rings later is no longer busy`() {
        incoming()
        send(Rang("dm-2", "c-7", "bob", "Bob", 0L))
        send(decline())
        send(Rang("dm-2", "c-7", "bob", "Bob", 1L))
        assertTrue(model.state is CallState.Incoming)
        assertTrue(model.busy.isEmpty())
        send(decline("dm-2"))
        send(CallEvent.BusyCap("dm-2", "c-7"))
        assertTrue(effects.none { it is Missed })
    }

    @Test
    fun `a ring from a notification that is already up is not posted again`() {
        send(Rang(dm, "c-1", "anna", "Anna", 0L, fromNotification = true))
        assertTrue(model.state is CallState.Incoming)
        // Not rung again; what the notification says is checked with the server at once.
        assertEquals(listOf(CallEffect.Resync(dm, model.state), ArmRingCap(dm, 0L)), effects)
    }

    // -- Every action names its call ---------------------------------------------------

    @Test
    fun `accept for another call than the one ringing answers nothing`() {
        incoming("c-2")
        assertTrue(send(Accept(dm, "c-1")) is CallState.Incoming)
        assertTrue(effects.isEmpty())
        assertTrue(send(Accept("dm-2", "c-2")) is CallState.Incoming)
        assertTrue(send(Accept(dm, "c-2")) is CallState.Connecting)
    }

    @Test
    fun `decline for another call than the one ringing in that conversation declines nothing`() {
        incoming("c-2")
        assertTrue(send(Decline(dm, "c-1")) is CallState.Incoming)
        assertTrue(effects.isEmpty())
    }

    @Test
    fun `decline for a ring that is not shown asks the server first, once`() {
        assertEquals(CallState.Idle, send(Decline(dm, "c-5")))
        assertEquals(listOf(StopRing(dm, "c-5"), RestDecline(dm, "c-5", verify = true)), effects)
        // A second tap, or one for a call known to be over, sends nothing.
        send(Decline(dm, "c-5"))
        assertTrue(effects.isEmpty())
        send(ServerEnded(dm, "c-6", "cancelled"))
        send(Decline(dm, "c-6"))
        assertTrue(effects.none { it is RestDecline })
    }

    @Test
    fun `hang up for another call than the shown one hangs up nothing`() {
        connectedAsCallee()
        assertTrue(send(CallEvent.HangUp(dm, "c-0")) is CallState.Connected)
        assertTrue(send(CallEvent.HangUp("dm-2", "c-1")) is CallState.Connected)
        assertTrue(effects.isEmpty())
        assertEquals(CallState.Ended(dm, CallEndReason.Left), send(CallEvent.HangUp(dm, "c-1")))
    }

    @Test
    fun `the end of an earlier call's voice does not end the next call in the same conversation`() {
        connectedAsCallee()
        val first = voice
        send(hangUp())
        send(CallEvent.EndedShown())
        incoming("c-2")
        send(Accept(dm, "c-2"))
        send(JoinOk(dm, "c-2"))
        // The first call's voice session reports its end only now.
        assertTrue(send(VoiceEnded(first, VoiceEnd.Left)) is CallState.Connecting)
        assertTrue(effects.isEmpty())
        assertTrue(send(VoiceJoined(dm, 5L, voice)) is CallState.Connected)
    }

    @Test
    fun `each call gets its own voice id`() {
        connectedAsCallee()
        val first = voice
        send(hangUp())
        send(CallEvent.EndedShown())
        send(Start(dm, 9L))
        assertTrue(voice != first)
        assertEquals(voice, (model.state as CallState.Outgoing).voiceId)
    }

    // -- Rings known from a notification, and rings by push -----------------------------

    @Test
    fun `a ring known only from its notification is verified before it is answered or declined`() {
        send(Rang(dm, "c-1", "anna", "Anna", 0L, fromNotification = true))
        assertEquals(false, (model.state as CallState.Incoming).verified)
        send(Accept(dm, "c-1"))
        assertEquals(listOf(StopRing(dm, "c-1"), CallEffect.HoldVoice(dm, voice), RestJoin(dm, "c-1", verify = true)), effects)

        model = CallModel()
        send(Rang(dm, "c-1", "anna", "Anna", 0L, fromNotification = true))
        send(Decline(dm, "c-1"))
        assertEquals(listOf(StopRing(dm, "c-1"), RestDecline(dm, "c-1", verify = true)), effects)
    }

    @Test
    fun `the server confirming a ring from a notification makes it a normal ring`() {
        send(Rang(dm, "c-1", "anna", "Anna", 0L, fromNotification = true))
        val asked = model.state
        send(Resynced(dm, snapshot(callId = "c-1", ringing = true, active = false, startedBy = "anna", participants = emptySet()), 1L, asked))
        assertEquals(true, (model.state as CallState.Incoming).verified)
        send(Accept(dm, "c-1"))
        assertEquals(listOf(StopRing(dm, "c-1"), CallEffect.HoldVoice(dm, voice), RestJoin(dm, "c-1", verify = false)), effects)
    }

    @Test
    fun `the socket or a push saying the same ring verifies it without ringing twice`() {
        send(Rang(dm, "c-1", "anna", "Anna", 0L, fromNotification = true))
        send(Rang(dm, "c-1", "anna", "Anna", 1L))
        assertEquals(true, (model.state as CallState.Incoming).verified)
        assertTrue(effects.none { it is Ring })
    }

    @Test
    fun `another call ringing in the same conversation replaces the one shown`() {
        incoming("c-1")
        assertEquals("c-2", (send(Rang(dm, "c-2", "anna", "Anna", 7L)) as CallState.Incoming).callId)
        assertEquals(listOf(Ring(dm, "c-2", "anna", "Anna"), ArmRingCap(dm, 7L)), effects)
        // The replaced one is over: its late events and a late accept of it do nothing.
        send(Accept(dm, "c-1"))
        assertEquals("c-2", (model.state as CallState.Incoming).callId)
        send(Rang(dm, "c-1", "anna", "Anna", 8L))
        assertEquals("c-2", (model.state as CallState.Incoming).callId)
    }

    @Test
    fun `an ended wake for a ring never seen here is a missed call, once`() {
        send(ServerEnded(dm, "c-4", "no_answer", rungHere = true, callerId = "anna", callerName = "Anna"))
        assertEquals(listOf(StopRing(dm, "c-4"), Missed(dm, "c-4", "anna", "Anna")), effects)
        send(ServerEnded(dm, "c-4", "no_answer", rungHere = true, callerId = "anna", callerName = "Anna"))
        assertTrue(effects.isEmpty())
        // From the socket alone (it also goes to the caller and to people not rung) it is none.
        send(ServerEnded(dm, "c-5", "no_answer"))
        assertTrue(effects.isEmpty())
    }

    @Test
    fun `an ended wake ends the shown ring like the socket's event`() {
        incoming()
        assertEquals(CallState.Idle, send(ServerEnded(dm, "c-1", "cancelled", rungHere = true, callerId = "anna", callerName = "Anna")))
        assertEquals(listOf(StopRing(dm, "c-1"), Missed(dm, "c-1", "anna", "Anna")), effects)
    }

    @Test
    fun `the ring notification timing out ends the shown ring and asks the server`() {
        incoming()
        assertEquals(CallState.Idle, send(CallEvent.RingExpired(dm, "c-1", "Anna")))
        assertEquals(listOf(SilenceRing(dm, "c-1"), AskRing(dm, "c-1")), effects)
        send(RingChecked(dm, "c-1", answer = null, asked = true))
        assertEquals(emptyList<CallEffect>(), effects)
    }

    @Test
    fun `a timed-out notification of a call that is not the one shown ends nothing`() {
        incoming("c-2")
        assertTrue(send(CallEvent.RingExpired(dm, "c-1", "Anna")) is CallState.Incoming)
        assertTrue(effects.isEmpty())
        // Nothing shown there: only if that ring is still the last thing known about its call.
        model = CallModel()
        send(CallEvent.RingExpired(dm, "c-1", "Anna"))
        assertEquals(listOf(AskRing(dm, "c-1")), effects)
        send(RingChecked(dm, "c-1", answer = null, asked = true))
        assertEquals(emptyList<CallEffect>(), effects)
        // Its end says nobody answered: missed.
        send(ServerEnded(dm, "c-1", "no_answer", rungHere = true, callerId = "anna"))
        assertEquals(listOf(Missed(dm, "c-1", "", "Anna")), effects)
    }

    @Test
    fun `a dismissed ended line is only that conversation's`() {
        connectedAsCallee()
        send(hangUp())
        assertTrue(send(CallEvent.EndedShown("dm-2")) is CallState.Ended)
        assertEquals(CallState.Idle, send(CallEvent.EndedShown(dm)))
    }

    // -- The microphone service ---------------------------------------------------------

    @Test
    fun `a microphone service that cannot start fails a call whose start is still out without leaving twice`() {
        send(Start(dm, 0L))
        assertEquals(CallState.Ended(dm, CallEndReason.Failed), send(VoiceJoinFailed(dm, voice)))
        // No leave yet: the start has not answered. Its answer is left, once.
        assertEquals(listOf(ArmEndedTimer), effects)
        send(StartOk(dm, "c-1", 0L))
        assertEquals(listOf(RestLeave(dm)), effects)
    }

    @Test
    fun `a microphone service that cannot start fails an answer and leaves once`() {
        incoming()
        send(accept())
        assertEquals(CallState.Ended(dm, CallEndReason.Failed), send(VoiceJoinFailed(dm, voice)))
        assertEquals(listOf(RestLeave(dm), ArmEndedTimer), effects)
        // The answer is accepted late: already left.
        send(JoinOk(dm, "c-1"))
        assertTrue(effects.isEmpty())
    }

    // -- The call's mute ------------------------------------------------------------------

    @Test
    fun `mute on the calling screen is kept with the call until it has a microphone`() {
        send(Start(dm, 0L))
        send(CallEvent.SetMuted(dm, null, muted = true))
        assertEquals(listOf(CallEffect.ApplyMute(voice, true)), effects)
        assertEquals(true, (model.state as CallState.Outgoing).muted)
        // The id arrives, the voice join is asked for, the other side answers: still muted.
        send(StartOk(dm, "c-1", 0L))
        assertEquals(true, (model.state as CallState.Outgoing).muted)
        send(ParticipantJoined(dm, "c-1", "anna", 5L))
        assertEquals(true, (model.state as CallState.Connected).muted)
    }

    @Test
    fun `mute while answering stays through the join`() {
        incoming()
        send(accept())
        send(CallEvent.SetMuted(dm, "c-1", muted = true))
        send(JoinOk(dm, "c-1"))
        assertEquals(true, (model.state as CallState.Connecting).muted)
        send(VoiceJoined(dm, 1L, voice))
        assertEquals(true, (model.state as CallState.Connected).muted)
        send(CallEvent.SetMuted(dm, "c-1", muted = false))
        assertEquals(listOf(CallEffect.ApplyMute(voice, false)), effects)
        assertEquals(false, (model.state as CallState.Connected).muted)
    }

    @Test
    fun `mute for another call or without a call changes nothing`() {
        send(CallEvent.SetMuted(dm, "c-1", muted = true))
        assertTrue(effects.isEmpty())
        connectedAsCallee()
        send(CallEvent.SetMuted(dm, "c-0", muted = true))
        send(CallEvent.SetMuted("dm-2", "c-1", muted = true))
        send(CallEvent.MuteToggled(voice + 3))
        assertTrue(effects.isEmpty())
        assertEquals(false, (model.state as CallState.Connected).muted)
        // The same value again is no change.
        send(CallEvent.SetMuted(dm, "c-1", muted = false))
        assertTrue(effects.isEmpty())
    }

    @Test
    fun `mute tapped on the call's notification toggles the call's mute`() {
        connectedAsCallee()
        send(CallEvent.MuteToggled(voice))
        assertEquals(listOf(CallEffect.ApplyMute(voice, true)), effects)
        assertEquals(true, (model.state as CallState.Connected).muted)
        send(CallEvent.MuteToggled(voice))
        assertEquals(false, (model.state as CallState.Connected).muted)
    }

    // -- An end that arrives before the start's answer ---------------------------------------

    @Test
    fun `a call declined before the start's answer arrived never joins voice`() {
        send(Start(dm, 0L))
        // The other side declines at once: the end names an id this phone does not know yet.
        assertTrue(send(ServerEnded(dm, "c-1", "all_declined")) is CallState.Outgoing)
        // The start's answer arrives with that very id.
        assertEquals(CallState.Ended(dm, CallEndReason.Declined), send(StartOk(dm, "c-1", 0L)))
        assertTrue("voice joined for a call that is over: $effects", effects.none { it is JoinVoice })
        assertTrue("left a call that is over: $effects", effects.none { it is RestLeave })
        // The microphone service held since the tap is given up.
        assertEquals(listOf(LeaveVoice(dm, voice), ArmEndedTimer), effects)
    }

    @Test
    fun `the end of another call of that conversation does not end a start that is still out`() {
        send(Start(dm, 0L))
        send(ServerEnded(dm, "c-0", "cancelled")) // an older call's end, late
        assertEquals(CallState.Outgoing(dm, "c-1", 0L, voice = true), send(StartOk(dm, "c-1", 0L)))
        assertEquals(listOf(JoinVoice(dm, "c-1", voice)), effects)
    }

    // -- Participants changing while the answer is on its way ---------------------------------

    @Test
    fun `someone leaving while the answer is on its way is not put back by the answer's older list`() {
        incoming()
        send(accept())
        // Anna leaves after the server accepted the answer; her leave gets here first.
        send(ParticipantLeft(dm, "c-1", "anna"))
        send(JoinOk(dm, "c-1", setOf("anna", me)))
        assertEquals(emptySet<String>(), (model.state as CallState.Connecting).others)
        // Alone in the call once voice is up: over, and left on the server.
        assertEquals(CallState.Ended(dm, CallEndReason.Ended), send(VoiceJoined(dm, 1L, voice)))
        assertEquals(listOf(LeaveVoice(dm, voice), RestLeave(dm), ArmEndedTimer), effects)
    }

    @Test
    fun `someone joining while the answer is on its way is kept over the answer's older list`() {
        incoming()
        send(accept())
        send(ParticipantJoined(dm, "c-1", "carl", 1L))
        send(JoinOk(dm, "c-1", setOf("anna", me)))
        assertEquals(setOf("anna", "carl"), (model.state as CallState.Connecting).others)
    }

    @Test
    fun `the answer's list still corrects what was only assumed before answering`() {
        incoming()
        send(accept())
        // The caller had left a group call before this phone answered; only the list says so.
        send(JoinOk(dm, "c-1", setOf("carl", me)))
        assertEquals(setOf("carl"), (model.state as CallState.Connecting).others)
        // After the answer, events apply directly.
        send(ParticipantLeft(dm, "c-1", "carl"))
        assertEquals(emptySet<String>(), (model.state as CallState.Connecting).others)
    }

    // -- A busy ring someone else answers ------------------------------------------------------

    @Test
    fun `a busy group call that someone else answers is not a missed call`() {
        connectedAsCallee()
        send(Rang("dm-2", "c-7", "bob", "Bob", 0L)) // a group call rings while this call is on
        send(ParticipantJoined("dm-2", "c-7", "carl", 1L)) // Carl answers it
        send(CallEvent.BusyCap("dm-2", "c-7"))
        assertTrue("missed although it was answered: $effects", effects.none { it is Missed })
        // Its later end is none either.
        send(ServerEnded("dm-2", "c-7", "last_left"))
        assertTrue(effects.none { it is Missed })
    }

    @Test
    fun `a busy ring nobody answers is still missed when its end comes after its 100 s`() {
        connectedAsCallee()
        send(Rang("dm-2", "c-7", "bob", "Bob", 0L))
        send(ParticipantJoined("dm-2", "c-8", "carl", 1L)) // another call's event
        send(CallEvent.BusyCap("dm-2", "c-7"))
        send(ServerEnded("dm-2", "c-7", "no_answer", rungHere = true, callerId = "bob"))
        assertEquals(listOf(Missed("dm-2", "c-7", "bob", "Bob")), effects)
    }

    // -- An outgoing call never takes the id of a call that is already over ---------------------

    private fun snapshot(callId: String, active: Boolean, participants: Set<String> = emptySet()) =
        CallSnapshot(callId, ringing = !active, active = active, startedBy = me, participants = participants, declinedBy = emptySet())

    @Test
    fun `a late resync answer for a call that was declined meanwhile joins no voice`() {
        send(Start(dm, 0L))
        // The end comes while the id is unknown and is only recorded.
        send(ServerEnded(dm, "c-1", "all_declined"))
        assertTrue(model.state is CallState.Outgoing)
        // The resync's answer was taken before that end: "ringing, started by me".
        assertEquals(CallState.Ended(dm, CallEndReason.Declined), send(Resynced(dm, snapshot("c-1", active = false), 1L)))
        assertEquals(listOf(LeaveVoice(dm, voice), ArmEndedTimer), effects)
        // The start's own answer: nothing to join and nothing to leave.
        send(StartOk(dm, "c-1", 0L))
        assertEquals(emptyList<CallEffect>(), effects)
        assertEquals(CallState.Ended(dm, CallEndReason.Declined), model.state)
    }

    @Test
    fun `a late resync answer that shows an ended call as active connects nothing`() {
        send(Start(dm, 0L))
        send(ServerEnded(dm, "c-1", "last_left"))
        val state = send(Resynced(dm, snapshot("c-1", active = true, participants = setOf(me, "anna")), 1L))
        assertEquals(CallState.Ended(dm, CallEndReason.Ended), state)
        assertTrue("voice joined for a call that is over: $effects", effects.none { it is JoinVoice })
        assertTrue(effects.any { it is LeaveVoice })
    }

    @Test
    fun `someone joining a call that is already over does not connect the start that is still out`() {
        send(Start(dm, 0L))
        send(ServerEnded(dm, "c-1", "cancelled"))
        send(ParticipantJoined(dm, "c-1", "anna", 1L))
        assertTrue(effects.none { it is JoinVoice })
        assertTrue(model.state is CallState.Outgoing)
    }

    // -- Missed or not: the end's reason, and what this phone knows of its own user ----------

    private fun ringOver() {
        incoming()
        send(RingCap(dm, 0L))
        assertEquals(listOf(SilenceRing(dm, "c-1"), AskRing(dm, "c-1")), effects)
    }

    private fun answer(ringing: Boolean = false, active: Boolean = false, participants: Set<String> = emptySet(), declinedBy: Set<String> = emptySet(), callId: String = "c-1") =
        CallSnapshot(callId, ringing = ringing, active = active, startedBy = "anna", participants = participants, declinedBy = declinedBy)

    @Test
    fun `the decision - nobody answered is missed unless this user is known to have handled it, no reason claims nothing`() {
        assertTrue(CallMachine.missedOnEnd(CallEndReason.NoAnswer, handledHere = false))
        assertTrue(CallMachine.missedOnEnd(CallEndReason.Cancelled, handledHere = false))
        assertFalse(CallMachine.missedOnEnd(CallEndReason.NoAnswer, handledHere = true))
        assertFalse(CallMachine.missedOnEnd(CallEndReason.Cancelled, handledHere = true))
        assertFalse(CallMachine.missedOnEnd(CallEndReason.Declined, handledHere = false))
        assertFalse(CallMachine.missedOnEnd(CallEndReason.Ended, handledHere = false))
        assertFalse("no end reason is no knowledge", CallMachine.missedOnEnd(null, handledHere = false))
    }

    @Test
    fun `accepted - a group ring still shown here ends as missed when nobody answered, whatever happened on another device`() {
        incoming()
        // Declined on the desktop; this phone heard nothing of it. The end says nobody answered.
        send(ServerEnded(dm, "c-1", "no_answer", rungHere = true, callerId = "anna"))
        assertEquals(listOf(StopRing(dm, "c-1"), Missed(dm, "c-1", "anna", "Anna")), effects)
    }

    @Test
    fun `a ring over here that the server shows as answered by this user is no missed call`() {
        ringOver()
        send(RingChecked(dm, "c-1", answer(active = true, participants = setOf("anna", me)), asked = true))
        assertEquals(listOf(StopRing(dm, "c-1")), effects)
        // Its end later changes nothing.
        send(ServerEnded(dm, "c-1", "last_left", rungHere = true, callerId = "anna"))
        assertTrue(effects.none { it is Missed })
    }

    @Test
    fun `a ring over here that the server shows as declined by this user is no missed call, also when nobody else answered`() {
        ringOver()
        send(RingChecked(dm, "c-1", answer(ringing = true, declinedBy = setOf(me)), asked = true))
        assertEquals(listOf(StopRing(dm, "c-1")), effects)
        send(ServerEnded(dm, "c-1", "no_answer", rungHere = true, callerId = "anna"))
        assertTrue("a call this user declined shows as missed: $effects", effects.none { it is Missed })
    }

    @Test
    fun `a ring over here that the server no longer has claims nothing until its end says`() {
        for ((reason, missed) in listOf("no_answer" to true, "cancelled" to true, "last_left" to false, "all_declined" to false)) {
            model = CallModel()
            ringOver()
            send(RingChecked(dm, "c-1", answer = null, asked = true))
            assertEquals(reason, emptyList<CallEffect>(), effects)
            send(ServerEnded(dm, "c-1", reason, rungHere = true, callerId = "anna"))
            assertEquals(
                reason,
                if (missed) listOf(Missed(dm, "c-1", "anna", "Anna")) else listOf(StopRing(dm, "c-1")),
                effects,
            )
        }
    }

    @Test
    fun `a ring over here whose call still rings, or rings on as another call, or is active without this user, claims nothing`() {
        for (said in listOf(answer(ringing = true), answer(ringing = true, callId = "c-2"), answer(active = true, participants = setOf("anna", "bob")))) {
            model = CallModel()
            ringOver()
            send(RingChecked(dm, "c-1", said, asked = true))
            assertEquals("$said", emptyList<CallEffect>(), effects)
            assertEquals(1, model.openRings.size)
        }
    }

    @Test
    fun `a ring over here while the server cannot be asked is decided by its end`() {
        ringOver()
        send(RingChecked(dm, "c-1", answer = null, asked = false))
        assertEquals(emptyList<CallEffect>(), effects)
        send(ServerEnded(dm, "c-1", "no_answer", rungHere = true, callerId = "anna"))
        assertEquals(listOf(Missed(dm, "c-1", "anna", "Anna")), effects)
        // Said once.
        send(ServerEnded(dm, "c-1", "no_answer", rungHere = true, callerId = "anna"))
        assertTrue(effects.none { it is Missed })
    }

    @Test
    fun `this user answering or declining elsewhere after the ring stopped here closes it without a missed call`() {
        ringOver()
        send(ParticipantJoined(dm, "c-1", me, 1L))
        assertEquals(listOf(StopRing(dm, "c-1")), effects)
        send(ServerEnded(dm, "c-1", "no_answer", rungHere = true, callerId = "anna"))
        assertTrue(effects.none { it is Missed })

        model = CallModel()
        ringOver()
        send(Declined(dm, "c-1", me))
        assertEquals(listOf(StopRing(dm, "c-1")), effects)
        send(ServerEnded(dm, "c-1", "cancelled", rungHere = true, callerId = "anna"))
        assertTrue(effects.none { it is Missed })
    }

    @Test
    fun `a ring over here is asked about again once a socket is up`() {
        ringOver()
        send(RingChecked(dm, "c-1", answer = null, asked = false))
        send(CallEvent.SocketReady)
        assertEquals(listOf(AskRing(dm, "c-1")), effects)
    }

    @Test
    fun `a ring over here does not ring again`() {
        ringOver()
        send(Rang(dm, "c-1", "anna", "Anna", 5L))
        assertEquals(CallState.Idle, model.state)
        assertTrue(effects.none { it is Ring })
    }

    @Test
    fun `a wake of a call this owner never showed is missed when nobody answered, group or not`() {
        send(ServerEnded(dm, "c-1", "no_answer", rungHere = true, callerId = "anna"))
        assertEquals(listOf(StopRing(dm, "c-1"), Missed(dm, "c-1", "anna", null)), effects)
        model = CallModel()
        send(ServerEnded(dm, "c-1", "last_left", rungHere = true, callerId = "anna"))
        assertEquals(listOf(StopRing(dm, "c-1")), effects)
    }

    // -- "Missed" is decided once per call, by its end -----------------------------------------

    /** Every missed call posted over the events, with their effects. */
    private fun missedOver(vararg events: CallEvent): List<CallEffect> =
        events.flatMap { send(it); effects }.filterIsInstance<Missed>()

    @Test
    fun `a call never shown here that nobody answered is missed once, socket end first`() {
        val missed = missedOver(
            ServerEnded("dm-2", "c-9", "no_answer"),
            ServerEnded("dm-2", "c-9", "no_answer", rungHere = true, callerId = "anna", callerName = "Anna"),
        )
        assertEquals(listOf(Missed("dm-2", "c-9", "anna", "Anna")), missed)
    }

    @Test
    fun `a call never shown here that nobody answered is missed once, end push first`() {
        val missed = missedOver(
            ServerEnded("dm-2", "c-9", "no_answer", rungHere = true, callerId = "anna", callerName = "Anna"),
            ServerEnded("dm-2", "c-9", "no_answer"),
        )
        assertEquals(listOf(Missed("dm-2", "c-9", "anna", "Anna")), missed)
    }

    @Test
    fun `a busy ring this user declined on another device is no missed call when its end push comes`() {
        outgoingWithId("c-0")
        send(Rang("dm-2", "c-7", "bob", "Bob", 0L))
        val missed = missedOver(
            Declined("dm-2", "c-7", me),
            ServerEnded("dm-2", "c-7", "no_answer", rungHere = true, callerId = "bob"),
        )
        assertEquals(emptyList<CallEffect>(), missed)
    }

    @Test
    fun `a call never shown here that this user declined or answered elsewhere is no missed call when its end push comes`() {
        for (seen in listOf(Declined("dm-2", "c-9", me), ParticipantJoined("dm-2", "c-9", me, 1L))) {
            model = CallModel()
            val missed = missedOver(seen, ServerEnded("dm-2", "c-9", "no_answer", rungHere = true, callerId = "anna"))
            assertEquals("$seen", emptyList<CallEffect>(), missed)
        }
    }

    @Test
    fun `the busy cap alone claims no missed call, and the end decides once`() {
        connectedAsCallee()
        send(Rang("dm-2", "c-7", "bob", "Bob", 0L))
        send(CallEvent.BusyCap("dm-2", "c-7"))
        assertTrue("the busy cap claimed a missed call: $effects", effects.none { it is Missed })
        val missed = missedOver(
            ServerEnded("dm-2", "c-7", "no_answer", rungHere = true, callerId = "bob"),
            ServerEnded("dm-2", "c-7", "no_answer"),
        )
        assertEquals(listOf(Missed("dm-2", "c-7", "bob", "Bob")), missed)
    }

    @Test
    fun `a busy ring whose end says nobody answered is missed once, before its cap`() {
        connectedAsCallee()
        send(Rang("dm-2", "c-7", "bob", "Bob", 0L))
        val missed = missedOver(
            ServerEnded("dm-2", "c-7", "no_answer"),
            ServerEnded("dm-2", "c-7", "no_answer", rungHere = true, callerId = "bob"),
            CallEvent.BusyCap("dm-2", "c-7"),
        )
        assertEquals(listOf(Missed("dm-2", "c-7", "bob", "Bob")), missed)
    }

    @Test
    fun `a busy ring whose end says somebody answered is no missed call`() {
        connectedAsCallee()
        send(Rang("dm-2", "c-7", "bob", "Bob", 0L))
        send(CallEvent.BusyCap("dm-2", "c-7"))
        assertEquals(emptyList<CallEffect>(), missedOver(ServerEnded("dm-2", "c-7", "last_left", rungHere = true, callerId = "bob")))
    }
}
