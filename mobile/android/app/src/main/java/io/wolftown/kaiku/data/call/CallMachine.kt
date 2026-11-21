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

    /** The connection was lost and could not be recovered (voice gave up). */
    ConnectionLost,

    /** The server removed this user's voice peer; see [CallState.Ended.removed]. */
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
        /** The voice join of this call was asked for (once, when its id became known). */
        val voice: Boolean = false,
        /** This call's name for its voice session; what the voice layer says about it carries it. */
        val voiceId: Long = 0L,
        /** The user's Mute for this call; applied to this call's microphone and to no other. */
        val muted: Boolean = false,
    ) : CallState

    /** The call rings this user. [callerName] is the best known name (null: unknown). */
    data class Incoming(
        override val channelId: String,
        val callId: String,
        val callerId: String,
        val callerName: String?,
        val startedAtMs: Long,
        /** Others who answered (a group call) while it rang here; known after answering. */
        val joined: Set<String> = emptySet(),
        /** The caller left (a group call that others keep going) while it rang here. */
        val callerLeft: Boolean = false,
        /**
         * The server said so (socket or push). False for a ring known only from its
         * notification: it is checked with the server before anything is done for it.
         */
        val verified: Boolean = true,
    ) : CallState

    /** Answered here: the join and the voice connection are being set up. */
    data class Connecting(
        override val channelId: String,
        val callId: String,
        val others: Set<String> = emptySet(),
        /** The server accepted the answer and the voice join was asked for. */
        val voice: Boolean = false,
        val voiceId: Long = 0L,
        val muted: Boolean = false,
        /**
         * Who joined (true) or left (false) while the answer was on its way to the server and
         * back: the server's list in that answer may be older than these events.
         */
        val changed: Map<String, Boolean> = emptyMap(),
    ) : CallState

    /** In the call since [sinceMs]; [others] are the other participants known to be in it. */
    data class Connected(
        override val channelId: String,
        val callId: String?,
        val sinceMs: Long,
        val others: Set<String> = emptySet(),
        val voiceId: Long = 0L,
        val muted: Boolean = false,
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

    /** The server removed the peer. */
    data class Removed(val reason: VoiceRemovedReason) : VoiceEnd

    /** The session was given up (attempts, 60 s, or it never connected). */
    data object Lost : VoiceEnd

    /** The rejoin gate said the call is over (rejoin of a DM call); its answer is applied as a resync. */
    data object Refused : VoiceEnd
}

/**
 * A DM call's voice session ended ([io.wolftown.kaiku.data.repository.VoiceRepository.callEnds]).
 * [tag] is the name the call layer gave that session when it asked for it.
 */
data class CallVoiceEnd(val tag: Long, val why: VoiceEnd)

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
    /** [startedAtMs] names the start it answers (the [Start]'s time). */
    data class StartOk(val channelId: String, val callId: String, val startedAtMs: Long) : CallEvent
    data class StartFailed(val channelId: String, val glare: Boolean, val blocked: Boolean, val startedAtMs: Long) : CallEvent

    /**
     * A ring: `incoming_call`, a push wake ([nowMs] is then the time the wake was
     * received) or, with [fromNotification], a screen opened from its notification.
     */
    data class Rang(
        val channelId: String,
        val callId: String,
        val callerId: String,
        val callerName: String?,
        val nowMs: Long,
        /** Known from a notification that is already up (a screen opened from it): not rung again. */
        val fromNotification: Boolean = false,
    ) : CallEvent

    /** Accept of the ring [callId]; another ring in that conversation is not answered by it. */
    data class Accept(val channelId: String, val callId: String) : CallEvent
    /** The answer was accepted; [participants] are those the server has in the call. */
    data class JoinOk(val channelId: String, val callId: String, val participants: Set<String> = emptySet()) : CallEvent

    /** [gone]: the server says the call is over (404). */
    data class JoinFailed(val channelId: String, val callId: String, val gone: Boolean) : CallEvent
    /** The voice session [voiceId] is set up. */
    data class VoiceJoined(val channelId: String, val nowMs: Long, val voiceId: Long) : CallEvent
    data class VoiceJoinFailed(val channelId: String, val voiceId: Long) : CallEvent

    /**
     * The microphone service could not be started for the voice session [voiceId] when the
     * user tapped Call or Accept. The request of that tap was not sent: the call was neither
     * started nor answered on the server.
     */
    data class HoldRefused(val channelId: String, val voiceId: Long) : CallEvent

    /** A new authenticated socket: the shown call is checked with the server (resync). */
    data object SocketReady : CallEvent

    /** The 100 s cap of a ring that was not rung because a call was on. */
    data class BusyCap(val channelId: String, val callId: String) : CallEvent
    /** Decline of the ring [callId] (the screen, or its notification's action). */
    data class Decline(val channelId: String, val callId: String) : CallEvent

    /** Hang up the shown call; [callId] is null while an outgoing call has no id yet. */
    data class HangUp(val channelId: String, val callId: String?) : CallEvent

    /** Mute or unmute the shown call (the call screen's switch). */
    data class SetMuted(val channelId: String, val callId: String?, val muted: Boolean) : CallEvent

    /** Mute was tapped on a control of the call's voice session [voiceId] (its notification, the voice bar). */
    data class MuteToggled(val voiceId: Long) : CallEvent

    data class ParticipantJoined(val channelId: String, val callId: String, val userId: String, val nowMs: Long) : CallEvent
    data class ParticipantLeft(val channelId: String, val callId: String, val userId: String) : CallEvent
    data class Declined(val channelId: String, val callId: String, val userId: String) : CallEvent

    /**
     * `call_ended`; [reason] is the wire name. [rungHere]: it came as a push wake, which only
     * goes to someone the call rang for, so an unanswered end is a missed call also when this
     * process never saw the ring ([callerId] and [callerName] say whose). Whether it is a
     * missed call is [CallMachine.missedOnEnd]'s decision.
     */
    data class ServerEnded(
        val channelId: String,
        val callId: String,
        val reason: String,
        val rungHere: Boolean = false,
        val callerId: String? = null,
        val callerName: String? = null,
    ) : CallEvent

    /** The ring notification of [callId] timed out (100 s after the ring was received). */
    data class RingExpired(val channelId: String, val callId: String, val callerName: String?) : CallEvent

    /**
     * What the server said about the call [callId] whose ring had just stopped here
     * ([CallEffect.AskRing]): [answer] (null: no call), or, with [asked] false, nothing,
     * because it could not be asked (no restored session, no connection).
     */
    data class RingChecked(
        val channelId: String,
        val callId: String,
        val answer: CallSnapshot?,
        val asked: Boolean,
    ) : CallEvent

    /** The 100 s ring cap of the ring that started at [startedAtMs]. */
    data class RingCap(val channelId: String, val startedAtMs: Long) : CallEvent

    /** The voice session [voiceId] ended by itself. */
    data class VoiceEnded(val voiceId: Long, val why: VoiceEnd) : CallEvent

    /** The call resync (or the rejoin gate's look): [answer] null = no call. */
    data class Resynced(
        val channelId: String,
        val answer: CallSnapshot?,
        val nowMs: Long,
        /** The state the question was asked for; a changed state since then drops the answer. */
        val askedFor: CallState? = null,
    ) : CallEvent

    /** The ended line has been shown long enough ([channelId] null), or was dismissed for that conversation. */
    data class EndedShown(val channelId: String? = null) : CallEvent
}

