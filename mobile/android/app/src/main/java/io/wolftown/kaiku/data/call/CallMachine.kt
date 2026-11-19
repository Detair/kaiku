package io.wolftown.kaiku.data.call

import io.wolftown.kaiku.data.ws.VoiceRemovedReason

/** Why a call shows as ended (one line of text on the call screen). */
enum class CallEndReason {
    /** The caller hung up before anyone answered (`cancelled`). */
    Cancelled,

    /** Everyone rung declined (`all_declined`). */
    Declined,

    /** Nobody answered (`no_answer`, or the local 100 s ring cap). */
    NoAnswer,

    /** The call is over (`last_left`, an unknown reason, or the other side left). */
    Ended,

    /** This user hung up. */
    Left,

    /** Starting failed (network, server). */
    StartFailed,

    /** Starting was refused because of a block. */
    Blocked,

    /** Answering or setting up the voice connection failed. */
    Failed,

    /** The call was already over when this user answered. */
    Gone,

    /** The connection was lost and could not be recovered (C32 give-up). */
    ConnectionLost,

    /** The server removed this user's voice peer (C31); see [CallState.Ended.removed]. */
    Removed,
}

/** The one call this process shows. */
sealed interface CallState {
    /** The DM the call is in, or null for [Idle]. */
    val channelId: String?

    data object Idle : CallState {
        override val channelId: String? get() = null
    }

    /** This user calls; [callId] is null until the server answered the start. */
    data class Outgoing(
        override val channelId: String,
        val callId: String?,
        val startedAtMs: Long,
    ) : CallState

    /** The call rings this user. [callerName] is the best known name (null: unknown). */
    data class Incoming(
        override val channelId: String,
        val callId: String,
        val callerId: String,
        val callerName: String?,
        val startedAtMs: Long,
    ) : CallState

    /** Answered here: the join and the voice connection are being set up. */
    data class Connecting(
        override val channelId: String,
        val callId: String,
        val others: Set<String> = emptySet(),
    ) : CallState

    /** In the call since [sinceMs]; [others] are the other participants known to be in it. */
    data class Connected(
        override val channelId: String,
        val callId: String?,
        val sinceMs: Long,
        val others: Set<String> = emptySet(),
    ) : CallState

    /** Shown briefly with its [reason], then [Idle]. For new calls it counts as idle. */
    data class Ended(
        override val channelId: String,
        val reason: CallEndReason,
        val removed: VoiceRemovedReason? = null,
    ) : CallState
}

/** How a call's voice session ended without the call layer asking for it. */
sealed interface VoiceEnd {
    /** The user left voice another way (the voice screen, joining another channel, the notification). */
    data object Left : VoiceEnd

    /** C31: the server removed the peer. */
    data class Removed(val reason: VoiceRemovedReason) : VoiceEnd

    /** C32: the session was given up (attempts, 60 s, or it never connected). */
    data object Lost : VoiceEnd

    /** The rejoin gate said the call is over (C32 "DM calls"); its answer is applied as a resync. */
    data object Refused : VoiceEnd
}

/** A DM call's voice session ended ([io.wolftown.kaiku.data.repository.VoiceRepository.callEnds]). */
data class CallVoiceEnd(val channelId: String, val why: VoiceEnd)

/** What `GET /api/dm/{id}/call` said, reduced to what the machine needs. */
data class CallSnapshot(
    val callId: String,
    val ringing: Boolean,
    val active: Boolean,
    val startedBy: String?,
    val participants: Set<String>,
    val declinedBy: Set<String>,
)

/** Inputs of the machine. Times are elapsed-clock milliseconds. */
sealed interface CallEvent {
    data class Start(val channelId: String, val nowMs: Long) : CallEvent
    data class StartOk(val channelId: String, val callId: String) : CallEvent
    data class StartFailed(val channelId: String, val glare: Boolean, val blocked: Boolean) : CallEvent

    /** `incoming_call`, or a ring known from a push wake / its notification. */
    data class Rang(
        val channelId: String,
        val callId: String,
        val callerId: String,
        val callerName: String?,
        val nowMs: Long,
    ) : CallEvent

    data class Accept(val channelId: String) : CallEvent
    data class JoinOk(val channelId: String) : CallEvent

