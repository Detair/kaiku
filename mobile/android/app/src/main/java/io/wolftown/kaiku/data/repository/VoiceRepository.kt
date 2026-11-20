package io.wolftown.kaiku.data.repository

import android.content.Context
import dagger.hilt.android.qualifiers.ApplicationContext
import android.net.ConnectivityManager
import android.net.NetworkCapabilities
import io.wolftown.kaiku.R
import io.wolftown.kaiku.data.call.CallVoiceEnd
import io.wolftown.kaiku.data.call.VoiceEnd
import io.wolftown.kaiku.data.local.AuthSession
import io.wolftown.kaiku.data.session.SessionGeneration
import io.wolftown.kaiku.data.voice.AudioRouteManager
import io.wolftown.kaiku.data.voice.DeadSocketReporter
import io.wolftown.kaiku.data.voice.RejoinGate
import io.wolftown.kaiku.data.voice.RejoinVerdict
import io.wolftown.kaiku.data.voice.ElapsedClock
import io.wolftown.kaiku.data.voice.IceTrouble
import io.wolftown.kaiku.data.voice.MicPermissionChecker
import io.wolftown.kaiku.data.voice.MissingMicPermissionException
import io.wolftown.kaiku.data.voice.SessionGuard
import io.wolftown.kaiku.data.voice.VoiceEndNotice
import io.wolftown.kaiku.data.voice.VoiceLinkState
import io.wolftown.kaiku.data.voice.VoiceRemovedException
import io.wolftown.kaiku.data.voice.VoiceServiceEvent
import io.wolftown.kaiku.data.voice.VoiceServiceEvents
import io.wolftown.kaiku.data.voice.VoiceServiceStartException
import io.wolftown.kaiku.data.voice.VoiceSessionInfo
import io.wolftown.kaiku.data.voice.VoiceSignalling
import io.wolftown.kaiku.data.voice.WebRtcManager
import io.wolftown.kaiku.data.ws.ClientEvent
import io.wolftown.kaiku.data.ws.KaikuWebSocket
import io.wolftown.kaiku.data.ws.ScreenShareInfo
import io.wolftown.kaiku.data.ws.ServerEvent
import io.wolftown.kaiku.data.ws.VoiceParticipant
import io.wolftown.kaiku.data.ws.VoiceRemovedReason
import io.wolftown.kaiku.di.VoiceCoroutineScope
import io.wolftown.kaiku.service.VoiceCallService
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.CoroutineStart
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.asCoroutineDispatcher
import kotlinx.coroutines.cancel
import kotlinx.coroutines.cancelAndJoin
import kotlinx.coroutines.currentCoroutineContext
import kotlinx.coroutines.ensureActive
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharedFlow
import kotlinx.coroutines.flow.asSharedFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.NonCancellable
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import kotlinx.coroutines.withTimeoutOrNull
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import java.io.Closeable
import java.util.logging.Level
import java.util.logging.Logger
import javax.inject.Inject
import javax.inject.Singleton

/**
 * Orchestrates WebRtcManager + KaikuWebSocket for the complete voice flow.
 *
 * Manages:
 * - Joining and leaving voice channels
 * - Participant tracking via WebSocket events
 * - Mute/unmute state
 * - Screen share tracking
 * - Audio focus and foreground service lifecycle
 *
 * **Locks and WebRTC's threads.** Calls into WebRTC wait for its signalling
 * thread, and its callbacks run on that thread (see [WebRtcManager]). The
 * callbacks wired here send frames to the peer ([sendToPeer]) and report
 * trouble, so they take [sessionLock] (and, inside it, the socket lock) and
 * the signalling state lock. The rule:
 * - Never call into [WebRtcManager] (mute, create, close) while holding
 *   [sessionLock], the socket lock or the signalling state lock. Decide under
 *   the lock, act on WebRTC after leaving it ([toggleMute]). The install
 *   step of a new connection ([guardFor]) only assigns a field.
 * - [muteLock], the [door] and [joinLock] may be held across calls into
 *   WebRTC: no callback takes them.
 * - Where locks nest, the order is [sessionLock], then the socket lock or
 *   WebRtcManager's connection lock. Nothing nests the other way.
 */
