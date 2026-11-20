package io.wolftown.kaiku.data.call

import io.ktor.http.HttpStatusCode
import io.wolftown.kaiku.data.api.ApiException
import io.wolftown.kaiku.data.api.CallApi
import io.wolftown.kaiku.data.api.CallStateDto
import io.wolftown.kaiku.data.repository.VoiceRepository
import io.wolftown.kaiku.data.session.SessionGeneration
import io.wolftown.kaiku.data.voice.ElapsedClock
import io.wolftown.kaiku.data.voice.RejoinVerdict
import io.wolftown.kaiku.data.voice.VoiceRemovedException
import io.wolftown.kaiku.data.ws.KaikuWebSocket
import io.wolftown.kaiku.data.ws.ServerEvent
import io.wolftown.kaiku.di.VoiceCoroutineScope
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.CoroutineStart
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancelAndJoin
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.currentCoroutineContext
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch
import kotlinx.coroutines.selects.select
import java.util.concurrent.atomic.AtomicBoolean
import java.util.logging.Level
import java.util.logging.Logger
import javax.inject.Inject
import javax.inject.Singleton

/**
 * Posts and removes the call notifications (the Android side is
 * `SocketCallNotifier`). Every call names the signed-in session it is for:
 * nothing is posted for a session that is over.
 */
interface CallNotifier {
    /** The incoming-call notification: ringtone, full-screen where allowed, Accept and Decline. */
    fun ring(session: SessionGeneration.Ticket, channelId: String, callId: String, callerId: String, callerName: String?)

    fun stopRing(session: SessionGeneration.Ticket, channelId: String, callId: String)

    /** A missed call (once per call; tap opens the DM). */
    fun missed(session: SessionGeneration.Ticket, channelId: String, callId: String, callerName: String?)

    /** A missed call for a ring whose notification timed out, unless something else was said about that call since. */
    fun missedIfRinging(session: SessionGeneration.Ticket, channelId: String, callId: String, callerName: String?)

    /** Every ring of this process (sign-out). */
    fun stopAll()
}

/** Names for the call screens and notifications, from the DM list. */
interface CallDirectory {
    /** Display name of [userId] in DM [channelId], or null. */
    fun nameOf(channelId: String, userId: String): String?

    /** The DM's title as the chat list shows it (the other person, or the group), or null. */
    fun titleOf(channelId: String): String?
}

/**
 * Which call something is for: the account it rings or runs for, the
 * conversation and the call. [callId] is null only for an outgoing call
 * whose start has not been answered yet.
 */
data class CallRef(val account: String, val channelId: String, val callId: String?)

/**
 * What comes to the call owner from outside ([CallController.submit]): the
 * screens, the notification actions, permission results and the push
 * wakes. Each says which account, conversation and call it is for; what
 * does not fit the owner's call is dropped there.
 */
sealed interface CallInput {
    val account: String

    data class Start(override val account: String, val channelId: String) : CallInput

    /** Accept of exactly this ring; the microphone permission is granted. */
    data class Accept(val call: CallRef) : CallInput {
        override val account get() = call.account
    }

    data class Decline(val call: CallRef) : CallInput {
        override val account get() = call.account
    }

    data class HangUp(val call: CallRef) : CallInput {
        override val account get() = call.account
    }

    /** The ended line of [channelId] was seen. */
    data class DismissEnded(override val account: String, val channelId: String) : CallInput

    /**
     * A screen was opened from the ring's notification. [receivedAtMs] is
     * the wall-clock time the ring was received, if the notification says.
     */
    data class RingNotification(
        val call: CallRef,
        val callerId: String,
        val callerName: String?,
        val receivedAtMs: Long? = null,
    ) : CallInput {
        override val account get() = call.account
    }

    /** The ring's notification timed out. */
    data class RingExpired(val call: CallRef, val callerName: String?) : CallInput {
        override val account get() = call.account
    }

    /** A `ringing` push wake, received at [receivedAtMs] (wall clock). */
    data class PushRing(
        val call: CallRef,
        val callerId: String,
        val callerName: String?,
        val receivedAtMs: Long,
    ) : CallInput {
        override val account get() = call.account
    }