    /** [gone]: the server says the call is over (404). */
    data class JoinFailed(val channelId: String, val gone: Boolean) : CallEvent
    data class VoiceJoined(val channelId: String, val nowMs: Long) : CallEvent
    data class VoiceJoinFailed(val channelId: String) : CallEvent
    data class Decline(val channelId: String) : CallEvent
    data object HangUp : CallEvent

    data class ParticipantJoined(val channelId: String, val callId: String, val userId: String, val nowMs: Long) : CallEvent
    data class ParticipantLeft(val channelId: String, val callId: String, val userId: String) : CallEvent
    data class Declined(val channelId: String, val callId: String, val userId: String) : CallEvent

    /** `call_ended`; [reason] is the wire name. */
    data class ServerEnded(val channelId: String, val callId: String, val reason: String) : CallEvent

    /** The 100 s ring cap of the ring that started at [startedAtMs] (C33). */
    data class RingCap(val channelId: String, val startedAtMs: Long) : CallEvent

    data class VoiceEnded(val channelId: String, val why: VoiceEnd) : CallEvent

    /** C33 resync (or the rejoin gate's look): [answer] null = no call. */
    data class Resynced(val channelId: String, val answer: CallSnapshot?, val nowMs: Long) : CallEvent

    /** Sign-out, session expiry or another signed-in user. */
    data object SignedOut : CallEvent

    /** The ended line has been shown long enough. */
    data object EndedShown : CallEvent
}

/** What the controller does for a transition. */
sealed interface CallEffect {
    data class RestStart(val channelId: String) : CallEffect
    data class RestJoin(val channelId: String) : CallEffect
    data class RestDecline(val channelId: String) : CallEffect
    data class RestLeave(val channelId: String) : CallEffect
    data class JoinVoice(val channelId: String) : CallEffect

    /** Leave voice if it is still in [channelId] (never a guild channel the user moved to). */
    data class LeaveVoice(val channelId: String) : CallEffect

    /** Post the incoming-call notification (ringtone, full-screen where allowed). */
    data class Ring(
        val channelId: String,
        val callId: String,
        val callerId: String,
        val callerName: String?,
    ) : CallEffect

    data class StopRing(val channelId: String, val callId: String) : CallEffect

    /** Post a missed call for [callId]. */
    data class Missed(val channelId: String, val callId: String, val callerId: String, val callerName: String?) : CallEffect

    /** Arm the 100 s ring cap; a later arm replaces it. */
    data class ArmRingCap(val channelId: String, val startedAtMs: Long) : CallEffect

    /** Arm the timer that turns [CallState.Ended] into [CallState.Idle]. */
    data object ArmEndedTimer : CallEffect

    /** Cancel every ring notification this process posted (sign-out). */
    data object StopAllRings : CallEffect
}

/** A ring this user did not get rung for because a call was already on (busy, like the web). */
data class BusyRing(val callId: String, val callerId: String, val callerName: String?)

/**
 * The machine's whole state: the shown call, the ids of recently ended calls
 * per DM (their late events are ignored) and busy rings per DM.
 */
data class CallModel(
    val state: CallState = CallState.Idle,
    val ended: Map<String, List<String>> = emptyMap(),
    val busy: Map<String, BusyRing> = emptyMap(),
)

/** One transition: the next model and what to do. */
data class CallStep(val model: CallModel, val effects: List<CallEffect> = emptyList())

/**
 * The DM call state machine (plan N2b §2), pure: no Android, no coroutines,
 * no clock of its own. [CallController] owns the one instance of the model and
 * runs the effects.
 */
object CallMachine {
    /** Ended call ids remembered per DM. */
    const val MAX_ENDED = 5

    /** C33: a ring ends locally this long after it started, whatever the server said. */
    const val RING_CAP_MS = 100_000L

    /** How long the ended line is shown. */
    const val ENDED_SHOWN_MS = 3_000L

    fun reduce(model: CallModel, event: CallEvent, me: String?): CallStep {
        if (event is CallEvent.SignedOut) {
            return CallStep(CallModel(), listOf(CallEffect.StopAllRings))
        }
        if (me == null) return CallStep(model)
        return Reducer(model, me).apply(event)
    }

    /**
     * Whether a call session may rejoin (C32 "DM calls"): the server's call
     * is the one shown and still active with this user in it, or still ringing
     * and started by this user.
     */
    fun mayRejoin(state: CallState, answer: CallSnapshot?, me: String): Boolean {
        val callId = when (state) {
            is CallState.Outgoing -> state.callId
            is CallState.Connecting -> state.callId
            is CallState.Connected -> state.callId
            else -> return false
        }
        if (answer == null || (callId != null && answer.callId != callId)) return false
        return (answer.active && me in answer.participants) || (answer.ringing && answer.startedBy == me)
    }