@Singleton
class VoiceRepository @Inject constructor(
    private val webRtcManager: WebRtcManager,
    private val webSocket: KaikuWebSocket,
    private val audioRouteManager: AudioRouteManager,
    private val voiceServiceEvents: VoiceServiceEvents,
    private val micPermissionChecker: MicPermissionChecker,
    @ApplicationContext private val context: Context,
    @VoiceCoroutineScope voiceScope: CoroutineScope,
    /** The 60 s are real elapsed time, including device suspension. */
    elapsedClock: ElapsedClock = ElapsedClock.STOPPED,
    /** A failed proof of carried makes the connection reconnect at once. */
    private val deadSocket: DeadSocketReporter = DeadSocketReporter.NONE,
    /** Voice sessions belong to the signed-in session they were opened for. */
    private val sessions: SessionGeneration = SessionGeneration({ AuthSession.LoggedIn("", 1L) }),
) : Closeable {
    companion object {
        private val logger = Logger.getLogger("VoiceRepository")

        /** How long a sign-out waits for the microphone to be released. */
        const val RELEASE_WAIT_MS = 2_000L
    }

    // Child of the injected voice scope (Dispatchers.IO in production, the test
    // scheduler in unit tests). Own SupervisorJob so close() only cancels this
    // repository's collectors, not WebRtcManager's work on the shared scope.
    private val scope = CoroutineScope(
        voiceScope.coroutineContext + SupervisorJob(voiceScope.coroutineContext[Job])
    )

    /**
     * The door: the one lock under which a voice session is replaced
     * ([replaceSession]) or torn down. Whoever holds it is the only one
     * ending or opening a session, so there is never more than one, and a
     * new one opens only after the previous one is torn down completely. A
     * coroutine mutex that no WebRTC callback takes: it may be held across
     * calls into WebRTC.
     */
    private val door = Mutex()

    /** Result of one rejoin attempt, completed by ICE, WebRTC and `voice_error` events. */
    private enum class AttemptResult { Connected, Failed }

    // -- The session and its owner ---------------------------------------------
    //
    // A session is one stay in a voice channel or in a DM call's room, from
    // its opening to its end (own leave, `voice_removed`, give-up, failed
    // join, replacement by another session, sign-out).
    //
    // Invariants:
    //
    //  1. One owner. The session's signalling (first `voice_join`, held join,
    //     rejoin attempts, give-up) is one coroutine, [Session.owner], running
    //     [VoiceSignalling.run]. Nothing else sends `voice_join` or creates a
    //     peer connection.
    //  2. One socket per peer. `voice_join` goes out on the socket the owner
    //     read as authenticated, and only if it still is ([sendJoinOn]);
    //     [Session.joinSocket] records which. Every other frame of the session
    //     (mute, offers, ICE, leave) goes to that same socket or is not sent
    //     ([sendToPeer]): the server-side peer lives and dies with it. "Socket
    //     loss" is the plain comparison of that id with the authenticated one.
    //  3. Ending wins. [live] is cleared first thing when a session ends, by
    //     exactly one closer ([closeSession]), who also says why
    //     ([Session.end]). `voice_join` and the install of a new peer
    //     connection check it in one step with their effect, under
    //     [sessionLock], so neither can slip in after the end.
    //  4. One door. A session is torn down ([tearDown], once) and the next
    //     one opened only under the [door], and a teardown first waits until
    //     the cancelled owner is quiet ([Session.quiet]): nothing is closed
    //     or disposed while the owner can still call into WebRTC, and whatever
    //     the owner installed just before the end is closed too.
    //  5. A session never outlives its owner: when the owner is over without
    //     having been cancelled, it ends the session itself ([own]). Nor does
    //     it outlive the signed-in session it was opened for ([Session.ticket]).

    /**
     * A DM call's voice session: its rejoin gate, the name for the ongoing
     * notification, and the call layer's [tag] for it, which every event
     * about this session carries ([callEnds], [current]).
     */
    class CallSession(
        val gate: RejoinGate,
        val title: String?,
        val tag: Long = 0L,
        /** The call's mute, read when the session opens: a call brings its own. */
        val muted: () -> Boolean = { false },
        /** Mute tapped on a control of this session outside the call screen: the call decides. */
        val onMuteToggle: (() -> Unit)? = null,
    )

    /** Why a session ended; set by whoever closed it. */
    private sealed interface End {
        /** Left by the user or the call layer, or replaced by another session. */
        data object Left : End

        /** Given up after it was joined (attempts, 60 s) or because it never connected. */
        data class GaveUp(val neverConnected: Boolean) : End

        /** A rejoin was refused: the call it belongs to is over. */
        data object Refused : End

        /** The server removed the peer (no `voice_leave`). */
        data class Removed(val reason: VoiceRemovedReason) : End

        /** The first join failed; its error reaches the caller of the join. */
        data object Failed : End

        /** The signed-in session ended; `voice_leave` went out at once. */
        data object SignOut : End
    }

    private class Session(
        val number: Long,
        val channelId: String,
        val call: CallSession?,
        /** The signed-in session this voice session belongs to. */
        val ticket: SessionGeneration.Ticket,
    ) {
        /** The coroutine that owns the session's signalling. Set once, before the session is visible. */
        lateinit var owner: Job

        /**
         * The authenticated socket that carried the session's last
         * `voice_join`, or null while none went out (the join is held).
         * Written under [sessionLock], in one step with the send. A state
         * the owner's 60 s deadline reads: it runs while this is not the
         * authenticated socket.
         */
        val joinSocket = MutableStateFlow<Long?>(null)

        /**
         * Completes when the first join is set up. Fails with the join's
         * error, or with [SessionEnded] if the session ended before that.
         */
        val firstJoin = CompletableDeferred<Unit>()

        /**
         * Completes when the owner will not touch WebRTC, the call service
         * or the session's state any more: its signalling returned or threw,
         * or it gave the session up. A teardown waits for it.
         */
        val quiet = CompletableDeferred<Unit>()

        /** Why the session ended. Written with the close, under [sessionLock]. */
        var end: End? = null

        /** Whether [tearDown] ran for this session. Written under the [door]. */
        @Volatile
        var tornDown = false

        /** Set when the server removed the peer; a join still being set up reports it. */
        @Volatile
        var removedReason: VoiceRemovedReason? = null

        val info get() = VoiceSessionInfo(number, channelId, call?.tag)
    }

    private val sessionLock = Any()

    /** The session that is running; cleared first thing when it ends. Guarded by [sessionLock]. */
    private var live: Session? = null

    /** The most recent session, running or over (its teardown may still be due). Guarded by [sessionLock]. */
    private var last: Session? = null

    /** Numbers the sessions. Written under the [door]. */
    private var sessionNumbers = 0L

    /** A step found its session ended, or a pending [joinChannel] learns that it did. */
    private class SessionEnded : CancellationException("voice session ended")

    /** What to open: a voice channel or a DM call's room, for the signed-in session of [ticket]. */
    private class Opening(
        val channelId: String,
        val call: CallSession?,
        val ticket: SessionGeneration.Ticket,
        val muted: () -> Boolean,
    )

    /** What a caller of [replaceSession] expects to be the current session. */
    private sealed interface Expected {
        /** Whatever is there is replaced (a join). */
        data object Anything : Expected

        /** Only this session: a leave names the session it means. */
        class This(val session: Session) : Expected

        /** Only the session of this call. */
        class OfCall(val tag: Long) : Expected
    }

    /**
     * The one way the voice session changes hands: "the session I expect is
     * the current one → [next], or none". Voice channel joins, DM call
     * joins and leaves all come through here, one at a time ([door]):
     *
     * 1. If another session than the expected one is current, nothing
     *    happens (null).
     * 2. The current session is ended (closed, its owner cancelled and
     *    waited for until it is quiet, torn down); so is one that ended by
     *    itself or at a sign-out and whose teardown is still due.
     * 3. Only then [next] is opened, if there is one and the signed-in
     *    session it was asked for is still the current one.
     *
     * Returns the opened session. The wait for its first join happens
     * outside the door. Once the door is entered, the replacement runs to
     * its end even if the caller is cancelled (then nothing is opened).
     */
    private suspend fun replaceSession(expected: Expected, next: Opening?): Session? = door.withLock {
        val caller = currentCoroutineContext()[Job]
        withContext(NonCancellable) {
            val pending = synchronized(sessionLock) {
                val running = live
                val current = running ?: last
                val named = when (expected) {
                    Expected.Anything -> true
                    is Expected.This -> expected.session === current
                    is Expected.OfCall -> current?.call != null && current.call.tag == expected.tag
                }
                if (!named) return@withContext null
                if (running != null) {
                    live = null
                    running.end = End.Left
                }
                last?.takeIf { !it.tornDown }
            }
            pending?.let { finish(it) }
            if (next == null || caller?.isActive == false) return@withContext null
            open(next)
        }
    }

    /**
     * Ends [session] for good: its owner is cancelled and has finished
     * whatever it was doing, then the teardown runs. Call with the [door] held.
     */
    private suspend fun finish(session: Session) {
        session.owner.cancel()
        session.quiet.await()
        tearDown(session)
    }

    /** [finish] for a session somebody already closed (a removal, a dropped join, a sign-out). */
    private suspend fun finishInDoor(session: Session) {
        withContext(NonCancellable) { door.withLock { finish(session) } }
    }

    /** The coroutine scope of the signed-in session the owners run in. Guarded by the [door]. */
    private var ownerScopeTicket: SessionGeneration.Ticket? = null
    private var ownerScope: CoroutineScope? = null

    /** Opens the session for [next] and starts its owner. Call with the [door] held. */
    private fun open(next: Opening): Session? {
        val session = Session(++sessionNumbers, next.channelId, next.call, next.ticket)
        if (ownerScopeTicket != next.ticket) {
            ownerScopeTicket = next.ticket
            ownerScope = next.ticket.scope(scope)
        }
        val owners = ownerScope!!
        val opened = synchronized(sessionLock) {
            // A sign-out advances the session generation first and then closes under this
            // lock: it either finds this session and closes it, or the ticket is dead here.
            if (!next.ticket.alive) return@synchronized false
            // Started below: `owner` is set before the owner runs or anybody can cancel it.
            session.owner = owners.launch(start = CoroutineStart.LAZY) { own(session) }
            live = session
            last = session
            _currentChannelId.value = next.channelId
            _current.value = session.info
            _endNotice.value = null
            // Both layers start from the same mute state (a Rejoin restores the user's
            // last choice; a call brings its own).
            _isMuted.value = next.muted()
            true
        }
        if (!opened) return null
        applyMuteToMicrophone()
        // Whatever ends the owner (also before it ran), a teardown and a pending join hear of it.
        session.owner.invokeOnCompletion {
            session.quiet.complete(Unit)
            session.firstJoin.completeExceptionally(SessionEnded())
        }
        session.owner.start()
        return session
    }

    /**
     * Ends [session] if it is still running, for the reason [end]. True for
     * exactly one caller, who then has to see to its teardown under the
     * [door] ([finishInDoor], or [tearDown] for the owner itself).
     */
    private fun closeSession(session: Session, end: End): Boolean = synchronized(sessionLock) {
        if (live !== session) return false
        live = null
        session.end = end
        true
    }

    private fun isLive(session: Session): Boolean = synchronized(sessionLock) { live === session }

    /** Installs a new peer connection only while [session] runs and [job] is active, in one step. */
    private fun guardFor(session: Session, job: Job?) = SessionGuard { install ->
        synchronized(sessionLock) {
            if (live !== session || job?.isActive == false || !session.ticket.alive) {
                false
            } else {
                install()
                true
            }
        }
    }

    /**
     * Sends `voice_join` on [socket] and records it as the session's join
     * socket; the current mute state follows on the same socket. Nothing is
     * sent and false is returned if the session ended, [job] was cancelled,
     * the signed-in session it belongs to is over or [socket] is not the
     * authenticated socket any more. All in one step under [sessionLock]: no
     * session end and no mute toggle in between.
     *
     * This is the point of no return, so everything that decides whether a
     * `voice_join` may still go out is looked at here, after whatever the
     * step had to wait for: the session's identity, and [permit] (the 60 s;
     * it throws when they are over, and nothing is sent).
     */
    private fun sendJoinOn(
        session: Session,
        socket: Long,
        job: Job?,
        permit: VoiceSignalling.JoinPermit,
    ): Boolean = synchronized(sessionLock) {
        if (live !== session || job?.isActive == false || !session.ticket.alive) return false
        permit.check()
        if (!webSocket.sendOn(socket, ClientEvent.VoiceJoin(session.channelId))) return false
        session.joinSocket.value = socket
        // The new server-side peer starts unmuted; the local track stays disabled.
        if (live === session && _isMuted.value) webSocket.sendOn(socket, ClientEvent.VoiceMute(session.channelId))
        true
    }

    /**
     * Sends a frame of [session] to its server-side peer: on the socket that
     * carried its `voice_join`, or not at all (no join yet, that socket is
     * gone, or the session ended).
     */
    private fun sendToPeer(session: Session, event: ClientEvent): Boolean = synchronized(sessionLock) {
        val socket = session.joinSocket.value
        live === session && socket != null && webSocket.sendOn(socket, event)
    }

    /**
     * The owner of [session]: runs its signalling until the session is given
     * up or the owner is cancelled (by whoever ends the session). If it is
     * over for any other reason (the first join failed, a step broke), it ends
     * the session itself, so a session never stays behind without an owner.
     */
    private suspend fun own(session: Session) {
        var failure: Exception? = null
        try {
            signalling.run(SessionSteps(session))
        } catch (e: CancellationException) {
            // Cancelled by whoever ended the session: that side tears down.
            currentCoroutineContext().ensureActive()
            // Not cancelled: either a step found the session ended (same), or
            // a step was cancelled from inside although the session runs.
            if (isLive(session)) failure = IllegalStateException("voice signalling step cancelled from inside", e)
        } catch (e: Exception) {
            failure = e
        } finally {
            // From here on the owner touches nothing of the session but its own teardown.
            session.quiet.complete(Unit)
        }
        val joined = session.firstJoin.isCompleted
        if (failure != null) {
            logger.log(Level.SEVERE, "Voice signalling failed (joined=$joined)", failure)
        }
        withContext(NonCancellable) {
            // Already closed after a give-up. A call that was up offers "Rejoin";
            // a first join that failed reports its error through joinChannel.
            if (closeSession(session, if (joined) End.GaveUp(neverConnected = false) else End.Failed)) {
                door.withLock { tearDown(session) }
            }
        }
        session.firstJoin.completeExceptionally(failure ?: SessionEnded())
    }

    /** One rejoin attempt (C32). A success only counts for the publisher of [generation], the attempt's own connections. */
    private class RunningAttempt(val generation: Long) {
        val result = CompletableDeferred<AttemptResult>()
    }

    /** The running rejoin attempt, if any. */
    @Volatile
    private var attempt: RunningAttempt? = null

    /**
     * The generation of the peer connections that reported ICE `FAILED` last,
     * or null. A state, not an event: the owner compares it with the
     * generation of the session's connections whenever it looks.
     */
    private val failedGeneration = MutableStateFlow<Long?>(null)

    private val signalling = VoiceSignalling(socket = webSocket.authenticatedSocket, clock = elapsedClock)

    /**
     * Whether the call is connected, interrupted (ICE trouble, or no
     * authenticated socket carries its join) or being rejoined. Drives the
     * "reconnecting" indication in the voice screen, the overlay and the
     * ongoing-call notification.
     */
    val linkState: StateFlow<VoiceLinkState> = signalling.state

    private val _callEnds = MutableSharedFlow<CallVoiceEnd>(extraBufferCapacity = 8)

    /**
     * How a DM call's voice session ended, once it was set up, with the
     * call's tag for that session (the call layer turns it into the end of
     * exactly that call). Call sessions publish no [endNotice]: a call is
     * never offered "Rejoin" by the voice layer.
     */
    val callEnds: SharedFlow<CallVoiceEnd> = _callEnds.asSharedFlow()

    private val _endNotice = MutableStateFlow<VoiceEndNotice?>(null)
    /**
     * Why the last call ended without the user leaving: the rejoin gave up
     * (with the "Rejoin" action) or the server removed the user (with its
     * reason). Cleared by [dismissEndNotice], [rejoin], any new join, an own
     * leave and logout.
     */
    val endNotice: StateFlow<VoiceEndNotice?> = _endNotice.asStateFlow()

    /** The running [startJoin] job; guarded by [joinLock]. */
    private val joinLock = Any()
    private var joinJob: Job? = null
    private var joiningChannelId: String? = null

    private val _currentChannelId = MutableStateFlow<String?>(null)
    /** The channel ID currently connected to, or null if not in a voice channel. */
    val currentChannelId: StateFlow<String?> = _currentChannelId.asStateFlow()

    private val _current = MutableStateFlow<VoiceSessionInfo?>(null)

    /**
     * The identity of the current voice session, or null. What the other
     * state of this repository ([linkState], [isMuted], [participants])
     * describes is this session; a consumer that means one particular
     * session (a DM call) checks it.
     */
    val current: StateFlow<VoiceSessionInfo?> = _current.asStateFlow()

    private val _participants = MutableStateFlow<List<VoiceParticipant>>(emptyList())
    /** List of participants in the current voice channel. */
    val participants: StateFlow<List<VoiceParticipant>> = _participants.asStateFlow()

    private val _isMuted = MutableStateFlow(false)
    /** Whether the local user's microphone is muted. */
    val isMuted: StateFlow<Boolean> = _isMuted.asStateFlow()

    private val _isConnected = MutableStateFlow(false)
    /** Whether the WebRTC connection is established. */
    val isConnected: StateFlow<Boolean> = _isConnected.asStateFlow()

    private val _error = MutableStateFlow<Int?>(null)
    /** `@StringRes` of the latest voice error (start failure, WebRTC failure), or null. */
    val error: StateFlow<Int?> = _error.asStateFlow()

    /** Dismisses the current [error]. */
    fun clearError() {
        _error.value = null
    }

    private val _screenShares = MutableStateFlow<List<ScreenShareInfo>>(emptyList())
    /** Active screen shares in the current voice channel. */
    val screenShares: StateFlow<List<ScreenShareInfo>> = _screenShares.asStateFlow()

    private val _layerPreferences = MutableStateFlow<Map<String, String>>(emptyMap())
    /** Current layer preference per stream (streamId -> "auto"|"high"|"medium"|"low"). */
    val layerPreferences: StateFlow<Map<String, String>> = _layerPreferences.asStateFlow()

    /** WebSocket event collection job — cancelled when leaving a channel. */
    private var eventCollectionJob: Job? = null

    /** Service event collection job — cancelled when leaving a channel. */
    private var serviceEventJob: Job? = null

    /** Dual-PC ICE-connected collection job — cancelled when leaving a channel. */
    private var iceConnectedJob: Job? = null

    // -- Public API ------------------------------------------------------------

    /**
     * Joins a voice channel and returns once the join is set up: the session's
     * owner ([own]) initialised WebRTC, created the subscriber connection,
     * started the foreground service, sent `voice_join` on an authenticated
     * socket and created the publisher offer. A session that is current (a
     * voice channel or a DM call) is ended first ([replaceSession]).
     *
     * While no socket is authenticated the join is held (and this call with
     * it); 60 s without the `voice_join` going out give the session up. Throws what made the join
     * fail, or a [CancellationException] if the session ended before the join
     * was set up (leave, `voice_removed`, give-up, sign-out). Cancelling the
     * caller ends the session.
     */
    suspend fun joinChannel(channelId: String, muted: Boolean = false) {
        join(channelId, { muted }, call = null)
    }

    /**
     * Joins the voice room of DM call [channelId] (the DM's id), like
     * [joinChannel]. Before every rejoin attempt the session asks the call's
     * gate (rejoin of a DM call). Its end, once set up, is published on
     * [callEnds] with the call's tag.
     */
    suspend fun joinCall(channelId: String, call: CallSession) {
        join(channelId, call.muted, call)
    }

    suspend fun joinCall(channelId: String, title: String?, gate: RejoinGate) = joinCall(channelId, CallSession(gate, title))

    private suspend fun join(channelId: String, muted: () -> Boolean, call: CallSession?) {
        // Fail fast: the microphone foreground service and capture cannot start
        // without RECORD_AUDIO, so don't touch WebRTC at all.
        if (!micPermissionChecker.isGranted()) {
            throw MissingMicPermissionException()
        }
        // A voice session belongs to the signed-in session it was asked for; one that is being
        // signed out gets no new voice session.
        val ticket = sessions.enter()?.takeIf { it.open } ?: throw SessionEnded()
        val session = replaceSession(Expected.Anything, Opening(channelId, call, ticket, muted)) ?: throw SessionEnded()
        try {
            session.firstJoin.await()
            logger.info("Joined a voice channel")
        } catch (e: CancellationException) {
            // The caller dropped the join: its session ends here. (If the
            // session ended by itself, that side tears it down.)
            withContext(NonCancellable) {
                if (closeSession(session, End.Left)) finishInDoor(session)
            }
            // Not cancelled ourselves: the server removed the peer before the join was set up.
            val removed = session.removedReason
            if (removed != null && currentCoroutineContext().isActive) throw VoiceRemovedException(removed)
            throw e
        }
    }

    /**
     * Joins [channelId] on the repository's own scope, for the UI: leaving the
     * voice screen (Back) during setup does not cancel the join. Only Leave
     * ([leaveChannel], [leaveInBackground]) or logout cancel it. A failure is
     * published as [error] (`@StringRes`). No-op while a join of the same
     * channel runs; a join of another channel replaces it.
     */
    fun startJoin(channelId: String, muted: Boolean = false) {
        synchronized(joinLock) {
            if (joiningChannelId == channelId && joinJob?.isActive == true) return
            val previous = joinJob
            joiningChannelId = channelId
            joinJob = scope.launch {
                previous?.cancelAndJoin()
                try {
                    joinChannel(channelId, muted)
                } catch (e: CancellationException) {
                    throw e
                } catch (e: MissingMicPermissionException) {
                    _error.value = R.string.voice_mic_permission_required
                } catch (e: VoiceRemovedException) {
                    // Removed by the server while joining: [endNotice] says why, no error on top.
                } catch (e: VoiceServiceStartException) {
                    _error.value = R.string.voice_start_failed
                } catch (e: Exception) {
                    // The owner already logged and cleaned up (which clears the error).
                    _error.value = R.string.voice_join_failed
                }
            }
        }
    }

    /**
     * Cancels a running [startJoin] and waits until it is gone, so a leave
     * never interleaves with a join that has not opened its session yet.
     * [self] is the caller's job: a leave that runs inside the join's own
     * job does not cancel itself.
     */
    private suspend fun cancelJoin(self: Job?) {
        val job = synchronized(joinLock) {
            val running = joinJob
            if (running == null || running === self) {
                null
            } else {
                joinJob = null
                joiningChannelId = null
                running
            }
        } ?: return
        job.cancelAndJoin()
    }

    /**
     * Leaves the voice session that is current when this is called (or the
     * one being joined), and no other: a session opened meanwhile by someone
     * else stays.
     *
     * 1. End that session: from here on no `voice_join` and no new connection
     * 2. Cancel its owner and a running join and wait for both
     * 3. Tear down under the door ([replaceSession] to "none"): clear the
     *    channel at once (the overlay disappears without a flash), send
     *    `voice_leave`, close the peer connections, abandon audio focus, stop
     *    the foreground service
     *
     * Once the session is ended, the rest runs to its end even if the caller
     * is cancelled, so a call is never left half torn down.
     */
    suspend fun leaveChannel() {
        val self = currentCoroutineContext()[Job]
        // An own leave wins, at once.
        val named = synchronized(sessionLock) {
            val running = live
            if (running != null) {
                live = null
                running.end = End.Left
            }
            running ?: last
        }
        withContext(NonCancellable) {
            named?.owner?.cancel()
            cancelJoin(self)
            // Also for a session that is over: a teardown of it that is running holds the door.
            if (named != null) replaceSession(Expected.This(named), null)
            // Own leave and logout: a notice of an earlier call is no longer
            // relevant. After the teardown (the door), so that a removal tearing
            // down the same session right now has set its notice by this point.
            _endNotice.value = null
        }
    }

    /**
     * Leaves the voice session of the DM call [tag] if it is the current
     * one. Any other session (a voice channel the user moved to, another
     * call) is left alone: check and leave are one step under the door.
     */
    suspend fun leaveCall(tag: Long) {
        // Wins at once: no voice_join of that session after this.
        synchronized(sessionLock) {
            live?.takeIf { it.call != null && it.call.tag == tag }?.let { running ->
                live = null
                running.end = End.Left
                running.owner.cancel()
            }
        }
        withContext(NonCancellable) {
            replaceSession(Expected.OfCall(tag), null)
            // A call that ends before its session took the microphone service over gives it up.
            releaseHold(tag)
        }
    }

    // -- The microphone service ----------------------------------------------------

    /**
     * Orders the starts and stops of the microphone service and guards
     * [heldForCall]. No WebRTC callback takes it; where it nests,
     * [sessionLock] is taken inside it, never the other way round.
     */
    private val serviceLock = Any()

    /** The call (its tag) the service was started for before its session exists, or null. Guarded by [serviceLock]. */
    private var heldForCall: Long? = null

    /** Listens to the service while it is held for a call without a session. Guarded by [serviceLock]. */
    private var holdEvents: Job? = null

    /**
     * Starts the microphone service for the DM call [tag] now, before the
     * call's voice session exists: when the user taps Call or Accept the
     * app is in front, which Android requires to start a microphone
     * service, while the session is opened only after the requests to the
     * server and the ICE configuration, when the user may have left the app.
     * The call's session takes the service over ([SessionSteps.prepare]);
     * [leaveCall] gives it up if no session ever did. A session that is
     * current meanwhile (a voice channel being left for the call) keeps
     * running under it.
     *
     * Returns false if Android refused the start. If the service itself
     * reports later that it could not go to the foreground while no session
     * has taken it over yet, [onStartFailed] is called (once).
     */
    fun holdForCall(tag: Long, channelId: String, title: String?, onStartFailed: () -> Unit = {}): Boolean =
        synchronized(serviceLock) {
            // Listen first, so the service's own report cannot be missed (the events have no replay).
            holdEvents?.cancel()
            holdEvents = scope.launch(start = CoroutineStart.UNDISPATCHED) {
                voiceServiceEvents.events.collect { event ->
                    if (event == VoiceServiceEvent.StartFailed) {
                        val mine = synchronized(serviceLock) { (heldForCall == tag).also { if (it) heldForCall = null } }
                        if (mine) onStartFailed()
                    }
                }
            }
            try {
                VoiceCallService.start(context, channelId, title ?: channelId, forCall = true)
            } catch (e: IllegalStateException) {
                // ForegroundServiceStartNotAllowedException (API 31+) is one.
                logger.warning("Call service refused for a call: ${e::class.simpleName}")
                stopHoldEvents()
                return false
            } catch (e: SecurityException) {
                logger.warning("Call service refused for a call: ${e::class.simpleName}")
                stopHoldEvents()
                return false
            }
            heldForCall = tag
            true
        }

    /** Call with [serviceLock] held. */
    private fun stopHoldEvents() {
        holdEvents?.cancel()
        holdEvents = null
    }

    /** Gives up the service held for call [tag] if no session took it over. */
    private fun releaseHold(tag: Long) = synchronized(serviceLock) {
        if (heldForCall != tag) return
        heldForCall = null
        stopHoldEvents()
        val running = synchronized(sessionLock) { live }
        if (running == null) {
            VoiceCallService.stop(context)
            return
        }
        // The user is still in a voice session (the call never replaced it): the service is its again.
        try {
            VoiceCallService.start(context, running.channelId, running.call?.title ?: running.channelId, forCall = running.call != null)
        } catch (e: IllegalStateException) {
            logger.warning("Call service not handed back: ${e::class.simpleName}")
        } catch (e: SecurityException) {
            logger.warning("Call service not handed back: ${e::class.simpleName}")
        }
    }

    /** Whether the microphone service is up for a call whose session is not open yet (its first notification says "Connecting…"). */
    val holdsServiceForCall: Boolean get() = synchronized(serviceLock) { heldForCall != null }

    /**
     * Signed-in user: ends the call at once, synchronously, before the
     * account state is cleared ([io.wolftown.kaiku.data.session.SessionEndHook]).
     * From here on no `voice_join` and no new connection; `voice_leave` goes
     * to the peer on its socket; no "Rejoin" is offered.
     *
     * The connections are closed (the microphone is released) and audio
     * focus and the call service are given up on another thread, under the
     * door and only after the cancelled owner is quiet: nothing is disposed
     * while the owner may still be setting a connection up. The caller waits
     * for that for at most [RELEASE_WAIT_MS]; whatever joins later waits
     * behind it, because it needs the same door.
     *
     * Holds no lock while it calls into WebRTC. Its caller may hold the token
     * store's monitor, which no WebRTC callback takes.
     */
    fun endForSignOut() {
        var closedHere = false
        val session = synchronized(sessionLock) {
            val running = live
            if (running != null) {
                live = null
                running.end = End.SignOut
                closedHere = true
            }
            running ?: last?.takeIf { !it.tornDown }
        }
        synchronized(joinLock) { joinJob?.cancel() }
        _endNotice.value = null
        // A call that was being answered: its hold on the microphone service ends with the session.
        val held = synchronized(serviceLock) {
            stopHoldEvents()
            heldForCall.also { heldForCall = null }
        }
        if (session == null) {
            // No session to tear down: the service goes by itself (off the caller's thread).
            if (held != null) releaseScope.launch { synchronized(serviceLock) { VoiceCallService.stop(context) } }
            return
        }
        session.owner.cancel()
        if (closedHere) {
            session.joinSocket.value?.let { socket ->
                try {
                    webSocket.sendOn(socket, ClientEvent.VoiceLeave(session.channelId))
                } catch (e: Exception) {
                    logger.log(Level.WARNING, "Failed to send VoiceLeave at sign-out", e)
                }
            }
        }
        // Everything of the session that is not WebRTC, now: its state is gone at once.
        clearSessionState()
        // The microphone and the connections: behind the owner, under the door, never on the
        // caller's thread (which may be the main thread holding the token store's lock).
        val released = java.util.concurrent.CountDownLatch(1)
        lastRelease = released
        releaseScope.launch {
            try {
                door.withLock { finish(session) }
            } catch (e: Exception) {
                logger.log(Level.WARNING, "Releasing voice media at sign-out failed: ${e::class.simpleName}")
            } finally {
                released.countDown()
            }
        }
        if (!released.await(releaseWaitMs, java.util.concurrent.TimeUnit.MILLISECONDS)) {
            logger.warning("Voice media still being released at sign-out")
        }
        logger.info("Voice session ended at sign-out")
    }

    /** Runs the release of a sign-out ([endForSignOut]) on its own thread. */
    private val releaseScope = CoroutineScope(
        SupervisorJob() + java.util.concurrent.Executors.newSingleThreadExecutor { r ->
            Thread(r, "voice-release").apply { isDaemon = true }
        }.asCoroutineDispatcher()
    )

    /** How long [endForSignOut] waits for the release; shortened by tests whose owner runs on the test thread. */
    @androidx.annotation.VisibleForTesting
    internal var releaseWaitMs = RELEASE_WAIT_MS

    /** Counted down when the release of the last sign-out is done (tests wait for it). */
    @androidx.annotation.VisibleForTesting
    @Volatile
    internal var lastRelease: java.util.concurrent.CountDownLatch? = null

    /**
     * The teardown of [session], once per session, under the [door] and
     * after its owner is quiet: clears the channel, sends `voice_leave`
     * unless the peer is gone already, closes the peer connections, releases
     * audio focus and the foreground service, clears the state ([cleanUp])
     * and says how it ended ([Session.end]): a voice channel through
     * [endNotice], a DM call through [callEnds].
     *
     * `voice_leave` goes only to a peer the session's `voice_join` created on
     * a socket that is still there: a join that was held never reached the
     * server, and a peer whose socket is gone is gone with it.
     */
    private fun tearDown(session: Session) {
        if (session.tornDown) return
        session.tornDown = true
        val end = synchronized(sessionLock) { session.end } ?: End.Left
        // Before the clean-up resets what the notice reads (the mute state).
        val notice = when (end) {
            is End.GaveUp -> rejoinFailedNotice(session, end.neverConnected)
            is End.Removed -> VoiceEndNotice.Removed(session.channelId, end.reason)
            else -> null
        }
        val firstJoinDone = session.firstJoin.isCompleted && !session.firstJoin.isCancelled
        // The channel goes before the call service is stopped ([cleanUp]):
        // from here on the service posts no notification any more.
        _currentChannelId.value = null
        _current.value = null
        if (end !is End.Removed && end != End.SignOut) {
            val socket = session.joinSocket.value
            if (socket != null) {
                try {
                    webSocket.sendOn(socket, ClientEvent.VoiceLeave(session.channelId))
                } catch (e: Exception) {
                    logger.log(Level.WARNING, "Failed to send VoiceLeave", e)
                }
            }
        }
        cleanUp()
        if (session.call == null) {
            if (notice != null) _endNotice.value = notice
        } else if (firstJoinDone && end != End.SignOut) {
            // A call's end goes to the call layer instead, with the call's tag (only once its
            // join was set up: a join that fails before reports through joinCall).
            val why = when (end) {
                End.Refused -> VoiceEnd.Refused
                is End.GaveUp -> VoiceEnd.Lost
                is End.Removed -> VoiceEnd.Removed(end.reason)
                else -> VoiceEnd.Left
            }
            _callEnds.tryEmit(CallVoiceEnd(session.call.tag, why))
        }
        logger.info("Voice session ended")
    }

    /**
     * Leaves on the repository's own scope, for UI callers (the Leave button,
     * the overlay's disconnect) whose scope may end right away, e.g. when the
     * screen navigates back after the tap.
     */
    fun leaveInBackground() {
        scope.launch {
            try {
                leaveChannel()
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                logger.log(Level.WARNING, "Failed to leave voice channel: ${e::class.simpleName}")
            }
        }
    }

    /**
     * Toggles the local microphone mute state.
     */
    fun toggleMute() {
        if (_currentChannelId.value == null) return
        // A call's mute belongs to the call: its controls outside the call screen (the ongoing
        // notification, the voice bar) only ask the call, which answers with [setCallMuted].
        val call = synchronized(sessionLock) { live?.call }
        if (call?.onMuteToggle != null) {
            call.onMuteToggle.invoke()
            return
        }
        // The decision and its frame are one step with [sendJoinOn]: a toggle
        // and the mute state that follows a voice_join can never cross, so the
        // server's view stays the local one.
        synchronized(sessionLock) {
            val newMuted = !_isMuted.value
            _isMuted.value = newMuted

            // Only to the peer our voice_join created, on its socket. While the
            // join is held or a rejoin is pending, the state goes out right
            // after the next voice_join instead.
            val session = live ?: return@synchronized
            val channelId = session.channelId
            sendToPeer(session, if (newMuted) ClientEvent.VoiceMute(channelId) else ClientEvent.VoiceUnmute(channelId))
        }
        // The microphone follows outside the session lock: muting the track
        // waits for WebRTC's signalling thread, whose callbacks take that lock.
        applyMuteToMicrophone()
    }

    /**
     * Mutes or unmutes the voice session of the DM call [tag], and only that
     * one: while another session is current (a voice channel the user is
     * still in while the call starts) nothing changes, and the call's
     * session later opens with the call's mute ([CallSession.muted], read in
     * the same step as the session is opened).
     */
    fun setCallMuted(tag: Long, muted: Boolean) {
        synchronized(sessionLock) {
            val session = live?.takeIf { it.call != null && it.call.tag == tag } ?: return
            if (_isMuted.value == muted) return
            _isMuted.value = muted
            val channelId = session.channelId
            sendToPeer(session, if (muted) ClientEvent.VoiceMute(channelId) else ClientEvent.VoiceUnmute(channelId))
        }
        applyMuteToMicrophone()
    }

    /**
     * Guards [applyMuteToMicrophone]. No WebRTC callback takes it, so it may
     * be held across the call into WebRTC.
     */
    private val muteLock = Any()

    /**
     * Brings the microphone in line with [isMuted] as it is at this moment.
     * Every change of [isMuted] is followed by a call, and each call applies
     * the latest state, so the last one to finish leaves the microphone as
     * [isMuted] says, in whatever order concurrent toggles get here.
     */
    private fun applyMuteToMicrophone() {
        synchronized(muteLock) { webRtcManager.setMuted(_isMuted.value) }
    }

    override fun close() {
        scope.cancel()
    }

    /**
     * Sets the preferred simulcast layer for a screen share stream.
     *
     * @param streamId The screen share stream ID.
     * @param layer One of "auto", "high", "medium", "low".
     */
    fun setLayerPreference(streamId: String, layer: String) {
        val channelId = _currentChannelId.value ?: return

        // Find the screen share to get the userId for the target
        val share = _screenShares.value.find { it.streamId == streamId } ?: return

        // One step with its frame (and with a rejoin's resend): the last frame is the latest choice.
        synchronized(layerLock) {
            _layerPreferences.value = _layerPreferences.value.toMutableMap().apply {
                put(streamId, layer)
            }
            val session = synchronized(sessionLock) { live } ?: return
            sendToPeer(
                session,
                ClientEvent.VoiceSetLayerPreference(
                    channelId = channelId,
                    targetUserId = share.userId,
                    trackSource = "screen_video:$streamId",
                    preferredLayer = layer
                )
            )
        }
        logger.info("Set a screen share layer preference: $layer")
    }

    /**
     * Orders a layer choice (map update and its frame) against a rejoin's
     * resend. Taken before [sessionLock], never inside it; nothing in it
     * calls into WebRTC.
     */
    private val layerLock = Any()

    /**
     * Returns the default layer based on the current network type.
     *
     * - WiFi: "auto" (server picks best layer based on bandwidth)
     * - Cellular: "low" (conserve data)
     */
    private fun defaultLayerForNetwork(): String {
        val connectivityManager =
            context.getSystemService(Context.CONNECTIVITY_SERVICE) as? ConnectivityManager
                ?: return "auto"
        val network = connectivityManager.activeNetwork ?: return "low"
        val capabilities = connectivityManager.getNetworkCapabilities(network) ?: return "low"
        return if (capabilities.hasTransport(NetworkCapabilities.TRANSPORT_WIFI)) {
            "auto"
        } else {
            "low"
        }
    }

    // -- The steps of a session (run by its owner only) -------------------------

    private inner class SessionSteps(private val session: Session) : VoiceSignalling.Steps {
        private val channelId = session.channelId

        override val joinSocket: StateFlow<Long?> get() = session.joinSocket

        /**
         * Before the first `voice_join`: WebRTC, the subscriber connection
         * (it receives the server's offer), the signalling callbacks, the
         * event collectors and the call service. Needs no socket.
         */
        override suspend fun prepare() {
            val guard = guardFor(session, currentCoroutineContext()[Job])
            webRtcManager.initialize()
            if (!webRtcManager.createSubscriberPeerConnection(guard)) throw SessionEnded()
            wirePeerCallbacks(session)
            startCollectingEvents(session)

            // The call is up once the publisher PC is connected; the subscriber
            // is not required (an empty channel never gets a subscriber offer).
            // Same criterion for the first join and for a rejoin attempt.
            iceConnectedJob?.cancel()
            iceConnectedJob = scope.launch {
                webRtcManager.publisherLink.collect { link ->
                    _isConnected.value = link.connected
                    if (link.connected) {
                        signalling.onIceConnected()
                        // Only the attempt's own publisher counts (identity check).
                        attempt?.takeIf { it.generation == link.generation }
                            ?.result?.complete(AttemptResult.Connected)
                    }
                }
            }

            // Last, when the set-up stands: from here on the join may be held
            // for the socket, for up to a minute.
            startCallService()
        }

        /**
         * The first `voice_join`, on [socket], and what follows it: the
         * publisher offer (Android-initiated; its offer and ICE frames follow
         * the join on the same socket) and audio focus. The microphone is
         * captured from here on; the call service is already running.
         */
        override suspend fun join(socket: Long, permit: VoiceSignalling.JoinPermit): Boolean {
            val job = currentCoroutineContext()[Job]
            if (!sendJoinOn(session, socket, job, permit)) {
                if (!isLive(session)) throw SessionEnded()
                return false
            }

            // This adds the mic track, creates an SDP offer, and fires
            // onPublisherOffer when ready — which sends VoicePublisherOffer.
            if (!webRtcManager.createPublisherOffer(guardFor(session, job))) throw SessionEnded()

            // A DM call starts on the earpiece (held to the ear), a voice channel on the speaker.
            audioRouteManager.requestAudioFocus(preferEarpiece = session.call != null)

            session.firstJoin.complete(Unit)
            return true
        }

        /**
         * Starts the call service (the microphone foreground service) as soon
         * as the set-up stands, before the join waits for a socket: right
         * after the user's tap, while the app is in the foreground. (A DM
         * call's service is started even earlier, at the tap itself,
         * [holdForCall]; its session only takes it over here.) Android
         * 12+ refuses to start a foreground service from the background, and
         * a join can be held for the socket long after the user left the
         * app. The service then stays for the whole session, through the
         * held join and every rejoin, and is stopped once, by the teardown.
         * (Not before the set-up: a set-up that fails at once, e.g. without
         * network, would stop the service before it reached the foreground.)
         *
         * If Android refuses the start all the same, the join ends here with
         * [VoiceServiceStartException]; if the service itself cannot go to
         * the foreground, it reports [VoiceServiceEvent.StartFailed].
         */
        private fun startCallService() {
            // Collect service events first (so StartFailed cannot be missed), then
            // start the foreground service.
            // UNDISPATCHED: subscribe before the service can emit (replay is 0).
            serviceEventJob?.cancel()
            serviceEventJob = scope.launch(start = CoroutineStart.UNDISPATCHED) {
                voiceServiceEvents.events.collect { event ->
                    when (event) {
                        VoiceServiceEvent.MuteToggle -> toggleMute()
                        VoiceServiceEvent.Disconnect -> leaveChannel()
                        VoiceServiceEvent.StartFailed -> {
                            try {
                                leaveChannel()
                            } finally {
                                // The teardown clears _error, so set it afterwards.
                                _error.value = R.string.voice_start_failed
                            }
                        }
                    }
                }
            }
            synchronized(serviceLock) {
                // A call's service was started when the user tapped Call or Accept: taken over.
                if (session.call != null && heldForCall == session.call.tag) {
                    heldForCall = null
                    // This session listens to the service itself from here on (subscribed above).
                    stopHoldEvents()
                    return
                }
                try {
                    VoiceCallService.start(context, channelId, session.call?.title ?: channelId, forCall = session.call != null)
                } catch (e: IllegalStateException) {
                    // ForegroundServiceStartNotAllowedException (API 31+) is one.
                    throw VoiceServiceStartException(e)
                } catch (e: SecurityException) {
                    throw VoiceServiceStartException(e)
                }
            }
        }

        /** The publisher of the session's current connections is connected (first join). */
        override suspend fun awaitConnected() {
            val generation = webRtcManager.currentGeneration
            webRtcManager.publisherLink.first { it.connected && it.generation == generation }
        }

        /**
         * Proof of carried after a gap: a fresh inbound frame arrives on
         * [socket] after a ping, within 5 s, and the publisher, read from
         * the connection, is connected and has not failed.
         */
        override suspend fun proveCarried(socket: Long): VoiceSignalling.Proof {
            // The socket first, whatever the media says: when both died, the dead socket is
            // found (and replaced) and the time without it counts.
            val before = webSocket.inbound.value.takeIf { it.socket == socket }?.frames ?: 0L
            if (!webSocket.sendOn(socket, ClientEvent.Ping)) return VoiceSignalling.Proof.SocketDead
            val fresh = withTimeoutOrNull(VoiceSignalling.PROOF_TIMEOUT_MS) {
                webSocket.inbound.first { it.socket == socket && it.frames > before }
            } != null
            if (!fresh) {
                // A socket that does not answer is dead: a new one now, not after the heartbeat watchdog.
                if (webSocket.authenticatedSocket.value == socket) deadSocket.report()
                return VoiceSignalling.Proof.SocketDead
            }
            // The socket answers. The media: read from the connection, outside every lock
            // (the read waits for WebRTC's thread).
            val generation = webRtcManager.currentGeneration
            if (failedGeneration.value == generation) return VoiceSignalling.Proof.MediaDown
            if (!webRtcManager.publisherConnectedNow()) return VoiceSignalling.Proof.MediaDown
            return VoiceSignalling.Proof.Carried
        }

        /** The connections of the session as they are now reported ICE `FAILED`. */
        override suspend fun awaitConnectionFailed() {
            val generation = webRtcManager.currentGeneration
            failedGeneration.first { it == generation }
        }

        /**
         * One rejoin attempt (C32): discards the old peer connections, sends
         * `voice_join` on [socket], restores the mute state and waits until
         * the publisher connection is connected (the subscriber is not
         * required). The foreground service, audio focus and the voice state
         * stay as they are.
         *
         * Whatever ends the attempt short of success (failure, timeout, socket
         * loss, session end) closes the connections it created before it
         * returns.
         */
        override suspend fun rejoin(socket: Long, permit: VoiceSignalling.JoinPermit): VoiceSignalling.Attempt {
            val job = currentCoroutineContext()[Job]
            val guard = guardFor(session, job)
            if (!isLive(session)) throw SessionEnded()
            // A DM call rejoins only while the call is still on with this user in it.
            session.call?.let { call ->
                when (call.gate.mayRejoin(channelId)) {
                    RejoinVerdict.Allowed -> Unit
                    RejoinVerdict.Refused -> return VoiceSignalling.Attempt.Refused
                    RejoinVerdict.Unknown -> return VoiceSignalling.Attempt.Failed
                }
            }
            logger.info("Rejoining the voice channel")
            val own = RunningAttempt(webRtcManager.closePeerConnections())
            attempt = own
            var connected = false
            try {
                if (!webRtcManager.createSubscriberPeerConnection(guard)) return VoiceSignalling.Attempt.Failed
                // After the call's check with the server and the ICE configuration: the send decides.
                if (!sendJoinOn(session, socket, job, permit)) return VoiceSignalling.Attempt.SocketGone
                if (!webRtcManager.createPublisherOffer(guard)) return VoiceSignalling.Attempt.Failed
                // Already connected before we started waiting?
                webRtcManager.publisherLink.value.let { link ->
                    if (link.connected && link.generation == own.generation) {
                        own.result.complete(AttemptResult.Connected)
                    }
                }
                connected = own.result.await() == AttemptResult.Connected
                // The server's new peer knows none of the old one's choices.
                if (connected) resendLayerPreferences()
                return if (connected) VoiceSignalling.Attempt.Connected else VoiceSignalling.Attempt.Failed
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                logger.log(Level.WARNING, "Voice rejoin attempt failed: ${e::class.simpleName}")
                return VoiceSignalling.Attempt.Failed
            } finally {
                if (attempt === own) attempt = null
                if (!connected) webRtcManager.closePeerConnections()
            }
        }

        /** Sends the stored layer choice of every screen share that is still on to the (new) peer. */
        private fun resendLayerPreferences() = synchronized(layerLock) {
            // Read under the same lock as a change: a choice made meanwhile is never overwritten.
            val shares = _screenShares.value
            for ((streamId, layer) in _layerPreferences.value) {
                val share = shares.find { it.streamId == streamId } ?: continue
                sendToPeer(
                    session,
                    ClientEvent.VoiceSetLayerPreference(
                        channelId = channelId,
                        targetUserId = share.userId,
                        trackSource = "screen_video:$streamId",
                        preferredLayer = layer,
                    )
                )
            }
        }

        /**
         * All attempts failed, or no authenticated socket carried the
         * session's `voice_join` for 60 s: ends the
         * call locally (clears state, stops the foreground service) and offers
         * "Rejoin" through [endNotice]. The owner keeps the "reconnecting"
         * state until this returns.
         */
        override suspend fun giveUp(why: VoiceSignalling.GiveUp) {
            val end = when (why) {
                VoiceSignalling.GiveUp.Refused -> End.Refused
                VoiceSignalling.GiveUp.NeverConnected -> End.GaveUp(neverConnected = true)
                VoiceSignalling.GiveUp.Lost -> End.GaveUp(neverConnected = false)
            }
            if (!closeSession(session, end)) return
            // The owner is done with the session; only its teardown is left.
            session.quiet.complete(Unit)
            // The session is ended now: finish its teardown whatever happens to us.
            withContext(NonCancellable) { door.withLock { tearDown(session) } }
            logger.warning("Voice session gave up")
        }
    }

    /**
     * "Couldn't reconnect" (or "Couldn't connect" for a session that never
     * connected) with "Rejoin", which restores the user's last mute choice.
     */
    private fun rejoinFailedNotice(session: Session, neverConnected: Boolean = false): VoiceEndNotice =
        VoiceEndNotice.RejoinFailed(session.channelId, _isMuted.value, neverConnected)

    /** Signaling callbacks of both peer connections for [session]: every frame goes to the session's peer. */
    private fun wirePeerCallbacks(session: Session) {
        val channelId = session.channelId
        webRtcManager.onPublisherOffer = { sdp ->
            sendToPeer(session, ClientEvent.VoicePublisherOffer(channelId, sdp))
        }
        webRtcManager.onPublisherIceCandidate = { candidateJson ->
            sendToPeer(session, ClientEvent.VoiceIceCandidate(channelId, candidateJson, pcType = "publisher"))
        }
        webRtcManager.onSubscriberAnswer = { sdp ->
            sendToPeer(session, ClientEvent.VoiceSubscriberAnswer(channelId, sdp))
        }
        webRtcManager.onSubscriberIceCandidate = { candidateJson ->
            sendToPeer(session, ClientEvent.VoiceIceCandidate(channelId, candidateJson, pcType = "subscriber"))
        }
        webRtcManager.onError = { errorMsg ->
            logger.warning("WebRTC error: $errorMsg")
            reportVoiceError()
        }
        webRtcManager.onIceTrouble = { trouble, generation ->
            // Troubles of retired connections are dropped (identity check).
            if (generation == webRtcManager.currentGeneration) when (trouble) {
                // Indication only; ICE may recover by itself.
                IceTrouble.Disconnected -> signalling.onIceDisconnected()
                IceTrouble.Recovered -> signalling.onIceConnected()
                IceTrouble.Failed -> {
                    attempt?.takeIf { it.generation == generation }?.result?.complete(AttemptResult.Failed)
                    // The owner rejoins when it sees this on the session's connections.
                    failedGeneration.value = generation
                }
            }
        }
    }

    /**
     * C31: the server removed us from the call. Ends it locally without a
     * `voice_leave` (the peer is gone), never rejoins, and says why.
     */
    private fun onRemovedByServer(session: Session, reason: VoiceRemovedReason) {
        // At once, on the event's thread: no voice_join or new connection after this.
        if (!closeSession(session, End.Removed(reason))) return
        session.removedReason = reason
        session.owner.cancel()
        // Own job: the teardown cancels the collector this event arrived on. It waits
        // until the owner (a running join or rejoin attempt) is quiet.
        scope.launch {
            finishInDoor(session)
            logger.info("Removed from the voice channel by the server: $reason")
        }
    }

    /** The "Rejoin" action of [endNotice]: joins the same channel again. */
    fun rejoin() {
        val notice = _endNotice.value as? VoiceEndNotice.RejoinFailed ?: return
        _endNotice.value = null
        // A user-initiated Rejoin restores the user's last mute choice (C32).
        startJoin(notice.channelId, muted = notice.wasMuted)
    }

    fun dismissEndNotice() {
        _endNotice.value = null
    }

    // -- Internal -------------------------------------------------------------

    private fun cleanUp() {
        clearSessionState()
        releaseMedia()
    }

    /** The WebRTC, audio and service part of a teardown: microphone, connections, focus, service. */
    private fun releaseMedia() {
        webRtcManager.closePeerConnections()
        audioRouteManager.abandonAudioFocus()
        synchronized(serviceLock) {
            // A call that is being answered holds the service for its own session: it stays
            // up while the session before it is torn down.
            if (heldForCall == null) VoiceCallService.stop(context)
        }
        // Nothing is captured any more: both layers say "not muted" (the next join sets both again).
        applyMuteToMicrophone()
    }

    /** Everything of a session that is not WebRTC: collectors, callbacks, state. No call into WebRTC. */
    private fun clearSessionState() {
        // Stop event collection
        eventCollectionJob?.cancel()
        eventCollectionJob = null
        serviceEventJob?.cancel()
        serviceEventJob = null
        iceConnectedJob?.cancel()
        iceConnectedJob = null

        // Clear all dual-PC callbacks
        webRtcManager.onPublisherOffer = null
        webRtcManager.onPublisherIceCandidate = null
        webRtcManager.onSubscriberAnswer = null
        webRtcManager.onSubscriberIceCandidate = null
        webRtcManager.onError = null
        webRtcManager.onIceTrouble = null
        attempt?.result?.complete(AttemptResult.Failed)

        // Clear state
        _currentChannelId.value = null
        _current.value = null
        _participants.value = emptyList()
        _screenShares.value = emptyList()
        _layerPreferences.value = emptyMap()
        _isConnected.value = false
        // Nothing is captured any more: both layers say "not muted" (the next
        // join sets both again).
        _isMuted.value = false
        _error.value = null
    }

    private fun startCollectingEvents(session: Session) {
        eventCollectionJob?.cancel()
        // UNDISPATCHED: subscribe to the replay-0 SharedFlow before the owner
        // sends VoiceJoin, so a fast VoiceSubscriberOffer/PublisherAnswer cannot
        // be emitted while the collector is still waiting for an IO thread.
        eventCollectionJob = scope.launch(start = CoroutineStart.UNDISPATCHED) {
            webSocket.events.collect { event ->
                handleServerEvent(session, event)
            }
        }
    }

    /**
     * A WebRTC or `voice_error` failure. During a rejoin it belongs to the
     * running attempt (which fails at once) and never shows the generic error.
     */
    private fun reportVoiceError() {
        val running = attempt
        when {
            running != null -> running.result.complete(AttemptResult.Failed)
            signalling.state.value is VoiceLinkState.Rejoining -> { /* the owner is between attempts */ }
            else -> _error.value = R.string.voice_connection_error
        }
    }

    /**
     * Test hook: handles [event] synchronously, as the event collector of the
     * session in [channelId] would, so a test can deliver it in the middle of
     * a step.
     */
    @androidx.annotation.VisibleForTesting
    internal fun handleServerEventNow(channelId: String, event: ServerEvent) {
        val session = synchronized(sessionLock) { last?.takeIf { it.channelId == channelId } } ?: return
        handleServerEvent(session, event)
    }

    private fun handleServerEvent(session: Session, event: ServerEvent) {
        val channelId = session.channelId
        when (event) {
            is ServerEvent.VoiceRoomState -> {
                if (event.channelId == channelId) {
                    _participants.value = event.participants
                    _screenShares.value = event.screenShares
                }
            }

            is ServerEvent.VoicePublisherAnswer -> {
                if (event.channelId == channelId) {
                    webRtcManager.handlePublisherAnswer(event.sdp)
                }
            }

            is ServerEvent.VoiceSubscriberOffer -> {
                if (event.channelId == channelId) {
                    webRtcManager.handleSubscriberOffer(event.sdp)
                }
            }

            is ServerEvent.VoiceIceCandidate -> {
                if (event.channelId == channelId) {
                    webRtcManager.addIceCandidate(event.pcType, event.candidate)
                }
            }

            is ServerEvent.VoiceUserJoined -> {
                if (event.channelId == channelId) {
                    val newParticipant = VoiceParticipant(
                        userId = event.userId,
                        username = event.username,
                        displayName = event.displayName
                    )
                    val current = _participants.value
                    if (current.none { it.userId == event.userId }) {
                        _participants.value = current + newParticipant
                    }
                }
            }

            is ServerEvent.VoiceUserLeft -> {
                if (event.channelId == channelId) {
                    _participants.value = _participants.value.filter {
                        it.userId != event.userId
                    }
                }
            }

            is ServerEvent.VoiceUserMuted -> {
                if (event.channelId == channelId) {
                    _participants.value = _participants.value.map {
                        if (it.userId == event.userId) it.copy(muted = true) else it
                    }
                }
            }

            is ServerEvent.VoiceUserUnmuted -> {
                if (event.channelId == channelId) {
                    _participants.value = _participants.value.map {
                        if (it.userId == event.userId) it.copy(muted = false) else it
                    }
                }
            }

            is ServerEvent.ScreenShareStarted -> {
                if (event.channelId == channelId) {
                    val info = ScreenShareInfo(
                        streamId = event.streamId,
                        userId = event.userId,
                        username = event.username,
                        sourceLabel = event.sourceLabel,
                        hasAudio = event.hasAudio,
                        quality = event.quality,
                        startedAt = event.startedAt
                    )
                    val current = _screenShares.value
                    if (current.none { it.streamId == event.streamId }) {
                        _screenShares.value = current + info

                        // Auto-request stream with network-aware layer preference
                        val defaultLayer = defaultLayerForNetwork()
                        _layerPreferences.value = _layerPreferences.value.toMutableMap().apply {
                            put(event.streamId, defaultLayer)
                        }
                        sendToPeer(
                            session,
                            ClientEvent.VoiceSetLayerPreference(
                                channelId = channelId,
                                targetUserId = event.userId,
                                trackSource = "screen_video:${event.streamId}",
                                preferredLayer = defaultLayer
                            )
                        )
                        logger.info("Auto-requested a screen share at layer $defaultLayer")
                    }
                }
            }

            is ServerEvent.ScreenShareStopped -> {
                if (event.channelId == channelId) {
                    _screenShares.value = _screenShares.value.filter {
                        it.streamId != event.streamId
                    }
                    // Clean up layer preference for the stopped stream
                    _layerPreferences.value = _layerPreferences.value.toMutableMap().apply {
                        remove(event.streamId)
                    }
                    // Remove any video tracks associated with this stream
                    // Track IDs from the server follow the pattern: screen_video:{stream_id}
                    val videoTracks = webRtcManager.remoteVideoTracks.value
                    videoTracks.keys.filter { trackId ->
                        trackId.contains(event.streamId)
                    }.forEach { trackId ->
                        webRtcManager.removeVideoTrack(trackId)
                    }
                }
            }

            is ServerEvent.VoiceRemoved -> {
                // No channel id: the event only goes to the socket that owned the peer.
                if (event.channelId == null || event.channelId == channelId) {
                    onRemovedByServer(session, event.removedReason)
                }
            }

            is ServerEvent.VoiceError -> {
                logger.warning("Voice error from server: code=${event.code} message=${event.message}")
                reportVoiceError()
            }

            is ServerEvent.Error -> {
                logger.warning("Server error in voice context: code=${event.code} message=${event.message}")
            }

            else -> { /* Ignored — other events handled elsewhere */ }
        }
    }
}