/** What the controller does for a transition. */
sealed interface CallEffect {
    data class RestStart(val channelId: String, val startedAtMs: Long) : CallEffect
    /** [verify]: ask the server first whether [callId] is still the conversation's ringing call. */
    data class RestJoin(val channelId: String, val callId: String, val verify: Boolean = false) : CallEffect
    data class RestDecline(val channelId: String, val callId: String, val verify: Boolean = false) : CallEffect
    data class RestLeave(val channelId: String) : CallEffect
    data class JoinVoice(val channelId: String, val callId: String?, val voiceId: Long) : CallEffect

    /**
     * Start the microphone service for the call's voice session [voiceId]
     * now: the user has just tapped Call or Accept, so the app is in front,
     * which Android requires for a microphone service; by the time the
     * requests to the server are through it may not be any more.
     */
    data class HoldVoice(val channelId: String, val voiceId: Long) : CallEffect

    /** Ask the server about the shown call, for [askedFor]. */
    data class Resync(val channelId: String, val askedFor: CallState) : CallEffect

    /** Arm the cap of a busy ring. */
    data class ArmBusyCap(val channelId: String, val callId: String, val startedAtMs: Long) : CallEffect

    /** End the voice session [voiceId] (and only that one). */
    data class LeaveVoice(val channelId: String, val voiceId: Long) : CallEffect

    /** Mute or unmute the microphone of the voice session [voiceId], if it is on; no other session's. */
    data class ApplyMute(val voiceId: Long, val muted: Boolean) : CallEffect

    /** Post the incoming-call notification (ringtone, full-screen where allowed). */
    data class Ring(
        val channelId: String,
        val callId: String,
        val callerId: String,
        val callerName: String?,
    ) : CallEffect

    data class StopRing(val channelId: String, val callId: String) : CallEffect

    /**
     * The ring of [callId] stops here and its notification goes, while nothing is known yet
     * about how the call ended: what the server says about that call later is still heard.
     */
    data class SilenceRing(val channelId: String, val callId: String) : CallEffect

    /** Ask the server about the call [callId] whose ring just stopped here; answered by [CallEvent.RingChecked]. */
    data class AskRing(val channelId: String, val callId: String) : CallEffect

    /** Post a missed call for [callId]. */
    data class Missed(val channelId: String, val callId: String, val callerId: String, val callerName: String?) : CallEffect

    /** Post a missed call for [callId] if its ring notification is the last thing shown for it. */
    data class MissedIfRinging(val channelId: String, val callId: String, val callerName: String?) : CallEffect

    /** Arm the 100 s ring cap; a later arm replaces it. */
    data class ArmRingCap(val channelId: String, val startedAtMs: Long) : CallEffect

    /** Arm the timer that turns [CallState.Ended] into [CallState.Idle]. */
    data object ArmEndedTimer : CallEffect
}

/** A ring this user did not get rung for because a call was already on (busy, like the web). */
data class BusyRing(val callId: String, val callerId: String, val callerName: String?)

/**
 * A ring that stopped here (its 100 s were over, or its notification timed out) before this
 * phone knew how the call ended for this user. [shown]: this owner showed it.
 */
data class OpenRing(
    val channelId: String,
    val callId: String,
    val callerId: String,
    val callerName: String?,
    val shown: Boolean,
)

/**
 * The machine's whole state: the shown call, the ids of recently ended calls
 * per DM (their late events are ignored) and busy rings per DM.
 */
data class CallModel(
    val state: CallState = CallState.Idle,
    val ended: Map<String, List<String>> = emptyMap(),
    val busy: Map<String, BusyRing> = emptyMap(),
    /** Calls this device already told the server it left (a leave goes out once per call). */
    val left: List<String> = emptyList(),
    /** The last voice id handed out; every call gets its own. */
    val voiceSeq: Long = 0L,
    /** How recently ended calls ended, by call id (for a start whose answer comes after its call's end). */
    val endedWhy: Map<String, CallEndReason> = emptyMap(),
    /**
     * Rings that stopped here without a missed call being claimed: a missed call is only
     * claimed once the server said that the call ended without this user answering or
     * declining it anywhere.
     */
    val openRings: List<OpenRing> = emptyList(),
    /**
     * Calls whose end this phone knows from the socket, without having been rung for them as
     * far as it knows: whether they were missed here is not decided yet. Their end push (it
     * goes only to those the call rang) decides, once.
     */
    val undecided: List<String> = emptyList(),
)

/** One transition: the next model and what to do. */
data class CallStep(val model: CallModel, val effects: List<CallEffect> = emptyList())

/**
 * The DM call state machine, pure: no Android, no coroutines,
 * no clock of its own. [CallController] owns the one instance of the model and
 * runs the effects.
 */