    private class Reducer(val model: CallModel, val me: String) {
        val state = model.state
        val effects = mutableListOf<CallEffect>()
        var ended = model.ended
        var busy = model.busy

        fun to(next: CallState): CallStep = CallStep(CallModel(next, ended, busy), effects.toList())
        fun same(): CallStep = CallStep(CallModel(state, ended, busy), effects.toList())

        fun rememberEnded(channelId: String, callId: String?) {
            if (callId.isNullOrEmpty()) return
            val list = (ended[channelId].orEmpty() - callId + callId).takeLast(MAX_ENDED)
            ended = ended + (channelId to list)
        }

        fun isEnded(channelId: String, callId: String): Boolean =
            callId.isNotEmpty() && callId in ended[channelId].orEmpty()

        /** Does an event of [callId] in [channelId] belong to the shown call [shownId]? */
        fun matches(channelId: String, shownChannel: String?, shownId: String?, callId: String): Boolean {
            if (channelId != shownChannel) return false
            if (callId.isEmpty()) return true
            if (shownId == null) return !isEnded(channelId, callId)
            return shownId == callId
        }

        fun endedState(channelId: String, reason: CallEndReason, removed: VoiceRemovedReason? = null): CallStep {
            effects += CallEffect.ArmEndedTimer
            return to(CallState.Ended(channelId, reason, removed))
        }

        fun serverReason(reason: String): CallEndReason = when (reason) {
            "cancelled" -> CallEndReason.Cancelled
            "all_declined" -> CallEndReason.Declined
            "no_answer" -> CallEndReason.NoAnswer
            else -> CallEndReason.Ended
        }

        val idle get() = state is CallState.Idle || state is CallState.Ended

        fun apply(event: CallEvent): CallStep = when (event) {
            is CallEvent.Start -> start(event)
            is CallEvent.StartOk -> startOk(event)
            is CallEvent.StartFailed -> startFailed(event)
            is CallEvent.Rang -> rang(event)
            is CallEvent.Accept -> accept(event)
            is CallEvent.JoinOk -> joinOk(event)
            is CallEvent.JoinFailed -> joinFailed(event)
            is CallEvent.VoiceJoined -> voiceJoined(event)
            is CallEvent.VoiceJoinFailed -> voiceJoinFailed(event)
            is CallEvent.Decline -> decline(event)
            CallEvent.HangUp -> hangUp()
            is CallEvent.ParticipantJoined -> participantJoined(event)
            is CallEvent.ParticipantLeft -> participantLeft(event)
            is CallEvent.Declined -> declined(event)
            is CallEvent.ServerEnded -> serverEnded(event)
            is CallEvent.RingCap -> ringCap(event)
            is CallEvent.VoiceEnded -> voiceEnded(event)
            is CallEvent.Resynced -> resynced(event)
            CallEvent.EndedShown -> if (state is CallState.Ended) to(CallState.Idle) else same()
            CallEvent.SignedOut -> to(CallState.Idle)
        }

        fun start(e: CallEvent.Start): CallStep {
            if (!idle) return same()
            effects += CallEffect.RestStart(e.channelId)
            effects += CallEffect.ArmRingCap(e.channelId, e.nowMs)
            return to(CallState.Outgoing(e.channelId, null, e.nowMs))
        }

        fun startOk(e: CallEvent.StartOk): CallStep {
            val s = state as? CallState.Outgoing ?: return same()
            if (s.channelId != e.channelId || s.callId != null) return same()
            effects += CallEffect.JoinVoice(e.channelId)
            return to(s.copy(callId = e.callId))
        }

        fun startFailed(e: CallEvent.StartFailed): CallStep {
            val s = state as? CallState.Outgoing ?: return same()
            if (s.channelId != e.channelId || s.callId != null) return same()
            // Glare: the other side's call exists; its ring arrives as incoming_call.
            if (e.glare) return to(CallState.Idle)
            return endedState(e.channelId, if (e.blocked) CallEndReason.Blocked else CallEndReason.StartFailed)
        }

