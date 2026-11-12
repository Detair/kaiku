package io.wolftown.kaiku.data.repository

import android.content.Context
import dagger.hilt.android.qualifiers.ApplicationContext
import android.net.ConnectivityManager
import android.net.NetworkCapabilities
import io.wolftown.kaiku.R
import io.wolftown.kaiku.data.voice.AudioRouteManager
import io.wolftown.kaiku.data.voice.IceTrouble
import io.wolftown.kaiku.data.voice.MicPermissionChecker
import io.wolftown.kaiku.data.voice.MissingMicPermissionException
import io.wolftown.kaiku.data.voice.SessionGuard
import io.wolftown.kaiku.data.voice.VoiceEndNotice
import io.wolftown.kaiku.data.voice.VoiceLinkState
import io.wolftown.kaiku.data.voice.VoiceServiceEvent
import io.wolftown.kaiku.data.voice.VoiceServiceEvents
import io.wolftown.kaiku.data.voice.VoiceServiceStartException
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
import kotlinx.coroutines.cancel
import kotlinx.coroutines.cancelAndJoin
import kotlinx.coroutines.currentCoroutineContext
import kotlinx.coroutines.ensureActive
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.NonCancellable
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
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
 * - [muteLock], [leaveMutex] and [joinLock] may be held across calls into
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
) : Closeable {
    companion object {
        private val logger = Logger.getLogger("VoiceRepository")
    }

    // Child of the injected voice scope (Dispatchers.IO in production, the test
    // scheduler in unit tests). Own SupervisorJob so close() only cancels this
    // repository's collectors, not WebRtcManager's work on the shared scope.
    private val scope = CoroutineScope(
        voiceScope.coroutineContext + SupervisorJob(voiceScope.coroutineContext[Job])
    )

    /** Serialises the teardowns ([tearDown]). */
    private val leaveMutex = Mutex()

    /** Result of one rejoin attempt, completed by ICE, WebRTC and `voice_error` events. */
    private enum class AttemptResult { Connected, Failed }

    // -- The session and its owner ---------------------------------------------
    //
    // A session is one stay in a voice channel, from [joinChannel] to its end
    // (own leave, `voice_removed`, give-up, failed join, channel switch, logout).
    //
    // Invariants (C31/C32 with the amendments):
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
    //     exactly one closer ([closeSession]). `voice_join` and the install of
    //     a new peer connection check it in one step with their effect, under
    //     [sessionLock], so neither can slip in after the end. The closer then
    //     cancels the owner, waits for it and only then tears down
    //     ([tearDown], once per session), so whatever the owner installed just
    //     before the end is closed too.
    //  4. A session never outlives its owner: when the owner is over without
    //     having been cancelled, it ends the session itself ([own]).

    private class Session(val channelId: String) {
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

        /** Whether [tearDown] ran for this session. Guarded by [leaveMutex]. */
        var tornDown = false
    }

    private val sessionLock = Any()

    /** The session that is running; cleared first thing when it ends. Guarded by [sessionLock]. */
    private var live: Session? = null

    /** The most recent session, running or over (its teardown may still be due). Guarded by [sessionLock]. */
    private var last: Session? = null

    /** A step found its session ended, or a pending [joinChannel] learns that it did. */
    private class SessionEnded : CancellationException("voice session ended")

    /** Opens the session for [channelId] and starts its owner. */
    private fun openSession(channelId: String): Session {
        val session = Session(channelId)
        synchronized(sessionLock) {
            // Started below: `owner` is set before the owner runs or anybody can cancel it.
            session.owner = scope.launch(start = CoroutineStart.LAZY) { own(session) }
            live = session
            last = session
        }
        // Whatever ends the owner, a pending joinChannel hears of it.
        session.owner.invokeOnCompletion { session.firstJoin.completeExceptionally(SessionEnded()) }
        session.owner.start()
        return session
    }

    /**
     * Ends [session] if it is still running. True for exactly one caller, who
     * then has to cancel the owner (unless it is the owner), wait for it and
     * [tearDown].
     */
    private fun closeSession(session: Session): Boolean = synchronized(sessionLock) {
        if (live !== session) return false
        live = null
        true
    }

    private fun isLive(session: Session): Boolean = synchronized(sessionLock) { live === session }

    /** Installs a new peer connection only while [session] runs and [job] is active, in one step. */
    private fun guardFor(session: Session, job: Job?) = SessionGuard { install ->
        synchronized(sessionLock) {
            if (live !== session || job?.isActive == false) {
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
     * sent and false is returned if the session ended, [job] was cancelled or
     * [socket] is not the authenticated socket any more. All in one step
     * under [sessionLock]: no session end and no mute toggle in between.
     */
    private fun sendJoinOn(session: Session, socket: Long, job: Job?): Boolean = synchronized(sessionLock) {
        if (live !== session || job?.isActive == false) return false
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
        }
        val joined = session.firstJoin.isCompleted
        if (failure != null) {
            logger.log(Level.SEVERE, "Voice signalling failed: ${session.channelId} (joined=$joined)", failure)
        }
        withContext(NonCancellable) {
            // Already closed after a give-up. A call that was up offers "Rejoin";
            // a first join that failed reports its error through joinChannel.
            if (closeSession(session)) {
                tearDown(session, sendLeave = true, notice = if (joined) ::rejoinFailedNotice else null)
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

    private val signalling = VoiceSignalling(socket = webSocket.authenticatedSocket)

    /**
     * Whether the call is connected, interrupted (ICE trouble, or no
     * authenticated socket carries its join) or being rejoined. Drives the
     * "reconnecting" indication in the voice screen, the overlay and the
     * ongoing-call notification.
     */
    val linkState: StateFlow<VoiceLinkState> = signalling.state

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
     * socket and created the publisher offer.
     *
     * While no socket is authenticated the join is held (and this call with
     * it); 60 s without the `voice_join` going out give the session up. Throws what made the join
     * fail, or a [CancellationException] if the session ended before the join
     * was set up (leave, `voice_removed`, give-up). Cancelling the caller
     * ends the session.
     */
    suspend fun joinChannel(channelId: String, muted: Boolean = false) {
        // Fail fast: the microphone foreground service and capture cannot start
        // without RECORD_AUDIO, so don't touch WebRTC at all.
        if (!micPermissionChecker.isGranted()) {
            throw MissingMicPermissionException()
        }

        // If already in a channel, leave first (this also waits for its owner).
        if (_currentChannelId.value != null) {
            leaveChannel()
        }

        _currentChannelId.value = channelId
        _endNotice.value = null
        // Both layers start from the same mute state (a Rejoin restores the user's last choice).
        _isMuted.value = muted
        applyMuteToMicrophone()
        val session = openSession(channelId)
        try {
            session.firstJoin.await()
            logger.info("Joined voice channel: $channelId")
        } catch (e: CancellationException) {
            // The caller dropped the join: its session ends here. (If the
            // session ended by itself, that side tears it down.)
            withContext(NonCancellable) {
                if (closeSession(session)) {
                    session.owner.cancelAndJoin()
                    tearDown(session, sendLeave = true)
                }
            }
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
     * [self] is the caller's job: a join that leaves a previous channel itself
     * (inside its own job) does not cancel itself.
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
     * Leaves the current voice channel (or the one being joined).
     *
     * 1. End the session: from here on no `voice_join` and no new connection
     * 2. Cancel its owner and a running join and wait for both
     * 3. Tear down ([tearDown]): clear the channel at once (the overlay
     *    disappears without a flash), send `voice_leave`, close the peer
     *    connections, abandon audio focus, stop the foreground service
     *
     * Once the session is ended, the rest runs to its end even if the caller
     * is cancelled, so a call is never left half torn down.
     */
    suspend fun leaveChannel() {
        val self = currentCoroutineContext()[Job]
        // An own leave wins.
        val (session, closedHere) = synchronized(sessionLock) {
            val running = live
            live = null
            last to (running != null)
        }
        withContext(NonCancellable) {
            session?.owner?.cancelAndJoin()
            cancelJoin(self)
            // A session that somebody else ended (removal, give-up) gets no voice_leave from here.
            if (session != null) tearDown(session, sendLeave = closedHere)
            // Own leave and logout: a notice of an earlier call is no longer
            // relevant. After the teardown, so that a removal tearing down the
            // same session right now has either set its notice by this point
            // or finds the session torn down and sets none.
            _endNotice.value = null
        }
    }

    /**
     * The teardown of [session], once per session, after its owner is gone:
     * clears the channel, sends `voice_leave` if [sendLeave], closes the peer
     * connections, releases audio focus and the foreground service, clears
     * the state ([cleanUp]) and publishes [notice].
     *
     * `voice_leave` goes only to a peer the session's `voice_join` created on
     * a socket that is still there: a join that was held never reached the
     * server, and a peer whose socket is gone is gone with it.
     */
    private suspend fun tearDown(
        session: Session,
        sendLeave: Boolean,
        notice: ((Session) -> VoiceEndNotice)? = null,
    ) {
        leaveMutex.withLock {
            if (session.tornDown) return@withLock
            session.tornDown = true
            // Before the clean-up resets what the notice reads (the mute state).
            val endNotice = notice?.invoke(session)
            // The channel goes before the call service is stopped ([cleanUp]):
            // from here on the service posts no notification any more.
            _currentChannelId.value = null
            if (sendLeave) {
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
            if (endNotice != null) _endNotice.value = endNotice
            logger.info("Voice session ended: ${session.channelId}")
        }
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
        logger.info("Set layer preference for stream $streamId: $layer")
    }

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
        override suspend fun join(socket: Long): Boolean {
            val job = currentCoroutineContext()[Job]
            if (!sendJoinOn(session, socket, job)) {
                if (!isLive(session)) throw SessionEnded()
                return false
            }

            // This adds the mic track, creates an SDP offer, and fires
            // onPublisherOffer when ready — which sends VoicePublisherOffer.
            if (!webRtcManager.createPublisherOffer(guardFor(session, job))) throw SessionEnded()

            audioRouteManager.requestAudioFocus()

            session.firstJoin.complete(Unit)
            return true
        }

        /**
         * Starts the call service (the microphone foreground service) as soon
         * as the set-up stands, before the join waits for a socket: right
         * after the user's tap, while the app is in the foreground. Android
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
            try {
                VoiceCallService.start(context, channelId, channelId)
            } catch (e: IllegalStateException) {
                // ForegroundServiceStartNotAllowedException (API 31+) is one.
                throw VoiceServiceStartException(e)
            } catch (e: SecurityException) {
                throw VoiceServiceStartException(e)
            }
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
        override suspend fun rejoin(socket: Long): VoiceSignalling.Attempt {
            val job = currentCoroutineContext()[Job]
            val guard = guardFor(session, job)
            if (!isLive(session)) throw SessionEnded()
            logger.info("Rejoining voice channel: $channelId")
            val own = RunningAttempt(webRtcManager.closePeerConnections())
            attempt = own
            var connected = false
            try {
                if (!webRtcManager.createSubscriberPeerConnection(guard)) return VoiceSignalling.Attempt.Failed
                if (!sendJoinOn(session, socket, job)) return VoiceSignalling.Attempt.SocketGone
                if (!webRtcManager.createPublisherOffer(guard)) return VoiceSignalling.Attempt.Failed
                // Already connected before we started waiting?
                webRtcManager.publisherLink.value.let { link ->
                    if (link.connected && link.generation == own.generation) {
                        own.result.complete(AttemptResult.Connected)
                    }
                }
                connected = own.result.await() == AttemptResult.Connected
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

        /**
         * All attempts failed, or no authenticated socket carried the
         * session's `voice_join` for 60 s: ends the
         * call locally (clears state, stops the foreground service) and offers
         * "Rejoin" through [endNotice]. The owner keeps the "reconnecting"
         * state until this returns.
         */
        override suspend fun giveUp() {
            if (!closeSession(session)) return
            // The session is ended now: finish its teardown whatever happens to us.
            withContext(NonCancellable) {
                tearDown(session, sendLeave = true, notice = ::rejoinFailedNotice)
            }
            logger.warning("Voice session gave up: $channelId")
        }
    }

    /** "Couldn't reconnect" with "Rejoin", which restores the user's last mute choice (C32). */
    private fun rejoinFailedNotice(session: Session): VoiceEndNotice =
        VoiceEndNotice.RejoinFailed(session.channelId, _isMuted.value)

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
        if (!closeSession(session)) return
        session.owner.cancel()
        // Own job: the teardown cancels the collector this event arrived on.
        scope.launch {
            // Wait until the owner (a running join or rejoin attempt) is gone.
            session.owner.join()
            tearDown(session, sendLeave = false) { VoiceEndNotice.Removed(it.channelId, reason) }
            logger.info("Removed from voice channel ${session.channelId} by the server: $reason")
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
        // Stop event collection
        eventCollectionJob?.cancel()
        eventCollectionJob = null
        serviceEventJob?.cancel()
        serviceEventJob = null
        iceConnectedJob?.cancel()
        iceConnectedJob = null

        // Close both PeerConnections
        webRtcManager.closePeerConnections()

        // Clear all dual-PC callbacks
        webRtcManager.onPublisherOffer = null
        webRtcManager.onPublisherIceCandidate = null
        webRtcManager.onSubscriberAnswer = null
        webRtcManager.onSubscriberIceCandidate = null
        webRtcManager.onError = null
        webRtcManager.onIceTrouble = null
        attempt?.result?.complete(AttemptResult.Failed)

        // Abandon audio focus
        audioRouteManager.abandonAudioFocus()

        // Stop foreground service
        VoiceCallService.stop(context)

        // Clear state
        _currentChannelId.value = null
        _participants.value = emptyList()
        _screenShares.value = emptyList()
        _layerPreferences.value = emptyMap()
        _isConnected.value = false
        // Nothing is captured any more: both layers say "not muted" (the next
        // join sets both again).
        _isMuted.value = false
        applyMuteToMicrophone()
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
                        logger.info("Auto-requested screen share ${event.streamId} at layer $defaultLayer")
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
