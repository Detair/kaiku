package io.wolftown.kaiku.data.repository

import android.content.Context
import dagger.hilt.android.qualifiers.ApplicationContext
import android.net.ConnectivityManager
import android.net.NetworkCapabilities
import io.wolftown.kaiku.data.voice.AudioRouteManager
import io.wolftown.kaiku.data.voice.VoiceServiceEvent
import io.wolftown.kaiku.data.voice.VoiceServiceEvents
import io.wolftown.kaiku.data.voice.WebRtcManager
import io.wolftown.kaiku.data.ws.ClientEvent
import io.wolftown.kaiku.data.ws.KaikuWebSocket
import io.wolftown.kaiku.data.ws.ScreenShareInfo
import io.wolftown.kaiku.data.ws.ServerEvent
import io.wolftown.kaiku.data.ws.VoiceParticipant
import io.wolftown.kaiku.service.VoiceCallService
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
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
    @ApplicationContext private val context: Context
) : Closeable {
    companion object {
        private val logger = Logger.getLogger("VoiceRepository")
    }

    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.IO)
    private val leaveMutex = Mutex()

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

    private val _error = MutableStateFlow<String?>(null)
    /** Error message from voice subsystem (SDP failure, ICE failure, etc.). */
    val error: StateFlow<String?> = _error.asStateFlow()

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
        // If already in a channel, leave first
        if (_currentChannelId.value != null) {
            leaveChannel()
        }

        try {
            _currentChannelId.value = channelId

            // 1. Initialize WebRTC
            webRtcManager.initialize()

            // 2. Create PeerConnection
            webRtcManager.createPeerConnection()

            // 3. Wire up signaling callbacks
            webRtcManager.onLocalDescription = { sdp ->
                webSocket.send(ClientEvent.VoiceAnswer(channelId, sdp))
            }
            webRtcManager.onIceCandidate = { candidateJson ->
                webSocket.send(ClientEvent.VoiceIceCandidate(channelId, candidateJson))
            }
            webRtcManager.onError = { errorMsg ->
                _error.value = errorMsg
                logger.warning("WebRTC error: $errorMsg")
            }

            // Start collecting WebSocket events
            startCollectingEvents(channelId)

            // 4. Send VoiceJoin
            webSocket.send(ClientEvent.VoiceJoin(channelId))

            // 5. Request audio focus
            audioRouteManager.requestAudioFocus()

            // 6. Start foreground service and collect notification action events
            VoiceCallService.start(context, channelId, channelId)
            serviceEventJob = scope.launch {
                voiceServiceEvents.events.collect { event ->
                    when (event) {
                        VoiceServiceEvent.MuteToggle -> toggleMute()
                        VoiceServiceEvent.Disconnect -> leaveChannel()
                    }
                }
            }

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
     * Leaves the current voice channel.
     *
     * 1. Send VoiceLeave via WebSocket
     * 2. Close PeerConnection
     * 3. Abandon audio focus
     * 4. Stop foreground service
     * 5. Clear state
     */
    suspend fun leaveChannel() {
        leaveMutex.withLock {
            val channelId = _currentChannelId.value ?: return@withLock

            try {
                // 1. Send VoiceLeave
                webSocket.send(ClientEvent.VoiceLeave(channelId))
            } catch (e: Exception) {
                logger.log(Level.WARNING, "Failed to send VoiceLeave", e)
            }

            cleanUp()
            logger.info("Left voice channel: $channelId")
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

        // Close PeerConnection
        webRtcManager.closePeerConnection()
        webRtcManager.onLocalDescription = null
        webRtcManager.onIceCandidate = null
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
        eventCollectionJob = scope.launch {
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
                    _isConnected.value = true
                }
            }

            is ServerEvent.VoiceOffer -> {
                if (event.channelId == channelId) {
                    webRtcManager.handleOffer(event.sdp)
                }
            }

            is ServerEvent.VoiceIceCandidate -> {
                if (event.channelId == channelId) {
                    webRtcManager.addIceCandidate(event.candidate)
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
                _error.value = "Voice error: ${event.message}"
                logger.warning("Voice error from server: code=${event.code} message=${event.message}")
            }

            is ServerEvent.Error -> {
                logger.warning("Server error in voice context: code=${event.code} message=${event.message}")
            }

            else -> { /* Ignored — other events handled elsewhere */ }
        }
    }
}