object CallMachine {
    /** Ended call ids remembered per DM. */
    const val MAX_ENDED = 5

    /**
     * Whether a call this phone rang for is a missed call, decided by the call's end
     * [reason] and what this phone knows of its own user: [handledHere] = it saw this user
     * answer or decline the call (on any device: a socket event, or the server's answer to
     * its question). An end that says nobody answered (`no_answer`, `cancelled`) is a missed
     * call, for a one-to-one and a group call alike, unless handled here; an end that says
     * somebody answered or everybody declined is none; no end reason (null) is no knowledge
     * and claims nothing. The only rule for every way a missed call is posted.
     *
     * Known limit: the server does not tell a phone without a connection that this user
     * declined a group call on another device; such a call then shows as missed.
     */
    fun missedOnEnd(reason: CallEndReason?, handledHere: Boolean): Boolean =
        !handledHere && (reason == CallEndReason.NoAnswer || reason == CallEndReason.Cancelled)

    /** A ring ends locally this long after it started, whatever the server said. */
    const val RING_CAP_MS = 100_000L

    /** How long the ended line is shown. */
    const val ENDED_SHOWN_MS = 3_000L

    fun reduce(model: CallModel, event: CallEvent, me: String?): CallStep {
        if (me == null) return CallStep(model)
        return Reducer(model, me).apply(event)
    }

    /**
     * Whether a call session may rejoin: the server's call
     * is the one shown and still active with this user in it, or still ringing
     * and started by this user.
     */
    fun mayRejoin(state: CallState, channelId: String, answer: CallSnapshot?, me: String): Boolean {
        if (state.channelId != channelId) return false
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
        var left = model.left
        var voiceSeq = model.voiceSeq
        var endedWhy = model.endedWhy
        var openRings = model.openRings
        var undecided = model.undecided

        fun to(next: CallState): CallStep =
            CallStep(CallModel(next, ended, busy, left, voiceSeq, endedWhy, openRings, undecided), effects.toList())
        fun same(): CallStep = to(state)

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
            is CallEvent.HoldRefused -> holdRefused(event)
            is CallEvent.Decline -> decline(event)
            is CallEvent.HangUp -> hangUp(event)
            is CallEvent.SetMuted -> setMuted(event)
            is CallEvent.MuteToggled -> if (shownVoiceId() == event.voiceId) mute(!shownMuted()) else same()
            is CallEvent.ParticipantJoined -> participantJoined(event)
            is CallEvent.ParticipantLeft -> participantLeft(event)
            is CallEvent.Declined -> declined(event)
            is CallEvent.ServerEnded -> serverEnded(event)
            is CallEvent.RingCap -> ringCap(event)
            is CallEvent.VoiceEnded -> voiceEnded(event)
            is CallEvent.Resynced -> resynced(event)
            is CallEvent.EndedShown ->
                if (state is CallState.Ended && (event.channelId == null || event.channelId == state.channelId)) {
                    to(CallState.Idle)
                } else {
                    same()
                }
            CallEvent.SocketReady -> socketReady()
            is CallEvent.BusyCap -> busyCap(e = event)
            is CallEvent.RingExpired -> ringExpired(event)
            is CallEvent.RingChecked -> ringChecked(event)
        }

        fun start(e: CallEvent.Start): CallStep {
            if (!idle) return same()
            val voiceId = ++voiceSeq
            // The microphone service first, while the app is in front.
            effects += CallEffect.HoldVoice(e.channelId, voiceId)
            effects += CallEffect.RestStart(e.channelId, e.nowMs)
            effects += CallEffect.ArmRingCap(e.channelId, e.nowMs)
            return to(CallState.Outgoing(e.channelId, null, e.nowMs, voiceId = voiceId))
        }

        fun startOk(e: CallEvent.StartOk): CallStep {
            val s = state as? CallState.Outgoing
            if (s != null && s.channelId == e.channelId && s.startedAtMs == e.startedAtMs) {
                // The id may already be known (a resync or the other side's answer came first).
                return learnOutgoingId(s, e.callId)
            }
            // Still the shown call under another state (connected before the answer): keep it.
            if (state.channelId == e.channelId && shownCallId() == e.callId) return same()
            // The start this answers is no longer shown (hung up, capped, replaced): leave that call,
            // or it would ring on the server with nobody behind it. Not a call that is known to
            // be over (the server ended it, or it was left already): nothing is left to leave.
            if (!isEnded(e.channelId, e.callId)) leave(e.channelId, e.callId)
            rememberEnded(e.channelId, e.callId)
            return same()
        }

        /** The id of the shown call, if a call is shown and its id is known. */
        fun shownCallId(): String? = when (state) {
            is CallState.Outgoing -> state.callId
            is CallState.Incoming -> state.callId
            is CallState.Connecting -> state.callId
            is CallState.Connected -> state.callId
            else -> null
        }

        /**
         * The outgoing call [s] is about to take the id [callId], and that call is already
         * known to be over: its end came from the server while the id was unknown and could
         * only be recorded. The call is over here too, with that end's reason; no voice is
         * joined for it and nothing is left to leave. Null: not so. Every way an outgoing call
         * takes an id ([learnOutgoingId], [outgoingConnected]) asks this first.
         */
        fun overBeforeKnown(s: CallState.Outgoing, callId: String?): CallStep? {
            if (s.callId != null || callId.isNullOrEmpty() || !isEnded(s.channelId, callId)) return null
            effects += CallEffect.LeaveVoice(s.channelId, s.voiceId)
            return endedState(s.channelId, endedWhy[callId] ?: CallEndReason.Ended)
        }

        /** [s] with its id known; the voice join is asked for once, when the id becomes known. */
        fun learnOutgoingId(s: CallState.Outgoing, callId: String): CallStep {
            overBeforeKnown(s, callId)?.let { return it }
            if (!s.voice) effects += CallEffect.JoinVoice(s.channelId, callId, s.voiceId)
            return to(s.copy(callId = s.callId ?: callId, voice = true))
        }