        fun rang(e: CallEvent.Rang): CallStep {
            if (e.callerId == me || isEnded(e.channelId, e.callId)) return same()
            val incoming = CallState.Incoming(e.channelId, e.callId, e.callerId, e.callerName, e.nowMs)
            when (state) {
                is CallState.Incoming -> if (state.channelId == e.channelId && state.callId == e.callId) {
                    // A repeat (socket and push both): keep the first, learn a missing name.
                    if (state.callerName == null && e.callerName != null) {
                        effects += CallEffect.Ring(e.channelId, e.callId, e.callerId, e.callerName)
                        return to(state.copy(callerName = e.callerName))
                    }
                    return same()
                }
                // Glare: our start has no id yet, their call rings for the same DM: theirs wins.
                is CallState.Outgoing -> if (state.channelId == e.channelId && state.callId == null) {
                    effects += CallEffect.Ring(e.channelId, e.callId, e.callerId, e.callerName)
                    effects += CallEffect.ArmRingCap(e.channelId, e.nowMs)
                    return to(incoming)
                }
                else -> if (idle) {
                    effects += CallEffect.Ring(e.channelId, e.callId, e.callerId, e.callerName)
                    effects += CallEffect.ArmRingCap(e.channelId, e.nowMs)
                    return to(incoming)
                }
            }
            // Busy: already in or ringing for another call. Not rung (as on the web).
            busy = busy + (e.channelId to BusyRing(e.callId, e.callerId, e.callerName))
            return same()
        }

        fun accept(e: CallEvent.Accept): CallStep {
            val s = state as? CallState.Incoming ?: return same()
            if (s.channelId != e.channelId) return same()
            effects += CallEffect.StopRing(s.channelId, s.callId)
            effects += CallEffect.RestJoin(s.channelId)
            return to(CallState.Connecting(s.channelId, s.callId, setOf(s.callerId)))
        }

        fun joinOk(e: CallEvent.JoinOk): CallStep {
            val s = state as? CallState.Connecting ?: return same()
            if (s.channelId != e.channelId) return same()
            effects += CallEffect.JoinVoice(s.channelId)
            return same()
        }

        fun joinFailed(e: CallEvent.JoinFailed): CallStep {
            val s = state as? CallState.Connecting ?: return same()
            if (s.channelId != e.channelId) return same()
            rememberEnded(s.channelId, s.callId)
            return endedState(s.channelId, if (e.gone) CallEndReason.Gone else CallEndReason.Failed)
        }

        fun voiceJoined(e: CallEvent.VoiceJoined): CallStep {
            val s = state as? CallState.Connecting ?: return same()
            if (s.channelId != e.channelId) return same()
            return to(CallState.Connected(s.channelId, s.callId, e.nowMs, s.others))
        }

        fun voiceJoinFailed(e: CallEvent.VoiceJoinFailed): CallStep {
            val (channelId, callId) = when (state) {
                is CallState.Outgoing -> state.channelId to state.callId
                is CallState.Connecting -> state.channelId to state.callId
                else -> return same()
            }
            if (channelId != e.channelId) return same()
            effects += CallEffect.RestLeave(channelId)
            rememberEnded(channelId, callId)
            return endedState(channelId, CallEndReason.Failed)
        }

        fun decline(e: CallEvent.Decline): CallStep {
            val s = state as? CallState.Incoming ?: return same()
            if (s.channelId != e.channelId) return same()
            effects += CallEffect.StopRing(s.channelId, s.callId)
            effects += CallEffect.RestDecline(s.channelId)
            rememberEnded(s.channelId, s.callId)
            return to(CallState.Idle)
        }

        fun hangUp(): CallStep = when (state) {
            is CallState.Outgoing -> {
                // The server cancels a ringing call on the caller's leave.
                effects += CallEffect.LeaveVoice(state.channelId)
                effects += CallEffect.RestLeave(state.channelId)
                rememberEnded(state.channelId, state.callId)
                endedState(state.channelId, CallEndReason.Left)
            }
            is CallState.Connecting -> leaveCall(state.channelId, state.callId, CallEndReason.Left)
            is CallState.Connected -> leaveCall(state.channelId, state.callId, CallEndReason.Left)
            else -> same()
        }

        fun leaveCall(channelId: String, callId: String?, reason: CallEndReason): CallStep {
            effects += CallEffect.LeaveVoice(channelId)
            effects += CallEffect.RestLeave(channelId)
            rememberEnded(channelId, callId)
            return endedState(channelId, reason)
        }

