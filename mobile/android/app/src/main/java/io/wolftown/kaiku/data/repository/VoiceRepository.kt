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
import io.wolftown.kaiku.data.voice.VoiceRecovery
import io.wolftown.kaiku.data.voice.VoiceServiceEvent
import io.wolftown.kaiku.data.voice.VoiceServiceEvents
import io.wolftown.kaiku.data.voice.WebRtcManager
import io.wolftown.kaiku.data.ws.ClientEvent
import io.wolftown.kaiku.data.ws.ConnectionState
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
    private val leaveMutex = Mutex()

    /** Result of one rejoin attempt, completed by ICE, WebRTC and `voice_error` events. */
    private enum class AttemptResult { Connected, Failed }

    // -- Session liveness -----------------------------------------------------
    //
    // Invariant (C31/C32 amendments): once a session ended (own leave,
    // `voice_removed`, give-up, logout), no `voice_join` is sent and no peer
    // connection is installed for it.
    //  - [liveChannel] is cleared first thing when a session ends.
    //  - Every `voice_join` (and the mute that follows it) goes through
    //    [sendIfLive], which checks and sends under [sessionLock], so it can
    //    never slip in after the end.
    //  - Every peer-connection creation gets [guardFor] as its guard:
    //    WebRtcManager installs the new connection inside [sessionLock] only
    //    while the session is live (one step with the check), and closes it
    //    otherwise.
    //  - The teardown waits for the join job and the rejoin loop to finish
    //    ([cancelJoin], [VoiceRecovery.endAndJoin]) before it closes the
    //    connections, so anything installed just before the end is closed too.

    private val sessionLock = Any()
    private var liveChannel: String? = null // guarded by [sessionLock]

    /**
     * The authenticated socket the session's last `voice_join` went out on, or
     * null while none went out (the join is held until the socket's Ready).
     * Guarded by [sessionLock].
     */
    private var joinedOnSocket: Long? = null

    /** The [joinChannel] that is still setting up the session (its join may be held for Ready). */
    @Volatile
    private var initialJoin: Job? = null

    /** Thrown inside a join when its session ended meanwhile. */
    private class SessionEnded : CancellationException("voice session ended")

    private fun openSession(channelId: String) = synchronized(sessionLock) {
        liveChannel = channelId
        joinedOnSocket = null
    }

    /** Ends the live session ([channelId] only, or any when null); returns whether it was live. */
    private fun closeSession(channelId: String? = null): Boolean = synchronized(sessionLock) {
        val live = liveChannel ?: return false
        if (channelId != null && live != channelId) return false
        liveChannel = null
        true
    }

    private fun isLive(channelId: String, job: Job?): Boolean =
        job?.isActive != false && synchronized(sessionLock) { liveChannel == channelId }

    /** Installs a new peer connection only while [channelId] is live for [job], atomically. */
    private fun guardFor(channelId: String, job: Job?) = SessionGuard { install ->
        synchronized(sessionLock) {
            if (liveChannel != channelId || job?.isActive == false) {
                false
            } else {
                install()
                true
            }
        }
    }

    /** Sends [event] only while [channelId] is live for [job]; null if it is not. */
    private fun sendIfLive(channelId: String, job: Job?, event: ClientEvent): Boolean? =
        synchronized(sessionLock) {
            if (liveChannel != channelId || job?.isActive == false) null else webSocket.send(event)
        }

    /**
     * Sends `voice_join` (then the current mute state) once the
     * socket is authenticated, and records on which socket it went out.
     * Voice signalling waits for Ready: while the socket is Connecting or
     * absent the join is held, so it goes out exactly once, on the
     * authenticated socket. Returns false if the session ended meanwhile (then
     * nothing was sent). If no Ready comes, the offline rules end the session.
     */
    private suspend fun sendJoinWhenAuthenticated(channelId: String, job: Job?): Boolean {
        while (true) {
            webSocket.connectionState.first { it == ConnectionState.Connected }
            val sentOn = synchronized(sessionLock) {
                if (liveChannel != channelId || job?.isActive == false) return false
                webSocket.sendIfAuthenticated(ClientEvent.VoiceJoin(channelId))?.also { joinedOnSocket = it }
            }
            if (sentOn != null) {
                // The new server-side peer starts unmuted; the local track stays disabled.
                if (_isMuted.value) sendIfLive(channelId, job, ClientEvent.VoiceMute(channelId))
                return true
            }
            // Not sent (the socket went away meanwhile): wait for the next Ready.
            webSocket.connectionState.first { it != ConnectionState.Connected }
        }
    }

    /** Whether this session's `voice_join` went out at all (nothing to leave otherwise). */
    private fun joinWentOut(): Boolean = synchronized(sessionLock) { joinedOnSocket != null }

    /**
     * Whether the session's `voice_join` went out on the socket that is
     * authenticated now. Then the server holds our peer on it, and its Ready
     * (delivered late) is no reason to rejoin.
     */
    private fun joinedOnCurrentSocket(): Boolean = synchronized(sessionLock) {
        val joined = joinedOnSocket
        liveChannel != null && joined != null && joined == webSocket.authenticatedSocket
    }

    /**
     * One rejoin attempt (C32). A success only counts for the publisher of
     * [generation], the attempt's own connections.
     */
    private class Attempt(val generation: Long) {
        val result = CompletableDeferred<AttemptResult>()
    }

    /** The running rejoin attempt, if any. */
    @Volatile
    private var attempt: Attempt? = null

    private val recovery = VoiceRecovery(
        scope = scope,
        rejoin = ::rejoinOnce,
        onGaveUp = ::onRejoinGaveUp,
    )

    /**
     * Whether the call is connected, interrupted (ICE trouble or WebSocket down,
     * waiting) or being rejoined. Drives the "reconnecting" indication in the
     * voice screen, the overlay and the ongoing-call notification.
     */
    val linkState: StateFlow<VoiceLinkState> = recovery.state

    private val _endNotice = MutableStateFlow<VoiceEndNotice?>(null)
    /**
     * Why the last call ended without the user leaving (rejoin gave up), with
     * the "Rejoin" action. Cleared by [dismissEndNotice], [rejoin] and any new join.
     */
    val endNotice: StateFlow<VoiceEndNotice?> = _endNotice.asStateFlow()

    init {
        // Channel-independent: outlives single calls. A new socket's `Ready`
        // is handled with the other voice events ([handleServerEvent]).
        scope.launch(start = CoroutineStart.UNDISPATCHED) {
            webSocket.connectionState.collect { state ->
                if (state != ConnectionState.Connected) recovery.onSocketLost()
            }
        }
    }

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
     * Joins a voice channel.
     *
     * 1. Initialize WebRtcManager (create PeerConnectionFactory if needed)
     * 2. Create PeerConnection (fetches ICE servers)
     * 3. Wire up signaling callbacks
     * 4. Send VoiceJoin via WebSocket
     * 5. Request audio focus
     * 6. Start foreground service
     */
    suspend fun joinChannel(channelId: String, muted: Boolean = false) {
        // Fail fast: the microphone foreground service and capture cannot start
        // without RECORD_AUDIO, so don't touch WebRTC at all.
        if (!micPermissionChecker.isGranted()) {
            throw MissingMicPermissionException()
        }

        // If already in a channel, leave first
        if (_currentChannelId.value != null) {
            leaveChannel()
        }

        val job = currentCoroutineContext()[Job]
        val guard = guardFor(channelId, job)
        initialJoin = job
        try {
            openSession(channelId)
            _currentChannelId.value = channelId
            _endNotice.value = null
            // Both layers start from the same mute state (a Rejoin restores the user's last choice).
            webRtcManager.setMuted(muted)
            _isMuted.value = muted
            recovery.begin(channelId, socketUp = webSocket.connectionState.value == ConnectionState.Connected)

            // 1. Initialize WebRTC
            webRtcManager.initialize()

            // 2. Create subscriber PeerConnection (will receive the server's offer)
            if (!webRtcManager.createSubscriberPeerConnection(guard)) throw SessionEnded()

            // 3. Wire up signaling callbacks (both PCs)
            wirePeerCallbacks(channelId)

            // Start collecting WebSocket events
            startCollectingEvents(channelId)

            // The call is up once the publisher PC is connected; the subscriber
            // is not required (an empty channel never gets a subscriber offer).
            // Same criterion for the first join and for a rejoin attempt.
            iceConnectedJob = scope.launch {
                webRtcManager.publisherLink.collect { link ->
                    _isConnected.value = link.connected
                    if (link.connected) {
                        recovery.onIceConnected()
                        // Only the attempt's own publisher counts (identity check).
                        attempt?.takeIf { it.generation == link.generation }
                            ?.result?.complete(AttemptResult.Connected)
                    }
                }
            }

            // 4. Send VoiceJoin
            if (!sendJoinWhenAuthenticated(channelId, job)) throw SessionEnded()

            // 5. Create the publisher offer (Android-initiated).
            // This adds the mic track, creates an SDP offer, and fires
            // onPublisherOffer when ready — which sends VoicePublisherOffer.
            if (!webRtcManager.createPublisherOffer(guard)) throw SessionEnded()

            // 6. Request audio focus
            audioRouteManager.requestAudioFocus()

            // 7. Collect service events first (so StartFailed cannot be missed), then
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
                                // leaveChannel clears _error, so set it afterwards.
                                _error.value = R.string.voice_start_failed
                            }
                        }
                    }
                }
            }
            VoiceCallService.start(context, channelId, channelId)

            logger.info("Joined voice channel: $channelId")
        } catch (e: CancellationException) {
            endFailedJoin(channelId)
            throw e
        } catch (e: Exception) {
            logger.log(Level.SEVERE, "Failed to join voice channel: $channelId", e)
            endFailedJoin(channelId)
            throw e
        } finally {
            if (initialJoin === job) initialJoin = null
        }
    }

    /** A join that did not complete: its session ends here (a leave may already have ended it). */
    private fun endFailedJoin(channelId: String) {
        if (closeSession(channelId)) recovery.end()
        cleanUp()
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
                } catch (e: Exception) {
                    // joinChannel already logged and cleaned up (which clears the error).
                    _error.value = R.string.voice_join_failed
                }
            }
        }
    }

    /**
     * Cancels a running [startJoin] and waits until its clean-up finished, so
     * a leave never interleaves with a half-done join. A join that leaves a
     * previous channel itself (inside its own job) does not cancel itself.
     */
    private suspend fun cancelJoin(onlyChannel: String? = null) {
        val self = currentCoroutineContext()[Job]
        val job = synchronized(joinLock) {
            val running = joinJob
            if (running == null || running === self ||
                (onlyChannel != null && joiningChannelId != onlyChannel)
            ) {
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
     * 1. Cancel a running join (its clean-up runs first)
     * 2. Clear the channel at once, so the overlay disappears without a flash
     * 3. Send VoiceLeave via WebSocket
     * 4. Close the peer connections, abandon audio focus, stop the foreground
     *    service, clear state ([cleanUp])
     */
    suspend fun leaveChannel() {
        val joiningOrJoined = _currentChannelId.value
        // An own leave wins: no voice_join or new connection from here on, and
        // the teardown waits until a running rejoin attempt and join are gone.
        closeSession()
        recovery.endAndJoin()
        cancelJoin()
        // Own leave and logout: a notice of an earlier call is no longer relevant.
        _endNotice.value = null
        leaveMutex.withLock {
            val channelId = _currentChannelId.value ?: joiningOrJoined ?: return@withLock
            _currentChannelId.value = null

            // A join still held for Ready never reached the server: nothing to leave.
            if (joinWentOut()) {
                try {
                    webSocket.send(ClientEvent.VoiceLeave(channelId))
                } catch (e: Exception) {
                    logger.log(Level.WARNING, "Failed to send VoiceLeave", e)
                }
            }

            cleanUp()
            logger.info("Left voice channel: $channelId")
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
        val channelId = _currentChannelId.value ?: return
        val newMuted = !_isMuted.value

        webRtcManager.setMuted(newMuted)
        _isMuted.value = newMuted

        // Only to the peer our voice_join created on the socket that is up now.
        // While the join is held (or a rejoin is pending) the state goes out
        // right after the next voice_join instead.
        synchronized(sessionLock) {
            if (!joinedOnCurrentSocket()) return
            if (newMuted) {
                webSocket.send(ClientEvent.VoiceMute(channelId))
            } else {
                webSocket.send(ClientEvent.VoiceUnmute(channelId))
            }
        }
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

        webSocket.send(
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

    // -- Rejoin (C32) ---------------------------------------------------------

    /** Signaling callbacks of both peer connections for [channelId]. */
    private fun wirePeerCallbacks(channelId: String) {
        webRtcManager.onPublisherOffer = { sdp ->
            webSocket.send(ClientEvent.VoicePublisherOffer(channelId, sdp))
        }
        webRtcManager.onPublisherIceCandidate = { candidateJson ->
            webSocket.send(
                ClientEvent.VoiceIceCandidate(channelId, candidateJson, pcType = "publisher")
            )
        }
        webRtcManager.onSubscriberAnswer = { sdp ->
            webSocket.send(ClientEvent.VoiceSubscriberAnswer(channelId, sdp))
        }
        webRtcManager.onSubscriberIceCandidate = { candidateJson ->
            webSocket.send(
                ClientEvent.VoiceIceCandidate(channelId, candidateJson, pcType = "subscriber")
            )
        }
        webRtcManager.onError = { errorMsg ->
            logger.warning("WebRTC error: $errorMsg")
            reportVoiceError()
        }
        webRtcManager.onIceTrouble = { trouble, generation ->
            // Troubles of retired connections are dropped (identity check).
            if (generation == webRtcManager.currentGeneration) when (trouble) {
                // Indication only; ICE may recover by itself.
                IceTrouble.Disconnected -> recovery.onIceDisconnected()
                IceTrouble.Recovered -> recovery.onIceConnected()
                IceTrouble.Failed -> {
                    attempt?.takeIf { it.generation == generation }?.result?.complete(AttemptResult.Failed)
                    recovery.onIceFailed()
                }
            }
        }
    }

    /**
     * One rejoin attempt for [VoiceRecovery]: discards the old peer connections,
     * sends `voice_join` again, restores the mute state and waits until the
     * publisher connection is connected (the subscriber is not required). The
     * foreground service, audio focus and the voice state stay as they are.
     *
     * Whatever ends the attempt short of success (failure, timeout, socket
     * loss, session end) closes the connections it created before it returns.
     */
    private suspend fun rejoinOnce(channelId: String): Boolean {
        val job = currentCoroutineContext()[Job]
        val guard = guardFor(channelId, job)
        // A Ready during the first join: let the join finish its setup first,
        // so the attempt never closes connections the join is still creating.
        val join = initialJoin
        if (join != null && join !== job) join.join()
        if (!isLive(channelId, job)) return false
        logger.info("Rejoining voice channel: $channelId")
        val own = Attempt(webRtcManager.closePeerConnections())
        attempt = own
        var connected = false
        try {
            if (!webRtcManager.createSubscriberPeerConnection(guard)) return false
            if (!sendJoinWhenAuthenticated(channelId, job)) return false
            if (!webRtcManager.createPublisherOffer(guard)) return false
            // Already connected before we started waiting?
            webRtcManager.publisherLink.value.let { link ->
                if (link.connected && link.generation == own.generation) {
                    own.result.complete(AttemptResult.Connected)
                }
            }
            connected = own.result.await() == AttemptResult.Connected
            return connected
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            logger.log(Level.WARNING, "Voice rejoin attempt failed: ${e::class.simpleName}")
            return false
        } finally {
            if (attempt === own) attempt = null
            if (!connected) webRtcManager.closePeerConnections()
        }
    }

    /**
     * The rejoin gave up (all attempts failed, or no socket for 60 s): ends the
     * call locally (clears state, stops the foreground service) and offers
     * "Rejoin" through [endNotice]. Runs inside the recovery loop, which keeps
     * the "reconnecting" state until this returns.
     */
    private suspend fun onRejoinGaveUp(channelId: String) {
        if (!closeSession(channelId)) return
        // The session is ended now: finish its teardown even if a channel
        // switch or a leave cancels us, or channel A stays half torn down.
        withContext(NonCancellable) {
            val wasMuted = _isMuted.value
            // Only a join of this channel; a join to another channel is the
            // user's new choice and goes ahead after this teardown.
            cancelJoin(onlyChannel = channelId)
            leaveMutex.withLock {
                if (_currentChannelId.value != channelId) return@withLock
                _currentChannelId.value = null
                if (joinWentOut()) {
                    try {
                        webSocket.send(ClientEvent.VoiceLeave(channelId))
                    } catch (e: Exception) {
                        logger.log(Level.WARNING, "Failed to send VoiceLeave", e)
                    }
                }
                cleanUp()
                _endNotice.value = VoiceEndNotice.RejoinFailed(channelId, wasMuted)
                logger.warning("Voice rejoin gave up: $channelId")
            }
        }
    }

    /**
     * C31: the server removed us from the call. Ends it locally without a
     * `voice_leave` (the peer is gone), never rejoins, and says why.
     */
    private fun onRemovedByServer(channelId: String, reason: VoiceRemovedReason) {
        // At once, on the event's thread: no voice_join or new connection after this.
        if (!closeSession(channelId)) return
        val stopped = recovery.end()
        // Own job: the teardown cancels the collector this event arrived on.
        scope.launch {
            // Wait until a running rejoin attempt has finished cancelling.
            stopped.forEach { it.join() }
            cancelJoin()
            leaveMutex.withLock {
                if (_currentChannelId.value != channelId) return@withLock
                _currentChannelId.value = null
                cleanUp()
                _endNotice.value = VoiceEndNotice.Removed(channelId, reason)
                logger.info("Removed from voice channel $channelId by the server: $reason")
            }
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
        webRtcManager.setMuted(false)
        _isMuted.value = false
        _error.value = null
    }

    private fun startCollectingEvents(channelId: String) {
        eventCollectionJob?.cancel()
        // UNDISPATCHED: subscribe to the replay-0 SharedFlow before joinChannel
        // sends VoiceJoin, so a fast VoiceSubscriberOffer/PublisherAnswer cannot
        // be emitted while the collector is still waiting for an IO thread.
        eventCollectionJob = scope.launch(start = CoroutineStart.UNDISPATCHED) {
            webSocket.events.collect { event ->
                handleServerEvent(channelId, event)
            }
        }
    }

    /**
     * A socket is authenticated. It needs a rejoin unless the session's
     * `voice_join` already went out on this very socket (decided by that fact,
     * not by timing). Voice signalling waits for Ready, so a live session's
     * join always went out on an authenticated socket; a different socket
     * number means a real socket loss. If the first join has not sent its
     * `voice_join` yet (held for this Ready), decide once it has.
     */
    private fun onSocketReady() {
        val join = initialJoin?.takeIf { it.isActive }
        if (join == null) {
            recovery.onSocketReady(startRejoin = !joinedOnCurrentSocket())
            return
        }
        recovery.onSocketReady(startRejoin = false)
        scope.launch {
            join.join()
            if (!joinedOnCurrentSocket()) recovery.requestRejoin()
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
            recovery.state.value is VoiceLinkState.Rejoining -> { /* the loop is between attempts */ }
            else -> _error.value = R.string.voice_connection_error
        }
    }

    /**
     * Test hook: handles [event] synchronously, as the event collector of
     * [channelId] would, so a test can deliver it in the middle of a rejoin step.
     */
    @androidx.annotation.VisibleForTesting
    internal fun handleServerEventNow(channelId: String, event: ServerEvent) =
        handleServerEvent(channelId, event)

    private fun handleServerEvent(channelId: String, event: ServerEvent) {
        when (event) {
            // A new socket is authenticated; the server dropped our old peer when
            // the old socket closed. Text re-subscription runs on the same event
            // ([io.wolftown.kaiku.data.sync.ResyncCoordinator]) and the first
            // attempt follows 1 s later.
            is ServerEvent.Ready -> onSocketReady()

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
                        webSocket.send(
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
                    onRemovedByServer(channelId, event.removedReason)
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