        /** The caller's call connected: its voice join is asked for if it was not yet. */
        fun outgoingConnected(s: CallState.Outgoing, callId: String?, nowMs: Long, others: Set<String>): CallStep {
            overBeforeKnown(s, callId)?.let { return it }
            val id = s.callId ?: callId
            if (!s.voice) effects += CallEffect.JoinVoice(s.channelId, id, s.voiceId)
            return to(CallState.Connected(s.channelId, id, nowMs, others, s.voiceId, s.muted))
        }

        fun startFailed(e: CallEvent.StartFailed): CallStep {
            val s = state as? CallState.Outgoing ?: return same()
            if (s.channelId != e.channelId || s.callId != null || s.startedAtMs != e.startedAtMs) return same()
            // Glare: the other side's call exists; its ring arrives as incoming_call.
            if (e.glare) return to(CallState.Idle)
            return endedState(e.channelId, if (e.blocked) CallEndReason.Blocked else CallEndReason.StartFailed)
        }

        fun rang(e: CallEvent.Rang): CallStep {
            if (e.callerId == me || isEnded(e.channelId, e.callId)) return same()
            val incoming = CallState.Incoming(
                e.channelId, e.callId, e.callerId, e.callerName, e.nowMs, verified = !e.fromNotification,
            )
            // A notification that is already up is not posted (and does not alert) again. What it
            // says is not known from the server: it is asked at once.
            fun ring() {
                if (e.fromNotification) {
                    effects += CallEffect.Resync(e.channelId, incoming)
                } else {
                    effects += CallEffect.Ring(e.channelId, e.callId, e.callerId, e.callerName)
                }
                effects += CallEffect.ArmRingCap(e.channelId, e.nowMs)
                // A ring that was busy before is the shown one now.
                if (busy[e.channelId]?.callId == e.callId) busy = busy - e.channelId
            }
            when (state) {
                is CallState.Incoming -> if (state.channelId == e.channelId && state.callId == e.callId) {
                    // A repeat (socket and push both): keep the first, learn a missing name. The
                    // server saying so makes a ring known from its notification a verified one.
                    val verified = state.verified || !e.fromNotification
                    if (state.callerName == null && e.callerName != null) {
                        effects += CallEffect.Ring(e.channelId, e.callId, e.callerId, e.callerName)
                        return to(state.copy(callerName = e.callerName, verified = verified))
                    }
                    return if (verified != state.verified) to(state.copy(verified = verified)) else same()
                } else if (state.channelId == e.channelId) {
                    // Another call rings in the same conversation. A conversation has one call at a
                    // time, so the one shown is over (its end did not get here): the new one rings.
                    rememberEnded(state.channelId, state.callId)
                    ring()
                    return to(incoming)
                }
                // Glare: our start has no id yet, their call rings for the same DM: theirs wins.
                is CallState.Outgoing -> if (state.channelId == e.channelId && state.callId == null) {
                    ring()
                    return to(incoming)
                }
                else -> if (idle) {
                    ring()
                    return to(incoming)
                }
            }
            // Busy: already in or ringing for another call. Not rung (as on the web); missed
            // if it ends unanswered or after its own 100 s.
            if (busy[e.channelId]?.callId != e.callId) {
                busy = busy + (e.channelId to BusyRing(e.callId, e.callerId, e.callerName))
                effects += CallEffect.ArmBusyCap(e.channelId, e.callId, e.nowMs)
            }
            return same()
        }

        fun accept(e: CallEvent.Accept): CallStep {
            val s = state as? CallState.Incoming ?: return same()
            // Only the ring that was accepted: another call ringing there now is not answered.
            if (s.channelId != e.channelId || s.callId != e.callId) return same()
            val voiceId = ++voiceSeq
            effects += CallEffect.StopRing(s.channelId, s.callId)
            // The microphone service first, while the screen the user answered on is in front.
            effects += CallEffect.HoldVoice(s.channelId, voiceId)
            effects += CallEffect.RestJoin(s.channelId, s.callId, verify = !s.verified)
            // Everyone seen answering while it rang is in the call too (group calls), and the
            // caller unless they left meanwhile.
            val others = if (s.callerLeft) s.joined else s.joined + s.callerId
            return to(CallState.Connecting(s.channelId, s.callId, others, voiceId = voiceId))
        }

        fun joinOk(e: CallEvent.JoinOk): CallStep {
            val s = state as? CallState.Connecting
            if (s != null && s.channelId == e.channelId && s.callId == e.callId) {
                if (s.voice) return same()
                effects += CallEffect.JoinVoice(s.channelId, s.callId, s.voiceId)
                // The server's list is the truth about who was in the call when it answered.
                // Whoever joined or left while that answer was on its way is newer than it.
                val listed = if (e.participants.isEmpty()) s.others else e.participants - me
                val others = listed - s.changed.filterValues { !it }.keys + s.changed.filterValues { it }.keys
                return to(s.copy(others = others, voice = true, changed = emptyMap()))
            }
            // An answer accepted for a call no longer shown here (hung up before it came):
            // leave it, unless a newer call of that conversation is shown (the old one is over).
            if (state.channelId != e.channelId || shownCallId() == null) {
                leave(e.channelId, e.callId)
                rememberEnded(e.channelId, e.callId)
            }
            return same()
        }

        fun joinFailed(e: CallEvent.JoinFailed): CallStep {
            val s = state as? CallState.Connecting ?: return same()
            if (s.channelId != e.channelId || s.callId != e.callId || s.voice) return same()
            rememberEnded(s.channelId, s.callId)
            return endedState(s.channelId, if (e.gone) CallEndReason.Gone else CallEndReason.Failed)
        }

        fun voiceJoined(e: CallEvent.VoiceJoined): CallStep {
            val s = state as? CallState.Connecting ?: return same()
            if (s.channelId != e.channelId || e.voiceId != s.voiceId) return same()
            // Everyone else left while this phone was still joining: the call is over here.
            if (s.others.isEmpty()) return leaveCall(s.channelId, s.callId, s.voiceId, CallEndReason.Ended)
            return to(CallState.Connected(s.channelId, s.callId, e.nowMs, s.others, s.voiceId, s.muted))
        }