        fun participantJoined(e: CallEvent.ParticipantJoined): CallStep {
            when (state) {
                is CallState.Incoming -> if (matches(e.channelId, state.channelId, state.callId, e.callId)) {
                    if (e.userId == me) {
                        // Answered on another device of this user (C33).
                        effects += CallEffect.StopRing(state.channelId, state.callId)
                        rememberEnded(state.channelId, state.callId)
                        return to(CallState.Idle)
                    }
                    return same() // someone else answered a group call; it still rings here
                }
                is CallState.Outgoing -> if (matches(e.channelId, state.channelId, state.callId, e.callId) && e.userId != me) {
                    return to(CallState.Connected(state.channelId, state.callId ?: e.callId.ifEmpty { null }, e.nowMs, setOf(e.userId)))
                }
                is CallState.Connecting -> if (matches(e.channelId, state.channelId, state.callId, e.callId) && e.userId != me) {
                    return to(state.copy(others = state.others + e.userId))
                }
                is CallState.Connected -> if (matches(e.channelId, state.channelId, state.callId, e.callId) && e.userId != me) {
                    return to(state.copy(others = state.others + e.userId))
                }
                else -> Unit
            }
            if (e.userId == me) dropBusy(e.channelId, e.callId)
            return same()
        }

        fun participantLeft(e: CallEvent.ParticipantLeft): CallStep {
            val s = state as? CallState.Connected ?: return same()
            if (!matches(e.channelId, s.channelId, s.callId, e.callId) || e.userId == me) return same()
            if (e.userId !in s.others) return same()
            val others = s.others - e.userId
            // The last other person left: this user is alone, the call is over.
            if (others.isEmpty()) return leaveCall(s.channelId, s.callId, CallEndReason.Ended)
            return to(s.copy(others = others))
        }

        fun declined(e: CallEvent.Declined): CallStep {
            val s = state as? CallState.Incoming
            if (s != null && e.userId == me && matches(e.channelId, s.channelId, s.callId, e.callId)) {
                // Declined on another device of this user.
                effects += CallEffect.StopRing(s.channelId, s.callId)
                rememberEnded(s.channelId, s.callId)
                return to(CallState.Idle)
            }
            if (e.userId == me) dropBusy(e.channelId, e.callId)
            return same()
        }

        fun dropBusy(channelId: String, callId: String) {
            val ring = busy[channelId] ?: return
            if (callId.isEmpty() || ring.callId == callId) busy = busy - channelId
        }

        fun serverEnded(e: CallEvent.ServerEnded): CallStep {
            val reason = serverReason(e.reason)
            val unanswered = reason == CallEndReason.NoAnswer || reason == CallEndReason.Cancelled
            busy[e.channelId]?.let { ring ->
                if (e.callId.isEmpty() || ring.callId == e.callId) {
                    busy = busy - e.channelId
                    if (unanswered) effects += CallEffect.Missed(e.channelId, ring.callId, ring.callerId, ring.callerName)
                }
            }
            return when (state) {
                is CallState.Incoming -> {
                    if (!matches(e.channelId, state.channelId, state.callId, e.callId)) return same()
                    effects += CallEffect.StopRing(state.channelId, state.callId)
                    if (unanswered) {
                        effects += CallEffect.Missed(state.channelId, state.callId, state.callerId, state.callerName)
                    }
                    rememberEnded(state.channelId, state.callId)
                    to(CallState.Idle)
                }
                is CallState.Outgoing -> {
                    if (!matches(e.channelId, state.channelId, state.callId, e.callId)) return same()
                    effects += CallEffect.LeaveVoice(state.channelId)
                    rememberEnded(state.channelId, state.callId ?: e.callId)
                    endedState(state.channelId, reason)
                }
                is CallState.Connecting -> {
                    if (!matches(e.channelId, state.channelId, state.callId, e.callId)) return same()
                    effects += CallEffect.LeaveVoice(state.channelId)
                    rememberEnded(state.channelId, state.callId)
                    endedState(state.channelId, if (reason == CallEndReason.Cancelled) CallEndReason.Gone else CallEndReason.Ended)
                }
                is CallState.Connected -> {
                    if (!matches(e.channelId, state.channelId, state.callId, e.callId)) return same()
                    effects += CallEffect.LeaveVoice(state.channelId)
                    rememberEnded(state.channelId, state.callId)
                    endedState(state.channelId, CallEndReason.Ended)
                }
                else -> {
                    rememberEnded(e.channelId, e.callId)
                    same()
                }
            }
        }