    /** An `ended` push wake with the wire [reason]. */
    data class PushEnded(
        val call: CallRef,
        val callerId: String,
        val callerName: String?,
        val reason: String,
    ) : CallInput {
        override val account get() = call.account
    }
}

/** The way into the call owner for the push path. */
interface CallDoor {
    /**
     * Hands [input] to the call owner of its account and returns once it was
     * handled (its notification posted or removed). False: nobody is signed
     * in as that account, so nothing was done.
     */
    suspend fun fromPush(input: CallInput): Boolean
}

/** The call shown and the account it is for, read in one step. */
data class ShownCall(val account: String?, val state: CallState) {
    /** The shown call as an action names it, or null when none is shown. */
    fun ref(): CallRef? {
        val channelId = state.channelId ?: return null
        val account = account ?: return null
        val callId = when (state) {
            is CallState.Outgoing -> state.callId
            is CallState.Incoming -> state.callId
            is CallState.Connecting -> state.callId
            is CallState.Connected -> state.callId
            else -> null
        }
        return CallRef(account, channelId, callId)
    }
}

/**
 * The one door into DM calls on this device, and the owner behind it.
 *
 * **One door.** Everything that concerns a call enters through here as a
 * typed input that says which account (and conversation and call) it is for:
 * the socket's call events (the account is the one of the socket's `Ready`),
 * the screens, notification actions and permission results ([submit]) and
 * the push wakes ([fromPush]). The door asks the session generation for that
 * account's ticket; without one the input is dropped.
 *
 * **One owner per signed-in session.** Behind the door one [Owner] holds
 * everything of the session's calls: the [CallMachine] model, the actor that
 * feeds it one event at a time, the request lanes, the timers and the voice
 * join. All of it runs in the ticket's coroutine scope, which the sign-out
 * hook cancels before the stored session changes, and every effect checks
 * the ticket first: nothing of a previous account runs, sends, publishes or
 * posts for the next one. Requests carry the session through the HTTP
 * client's required-session check.
 *
 * It never touches WebRTC: voice goes through [VoiceRepository.joinCall]
 * and [VoiceRepository.leaveCall] with a tag per call, and the voice owner
 * asks the call's gate before each rejoin.
 */