        fun voiceJoinFailed(e: CallEvent.VoiceJoinFailed): CallStep {
            val (channelId, callId) = when (state) {
                is CallState.Outgoing -> state.channelId to state.callId
                is CallState.Connecting -> state.channelId to state.callId
                // The caller's call can connect while its voice join still runs.
                is CallState.Connected -> state.channelId to state.callId
                else -> return same()
            }
            if (channelId != e.channelId || e.voiceId != shownVoiceId()) return same()
            // A start that has not answered yet is left when its answer comes (see startOk).
            if (callId != null || state !is CallState.Outgoing) leave(channelId, callId)
            rememberEnded(channelId, callId)
            return endedState(channelId, CallEndReason.Failed)
        }

        /**
         * No microphone for this call. Nothing of it reached the server from this phone, so
         * there is nothing to leave: an incoming call goes on ringing on this user's other
         * devices (here it is over and does not ring again), an outgoing one was never started.
         */
        fun holdRefused(e: CallEvent.HoldRefused): CallStep {
            val callId = when (state) {
                is CallState.Outgoing -> state.callId
                is CallState.Connecting -> state.callId
                is CallState.Connected -> state.callId
                else -> return same()
            }
            if (state.channelId != e.channelId || e.voiceId != shownVoiceId()) return same()
            rememberEnded(e.channelId, callId)
            return endedState(e.channelId, CallEndReason.Failed)
        }

        fun decline(e: CallEvent.Decline): CallStep {
            val s = state as? CallState.Incoming
            if (s != null && s.channelId == e.channelId) {
                // Another call rings there now: the declined one is over, and this one is not it.
                if (s.callId != e.callId) return same()
                effects += CallEffect.StopRing(s.channelId, s.callId)
                effects += CallEffect.RestDecline(s.channelId, s.callId, verify = !s.verified)
                rememberEnded(s.channelId, s.callId)
                return to(CallState.Idle)
            }
            // Not the shown ring. A conversation has one call at a time: if a call of it is shown
            // here, or the declined one is known to be over, there is nothing to decline.
            if (state.channelId == e.channelId && !idle) return same()
            if (e.callId.isEmpty() || isEnded(e.channelId, e.callId)) return same()
            // A ring this owner does not show (its notification outlived the process that
            // posted it): the server is asked whether it is still that call before declining.
            if (busy[e.channelId]?.callId == e.callId) busy = busy - e.channelId
            effects += CallEffect.StopRing(e.channelId, e.callId)
            effects += CallEffect.RestDecline(e.channelId, e.callId, verify = true)
            rememberEnded(e.channelId, e.callId)
            return same()
        }

        /** The voice id of the shown call, if it has one. */
        fun shownVoiceId(): Long? = when (state) {
            is CallState.Outgoing -> state.voiceId
            is CallState.Connecting -> state.voiceId
            is CallState.Connected -> state.voiceId
            else -> null
        }

        fun shownMuted(): Boolean = when (state) {
            is CallState.Outgoing -> state.muted
            is CallState.Connecting -> state.muted
            is CallState.Connected -> state.muted
            else -> false
        }

        /** The call screen's Mute, for the call it was shown for. */
        fun setMuted(e: CallEvent.SetMuted): CallStep {
            if (state.channelId != e.channelId) return same()
            val shown = shownCallId()
            if (e.callId != null && shown != null && shown != e.callId) return same()
            return mute(e.muted)
        }

        /**
         * The call's mute: kept with the call from the first screen on (also before its voice
         * session exists) and applied to that session's microphone only.
         */
        fun mute(muted: Boolean): CallStep {
            val voiceId = shownVoiceId() ?: return same()
            if (shownMuted() == muted) return same()
            effects += CallEffect.ApplyMute(voiceId, muted)
            return when (state) {
                is CallState.Outgoing -> to(state.copy(muted = muted))
                is CallState.Connecting -> to(state.copy(muted = muted))
                is CallState.Connected -> to(state.copy(muted = muted))
                else -> same()
            }
        }

        fun hangUp(e: CallEvent.HangUp): CallStep {
            // Only the call the button was shown for.
            if (state.channelId != e.channelId) return same()
            val shown = shownCallId()
            if (e.callId != null && shown != null && shown != e.callId) return same()
            return when (state) {
                is CallState.Outgoing -> {
                    // The server cancels a ringing call on the caller's leave. Before the start
                    // answered, the leave waits for its answer (a leave now would miss the call).
                    effects += CallEffect.LeaveVoice(state.channelId, state.voiceId)
                    if (state.callId != null) leave(state.channelId, state.callId)
                    rememberEnded(state.channelId, state.callId)
                    endedState(state.channelId, CallEndReason.Left)
                }
                is CallState.Connecting -> if (state.voice) {
                    leaveCall(state.channelId, state.callId, state.voiceId, CallEndReason.Left)
                } else {
                    // The answer is not accepted yet: a leave now would reach the server before the
                    // join. The join's late acceptance is left instead (see joinOk).
                    effects += CallEffect.LeaveVoice(state.channelId, state.voiceId)
                    rememberEnded(state.channelId, state.callId)
                    endedState(state.channelId, CallEndReason.Left)
                }
                is CallState.Connected -> leaveCall(state.channelId, state.callId, state.voiceId, CallEndReason.Left)
                else -> same()
            }
        }

        /** Tells the server this user leaves the call [callId] (once per call id). */
        fun leave(channelId: String, callId: String?) {
            if (callId != null && callId in left) return
            if (callId != null) left = (left - callId + callId).takeLast(MAX_ENDED * 2)
            effects += CallEffect.RestLeave(channelId)
        }

        fun leaveCall(channelId: String, callId: String?, voiceId: Long, reason: CallEndReason): CallStep {
            effects += CallEffect.LeaveVoice(channelId, voiceId)
            leave(channelId, callId)
            rememberEnded(channelId, callId)
            return endedState(channelId, reason)
        }