        fun ringCap(e: CallEvent.RingCap): CallStep = when (state) {
            is CallState.Incoming -> if (state.channelId == e.channelId && state.startedAtMs == e.startedAtMs) {
                // C33: an incoming ring that ends locally counts as missed; nothing is sent.
                effects += CallEffect.StopRing(state.channelId, state.callId)
                effects += CallEffect.Missed(state.channelId, state.callId, state.callerId, state.callerName)
                rememberEnded(state.channelId, state.callId)
                to(CallState.Idle)
            } else same()
            is CallState.Outgoing -> if (state.channelId == e.channelId && state.startedAtMs == e.startedAtMs) {
                effects += CallEffect.LeaveVoice(state.channelId)
                effects += CallEffect.RestLeave(state.channelId)
                rememberEnded(state.channelId, state.callId)
                endedState(state.channelId, CallEndReason.NoAnswer)
            } else same()
            else -> same()
        }

        fun voiceEnded(e: CallEvent.VoiceEnded): CallStep {
            val (channelId, callId) = when (state) {
                is CallState.Outgoing -> state.channelId to state.callId
                is CallState.Connecting -> state.channelId to state.callId
                is CallState.Connected -> state.channelId to state.callId
                else -> return same()
            }
            if (channelId != e.channelId) return same()
            rememberEnded(channelId, callId)
            return when (val why = e.why) {
                // The gate's answer follows as a resync; the call is simply over here.
                VoiceEnd.Refused -> to(CallState.Idle)
                VoiceEnd.Left -> {
                    effects += CallEffect.RestLeave(channelId)
                    endedState(channelId, CallEndReason.Left)
                }
                VoiceEnd.Lost -> {
                    effects += CallEffect.RestLeave(channelId)
                    endedState(channelId, CallEndReason.ConnectionLost)
                }
                is VoiceEnd.Removed -> {
                    // C31: no rejoin; the call is left on the server too.
                    effects += CallEffect.RestLeave(channelId)
                    endedState(channelId, CallEndReason.Removed, why.reason)
                }
            }
        }

        fun resynced(e: CallEvent.Resynced): CallStep {
            val answer = e.answer
            when (state) {
                is CallState.Incoming -> {
                    if (state.channelId != e.channelId) return same()
                    val sameCall = answer != null && answer.callId == state.callId
                    return when {
                        // The ring vanished while offline: a missed call (C33).
                        !sameCall -> {
                            effects += CallEffect.StopRing(state.channelId, state.callId)
                            effects += CallEffect.Missed(state.channelId, state.callId, state.callerId, state.callerName)
                            rememberEnded(state.channelId, state.callId)
                            to(CallState.Idle)
                        }
                        answer!!.active && me in answer.participants || me in answer.declinedBy -> {
                            effects += CallEffect.StopRing(state.channelId, state.callId)
                            rememberEnded(state.channelId, state.callId)
                            to(CallState.Idle)
                        }
                        else -> same()
                    }
                }
                is CallState.Outgoing -> {
                    if (state.channelId != e.channelId) return same()
                    val sameCall = answer != null && (state.callId == null || answer.callId == state.callId)
                    if (!sameCall) return silentEnd(state.channelId, state.callId)
                    if (answer!!.active) {
                        return to(CallState.Connected(state.channelId, answer.callId, e.nowMs, answer.participants - me))
                    }
                    return to(state.copy(callId = answer.callId))
                }
                is CallState.Connecting, is CallState.Connected -> {
                    if (state.channelId != e.channelId) return same()
                    val callId = if (state is CallState.Connecting) state.callId else (state as CallState.Connected).callId
                    val stillIn = answer != null && (callId == null || answer.callId == callId) &&
                        answer.active && me in answer.participants
                    if (!stillIn) return silentEnd(e.channelId, callId)
                    if (state is CallState.Connected) return to(state.copy(others = answer!!.participants - me))
                    return same()
                }
                else -> return same()
            }
        }

        /** C33: "no call" clears the local state silently. */
        fun silentEnd(channelId: String, callId: String?): CallStep {
            effects += CallEffect.LeaveVoice(channelId)
            rememberEnded(channelId, callId)
            return to(CallState.Idle)
        }
    }
}
