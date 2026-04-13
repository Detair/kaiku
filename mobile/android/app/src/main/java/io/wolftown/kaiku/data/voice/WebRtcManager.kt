package io.wolftown.kaiku.data.voice

import android.content.Context
import dagger.hilt.android.qualifiers.ApplicationContext
import io.wolftown.kaiku.data.api.IceServer
import io.wolftown.kaiku.data.api.VoiceApi
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import org.webrtc.AudioSource
import org.webrtc.AudioTrack
import org.webrtc.DataChannel
import org.webrtc.EglBase
import org.webrtc.IceCandidate
import org.webrtc.MediaConstraints
import org.webrtc.MediaStream
import org.webrtc.MediaStreamTrack
import org.webrtc.PeerConnection
import org.webrtc.PeerConnectionFactory
import org.webrtc.RtpTransceiver
import org.webrtc.SdpObserver
import org.webrtc.SessionDescription
import org.webrtc.VideoTrack
import org.webrtc.audio.JavaAudioDeviceModule
import java.util.logging.Level
import java.util.logging.Logger
import javax.inject.Inject
import javax.inject.Singleton

/**
 * Manages the WebRTC PeerConnection for voice chat.
 *
 * Uses `stream-webrtc-android` (package: `io.getstream.webrtc.android`)
 * which re-exports Google WebRTC classes under the `org.webrtc` package.
 *
 * Signaling flow:
 * 1. Server sends SDP offer via WebSocket (`VoiceOffer`)
 * 2. [handleOffer] sets the remote description and creates an SDP answer
 * 3. The answer is delivered via [onLocalDescription] callback
 * 4. ICE candidates are exchanged bidirectionally
 *
 * Testable logic (ICE serialization, mute state) is kept in pure
 * data classes ([IceCandidateData]) and simple boolean fields so that
 * JVM unit tests can validate them without the Android WebRTC runtime.
 */
