package io.wolftown.kaiku.data.call

import io.ktor.http.HttpStatusCode
import io.wolftown.kaiku.data.api.ApiException
import io.wolftown.kaiku.data.api.CallApi
import io.wolftown.kaiku.data.api.CallStateDto
import io.wolftown.kaiku.data.local.AuthState
import io.wolftown.kaiku.data.repository.VoiceRepository
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
import java.util.concurrent.atomic.AtomicBoolean
import java.util.logging.Level
import java.util.logging.Logger
import javax.inject.Inject
import javax.inject.Singleton

/** Posts and removes the call notifications (the Android side is `AndroidCallNotifier`). */
interface CallNotifier {
    /** The incoming-call notification: ringtone, full-screen where allowed, Accept and Decline. */
    fun ring(channelId: String, callId: String, callerId: String, callerName: String?)

    fun stopRing(channelId: String, callId: String)

    /** A missed call (once per call; tap opens the DM). */
    fun missed(channelId: String, callId: String, callerName: String?)

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
 * The one owner of DM call state on this device: it feeds
 * [CallMachine] from the socket (call events, every `Ready` for the call
 * resync), the voice layer (how a call's voice session ended), the signed-in
 * user, the screens and the notification actions, one event at a time
 * through a single actor, and runs the machine's effects. REST calls and the
 * voice join run as their own coroutines and come back as events.
 *
 * It never touches WebRTC: voice goes through [VoiceRepository.joinCall] and
 * [VoiceRepository.leaveChannel], whose owner decides the rest, asking
 * [mayRejoin] before each rejoin.
 */
@Singleton
class CallController @Inject constructor(
    private val api: CallApi,
    private val voice: VoiceRepository,
    private val webSocket: KaikuWebSocket,
    private val authState: AuthState,
    private val notifier: CallNotifier,
    private val directory: CallDirectory,
    private val clock: ElapsedClock,
    @VoiceCoroutineScope voiceScope: CoroutineScope,
) {
    companion object {
        private val logger = Logger.getLogger("CallController")

        /** The ring cap wakes at least this often to compare the real clock (device sleep). */
        const val CAP_CHECK_MS = 5_000L
    }

    private val scope = CoroutineScope(voiceScope.coroutineContext + SupervisorJob(voiceScope.coroutineContext[Job]))
    private val inbox = Channel<CallEvent>(Channel.UNLIMITED)
    private val started = AtomicBoolean(false)

    // Guarded by the actor (only it reads or writes them).
    private var model = CallModel()
    private var ringCapJob: Job? = null
    private var endedJob: Job? = null
    /** The voice join of the shown call; cancelled on every way out of that call. */
    private var voiceTicket: VoiceTicket? = null
    private val busyCapJobs = HashMap<String, Job>()

    private val _state = MutableStateFlow<CallState>(CallState.Idle)

    /** The call this device shows; written by the actor only. */
    val state: StateFlow<CallState> = _state.asStateFlow()

    /** Starts the actor and the observers; idempotent. Called at app start. */
    fun start() {
        if (!started.compareAndSet(false, true)) return
        scope.launch { for (event in inbox) handle(event) }
        // UNDISPATCHED: subscribed before start() returns (replay-0 flows).
        scope.launch(start = CoroutineStart.UNDISPATCHED) {
            webSocket.events.collect { onServerEvent(it) }
        }
        scope.launch(start = CoroutineStart.UNDISPATCHED) {
            voice.callEnds.collect { send(CallEvent.VoiceEnded(it.channelId, it.why)) }
        }
        // Every session that ends (sign-out, expiry, another user): the call is over
        // (signed-in user). A sign-in alone changes nothing.
        scope.launch(start = CoroutineStart.UNDISPATCHED) {
            authState.endedSessions.collect { send(CallEvent.SignedOut) }
        }
    }

    // -- Inputs from the screens and notifications ------------------------------

    fun startCall(channelId: String) = send(CallEvent.Start(channelId, clock.nowMs()))

    /** Accept; the screen has made sure the microphone permission is granted. */
    fun accept(channelId: String) = send(CallEvent.Accept(channelId))

    fun decline(channelId: String) = send(CallEvent.Decline(channelId))

    fun hangUp() = send(CallEvent.HangUp)

    fun dismissEnded() = send(CallEvent.EndedShown)

    /**
     * A ring this process only knows from its notification (the push wake
     * posted it while the app was not running): becomes the shown incoming
     * call unless it already ended.
     */
    fun onRingNotification(channelId: String, callId: String, callerId: String, callerName: String?) =
        send(CallEvent.Rang(channelId, known(callId), callerId, callerName, clock.nowMs(), fromNotification = true))

    /**
     * Decline from the notification's action. If this process does not show
     * that ring (it was started just for the action), the server is told
     * directly; returns once that request is done.
     */
    suspend fun declineFromNotification(channelId: String, callId: String) {
        val shown = _state.value
        if (shown is CallState.Incoming && shown.channelId == channelId && shown.callId == known(callId)) {
            // Through the call owner; returns once the server was told (or nothing was to tell).
            val done = CompletableDeferred<Unit>()
            pendingDeclines.put(channelId, done)?.complete(Unit)
            decline(channelId)
            done.await()
            return
        }
        notifier.stopRing(channelId, callId)
        quietly("decline") { api.decline(channelId) }
    }

    /** Declines from a notification waiting for their request, by DM. */
    private val pendingDeclines = java.util.concurrent.ConcurrentHashMap<String, CompletableDeferred<Unit>>()

    /**
     * Signed-in user: runs synchronously before the account state is
     * cleared. Every ring goes at once; the state follows through the actor.
     * (Voice and the microphone are ended by the voice layer's own hook.)
     */
    fun endForSignOut() {
        send(CallEvent.SignedOut)
        notifier.stopAll()
    }

    /**
     * Asked by the voice owner before a rejoin of a call session: allowed
     * only if the server still has this user in the call
     * shown here. A request that fails is [RejoinVerdict.Unknown] (the
     * attempt fails and is retried).
     */
    suspend fun mayRejoin(channelId: String): RejoinVerdict {
        val me = authState.signedInUserId() ?: return RejoinVerdict.Refused
        // The state the question is asked for, before the answer: what changes meanwhile drops it.
        val askedFor = _state.value
        val answer = try {
            getCall(channelId)
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            logger.info("call check before a rejoin failed: ${e::class.simpleName}")
            return RejoinVerdict.Unknown
        }
        if (!CallMachine.mayRejoin(askedFor, channelId, answer, me)) return RejoinVerdict.Refused
        send(CallEvent.Resynced(channelId, answer, clock.nowMs(), askedFor))
        return RejoinVerdict.Allowed
    }

    private fun send(event: CallEvent) {
        inbox.trySend(event)
    }

    // -- Socket ------------------------------------------------------------------

    private fun onServerEvent(event: ServerEvent) {
        when (event) {
            is ServerEvent.Ready -> send(CallEvent.SocketReady)
            is ServerEvent.IncomingCall -> send(
                CallEvent.Rang(
                    event.channelId,
                    known(event.callId),
                    event.initiator,
                    directory.nameOf(event.channelId, event.initiator) ?: event.initiatorName.ifBlank { null },
                    clock.nowMs(),
                )
            )
            is ServerEvent.CallParticipantJoined ->
                send(CallEvent.ParticipantJoined(event.channelId, known(event.callId), event.userId, clock.nowMs()))
            is ServerEvent.CallParticipantLeft ->
                send(CallEvent.ParticipantLeft(event.channelId, known(event.callId), event.userId))
            is ServerEvent.CallDeclined -> send(CallEvent.Declined(event.channelId, known(event.callId), event.userId))
            is ServerEvent.CallEnded -> send(CallEvent.ServerEnded(event.channelId, known(event.callId), event.reason))
            else -> Unit
        }
    }

    /** Asks the server about the call shown for [askedFor]; the answer is dropped if the state moved on. */
    private fun resync(channelId: String, askedFor: CallState) {
        scope.launch {
            try {
                val answer = getCall(channelId)
                send(CallEvent.Resynced(channelId, answer, clock.nowMs(), askedFor))
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
        api.get(channelId)?.toSnapshot()
    } catch (e: ApiException) {
        if (e.status == HttpStatusCode.Forbidden || e.status == HttpStatusCode.NotFound) null else throw e
    }

    // -- Serial lanes --------------------------------------------------------------
    //
    // The side effects of the calls run in the order the machine emitted them:
    // the requests of one conversation one after the other (a leave never
    // overtakes its start or join), and every voice join and leave in one lane
    // (an old call's leave never ends a newer call's voice).

    private val restLanes = HashMap<String, Channel<suspend () -> Unit>>()

    /** Runs [task] in the request lane of [channelId], after everything sent there before. */
    private fun inRestLane(channelId: String, task: suspend () -> Unit) {
        val lane = synchronized(restLanes) {
            restLanes.getOrPut(channelId) {
                Channel<suspend () -> Unit>(Channel.UNLIMITED).also { ch -> scope.launch { for (t in ch) t() } }
            }
        }
        lane.trySend(task)
    }

    private val voiceLane = Channel<suspend () -> Unit>(Channel.UNLIMITED).also { ch ->
        scope.launch { for (t in ch) t() }
    }

    /** One voice join of a call: cancelled by that call's leave, also before it started. */
    private class VoiceTicket(val channelId: String) {
        private val lock = Any()
        private var job: Job? = null
        private var cancelled = false

        /** Starts [start] unless the ticket was cancelled first. */
        fun launch(start: () -> Job) = synchronized(lock) { if (!cancelled) job = start() }

        /** Cancels the join (now or before it starts) and returns it to wait for. */
        fun cancel(): Job? = synchronized(lock) {
            cancelled = true
            job?.also { it.cancel() }
        }
    }

    // -- The actor ---------------------------------------------------------------

    private fun handle(event: CallEvent) {
        val before = model.state
        val step = CallMachine.reduce(model, event, authState.signedInUserId())
        model = step.model
        _state.value = step.model.state
        step.effects.forEach(::run)
        if (event is CallEvent.Decline && step.effects.none { it is CallEffect.RestDecline }) {
            pendingDeclines.remove(event.channelId)?.complete(Unit) // nothing to tell the server
        }
        // Every way out of a call (end, removal, sign-out, a resync) also ends its voice join,
        // wherever that join is: the microphone never stays on for a call that is over.
        val left = inVoiceCall(before)
        if (left != null && inVoiceCall(step.model.state) != left && step.effects.none { it is CallEffect.LeaveVoice }) {
            leaveVoice(left)
        }
    }

    /** The DM whose voice room [state] uses, if any. */
    private fun inVoiceCall(state: CallState): String? = when (state) {
        is CallState.Outgoing, is CallState.Connecting, is CallState.Connected -> state.channelId
        else -> null
    }

    private fun run(effect: CallEffect) {
        when (effect) {
            is CallEffect.RestStart -> inRestLane(effect.channelId) { restStart(effect.channelId, effect.startedAtMs) }
            is CallEffect.RestJoin -> inRestLane(effect.channelId) { restJoin(effect.channelId, effect.callId) }
            is CallEffect.RestDecline -> inRestLane(effect.channelId) {
                try {
                    quietly("decline") { api.decline(effect.channelId) }
                } finally {
                    pendingDeclines.remove(effect.channelId)?.complete(Unit)
                }
            }
            is CallEffect.RestLeave -> inRestLane(effect.channelId) { quietly("leave") { api.leave(effect.channelId) } }
            is CallEffect.JoinVoice -> joinVoice(effect.channelId, effect.callId)
            is CallEffect.Resync -> resync(effect.channelId, effect.askedFor)
            is CallEffect.ArmBusyCap -> armBusyCap(effect.channelId, effect.callId, effect.startedAtMs)
            is CallEffect.LeaveVoice -> leaveVoice(effect.channelId)
            is CallEffect.Ring -> notifier.ring(effect.channelId, effect.callId, effect.callerId, effect.callerName)
            is CallEffect.StopRing -> notifier.stopRing(effect.channelId, effect.callId)
            is CallEffect.Missed -> notifier.missed(effect.channelId, effect.callId, effect.callerName)
            CallEffect.StopAllRings -> {
                ringCapJob?.cancel()
                busyCapJobs.values.forEach { it.cancel() }
                busyCapJobs.clear()
                notifier.stopAll()
            }
            is CallEffect.ArmRingCap -> armRingCap(effect.channelId, effect.startedAtMs)
            CallEffect.ArmEndedTimer -> {
                endedJob?.cancel()
                endedJob = scope.launch {
                    delay(CallMachine.ENDED_SHOWN_MS)
                    send(CallEvent.EndedShown)
                }
            }
        }
    }

    private suspend fun restStart(channelId: String, startedAtMs: Long) {
        val event = try {
            CallEvent.StartOk(channelId, known(api.start(channelId).callId), startedAtMs)
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
        send(event)
    }

    private suspend fun restJoin(channelId: String, callId: String) {
        val event = try {
            val joined = api.join(channelId)
            CallEvent.JoinOk(channelId, callId, joined.participants.toSet())
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
        send(event)
    }

    private fun joinVoice(channelId: String, callId: String?) {
        voiceTicket?.cancel()
        val ticket = VoiceTicket(channelId)
        voiceTicket = ticket
        voiceLane.trySend {
            ticket.launch {
                scope.launch {
                    try {
                        voice.joinCall(channelId, directory.titleOf(channelId)) { mayRejoin(it) }
                        send(CallEvent.VoiceJoined(channelId, clock.nowMs(), callId))
                    } catch (e: VoiceRemovedException) {
                        // Removed by the server before the join was set up: an end, not a failure.
                        send(CallEvent.VoiceEnded(channelId, VoiceEnd.Removed(e.reason)))
                    } catch (e: CancellationException) {
                        // Cancelled by a leave of this call: nothing to report. Otherwise the
                        // session ended before it was set up: the join failed.
                        if (!currentCoroutineContext().isActive) throw e
                        send(CallEvent.VoiceJoinFailed(channelId, callId))
                    } catch (e: Exception) {
                        logger.log(Level.INFO, "call voice join failed: ${e::class.simpleName}")
                        send(CallEvent.VoiceJoinFailed(channelId, callId))
                    }
                }
            }
        }
    }

    /**
     * Ends the call's voice, in the voice lane after its join: first the join,
     * wherever it is (also before it started, or while it is still leaving
     * another voice channel), then the session if voice is in the call's
     * room. Never touches a guild channel the user moved to.
     */
    private fun leaveVoice(channelId: String) {
        val ticket = voiceTicket?.takeIf { it.channelId == channelId }
        if (ticket != null) voiceTicket = null
        val job = ticket?.cancel()
        voiceLane.trySend {
            (job ?: ticket?.cancel())?.cancelAndJoin()
            if (voice.currentChannelId.value == channelId) voice.leaveChannel()
        }
    }

    /** A busy ring is missed 100 s after it came, unless it ended before. */
    private fun armBusyCap(channelId: String, callId: String, startedAtMs: Long) {
        busyCapJobs.remove(channelId)?.cancel()
        busyCapJobs[channelId] = scope.launch {
            awaitRealTime(startedAtMs, CallMachine.RING_CAP_MS)
            send(CallEvent.BusyCap(channelId, callId))
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
            send(CallEvent.RingCap(channelId, startedAtMs))
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
