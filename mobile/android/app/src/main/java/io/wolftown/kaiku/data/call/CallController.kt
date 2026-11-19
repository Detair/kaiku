package io.wolftown.kaiku.data.call

import io.ktor.http.HttpStatusCode
import io.wolftown.kaiku.data.api.ApiException
import io.wolftown.kaiku.data.api.CallApi
import io.wolftown.kaiku.data.api.CallStateDto
import io.wolftown.kaiku.data.local.AuthState
import io.wolftown.kaiku.data.repository.VoiceRepository
import io.wolftown.kaiku.data.voice.ElapsedClock
import io.wolftown.kaiku.data.voice.RejoinVerdict
import io.wolftown.kaiku.data.ws.KaikuWebSocket
import io.wolftown.kaiku.data.ws.ServerEvent
import io.wolftown.kaiku.di.VoiceCoroutineScope
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.CoroutineStart
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.currentCoroutineContext
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.drop
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
    fun ring(channelId: String, callId: String, callerName: String?)

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
 * The one owner of DM call state on this device (plan N2b §1): it feeds
 * [CallMachine] from the socket (call events, every `Ready` for the C33
 * resync), the voice layer (how a call's voice session ended), the signed-in
 * user, the screens and the notification actions, one event at a time
 * through a single actor, and runs the machine's effects. REST calls and the
 * voice join run as their own coroutines and come back as events.
 *
 * It never touches WebRTC: voice goes through [VoiceRepository.joinCall] and
 * [VoiceRepository.leaveChannel], whose owner decides the rest (C32), asking
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

        /** The ring cap wakes at least this often to compare the real clock (device sleep, C33). */
        const val CAP_CHECK_MS = 5_000L
    }

    private val scope = CoroutineScope(voiceScope.coroutineContext + SupervisorJob(voiceScope.coroutineContext[Job]))
    private val inbox = Channel<CallEvent>(Channel.UNLIMITED)
    private val started = AtomicBoolean(false)

    // Guarded by the actor (only it reads or writes them).
    private var model = CallModel()
    private var ringCapJob: Job? = null
    private var endedJob: Job? = null
    private var voiceJob: Job? = null

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
        // Another user or nobody signed in: the call is over (C32 "signed-in user").
        scope.launch(start = CoroutineStart.UNDISPATCHED) {
            authState.currentUserId.drop(1).collect { send(CallEvent.SignedOut) }
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
        send(CallEvent.Rang(channelId, callId, callerId, callerName, clock.nowMs()))

    /**
     * Decline from the notification's action. If this process does not show
     * that ring (it was started just for the action), the server is told directly.
     */
    fun declineFromNotification(channelId: String, callId: String) {
        val shown = _state.value
        if (shown is CallState.Incoming && shown.channelId == channelId) {
            decline(channelId)
            return
        }
        notifier.stopRing(channelId, callId)
        scope.launch { quietly("decline") { api.decline(channelId) } }
    }

    /**
     * C32 "signed-in user": runs synchronously before the account state is
     * cleared. Every ring goes at once; the state follows through the actor.
     * (Voice and the microphone are ended by the voice layer's own hook.)
     */
    fun endForSignOut() {
        send(CallEvent.SignedOut)
        notifier.stopAll()
    }

    /**
     * Asked by the voice owner before a rejoin of a call session (C32 "DM
     * calls"): allowed only if the server still has this user in the call
     * shown here. A request that fails is [RejoinVerdict.Unknown] (the
     * attempt fails and is retried).
     */
    suspend fun mayRejoin(channelId: String): RejoinVerdict {
        val me = authState.currentUserId.value ?: return RejoinVerdict.Refused
        val answer = try {
            api.get(channelId)?.toSnapshot()
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            logger.info("call check before a rejoin failed: ${e::class.simpleName}")
            return RejoinVerdict.Unknown
        }
        if (!CallMachine.mayRejoin(_state.value, answer, me)) return RejoinVerdict.Refused
        send(CallEvent.Resynced(channelId, answer, clock.nowMs()))
        return RejoinVerdict.Allowed
    }

    private fun send(event: CallEvent) {
        inbox.trySend(event)
    }

    // -- Socket ------------------------------------------------------------------

    private fun onServerEvent(event: ServerEvent) {
        when (event) {
            is ServerEvent.Ready -> resync()
            is ServerEvent.IncomingCall -> send(
                CallEvent.Rang(
                    event.channelId,
                    event.callId,
                    event.initiator,
                    directory.nameOf(event.channelId, event.initiator) ?: event.initiatorName.ifBlank { null },
                    clock.nowMs(),
                )
            )
            is ServerEvent.CallParticipantJoined ->
                send(CallEvent.ParticipantJoined(event.channelId, event.callId, event.userId, clock.nowMs()))
            is ServerEvent.CallParticipantLeft ->
                send(CallEvent.ParticipantLeft(event.channelId, event.callId, event.userId))
            is ServerEvent.CallDeclined -> send(CallEvent.Declined(event.channelId, event.callId, event.userId))
            is ServerEvent.CallEnded -> send(CallEvent.ServerEnded(event.channelId, event.callId, event.reason))
            else -> Unit
        }
    }

    /** C33: after every new authenticated socket, ask the server about the call shown here. */
    private fun resync() {
        val channelId = when (val s = _state.value) {
            is CallState.Incoming, is CallState.Outgoing, is CallState.Connecting, is CallState.Connected -> s.channelId
            else -> null
        } ?: return
        scope.launch {
            try {
                val answer = api.get(channelId)?.toSnapshot()
                send(CallEvent.Resynced(channelId, answer, clock.nowMs()))
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                // Unchanged; the next socket asks again, and the ring cap still applies.
                logger.info("call resync failed: ${e::class.simpleName}")
            }
        }
    }

    // -- The actor ---------------------------------------------------------------

    private fun handle(event: CallEvent) {
        val step = CallMachine.reduce(model, event, authState.currentUserId.value)
        model = step.model
        _state.value = step.model.state
        step.effects.forEach(::run)
    }

    private fun run(effect: CallEffect) {
        when (effect) {
            is CallEffect.RestStart -> scope.launch { restStart(effect.channelId) }
            is CallEffect.RestJoin -> scope.launch { restJoin(effect.channelId) }
            is CallEffect.RestDecline -> scope.launch { quietly("decline") { api.decline(effect.channelId) } }
            is CallEffect.RestLeave -> scope.launch { quietly("leave") { api.leave(effect.channelId) } }
            is CallEffect.JoinVoice -> joinVoice(effect.channelId)
            is CallEffect.LeaveVoice -> leaveVoice(effect.channelId)
            is CallEffect.Ring -> notifier.ring(effect.channelId, effect.callId, effect.callerName)
            is CallEffect.StopRing -> notifier.stopRing(effect.channelId, effect.callId)
            is CallEffect.Missed -> notifier.missed(effect.channelId, effect.callId, effect.callerName)
            CallEffect.StopAllRings -> {
                ringCapJob?.cancel()
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

    private suspend fun restStart(channelId: String) {
        val event = try {
            CallEvent.StartOk(channelId, api.start(channelId).callId)
        } catch (e: CancellationException) {
            throw e
        } catch (e: ApiException) {
            logger.info("call start refused: ${e.status.value} ${e.normalizedCode}")
            CallEvent.StartFailed(
                channelId,
                glare = e.normalizedCode == CallApi.CALL_ALREADY_EXISTS,
                blocked = e.normalizedCode == CallApi.BLOCKED,
            )
        } catch (e: Exception) {
            logger.info("call start failed: ${e::class.simpleName}")
            CallEvent.StartFailed(channelId, glare = false, blocked = false)
        }
        send(event)
    }

    private suspend fun restJoin(channelId: String) {
        val event = try {
            api.join(channelId)
            CallEvent.JoinOk(channelId)
        } catch (e: CancellationException) {
            throw e
        } catch (e: ApiException) {
            logger.info("call join refused: ${e.status.value} ${e.normalizedCode}")
            val gone = e.status == HttpStatusCode.NotFound || e.status == HttpStatusCode.Conflict
            CallEvent.JoinFailed(channelId, gone)
        } catch (e: Exception) {
            logger.info("call join failed: ${e::class.simpleName}")
            CallEvent.JoinFailed(channelId, gone = false)
        }
        send(event)
    }

    private fun joinVoice(channelId: String) {
        voiceJob = scope.launch {
            try {
                voice.joinCall(channelId, directory.titleOf(channelId)) { mayRejoin(it) }
                send(CallEvent.VoiceJoined(channelId, clock.nowMs()))
            } catch (e: CancellationException) {
                // The session ended before it was set up (a leave, a removal):
                // unless this controller is being shut down, the join failed.
                if (!currentCoroutineContext().isActive) throw e
                send(CallEvent.VoiceJoinFailed(channelId))
            } catch (e: Exception) {
                logger.log(Level.INFO, "call voice join failed: ${e::class.simpleName}")
                send(CallEvent.VoiceJoinFailed(channelId))
            }
        }
    }

    /** Leaves voice only while it is in the call's room (never a guild channel the user moved to). */
    private fun leaveVoice(channelId: String) {
        scope.launch {
            if (voice.currentChannelId.value == channelId) voice.leaveChannel()
        }
    }

    /** C33: the ring ends locally 100 s of real time after it started (device sleep included). */
    private fun armRingCap(channelId: String, startedAtMs: Long) {
        ringCapJob?.cancel()
        ringCapJob = scope.launch {
            var waited = 0L
            while (true) {
                val elapsed = maxOf(waited, clock.nowMs() - startedAtMs)
                if (elapsed >= CallMachine.RING_CAP_MS) break
                val step = minOf(CallMachine.RING_CAP_MS - elapsed, CAP_CHECK_MS)
                delay(step)
                waited += step
            }
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

/** The machine's view of a server answer. */
internal fun CallStateDto.toSnapshot() = CallSnapshot(
    callId = callId,
    ringing = status == "ringing",
    active = status == "active",
    startedBy = startedBy,
    participants = participants.toSet(),
    declinedBy = declinedBy.toSet(),
)