@Singleton
class WebRtcManager @Inject constructor(
    @ApplicationContext private val context: Context,
    private val voiceApi: VoiceApi
) {
    companion object {
        private val logger = Logger.getLogger("WebRtcManager")
        private const val LOCAL_AUDIO_TRACK_ID = "kaiku-local-audio"
    }

    // -- State ----------------------------------------------------------------

    private val initMutex = Mutex()
    private var factory: PeerConnectionFactory? = null
    private var peerConnection: PeerConnection? = null
    private var audioSource: AudioSource? = null
    private var audioDeviceModule: org.webrtc.audio.AudioDeviceModule? = null
    private var remoteDescriptionSet = false
    private val pendingCandidates = mutableListOf<String>()  // buffered JSON strings

    /** Shared EGL context for video rendering (SurfaceViewRenderer). */
    val eglBase: EglBase = EglBase.create()

    /** The local microphone audio track, null until [createPeerConnection] is called. */
    var localAudioTrack: AudioTrack? = null
        private set

    private val _remoteAudioTracks = MutableStateFlow<Map<String, AudioTrack>>(emptyMap())
    /** Remote audio tracks keyed by track ID. */
    val remoteAudioTracks: StateFlow<Map<String, AudioTrack>> = _remoteAudioTracks.asStateFlow()

    private val _remoteVideoTracks = MutableStateFlow<Map<String, VideoTrack>>(emptyMap())
    /** Remote video tracks (screen shares) keyed by track ID. */
    val remoteVideoTracks: StateFlow<Map<String, VideoTrack>> = _remoteVideoTracks.asStateFlow()

    /** Whether the local audio track is muted. */
    var isMuted: Boolean = false
        private set

    // -- Callbacks ------------------------------------------------------------

    /** Called when an SDP answer has been created and is ready to send. */
    @Volatile var onLocalDescription: ((String) -> Unit)? = null

    /** Called when a new local ICE candidate is available (JSON string). */
    @Volatile var onIceCandidate: ((String) -> Unit)? = null

    /** Called when a remote track is received. */
    @Volatile var onTrackAdded: ((MediaStreamTrack) -> Unit)? = null

    /** Called when an SDP or ICE error occurs that prevents voice connection. */
    @Volatile var onError: ((String) -> Unit)? = null

    // -- Lifecycle ------------------------------------------------------------

    /**
     * Initializes the [PeerConnectionFactory].
     *
     * Must be called once before [createPeerConnection]. Safe to call multiple
     * times — subsequent calls are no-ops if the factory already exists.
     */
    suspend fun initialize() = initMutex.withLock {
        if (factory != null) return

        PeerConnectionFactory.initialize(
            PeerConnectionFactory.InitializationOptions.builder(context)
                .setEnableInternalTracer(false)
                .createInitializationOptions()
        )

        audioDeviceModule = JavaAudioDeviceModule.builder(context)
            .createAudioDeviceModule()

        factory = PeerConnectionFactory.builder()
            .setAudioDeviceModule(audioDeviceModule)
            .createPeerConnectionFactory()

        logger.info("PeerConnectionFactory initialized")
    }

    /**
     * Creates a new PeerConnection, fetching ICE server configuration from
     * the server API.
     *
     * Also creates the local audio track and adds it to the connection.
     * Call [initialize] first.
     */
    suspend fun createPeerConnection() {
        val pcFactory = factory ?: throw IllegalStateException(
            "PeerConnectionFactory not initialized. Call initialize() first."
        )

        // Fetch ICE servers from the Kaiku server
        val iceConfig = try {
            voiceApi.getIceServers()
        } catch (e: kotlin.coroutines.cancellation.CancellationException) {
            throw e
        } catch (e: Exception) {
            logger.log(Level.WARNING, "Failed to fetch ICE servers", e)
            onError?.invoke("Failed to fetch ICE servers: ${e.message}")
            throw e
        }

        val rtcIceServers = iceConfig.iceServers.map { server ->
            server.toRtcIceServer()
        }

        val rtcConfig = PeerConnection.RTCConfiguration(rtcIceServers).apply {
            sdpSemantics = PeerConnection.SdpSemantics.UNIFIED_PLAN
            continualGatheringPolicy =
                PeerConnection.ContinualGatheringPolicy.GATHER_CONTINUALLY
        }

        peerConnection = pcFactory.createPeerConnection(rtcConfig, createPeerConnectionObserver())
            ?: throw IllegalStateException("Failed to create PeerConnection")

        // Create and add local audio track
        audioSource = pcFactory.createAudioSource(MediaConstraints())
        localAudioTrack = pcFactory.createAudioTrack(LOCAL_AUDIO_TRACK_ID, audioSource).also {
            it.setEnabled(!isMuted)
            peerConnection?.addTrack(it)
        }

        logger.info("PeerConnection created with ${rtcIceServers.size} ICE servers")
    }

    /** Closes the peer connection and releases audio resources. */
    fun closePeerConnection() {
        localAudioTrack?.dispose()
        localAudioTrack = null
        audioSource?.dispose()
        audioSource = null
        peerConnection?.close()
        peerConnection?.dispose()
        peerConnection = null
        remoteDescriptionSet = false
        pendingCandidates.clear()

        _remoteAudioTracks.value = emptyMap()
        _remoteVideoTracks.value = emptyMap()

        logger.info("PeerConnection closed")
    }

    /** Disposes of the factory and all resources. Call on application shutdown. */
    fun dispose() {
        closePeerConnection()
        factory?.dispose()
        factory = null
        audioDeviceModule?.release()
        audioDeviceModule = null
        try {
            eglBase.release()
        } catch (_: IllegalStateException) {
            // EglBase may already be released
        } catch (e: Exception) {
            logger.log(Level.WARNING, "Unexpected error releasing EglBase", e)
        }
        logger.info("WebRtcManager disposed")
    }

    // -- Signaling ------------------------------------------------------------

    /**
     * Handles an SDP offer from the server.
     *
     * Sets the remote description to the offer, then creates and sets a local
     * SDP answer. When the answer is ready, [onLocalDescription] is invoked.
     */
    fun handleOffer(sdp: String) {
        val pc = peerConnection ?: run {
            logger.warning("handleOffer called but PeerConnection is null")
            onError?.invoke("Voice connection error: not initialized")
            return
        }

        remoteDescriptionSet = false
        val offer = SessionDescription(SessionDescription.Type.OFFER, sdp)

        pc.setRemoteDescription(object : SdpObserverAdapter("setRemoteDescription", onError) {
            override fun onSetSuccess() {
                logger.info("Remote description set successfully")
                remoteDescriptionSet = true
                val candidates = pendingCandidates.toList()
                pendingCandidates.clear()
                candidates.forEach { addIceCandidate(it) }
                pc.createAnswer(object : SdpObserverAdapter("createAnswer", onError) {
                    override fun onCreateSuccess(desc: SessionDescription) {
                        pc.setLocalDescription(object : SdpObserverAdapter("setLocalDescription", onError) {
                            override fun onSetSuccess() {
                                super.onSetSuccess()
                                onLocalDescription?.invoke(desc.description)
                                logger.info("SDP answer created and set")
                            }
                        }, desc)
                    }
                }, MediaConstraints())
            }
        }, offer)
    }

    /** Returns the current local SDP description, or null if none is set. */
    fun getLocalDescription(): String? =
        peerConnection?.localDescription?.description

    /**
     * Adds a remote ICE candidate received from the server via WebSocket.
     *
     * @param candidateJson JSON string in the format:
     *   `{"candidate":"...","sdpMLineIndex":0,"sdpMid":"..."}`
     */
    fun addIceCandidate(candidateJson: String) {
        if (!remoteDescriptionSet) {
            pendingCandidates.add(candidateJson)
            return
        }
        val pc = peerConnection ?: run {
            logger.warning("addIceCandidate called but PeerConnection is null")
            onError?.invoke("Voice connection error: not initialized")
            return
        }

        try {
            val data = IceCandidateData.fromJson(candidateJson)
            val candidate = IceCandidate(data.sdpMid, data.sdpMLineIndex, data.candidate)
            pc.addIceCandidate(candidate)
        } catch (e: Exception) {
            logger.log(Level.WARNING, "Failed to parse ICE candidate: $candidateJson", e)
            onError?.invoke("Failed to process ICE candidate: ${e.message}")
        }
    }

    // -- Video track management -----------------------------------------------

    /**
     * Removes a remote video track by its track ID.
     *
     * Called when a screen share stops — the track is removed from
     * [remoteVideoTracks] so the UI can clean up the renderer.
     */
    fun removeVideoTrack(trackId: String) {
        val updated = _remoteVideoTracks.value.toMutableMap()
        val removed = updated.remove(trackId)
        if (removed != null) {
            _remoteVideoTracks.value = updated
            logger.info("Remote video track removed: $trackId")
        }
    }

    // -- Audio control --------------------------------------------------------

    /**
     * Mutes or unmutes the local audio track.
     *
     * When muted, the audio track is disabled (no audio is sent to peers).
     */
    fun setMuted(muted: Boolean) {
        isMuted = muted
        localAudioTrack?.setEnabled(!muted)
    }

    /**
     * Enables or disables local audio.
     *
     * This is the inverse of [setMuted]: `setAudioEnabled(true)` is equivalent
     * to `setMuted(false)`.
     */
    fun setAudioEnabled(enabled: Boolean) {
        setMuted(!enabled)
    }

    // -- PeerConnection.Observer ----------------------------------------------

    private fun createPeerConnectionObserver() = object : PeerConnection.Observer {
        override fun onIceCandidate(candidate: IceCandidate) {
            val data = IceCandidateData(
                candidate = candidate.sdp,
                sdpMLineIndex = candidate.sdpMLineIndex,
                sdpMid = candidate.sdpMid
            )
            onIceCandidate?.invoke(data.toJson())
        }

        override fun onTrack(transceiver: RtpTransceiver) {
            val track = transceiver.receiver.track() ?: return
            // Build a track key from the mid (maps to stream_id in SDP)
            val mid = transceiver.mid ?: track.id()
            when (track) {
                is AudioTrack -> {
                    val updated = _remoteAudioTracks.value.toMutableMap()
                    updated[track.id()] = track
                    _remoteAudioTracks.value = updated
                    logger.info("Remote audio track added: ${track.id()} (mid=$mid)")
                }
                is VideoTrack -> {
                    val updated = _remoteVideoTracks.value.toMutableMap()
                    updated[track.id()] = track
                    _remoteVideoTracks.value = updated
                    logger.info("Remote video track added: ${track.id()} (mid=$mid)")
                }
            }
            onTrackAdded?.invoke(track)
        }

        override fun onSignalingChange(state: PeerConnection.SignalingState?) {
            logger.info("Signaling state: $state")
        }

        override fun onIceConnectionChange(state: PeerConnection.IceConnectionState?) {
            logger.info("ICE connection state: $state")
            when (state) {
                PeerConnection.IceConnectionState.FAILED ->
                    onError?.invoke("Voice connection failed (ICE)")
                PeerConnection.IceConnectionState.DISCONNECTED ->
                    logger.warning("ICE connection disconnected, may recover")
                else -> {}
            }
        }

        override fun onIceConnectionReceivingChange(receiving: Boolean) {
            logger.info("ICE connection receiving: $receiving")
        }

        override fun onIceGatheringChange(state: PeerConnection.IceGatheringState?) {
            logger.info("ICE gathering state: $state")
        }

        override fun onIceCandidatesRemoved(candidates: Array<out IceCandidate>?) {
            logger.info("ICE candidates removed: ${candidates?.size ?: 0}")
        }

        override fun onAddStream(stream: MediaStream?) {
            // Deprecated, using onTrack instead
        }

        override fun onRemoveStream(stream: MediaStream?) {
            // Deprecated
        }

        override fun onDataChannel(channel: DataChannel?) {
            // Not used for voice
        }

        override fun onRenegotiationNeeded() {
            logger.info("Renegotiation needed")
        }
    }

    // -- Helpers --------------------------------------------------------------

    /**
     * Converts a Kaiku API [IceServer] to the WebRTC [PeerConnection.IceServer].
     */
    private fun IceServer.toRtcIceServer(): PeerConnection.IceServer {
        val builder = PeerConnection.IceServer.builder(urls)
        username?.let { builder.setUsername(it) }
        credential?.let { builder.setPassword(it) }
        return builder.createIceServer()
    }
}

/**
 * Base [SdpObserver] adapter that logs failures and propagates them via [onError].
 *
 * Override the specific success callback you need.
 */
private open class SdpObserverAdapter(
    private val label: String,
    private val onError: ((String) -> Unit)? = null
) : SdpObserver {
    private val logger = Logger.getLogger("SdpObserver")

    override fun onCreateSuccess(desc: SessionDescription) {}

    override fun onSetSuccess() {}

    override fun onCreateFailure(error: String?) {
        logger.warning("$label onCreateFailure: $error")
        onError?.invoke("SDP $label failed: $error")
    }

    override fun onSetFailure(error: String?) {
        logger.warning("$label onSetFailure: $error")
        onError?.invoke("SDP $label failed: $error")
    }
}