        fun participantJoined(e: CallEvent.ParticipantJoined): CallStep {
            when (state) {
                is CallState.Incoming -> if (matches(e.channelId, state.channelId, state.callId, e.callId)) {
                    if (e.userId == me) {
                        // Answered on another device of this user.
                        effects += CallEffect.StopRing(state.channelId, state.callId)
                        rememberEnded(state.channelId, state.callId)
                        return to(CallState.Idle)
                    }
                    // Someone else answered a group call: it still rings here, and they are in it.
                    return to(state.copy(joined = state.joined + e.userId))
                }
                is CallState.Outgoing -> if (matches(e.channelId, state.channelId, state.callId, e.callId) && e.userId != me) {
                    // Answered before the start's own answer came: connected, and the voice joins now.
                    return outgoingConnected(state, e.callId.ifEmpty { null }, e.nowMs, setOf(e.userId))
                }
                is CallState.Connecting -> if (matches(e.channelId, state.channelId, state.callId, e.callId) && e.userId != me) {
                    val changed = if (state.voice) state.changed else state.changed + (e.userId to true)
                    return to(state.copy(others = state.others + e.userId, changed = changed))
                }
                is CallState.Connected -> if (matches(e.channelId, state.channelId, state.callId, e.callId) && e.userId != me) {
                    return to(state.copy(others = state.others + e.userId))
                }
                else -> Unit
            }
            // A ring that was not rung here because a call was on: once anyone answered it (this
            // user elsewhere, or someone else in a group call) it is no unanswered call any more.
            if (e.userId == me) {
                // Answered by this user on another device: the busy ring stops as a ring.
                dropBusy(e.channelId, e.callId)?.let { effects += CallEffect.StopRing(e.channelId, it.callId) }
            } else if (e.callId.isNotEmpty()) {
                dropBusy(e.channelId, e.callId)
            }
            // Answered on another device after its ring had stopped here: not missed.
            if (e.userId == me) {
                closeOpenRing(e.channelId, e.callId)
                handledElsewhere(e.channelId, e.callId)
            }
            return same()
        }

        fun participantLeft(e: CallEvent.ParticipantLeft): CallStep {
            val ringing = state as? CallState.Incoming
            if (ringing != null && matches(e.channelId, ringing.channelId, ringing.callId, e.callId)) {
                if (e.userId == ringing.callerId) return to(ringing.copy(callerLeft = true))
                return if (e.userId in ringing.joined) to(ringing.copy(joined = ringing.joined - e.userId)) else same()
            }
            val joining = state as? CallState.Connecting
            if (joining != null && matches(e.channelId, joining.channelId, joining.callId, e.callId) && e.userId != me) {
                // Still joining: remembered; the join ends the call if nobody is left (voiceJoined).
                val changed = if (joining.voice) joining.changed else joining.changed + (e.userId to false)
                return to(joining.copy(others = joining.others - e.userId, changed = changed))
            }
            val s = state as? CallState.Connected ?: return same()
            if (!matches(e.channelId, s.channelId, s.callId, e.callId) || e.userId == me) return same()
            if (e.userId !in s.others) return same()
            val others = s.others - e.userId
            // The last other person left: this user is alone, the call is over.
            if (others.isEmpty()) return leaveCall(s.channelId, s.callId, s.voiceId, CallEndReason.Ended)
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
            if (e.userId == me) {
                // Declined by this user on another device: the busy ring stops as a ring.
                dropBusy(e.channelId, e.callId)?.let { effects += CallEffect.StopRing(e.channelId, it.callId) }
                // Declined on another device after its ring had stopped here: not missed.
                closeOpenRing(e.channelId, e.callId)
                handledElsewhere(e.channelId, e.callId)
            }
            return same()
        }

        /** Drops the busy ring of [channelId] if it is [callId] (or any, without an id); returns it. */
        fun dropBusy(channelId: String, callId: String): BusyRing? {
            val ring = busy[channelId] ?: return null
            if (callId.isNotEmpty() && ring.callId != callId) return null
            busy = busy - channelId
            return ring
        }

        fun serverEnded(e: CallEvent.ServerEnded): CallStep {
            val reason = serverReason(e.reason)
            // Known as over here, with how it ended for this user decided (an end known only
            // from the socket for a call not rung here as far as this phone knows is not).
            val knownEnded = isEnded(e.channelId, e.callId) && e.callId !in undecided
            // Whatever is shown: that call is over and never rings again. How it ended is kept
            // for a start of this user whose answer (with that id) is still on its way.
            rememberEnded(e.channelId, e.callId)
            if (e.callId.isNotEmpty()) {
                val kept = ended.values.flatten().toSet()
                endedWhy = (endedWhy + (e.callId to reason)).filterKeys { it in kept }
            }
            // A ring that had stopped here before anything was known: this end says how the
            // call ended. (An open ring is one this phone knows no answer or decline of.)
            val wasOpen = openRing(e.channelId, e.callId) != null
            openRing(e.channelId, e.callId)?.let { ring ->
                openRings = openRings - ring
                effects += if (missedOnEnd(reason, handledHere = false)) {
                    CallEffect.Missed(ring.channelId, ring.callId, ring.callerId, ring.callerName ?: e.callerName)
                } else {
                    CallEffect.StopRing(ring.channelId, ring.callId)
                }
            }
            var wasBusy = false
            busy[e.channelId]?.let { ring ->
                if (e.callId.isEmpty() || ring.callId == e.callId) {
                    wasBusy = true
                    busy = busy - e.channelId
                    // A busy ring is dropped when this user answers or declines it anywhere seen here.
                    effects += if (missedOnEnd(reason, handledHere = false)) {
                        CallEffect.Missed(e.channelId, ring.callId, ring.callerId, ring.callerName)
                    } else {
                        // Over without a missed call: it stops as a ring (its end is recorded).
                        CallEffect.StopRing(e.channelId, ring.callId)
                    }
                }
            }
            val shown = when (state) {
                is CallState.Incoming -> matches(e.channelId, state.channelId, state.callId, e.callId)
                is CallState.Outgoing -> matches(e.channelId, state.channelId, state.callId, e.callId)
                is CallState.Connecting -> matches(e.channelId, state.channelId, state.callId, e.callId)
                is CallState.Connected -> matches(e.channelId, state.channelId, state.callId, e.callId)
                else -> false
            }
            if (!shown) {
                // A ring this owner never saw or no longer shows (its notification may still be
                // up from an earlier process): the wake says it rang here. A call known here as
                // over was decided already (its answer or decline here or elsewhere was seen).
                if (e.rungHere && !wasBusy && !knownEnded && e.callId.isNotEmpty()) {
                    undecided = undecided - e.callId
                    effects += CallEffect.StopRing(e.channelId, e.callId)
                    if (missedOnEnd(reason, handledHere = false)) {
                        effects += CallEffect.Missed(e.channelId, e.callId, e.callerId.orEmpty(), e.callerName)
                    }
                } else if (!e.rungHere && !wasBusy && !wasOpen && !knownEnded && e.callId.isNotEmpty()) {
                    // From the socket, for a call this phone does not know it was rung for: its
                    // end push (sent only to those it rang) decides.
                    undecided = (undecided - e.callId + e.callId).takeLast(MAX_ENDED)
                }
                return same()
            }
            return when (state) {
                is CallState.Incoming -> {
                    effects += CallEffect.StopRing(state.channelId, state.callId)
                    // Still ringing: an answer or decline of this user seen here would have ended it.
                    if (missedOnEnd(reason, handledHere = false)) {
                        effects += CallEffect.Missed(state.channelId, state.callId, state.callerId, state.callerName)
                    }
                    rememberEnded(state.channelId, state.callId)
                    to(CallState.Idle)
                }
                is CallState.Outgoing -> {
                    effects += CallEffect.LeaveVoice(state.channelId, state.voiceId)
                    rememberEnded(state.channelId, state.callId ?: e.callId)
                    endedState(state.channelId, reason)
                }
                is CallState.Connecting -> {
                    effects += CallEffect.LeaveVoice(state.channelId, state.voiceId)
                    rememberEnded(state.channelId, state.callId)
                    endedState(state.channelId, if (reason == CallEndReason.Cancelled) CallEndReason.Gone else CallEndReason.Ended)
                }
                is CallState.Connected -> {
                    effects += CallEffect.LeaveVoice(state.channelId, state.voiceId)
                    rememberEnded(state.channelId, state.callId)
                    endedState(state.channelId, CallEndReason.Ended)
                }
                else -> same()
            }
        }