@Singleton
class CallController internal constructor(
    private val api: CallApi,
    private val voice: VoiceRepository,
    private val webSocket: KaikuWebSocket,
    private val sessions: SessionGeneration,
    private val notifier: CallNotifier,
    private val directory: CallDirectory,
    private val clock: ElapsedClock,
    voiceScope: CoroutineScope,
    /** Wall clock, only to place a push wake's receipt time on the elapsed clock. */
    private val wallClock: () -> Long,
) : CallDoor {

    @Inject
    constructor(
        api: CallApi,
        voice: VoiceRepository,
        webSocket: KaikuWebSocket,
        sessions: SessionGeneration,
        notifier: CallNotifier,
        directory: CallDirectory,
        clock: ElapsedClock,
        @VoiceCoroutineScope voiceScope: CoroutineScope,
    ) : this(api, voice, webSocket, sessions, notifier, directory, clock, voiceScope, System::currentTimeMillis)

    companion object {
        private val logger = Logger.getLogger("CallController")

        /** The ring cap wakes at least this often to compare the real clock (device sleep). */
        const val CAP_CHECK_MS = 5_000L

        /** Voice tags of one owner: its number times this, plus the call's voice id. */
        private const val TAGS_PER_OWNER = 1_000_000L

        private const val VOICE_LANE = "voice"
    }

    private val scope = CoroutineScope(voiceScope.coroutineContext + SupervisorJob(voiceScope.coroutineContext[Job]))
    private val started = AtomicBoolean(false)

    private val ownerLock = Any()

    /** The owner of the signed-in session's calls, or null. Guarded by [ownerLock]. */
    private var owner: Owner? = null
    private var owners = 0L

    private val _state = MutableStateFlow<CallState>(CallState.Idle)

    /** The call this device shows; written under [ownerLock], by the current owner or its reset. */
    val state: StateFlow<CallState> = _state.asStateFlow()

    /** The call shown and the account it is for. */
    fun shown(): ShownCall = synchronized(ownerLock) { ShownCall(owner?.ticket?.userId, _state.value) }

    /** The account of the authenticated socket (its `Ready`); only the socket collector writes it. */
    @Volatile
    private var socketAccount: String? = null

    /** Starts the observers; idempotent. Called at app start. */
    fun start() {
        if (!started.compareAndSet(false, true)) return
        // Inside the hook's advance: the old owner is gone before the stored session changes.
        sessions.onAdvance { reset() }
        // UNDISPATCHED: subscribed before start() returns (replay-0 flows).
        scope.launch(start = CoroutineStart.UNDISPATCHED) {
            webSocket.events.collect { onServerEvent(it) }
        }
        scope.launch(start = CoroutineStart.UNDISPATCHED) {
            voice.callEnds.collect { end -> synchronized(ownerLock) { owner }?.voiceEnded(end) }
        }
    }

    // -- The door ------------------------------------------------------------------

    /**
     * Hands [input] to the call owner of its account. False: dropped at the
     * door (nobody is signed in as that account, or the session is being
     * signed out and the input would start a call).
     */
    fun submit(input: CallInput): Boolean = offer(input, done = null) != null

    /**
     * Like [submit], and returns once the input was handled and a decline it
     * led to has reached the server (or the owner is gone).
     */
    suspend fun submitAndAwait(input: CallInput): Boolean {
        val done = CompletableDeferred<Unit>()
        val owner = offer(input, done) ?: return false
        select<Unit> {
            done.onAwait {}
            owner.gone.onAwait {}
        }
        return true
    }

    override suspend fun fromPush(input: CallInput): Boolean = submitAndAwait(input)

    private fun offer(input: CallInput, done: CompletableDeferred<Unit>?): Owner? {
        val ticket = sessions.enter(input.account) ?: return null
        val startsCall = input is CallInput.Start || input is CallInput.Accept ||
            input is CallInput.PushRing || input is CallInput.RingNotification
        // A session that is being signed out takes no new call.
        if (startsCall && !ticket.open) return null
        val owner = ownerFor(ticket) ?: return null
        val event = owner.eventOf(input) ?: return null
        return owner.takeIf { it.offer(event, done) }
    }

    /**
     * Signed-in user: runs synchronously in the session-end hook, after the
     * generation advanced. The owner of the ended session is dropped and its
     * call is gone from the screen at once; every ring goes.
     * (Voice and the microphone are ended by the voice layer's own part.)
     */
    fun endForSignOut() {
        reset()
        notifier.stopAll()
    }

    private fun reset() {
        val old = synchronized(ownerLock) {
            _state.value = CallState.Idle
            owner.also { owner = null }
        }
        old?.end()
    }

    /** The owner for [ticket]: the current one, or a new one once the previous session is over. */
    private fun ownerFor(ticket: SessionGeneration.Ticket): Owner? {
        var replaced: Owner? = null
        val current = synchronized(ownerLock) {
            owner?.takeIf { it.ticket == ticket } ?: run {
                // A sign-out advances the generation and then resets under this lock: a ticket
                // that is over here gets no owner, and one installed just before is reset.
                if (!ticket.alive) return null
                replaced = owner
                _state.value = CallState.Idle
                Owner(ticket, ++owners).also {
                    owner = it
                    it.start()
                }
            }
        }
        replaced?.end()
        return current
    }

    // -- Socket ------------------------------------------------------------------

    private fun onServerEvent(event: ServerEvent) {
        if (event is ServerEvent.Ready) socketAccount = event.userId
        if (event !is ServerEvent.Ready && event !is ServerEvent.IncomingCall &&
            event !is ServerEvent.CallParticipantJoined && event !is ServerEvent.CallParticipantLeft &&
            event !is ServerEvent.CallDeclined && event !is ServerEvent.CallEnded
        ) {
            return
        }
        // The socket's events belong to the account it authenticated as, and to nobody else.
        val account = socketAccount ?: return
        val ticket = sessions.enter(account) ?: return
        if (event is ServerEvent.Ready) {
            // Only an owner that exists has a call to check with the server.
            synchronized(ownerLock) { owner?.takeIf { it.ticket == ticket } }?.offer(CallEvent.SocketReady)
            return
        }
        if (event is ServerEvent.IncomingCall && !ticket.open) return
        val owner = ownerFor(ticket) ?: return
        val now = clock.nowMs()
        when (event) {
            is ServerEvent.IncomingCall -> owner.offer(
                CallEvent.Rang(
                    event.channelId,
                    known(event.callId),
                    event.initiator,
                    directory.nameOf(event.channelId, event.initiator) ?: event.initiatorName.ifBlank { null },
                    now,
                )
            )
            is ServerEvent.CallParticipantJoined ->
                owner.offer(CallEvent.ParticipantJoined(event.channelId, known(event.callId), event.userId, now))
            is ServerEvent.CallParticipantLeft ->
                owner.offer(CallEvent.ParticipantLeft(event.channelId, known(event.callId), event.userId))
            is ServerEvent.CallDeclined ->
                owner.offer(CallEvent.Declined(event.channelId, known(event.callId), event.userId))
            is ServerEvent.CallEnded ->
                owner.offer(CallEvent.ServerEnded(event.channelId, known(event.callId), event.reason))
            else -> Unit
        }
    }

    /** Request lanes that are still working, for the signed-in session (tests). */
    @androidx.annotation.VisibleForTesting
    internal fun busyLanes(): Int = synchronized(ownerLock) { owner }?.busyLanes() ?: 0

    // -- The owner -----------------------------------------------------------------

    private class Envelope(val event: CallEvent, val done: CompletableDeferred<Unit>?)

    /** One voice join of a call: cancelled by that call's leave, also before it started. */
    private class VoiceJob(val voiceId: Long) {
        private val lock = Any()
        private var job: Job? = null
        private var cancelled = false

        /** Starts [start] unless the join was cancelled first. */
        fun launch(start: () -> Job) = synchronized(lock) { if (!cancelled) job = start() }

        /** Cancels the join (now or before it starts) and returns it to wait for. */
        fun cancel(): Job? = synchronized(lock) {
            cancelled = true
            job?.also { it.cancel() }
        }
    }

    /**
     * Serial lanes that exist only while they have work: a lane and its
     * worker are created with the first task and removed, in one step, when
     * the last one is done. The requests of one conversation run one after
     * the other (a leave never overtakes its start or join), and so do the
     * voice joins and leaves of all calls (an old call's leave never ends a
     * newer call's voice).
     */
    private class Lanes(private val scope: CoroutineScope) {
        private val queues = HashMap<String, ArrayDeque<suspend () -> Unit>>()

        fun submit(key: String, task: suspend () -> Unit) {
            val first = synchronized(queues) {
                val queue = queues[key]
                if (queue != null) {
                    queue.addLast(task)
                    false
                } else {
                    queues[key] = ArrayDeque<suspend () -> Unit>().also { it.addLast(task) }
                    true
                }
            }
            if (first) scope.launch { drain(key) }
        }

        private suspend fun drain(key: String) {
            while (true) {
                val task = synchronized(queues) {
                    val queue = queues.getValue(key)
                    queue.removeFirstOrNull() ?: run {
                        queues.remove(key)
                        null
                    }
                } ?: return
                try {
                    task()
                } catch (e: CancellationException) {
                    if (!currentCoroutineContext().isActive) throw e
                } catch (e: Exception) {
                    logger.info("call task failed: ${e::class.simpleName}")
                }
            }
        }

        fun busy(): Int = synchronized(queues) { queues.size }
    }

    /**
     * Everything of one signed-in session's calls. Created by the door for a
     * ticket, gone with it: its scope is the ticket's, and whatever it does
     * to the outside (a request, a post, the published state, a voice join)
     * it does only while the ticket is alive.
     */
    private inner class Owner(val ticket: SessionGeneration.Ticket, number: Long) {
        private val me = ticket.userId
        private val scope = ticket.scope(this@CallController.scope)
        private val inbox = Channel<Envelope>(Channel.UNLIMITED)
        private val lanes = Lanes(scope)
        private val tagBase = number * TAGS_PER_OWNER

        /** Completed when this owner is dropped: whoever waits for it stops waiting. */
        val gone = CompletableDeferred<Unit>()

        // Guarded by the actor (only it reads or writes them).
        private var model = CallModel()
        private var ringCapJob: Job? = null
        private var endedJob: Job? = null
        private var voiceJob: VoiceJob? = null
        private val busyCapJobs = HashMap<String, Job>()

        /** The actor's state for readers on other threads (the rejoin gate). */
        @Volatile
        private var shown: CallState = CallState.Idle

        fun start() {
            scope.launch { for (envelope in inbox) handle(envelope) }
        }

        fun offer(event: CallEvent, done: CompletableDeferred<Unit>? = null): Boolean {
            if (!ticket.alive) return false
            return inbox.trySend(Envelope(event, done)).isSuccess
        }

        fun end() {
            inbox.close()
            gone.complete(Unit)
        }

        fun busyLanes(): Int = lanes.busy()

        /** The machine's event for [input], with this owner's clocks. Null: nothing to do. */
        fun eventOf(input: CallInput): CallEvent? = when (input) {
            is CallInput.Start -> CallEvent.Start(input.channelId, clock.nowMs())
            is CallInput.Accept -> input.call.callId?.let { CallEvent.Accept(input.call.channelId, known(it)) }
            is CallInput.Decline -> input.call.callId?.let { CallEvent.Decline(input.call.channelId, known(it)) }
            is CallInput.HangUp -> CallEvent.HangUp(input.call.channelId, input.call.callId?.let(::known))
            is CallInput.DismissEnded -> CallEvent.EndedShown(input.channelId)
            is CallInput.RingNotification -> input.call.callId?.let { callId ->
                CallEvent.Rang(
                    input.call.channelId, known(callId), input.callerId, input.callerName,
                    elapsedAt(input.receivedAtMs), fromNotification = true,
                )
            }
            is CallInput.RingExpired -> input.call.callId?.let {
                CallEvent.RingExpired(input.call.channelId, known(it), input.callerName)
            }
            is CallInput.PushRing -> input.call.callId?.let { callId ->
                CallEvent.Rang(
                    input.call.channelId, known(callId), input.callerId,
                    directory.nameOf(input.call.channelId, input.callerId) ?: input.callerName,
                    elapsedAt(input.receivedAtMs),
                )
            }
            is CallInput.PushEnded -> input.call.callId?.let { callId ->
                CallEvent.ServerEnded(
                    input.call.channelId, known(callId), input.reason,
                    rungHere = true, callerId = input.callerId, callerName = input.callerName,
                )
            }
        }

        /**
         * The elapsed-clock time of something received at wall-clock
         * [receivedAtMs] (null: now): a ring's 100 s count from when it was
         * first received, whichever way it came and however late it got here.
         */
        private fun elapsedAt(receivedAtMs: Long?): Long {
            val now = clock.nowMs()
            if (receivedAtMs == null) return now
            return now - (wallClock() - receivedAtMs).coerceIn(0L, CallMachine.RING_CAP_MS)
        }

        fun voiceEnded(end: CallVoiceEnd) {
            val voiceId = end.tag - tagBase
            if (voiceId in 1 until TAGS_PER_OWNER) offer(CallEvent.VoiceEnded(voiceId, end.why))
        }

        // -- The actor ---------------------------------------------------------------

        private fun handle(envelope: Envelope) {
            if (!ticket.alive) {
                envelope.done?.complete(Unit)
                return
            }
            val before = model.state
            val step = CallMachine.reduce(model, envelope.event, me)
            model = step.model
            shown = step.model.state
            publish(step.model.state)
            var waited = false
            for (effect in step.effects) waited = run(effect, envelope.done) || waited
            if (!waited) envelope.done?.complete(Unit)
            // Every way out of a call (end, removal, a resync) also ends its voice join,
            // wherever that join is: the microphone never stays on for a call that is over.
            val left = voiceOf(before)
            if (left != null && voiceOf(step.model.state) != left && step.effects.none { it is CallEffect.LeaveVoice }) {
                leaveVoice(left)
            }
        }

        /** Published only while this owner is the current one; a sign-out resets under the same lock. */
        private fun publish(state: CallState) {
            synchronized(ownerLock) { if (owner === this) _state.value = state }
        }

        /** The voice id [state] uses, if any. */
        private fun voiceOf(state: CallState): Long? = when (state) {
            is CallState.Outgoing -> state.voiceId
            is CallState.Connecting -> state.voiceId
            is CallState.Connected -> state.voiceId
            else -> null
        }

        /** Runs [effect]; true if it took over [done] (completes it when its request is through). */
        private fun run(effect: CallEffect, done: CompletableDeferred<Unit>?): Boolean {
            when (effect) {
                is CallEffect.RestStart -> lanes.submit(effect.channelId) { restStart(effect.channelId, effect.startedAtMs) }
                is CallEffect.RestJoin -> lanes.submit(effect.channelId) { restJoin(effect.channelId, effect.callId, effect.verify) }
                is CallEffect.RestDecline -> {
                    lanes.submit(effect.channelId) {
                        try {
                            restDecline(effect.channelId, effect.callId, effect.verify)
                        } finally {
                            done?.complete(Unit)
                        }
                    }
                    return true
                }
                is CallEffect.RestLeave -> lanes.submit(effect.channelId) {
                    quietly("leave") { api.leave(effect.channelId, ticket.requestSession()) }
                }
                is CallEffect.HoldVoice -> {
                    // Now, in the actor, right after the tap: the microphone service is up before
                    // any request goes out. If Android refuses it, the call cannot have a microphone.
                    val held = voice.holdForCall(tagBase + effect.voiceId, effect.channelId, directory.titleOf(effect.channelId))
                    if (!held) offer(CallEvent.VoiceJoinFailed(effect.channelId, effect.voiceId))
                }
                is CallEffect.JoinVoice -> joinVoice(effect.channelId, effect.voiceId)
                is CallEffect.Resync -> resync(effect.channelId, effect.askedFor)
                is CallEffect.ArmBusyCap -> armBusyCap(effect.channelId, effect.callId, effect.startedAtMs)
                is CallEffect.LeaveVoice -> leaveVoice(effect.voiceId)
                is CallEffect.Ring -> notifier.ring(ticket, effect.channelId, effect.callId, effect.callerId, effect.callerName)
                is CallEffect.StopRing -> notifier.stopRing(ticket, effect.channelId, effect.callId)
                is CallEffect.Missed -> notifier.missed(ticket, effect.channelId, effect.callId, effect.callerName)
                is CallEffect.MissedIfRinging ->
                    notifier.missedIfRinging(ticket, effect.channelId, effect.callId, effect.callerName)
                is CallEffect.ArmRingCap -> armRingCap(effect.channelId, effect.startedAtMs)
                CallEffect.ArmEndedTimer -> {
                    endedJob?.cancel()
                    endedJob = scope.launch {
                        delay(CallMachine.ENDED_SHOWN_MS)
                        offer(CallEvent.EndedShown(null))
                    }
                }
            }
            return false
        }

        // -- Requests ----------------------------------------------------------------
        //
        // Each is built for this owner's session: the HTTP client refuses to send it
        // once another session (or none) is the current one.

        private suspend fun restStart(channelId: String, startedAtMs: Long) {
            val event = try {
                CallEvent.StartOk(channelId, known(api.start(channelId, ticket.requestSession()).callId), startedAtMs)
            } catch (e: CancellationException) {
                throw e
            } catch (e: ApiException) {
                logger.info("call start refused: ${e.status.value} ${e.normalizedCode}")
                CallEvent.StartFailed(
                    channelId,
                    glare = e.normalizedCode == CallApi.CALL_ALREADY_EXISTS,
                    blocked = e.normalizedCode == CallApi.BLOCKED,
                    startedAtMs = startedAtMs,
                )
            } catch (e: Exception) {
                logger.info("call start failed: ${e::class.simpleName}")
                CallEvent.StartFailed(channelId, glare = false, blocked = false, startedAtMs = startedAtMs)
            }
            offer(event)
        }

        private suspend fun restJoin(channelId: String, callId: String, verify: Boolean) {
            val event = try {
                if (verify && !serverHas(channelId, callId, ringingOnly = false)) {
                    // Known only from a notification, and the server has another call (or none) now.
                    CallEvent.JoinFailed(channelId, callId, gone = true)
                } else {
                    val joined = api.join(channelId, ticket.requestSession())
                    val answered = known(joined.callId)
                    if (callId.isNotEmpty() && answered.isNotEmpty() && answered != callId) {
                        // The server answered for another call than the one accepted: not this
                        // user's choice. It is left again at once.
                        quietly("leave") { api.leave(channelId, ticket.requestSession()) }
                        CallEvent.JoinFailed(channelId, callId, gone = true)
                    } else {
                        CallEvent.JoinOk(channelId, callId, joined.participants.toSet())
                    }
                }
            } catch (e: CancellationException) {
                throw e
            } catch (e: ApiException) {
                logger.info("call join refused: ${e.status.value} ${e.normalizedCode}")
                val gone = e.status == HttpStatusCode.NotFound || e.status == HttpStatusCode.Conflict
                CallEvent.JoinFailed(channelId, callId, gone)
            } catch (e: Exception) {
                logger.info("call join failed: ${e::class.simpleName}")
                CallEvent.JoinFailed(channelId, callId, gone = false)
            }
            offer(event)
        }

        private suspend fun restDecline(channelId: String, callId: String, verify: Boolean) {
            quietly("decline") {
                // A ring known only from a notification: declined only if it is still that call.
                if (verify && !serverHas(channelId, callId, ringingOnly = true)) return@quietly
                api.decline(channelId, ticket.requestSession())
            }
        }

        /** Whether the conversation's call on the server is [callId] (and, with [ringingOnly], still rings). */
        private suspend fun serverHas(channelId: String, callId: String, ringingOnly: Boolean): Boolean {
            val now = getCall(channelId) ?: return false
            if (callId.isEmpty() || now.callId != callId) return false
            return now.ringing || (!ringingOnly && now.active)
        }

        /** Asks the server about the call shown for [askedFor]; the answer is dropped if the state moved on. */
        private fun resync(channelId: String, askedFor: CallState) {
            scope.launch {
                try {
                    val answer = getCall(channelId)
                    offer(CallEvent.Resynced(channelId, answer, clock.nowMs(), askedFor))
                } catch (e: CancellationException) {
                    throw e
                } catch (e: Exception) {
                    // Unchanged; the next socket asks again, and the ring cap still applies.
                    logger.info("call resync failed: ${e::class.simpleName}")
                }
            }
        }

        /**
         * The DM's call as the server sees it. 403 (no longer in the conversation)
         * and 404 (it is gone) are a definite "no call for this user", not a
         * passing failure; anything else throws.
         */
        private suspend fun getCall(channelId: String): CallSnapshot? = try {
            api.get(channelId, ticket.requestSession())?.toSnapshot()
        } catch (e: ApiException) {
            if (e.status == HttpStatusCode.Forbidden || e.status == HttpStatusCode.NotFound) null else throw e
        }

        /**
         * Asked by the voice owner before a rejoin of a call session: allowed
         * only if the server still has this user in the call shown here. A
         * request that fails is [RejoinVerdict.Unknown] (the attempt fails and
         * is retried).
         */
        private suspend fun mayRejoin(channelId: String): RejoinVerdict {
            if (!ticket.alive) return RejoinVerdict.Refused
            // The state the question is asked for, before the answer: what changes meanwhile drops it.
            val askedFor = shown
            val answer = try {
                getCall(channelId)
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                logger.info("call check before a rejoin failed: ${e::class.simpleName}")
                return RejoinVerdict.Unknown
            }
            if (!CallMachine.mayRejoin(askedFor, channelId, answer, me)) return RejoinVerdict.Refused
            offer(CallEvent.Resynced(channelId, answer, clock.nowMs(), askedFor))
            return RejoinVerdict.Allowed
        }

        // -- Voice -------------------------------------------------------------------

        private fun joinVoice(channelId: String, voiceId: Long) {
            voiceJob?.cancel()
            val join = VoiceJob(voiceId)
            voiceJob = join
            lanes.submit(VOICE_LANE) {
                join.launch {
                    scope.launch {
                        try {
                            voice.joinCall(
                                channelId,
                                VoiceRepository.CallSession(
                                    gate = { mayRejoin(it) },
                                    title = directory.titleOf(channelId),
                                    tag = tagBase + voiceId,
                                ),
                            )
                            offer(CallEvent.VoiceJoined(channelId, clock.nowMs(), voiceId))
                        } catch (e: VoiceRemovedException) {
                            // Removed by the server before the join was set up: an end, not a failure.
                            offer(CallEvent.VoiceEnded(voiceId, VoiceEnd.Removed(e.reason)))
                        } catch (e: CancellationException) {
                            // Cancelled by a leave of this call: nothing to report. Otherwise the
                            // session ended before it was set up: the join failed.
                            if (!currentCoroutineContext().isActive) throw e
                            offer(CallEvent.VoiceJoinFailed(channelId, voiceId))
                        } catch (e: Exception) {
                            logger.log(Level.INFO, "call voice join failed: ${e::class.simpleName}")
                            offer(CallEvent.VoiceJoinFailed(channelId, voiceId))
                        }
                    }
                }
            }
        }

        /**
         * Ends the voice of the call with [voiceId], in the voice lane after its
         * join: first the join, wherever it is (also before it started, or while
         * it is still ending another session), then that call's session and no
         * other. Which session is current is decided by the voice layer, in one
         * step with the leave.
         */
        private fun leaveVoice(voiceId: Long) {
            val join = voiceJob?.takeIf { it.voiceId == voiceId }
            if (join != null) voiceJob = null
            val running = join?.cancel()
            lanes.submit(VOICE_LANE) {
                (running ?: join?.cancel())?.cancelAndJoin()
                voice.leaveCall(tagBase + voiceId)
            }
        }

        // -- Timers ------------------------------------------------------------------

        /** A busy ring is missed 100 s after it came, unless it ended before. */
        private fun armBusyCap(channelId: String, callId: String, startedAtMs: Long) {
            busyCapJobs.remove(channelId)?.cancel()
            busyCapJobs[channelId] = scope.launch {
                awaitRealTime(startedAtMs, CallMachine.RING_CAP_MS)
                offer(CallEvent.BusyCap(channelId, callId))
            }
        }

        /** Waits until [ms] of real time (device sleep included) have passed since [startedAtMs]. */
        private suspend fun awaitRealTime(startedAtMs: Long, ms: Long) {
            var waited = 0L
            while (true) {
                val elapsed = maxOf(waited, clock.nowMs() - startedAtMs)
                if (elapsed >= ms) return
                val step = minOf(ms - elapsed, CAP_CHECK_MS)
                delay(step)
                waited += step
            }
        }

        /** The ring ends locally 100 s of real time after it started (device sleep included). */
        private fun armRingCap(channelId: String, startedAtMs: Long) {
            ringCapJob?.cancel()
            ringCapJob = scope.launch {
                awaitRealTime(startedAtMs, CallMachine.RING_CAP_MS)
                offer(CallEvent.RingCap(channelId, startedAtMs))
            }
        }

        private suspend fun quietly(what: String, block: suspend () -> Unit) {
            try {
                block()
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                // The call is over locally either way; the server ends it by its own rules.
                logger.info("call $what failed: ${e::class.simpleName}")
            }
        }
    }
}

/** Older servers send the nil id for "no call id"; it is treated as unknown (empty). */
internal fun known(callId: String): String = if (callId == NIL_CALL_ID) "" else callId

private const val NIL_CALL_ID = "00000000-0000-0000-0000-000000000000"

/** The machine's view of a server answer. */
internal fun CallStateDto.toSnapshot() = CallSnapshot(
    callId = known(callId),
    ringing = status == "ringing",
    active = status == "active",
    startedBy = startedBy,
    participants = participants.toSet(),
    declinedBy = declinedBy.toSet(),
)
