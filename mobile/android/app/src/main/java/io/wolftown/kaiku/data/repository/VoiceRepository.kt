package io.wolftown.kaiku.data.repository

import android.content.Context
import dagger.hilt.android.qualifiers.ApplicationContext
import android.net.ConnectivityManager
import android.net.NetworkCapabilities
import io.wolftown.kaiku.R
import io.wolftown.kaiku.data.voice.AudioRouteManager
import io.wolftown.kaiku.data.voice.MicPermissionChecker
import io.wolftown.kaiku.data.voice.MissingMicPermissionException
import io.wolftown.kaiku.data.voice.VoiceServiceEvent
import io.wolftown.kaiku.data.voice.VoiceServiceEvents
import io.wolftown.kaiku.data.voice.WebRtcManager
import io.wolftown.kaiku.data.ws.ClientEvent
import io.wolftown.kaiku.data.ws.KaikuWebSocket
import io.wolftown.kaiku.data.ws.ScreenShareInfo
import io.wolftown.kaiku.data.ws.ServerEvent
import io.wolftown.kaiku.data.ws.VoiceParticipant
import io.wolftown.kaiku.di.VoiceCoroutineScope
import io.wolftown.kaiku.service.VoiceCallService
import kotlinx.coroutines.CancellationException
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
import kotlinx.coroutines.launch
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
    suspend fun joinChannel(channelId: String) {
        // Fail fast: the microphone foreground service and capture cannot start
        // without RECORD_AUDIO, so don't touch WebRTC at all.
        if (!micPermissionChecker.isGranted()) {
            throw MissingMicPermissionException()
        }

        // If already in a channel, leave first
        if (_currentChannelId.value != null) {
            leaveChannel()
        }

        try {
            _currentChannelId.value = channelId

            // 1. Initialize WebRTC
            webRtcManager.initialize()

            // 2. Create subscriber PeerConnection (will receive the server's offer)
            webRtcManager.createSubscriberPeerConnection()

            // 3. Wire up signaling callbacks (both PCs)
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
                _error.value = R.string.voice_connection_error
                logger.warning("WebRTC error: $errorMsg")
            }

            // Start collecting WebSocket events
            startCollectingEvents(channelId)

            // Collect dual-PC ICE-connected flow to gate _isConnected.
            iceConnectedJob = scope.launch {
                webRtcManager.voiceIceConnected.collect { bothConnected ->
                    _isConnected.value = bothConnected
                }
            }

            // 4. Send VoiceJoin
            webSocket.send(ClientEvent.VoiceJoin(channelId))

            // 5. Create the publisher offer (Android-initiated).
            // This adds the mic track, creates an SDP offer, and fires
            // onPublisherOffer when ready — which sends VoicePublisherOffer.
            webRtcManager.createPublisherOffer()

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
            cleanUp()
            throw e
        } catch (e: Exception) {
            logger.log(Level.SEVERE, "Failed to join voice channel: $channelId", e)
            cleanUp()
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
    fun startJoin(channelId: String) {
        synchronized(joinLock) {
            if (joiningChannelId == channelId && joinJob?.isActive == true) return
            val previous = joinJob
            joiningChannelId = channelId
            joinJob = scope.launch {
                previous?.cancelAndJoin()
                try {
                    joinChannel(channelId)
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
    private suspend fun cancelJoin() {
        val self = currentCoroutineContext()[Job]
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
     * 1. Cancel a running join (its clean-up runs first)
     * 2. Clear the channel at once, so the overlay disappears without a flash
     * 3. Send VoiceLeave via WebSocket
     * 4. Close the peer connections, abandon audio focus, stop the foreground
     *    service, clear state ([cleanUp])
     */
    suspend fun leaveChannel() {
        val joiningOrJoined = _currentChannelId.value
        cancelJoin()
        leaveMutex.withLock {
            val channelId = _currentChannelId.value ?: joiningOrJoined ?: return@withLock
            _currentChannelId.value = null

            try {
                webSocket.send(ClientEvent.VoiceLeave(channelId))
            } catch (e: Exception) {
                logger.log(Level.WARNING, "Failed to send VoiceLeave", e)
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

        if (newMuted) {
            webSocket.send(ClientEvent.VoiceMute(channelId))
        } else {
            webSocket.send(ClientEvent.VoiceUnmute(channelId))
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

    private fun handleServerEvent(channelId: String, event: ServerEvent) {
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

            is ServerEvent.VoiceError -> {
                _error.value = R.string.voice_connection_error
                logger.warning("Voice error from server: code=${event.code} message=${event.message}")
            }

            is ServerEvent.Error -> {
                logger.warning("Server error in voice context: code=${event.code} message=${event.message}")
            }

            else -> { /* Ignored — other events handled elsewhere */ }
        }
    }
}