        fun openRing(channelId: String, callId: String): OpenRing? =
            if (callId.isEmpty()) null else openRings.firstOrNull { it.channelId == channelId && it.callId == callId }

        /** This user answered or declined the call of an open ring on another device: over, not missed. */
        fun closeOpenRing(channelId: String, callId: String) {
            val ring = openRing(channelId, callId) ?: return
            openRings = openRings - ring
            effects += CallEffect.StopRing(ring.channelId, ring.callId)
        }

        /**
         * The ring of [callId] is over here (its 100 s, or its notification timed out) and the
         * server has not said how the call ended. It stops ringing and never rings here again.
         * Whether it was missed is not known: this user may have answered or declined it on
         * another device, which a phone without a connection does not hear. So nothing is
         * claimed: the server is asked ([ringChecked], unless [ask] is false because it was
         * just asked), and the ring is kept as an open one until the call's end says by its
         * reason ([missedOnEnd]) or this user's own answer or decline is seen.
         */
        fun ringOver(channelId: String, callId: String, callerId: String, callerName: String?, shown: Boolean, ask: Boolean = true) {
            // A ring this owner does not show has lost its notification already (it timed out).
            if (shown) effects += CallEffect.SilenceRing(channelId, callId)
            if (ask) effects += CallEffect.AskRing(channelId, callId)
            val ring = OpenRing(channelId, callId, callerId, callerName, shown)
            openRings = (openRings.filterNot { it.channelId == channelId && it.callId == callId } + ring).takeLast(MAX_ENDED)
            rememberEnded(channelId, callId)
        }

        /**
         * The server's answer about an open ring's call. Declined by this user, or active
         * with this user in it: this phone now knows its user handled the call elsewhere, so
         * the ring is over without a missed call. Anything else (gone, still ringing, active
         * without this user, or not asked) says no end reason: nothing is claimed, and the
         * ring stays open for the call's end to decide by its reason ([missedOnEnd]).
         */
        fun ringChecked(e: CallEvent.RingChecked): CallStep {
            val ring = openRing(e.channelId, e.callId) ?: return same()
            val answer = e.answer?.takeIf { e.asked && it.callId == ring.callId }
            val handledHere = answer != null && (me in answer.declinedBy || answer.active && me in answer.participants)
            if (!handledHere) return same()
            openRings = openRings - ring
            effects += CallEffect.StopRing(ring.channelId, ring.callId)
            return same()
        }

        /**
         * The ring's notification timed out. For the shown ring that is the ring cap; a ring
         * this owner does not show (its notification outlived the process that posted it) is
         * over the same way.
         */
        fun ringExpired(e: CallEvent.RingExpired): CallStep {
            val s = state as? CallState.Incoming
            if (s != null && s.channelId == e.channelId && s.callId == e.callId) {
                ringOver(s.channelId, s.callId, s.callerId, s.callerName, shown = true)
                return to(CallState.Idle)
            }
            if (e.callId.isEmpty() || isEnded(e.channelId, e.callId)) return same()
            if (state.channelId == e.channelId && !idle) return same()
            ringOver(e.channelId, e.callId, "", e.callerName, shown = false)
            return same()
        }

        fun ringCap(e: CallEvent.RingCap): CallStep = when (state) {
            is CallState.Incoming -> if (state.channelId == e.channelId && state.startedAtMs == e.startedAtMs) {
                // An incoming ring ends locally; nothing is sent, and nothing is claimed yet.
                ringOver(state.channelId, state.callId, state.callerId, state.callerName, shown = true)
                to(CallState.Idle)
            } else same()
            is CallState.Outgoing -> if (state.channelId == e.channelId && state.startedAtMs == e.startedAtMs) {
                effects += CallEffect.LeaveVoice(state.channelId, state.voiceId)
                // Before the start answered, its answer is left when it comes (see startOk).
                if (state.callId != null) leave(state.channelId, state.callId)
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
            // Only this call's own voice session: the end of an earlier call's session (also one
            // in the same conversation) says nothing about this call.
            if (shownVoiceId() != e.voiceId) return same()
            rememberEnded(channelId, callId)
            return when (val why = e.why) {
                // The gate's answer follows as a resync; the call is simply over here.
                VoiceEnd.Refused -> to(CallState.Idle)
                // A start that has not answered yet is left when its answer comes (see startOk).
                VoiceEnd.Left -> {
                    if (callId != null || state !is CallState.Outgoing) leave(channelId, callId)
                    endedState(channelId, CallEndReason.Left)
                }
                VoiceEnd.Lost -> {
                    if (callId != null || state !is CallState.Outgoing) leave(channelId, callId)
                    endedState(channelId, CallEndReason.ConnectionLost)
                }
                is VoiceEnd.Removed -> {
                    // No rejoin, and no leave: the server already removed the peer, and a leave
                    // removes the user, which would end a call taken over on another device.
                    endedState(channelId, CallEndReason.Removed, why.reason)
                }
            }
        }

        /** A new socket: ask the server about the call shown (stamped with this state). */
        fun socketReady(): CallStep {
            // A ring that stopped here while nothing could be asked: the server can be asked now.
            for (ring in openRings) effects += CallEffect.AskRing(ring.channelId, ring.callId)
            val asked = when (state) {
                is CallState.Incoming, is CallState.Outgoing, is CallState.Connecting, is CallState.Connected -> state
                else -> return same()
            }
            effects += CallEffect.Resync(asked.channelId!!, asked)
            return same()
        }

        /**
         * The 100 s of a ring that did not ring here because a call was on are over. That says
         * nothing about how the call ended: the ring is kept open, and the call's end decides by
         * its reason ([missedOnEnd]), as for any ring that stopped here.
         */
        fun busyCap(e: CallEvent.BusyCap): CallStep {
            val ring = busy[e.channelId]?.takeIf { it.callId == e.callId } ?: return same()
            busy = busy - e.channelId
            openRings = (openRings.filterNot { it.channelId == e.channelId && it.callId == e.callId } +
                OpenRing(e.channelId, ring.callId, ring.callerId, ring.callerName, shown = false)).takeLast(MAX_ENDED)
            rememberEnded(e.channelId, e.callId)
            return same()
        }

        /**
         * This user answered or declined, on another device, a call this owner does not show:
         * remembered, so its end shows no missed call here.
         */
        fun handledElsewhere(channelId: String, callId: String) {
            if (callId.isEmpty()) return
            if (state.channelId == channelId && (shownCallId() == null || shownCallId() == callId)) return
            undecided = undecided - callId
            rememberEnded(channelId, callId)
        }

        fun resynced(e: CallEvent.Resynced): CallStep {
            // An answer for a state that has changed since it was asked says nothing about the present.
            if (e.askedFor != null && e.askedFor != state) return same()
            val answer = e.answer
            when (state) {
                is CallState.Incoming -> {
                    if (state.channelId != e.channelId) return same()
                    val sameCall = answer != null && answer.callId == state.callId
                    return when {
                        // The ring vanished while offline. The server's "no call" gives no end
                        // reason: the ring stops without a claim, and the call's end decides.
                        !sameCall -> {
                            ringOver(state.channelId, state.callId, state.callerId, state.callerName, shown = true, ask = false)
                            to(CallState.Idle)
                        }
                        answer!!.active && me in answer.participants || me in answer.declinedBy -> {
                            effects += CallEffect.StopRing(state.channelId, state.callId)
                            rememberEnded(state.channelId, state.callId)
                            to(CallState.Idle)
                        }
                        // Still ringing, says the server: a ring known from its notification is verified.
                        else -> if (state.verified) same() else to(state.copy(verified = true))
                    }
                }
                is CallState.Outgoing -> {
                    if (state.channelId != e.channelId) return same()
                    if (state.callId == null) {
                        // The start is still in flight. Only a call that is clearly ours is adopted:
                        // ringing and started by this user, or active with this user in it. "No call"
                        // or someone else's call (the other side called first) changes nothing; the
                        // start's own answer decides.
                        val ours = answer != null &&
                            ((answer.ringing && answer.startedBy == me) || (answer.active && me in answer.participants))
                        if (!ours) return same()
                    }
                    val sameCall = answer != null && (state.callId == null || answer.callId == state.callId)
                    if (!sameCall) return silentEnd(state.channelId, state.callId, state.voiceId)
                    if (answer!!.active) {
                        return outgoingConnected(state, answer.callId, e.nowMs, answer.participants - me)
                    }
                    return learnOutgoingId(state, answer.callId)
                }
                is CallState.Connecting, is CallState.Connected -> {
                    if (state.channelId != e.channelId) return same()
                    val callId = if (state is CallState.Connecting) state.callId else (state as CallState.Connected).callId
                    // "Still ringing" for this call is older than what this phone saw (an answer or
                    // its own join turn a call active on the server at once): unchanged.
                    if (answer != null && answer.callId == callId && answer.ringing) return same()
                    // This user's own answer is still on its way: the server's look may be older
                    // than it, so "active without me" for this call says nothing. Gone or another
                    // call still ends it; a call that really ended ends through the answer or its end.
                    if (state is CallState.Connecting && !state.voice && answer != null &&
                        answer.callId == callId && answer.active && me !in answer.participants
                    ) {
                        return same()
                    }
                    val stillIn = answer != null && (callId == null || answer.callId == callId) &&
                        answer.active && me in answer.participants
                    if (!stillIn) return silentEnd(e.channelId, callId, shownVoiceId() ?: 0L)
                    if (state is CallState.Connected) return to(state.copy(others = answer!!.participants - me))
                    return same()
                }
                else -> return same()
            }
        }

        /** "No call" clears the local state silently. */
        fun silentEnd(channelId: String, callId: String?, voiceId: Long): CallStep {
            effects += CallEffect.LeaveVoice(channelId, voiceId)
            rememberEnded(channelId, callId)
            return to(CallState.Idle)
        }
    }
}
