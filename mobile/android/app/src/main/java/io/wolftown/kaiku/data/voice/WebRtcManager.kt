package io.wolftown.kaiku.data.voice

import android.content.Context
import dagger.hilt.android.qualifiers.ApplicationContext
import io.wolftown.kaiku.data.api.IceServer
import io.wolftown.kaiku.data.api.VoiceApi
import io.wolftown.kaiku.di.VoiceCoroutineScope
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharingStarted
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.combine
import kotlinx.coroutines.flow.stateIn
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
 * Signaling flow (subscriber, server-initiated):
 * 1. Server sends SDP offer via WebSocket (`VoiceSubscriberOffer`)
 * 2. [handleSubscriberOffer] sets the remote description and creates an SDP answer
 * 3. The answer is delivered via [onSubscriberAnswer] callback
 * 4. ICE candidates are exchanged bidirectionally
 *
 * Signaling flow (publisher, Android-initiated):
 * 1. [createPublisherOffer] builds the publisher PC + mic track and creates an offer
 * 2. The offer is delivered via [onPublisherOffer] callback (sent to server)
 * 3. Server returns an SDP answer; [handlePublisherAnswer] applies it
 * 4. ICE candidates are exchanged bidirectionally with `pc_type = "publisher"`
 *
 * Testable logic (ICE serialization, mute state) is kept in pure
 * data classes ([IceCandidateData]) and simple boolean fields so that
 * JVM unit tests can validate them without the Android WebRTC runtime.
 */
@Singleton
class WebRtcManager @Inject constructor(
    @ApplicationContext private val context: Context,
    private val voiceApi: VoiceApi,
    private val eglBaseProvider: EglBaseProvider,
    @VoiceCoroutineScope private val voiceScope: CoroutineScope,
) {
    companion object {
        private val logger = Logger.getLogger("WebRtcManager")
        private const val LOCAL_AUDIO_TRACK_ID = "kaiku-local-audio"
        private const val MAX_PENDING_CANDIDATES = 100
    }

    // -- State ----------------------------------------------------------------

    private val initMutex = Mutex()
    private var factory: PeerConnectionFactory? = null
    /** Marked @Volatile so concurrent observers (WS event collector, WebRTC native callbacks)
     *  reliably see the null assignment in [closePeerConnections]. The check-then-use race
     *  in [addIceCandidate] / [handleSubscriberOffer] is tolerated by the surrounding try/catch. */
    @Volatile
    private var subscriberPc: PeerConnection? = null
    private var audioSource: AudioSource? = null
    private var audioDeviceModule: org.webrtc.audio.AudioDeviceModule? = null
    private var subscriberRemoteDescriptionSet = false
    private val subscriberPendingCandidates = mutableListOf<String>()  // buffered JSON strings

    // Publisher PC — uploads local mic to SFU
    @Volatile private var publisherPc: PeerConnection? = null
    // NOTE: publisherRemoteDescriptionSet and publisherPendingCandidates are
    // mutated from both the WebRTC signaling thread (via Observer callbacks) and
    // the IO dispatcher (via addIceCandidate from WS events). Phase 2 audit will
    // add @Volatile + synchronization for both PCs consistently.
    private var publisherRemoteDescriptionSet = false
    private val publisherPendingCandidates = mutableListOf<String>()

    /** Test-only accessor for the buffered subscriber ICE candidate count. */
    internal fun subscriberPendingCandidatesSize(): Int = subscriberPendingCandidates.size

    /** Test-only accessor for the buffered publisher ICE candidate count. */
    internal fun publisherPendingCandidatesSize(): Int = publisherPendingCandidates.size

    /** Test-only mutator for publisher ICE state (drives [voiceIceConnected] in unit tests). */
    internal fun setPublisherIceStateForTest(state: PeerConnection.IceConnectionState?) {
        publisherIceState.value = state
    }

    /** Test-only mutator for subscriber ICE state (drives [voiceIceConnected] in unit tests). */
    internal fun setSubscriberIceStateForTest(state: PeerConnection.IceConnectionState?) {
        subscriberIceState.value = state
    }

    /**
     * Lazy so tests that never touch video rendering never trigger EGL init,
     * and so production startup defers the native cost until first use.
     *
     * Exposed via the `eglBase` accessor below; dispose() uses `_eglBase.isInitialized()`
     * to avoid re-triggering init purely to release.
     */
    private val _eglBase: Lazy<EglBase> = lazy { eglBaseProvider.create() }

    /** Shared EGL context for video rendering (SurfaceViewRenderer). */
    val eglBase: EglBase get() = _eglBase.value

    /** The local microphone audio track, null until [createPublisherOffer] is called. */
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

    /** ICE connection state of the publisher PC (mic upload). */
    private val publisherIceState =
        MutableStateFlow<PeerConnection.IceConnectionState?>(null)

    /** ICE connection state of the subscriber PC (remote-track download). */
    private val subscriberIceState =
        MutableStateFlow<PeerConnection.IceConnectionState?>(null)

    /**
     * Emits true only when both the publisher and subscriber PCs reach
     * [PeerConnection.IceConnectionState.CONNECTED]. Emits false any time
     * either PC is in a different state (or null/disposed).
     *
     * Backed by an eagerly-collected [combine] flow scoped to an internal
     * IO scope so collectors always observe the latest combined value.
     */
    val voiceIceConnected: StateFlow<Boolean> =
        combine(publisherIceState, subscriberIceState) { p, s ->
            p == PeerConnection.IceConnectionState.CONNECTED &&
                s == PeerConnection.IceConnectionState.CONNECTED
        }.stateIn(voiceScope, SharingStarted.Eagerly, false)

    // -- Callbacks ------------------------------------------------------------

    /** Called when an SDP answer has been created and is ready to send. */
    @Volatile var onSubscriberAnswer: ((String) -> Unit)? = null

    /** Called when a new local ICE candidate is available (JSON string). */
    @Volatile var onSubscriberIceCandidate: ((String) -> Unit)? = null

    /** Called when the publisher SDP offer is ready to send to the server. */
    @Volatile var onPublisherOffer: ((String) -> Unit)? = null

    /** Called when a new publisher ICE candidate is available (JSON string). */
    @Volatile var onPublisherIceCandidate: ((String) -> Unit)? = null

    /** Called when a remote track is received. */
    @Volatile var onTrackAdded: ((MediaStreamTrack) -> Unit)? = null

    /** Called when an SDP or ICE error occurs that prevents voice connection. */
    @Volatile var onError: ((String) -> Unit)? = null

    // -- Lifecycle ------------------------------------------------------------

    /**
     * Initializes the [PeerConnectionFactory].
     *
     * Must be called once before [createSubscriberPeerConnection] or
     * [createPublisherOffer]. Safe to call multiple times — subsequent
     * calls are no-ops if the factory already exists.
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
     * Creates the subscriber [PeerConnection], fetching ICE server configuration
     * from the server API.
     *
     * The subscriber PC is receive-only — remote audio and video tracks
     * (other users' mics + screen shares) are surfaced via [remoteAudioTracks]
     * and [remoteVideoTracks]. The mic upload is handled by the publisher PC
     * (see [createPublisherOffer]), so no local track is added here.
     *
     * Call [initialize] first.
     */
    suspend fun createSubscriberPeerConnection() {
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

        subscriberPc = pcFactory.createPeerConnection(rtcConfig, createSubscriberObserver())
            ?: throw IllegalStateException("Failed to create subscriber PeerConnection")

        logger.info("Subscriber PeerConnection created with ${rtcIceServers.size} ICE servers")
    }

    /**
     * Creates the publisher [PeerConnection], adds the local mic track, and
     * creates an SDP offer. When the offer is ready, [onPublisherOffer] is
     * invoked with the SDP string.
     *
     * Android-initiated (the subscriber flow is server-initiated). Call
     * [initialize] first; safe to call after [closePeerConnections].
     *
     * The local mic ([audioSource] + [localAudioTrack]) is created here —
     * this is the single creation site post dual-PC refactor.
     */
    suspend fun createPublisherOffer() {
        val pcFactory = factory ?: throw IllegalStateException(
            "PeerConnectionFactory not initialized. Call initialize() first."
        )

        publisherRemoteDescriptionSet = false
        publisherPendingCandidates.clear()

        val iceConfig = try {
            voiceApi.getIceServers()
        } catch (e: kotlin.coroutines.cancellation.CancellationException) {
            throw e
        } catch (e: Exception) {
            logger.log(Level.WARNING, "Failed to fetch ICE servers", e)
            onError?.invoke("Failed to fetch ICE servers: ${e.message}")
            throw e
        }

        val rtcIceServers = iceConfig.iceServers.map { server -> server.toRtcIceServer() }
        val rtcConfig = PeerConnection.RTCConfiguration(rtcIceServers).apply {
            sdpSemantics = PeerConnection.SdpSemantics.UNIFIED_PLAN
            continualGatheringPolicy =
                PeerConnection.ContinualGatheringPolicy.GATHER_CONTINUALLY
        }

        val pub = pcFactory.createPeerConnection(rtcConfig, createPublisherObserver())
            ?: throw IllegalStateException("Failed to create publisher PeerConnection")
        publisherPc = pub

        // Create and attach the local mic track — single site of creation.
        audioSource = pcFactory.createAudioSource(MediaConstraints())
        val track = pcFactory.createAudioTrack(LOCAL_AUDIO_TRACK_ID, audioSource).also {
            it.setEnabled(!isMuted)
        }
        localAudioTrack = track

        pub.addTransceiver(
            track,
            RtpTransceiver.RtpTransceiverInit(
                RtpTransceiver.RtpTransceiverDirection.SEND_ONLY
            )
        )

        pub.createOffer(
            object : SdpObserverAdapter("createPublisherOffer", onError) {
                override fun onCreateSuccess(desc: SessionDescription) {
                    pub.setLocalDescription(
                        object : SdpObserverAdapter("setPublisherLocalDesc", onError) {
                            override fun onSetSuccess() {
                                super.onSetSuccess()
                                onPublisherOffer?.invoke(desc.description)
                                logger.info("Publisher SDP offer created and set")
                            }
                        },
                        desc
                    )
                }
            },
            MediaConstraints()
        )

        logger.info("Publisher PeerConnection created with ${rtcIceServers.size} ICE servers")
    }

    /**
     * Closes both the publisher and subscriber peer connections and releases
     * audio resources.
     *
     * Resets buffered ICE candidates, remote-description flags, and ICE state
     * flows so that a subsequent [createPublisherOffer] /
     * [createSubscriberPeerConnection] starts from a clean slate.
     */
    fun closePeerConnections() {
        localAudioTrack?.dispose()
        localAudioTrack = null
        audioSource?.dispose()
        audioSource = null

        // Clear remote-track flows first so observers don't reach into a disposed PC
        _remoteAudioTracks.value = emptyMap()
        _remoteVideoTracks.value = emptyMap()
        publisherRemoteDescriptionSet = false
        subscriberRemoteDescriptionSet = false
        publisherPendingCandidates.clear()
        subscriberPendingCandidates.clear()

        val pub = publisherPc
        // Null first (with @Volatile on the field) so concurrent readers see null
        // before close+dispose runs on the local reference.
        publisherPc = null
        pub?.close()
        pub?.dispose()

        val sub = subscriberPc
        subscriberPc = null
        sub?.close()
        sub?.dispose()

        publisherIceState.value = null
        subscriberIceState.value = null

        logger.info("Publisher and subscriber PeerConnections closed")
    }

    /** Disposes of the factory and all resources. Call on application shutdown. */
    fun dispose() {
        closePeerConnections()
        factory?.dispose()
        factory = null
        audioDeviceModule?.release()
        audioDeviceModule = null
        if (_eglBase.isInitialized()) {
            try {
                _eglBase.value.release()
            } catch (_: IllegalStateException) {
                // EglBase may already be released
            } catch (e: Exception) {
                logger.log(Level.WARNING, "Unexpected error releasing EglBase", e)
            }
        }
        logger.info("WebRtcManager disposed")
    }

    // -- Signaling ------------------------------------------------------------

    /**
     * Handles an SDP offer from the server for the subscriber PC.
     *
     * Sets the remote description to the offer, then creates and sets a local
     * SDP answer. When the answer is ready, [onSubscriberAnswer] is invoked.
     */
    fun handleSubscriberOffer(sdp: String) {
        val pc = subscriberPc ?: run {
            logger.warning("handleSubscriberOffer called but subscriber PC is null")
            onError?.invoke("Voice connection error: not initialized")
            return
        }

        subscriberRemoteDescriptionSet = false
        val offer = SessionDescription(SessionDescription.Type.OFFER, sdp)

        pc.setRemoteDescription(object : SdpObserverAdapter("setSubscriberRemoteDesc", onError) {
            override fun onSetSuccess() {
                logger.info("Subscriber remote description set successfully")
                subscriberRemoteDescriptionSet = true
                drainSubscriberCandidates()
                pc.createAnswer(object : SdpObserverAdapter("createSubscriberAnswer", onError) {
                    override fun onCreateSuccess(desc: SessionDescription) {
                        pc.setLocalDescription(object : SdpObserverAdapter("setSubscriberLocalDesc", onError) {
                            override fun onSetSuccess() {
                                super.onSetSuccess()
                                onSubscriberAnswer?.invoke(desc.description)
                                logger.info("Subscriber SDP answer created and set")
                            }
                        }, desc)
                    }
                }, MediaConstraints())
            }
        }, offer)
    }

    /**
     * Handles an SDP answer from the server for the publisher PC.
     *
     * Sets the remote description to the answer, then drains any ICE
     * candidates buffered while the answer was outstanding.
     */
    fun handlePublisherAnswer(sdp: String) {
        val pc = publisherPc ?: run {
            logger.warning("handlePublisherAnswer called but publisher PC is null")
            return
        }
        val answer = SessionDescription(SessionDescription.Type.ANSWER, sdp)
        pc.setRemoteDescription(object : SdpObserverAdapter("setPublisherRemoteDesc", onError) {
            override fun onSetSuccess() {
                super.onSetSuccess()
                publisherRemoteDescriptionSet = true
                logger.info("Publisher remote description set successfully")
                drainPublisherCandidates()
            }
        }, answer)
    }

    /** Replays subscriber ICE candidates buffered before remote description was set. */
    private fun drainSubscriberCandidates() {
        val drained = subscriberPendingCandidates.toList()
        subscriberPendingCandidates.clear()
        drained.forEach { candidateJson -> addIceCandidate("subscriber", candidateJson) }
    }

    /** Replays publisher ICE candidates buffered before remote description was set. */
    private fun drainPublisherCandidates() {
        val drained = publisherPendingCandidates.toList()
        publisherPendingCandidates.clear()
        drained.forEach { candidateJson -> addIceCandidate("publisher", candidateJson) }
    }

    /** Returns the current subscriber-PC local SDP description, or null if none is set. */
    fun getLocalDescription(): String? =
        subscriberPc?.localDescription?.description

    /**
     * Adds a remote ICE candidate received from the server via WebSocket,
     * routed to the publisher or subscriber PC based on [pcType].
     *
     * If the target PC's remote description has not yet been applied, the
     * candidate is buffered (capped at [MAX_PENDING_CANDIDATES]) and replayed
     * on the next successful `setRemoteDescription`.
     *
     * @param pcType `"publisher"` or `"subscriber"` (matches the wire-format
     *   `pc_type` field on `voice_ice_candidate` events).
     * @param candidateJson JSON string in the format:
     *   `{"candidate":"...","sdpMLineIndex":0,"sdpMid":"..."}`
     */
    fun addIceCandidate(pcType: String, candidateJson: String) {
        val pc: PeerConnection?
        val buffer: MutableList<String>
        val remoteSet: Boolean
        when (pcType) {
            "publisher" -> {
                pc = publisherPc
                buffer = publisherPendingCandidates
                remoteSet = publisherRemoteDescriptionSet
            }
            "subscriber" -> {
                pc = subscriberPc
                buffer = subscriberPendingCandidates
                remoteSet = subscriberRemoteDescriptionSet
            }
            else -> {
                logger.warning("Unknown pc_type: $pcType")
                return
            }
        }

        if (!remoteSet) {
            if (buffer.size >= MAX_PENDING_CANDIDATES) {
                logger.warning("ICE candidate buffer full for $pcType ($MAX_PENDING_CANDIDATES), dropping candidate")
                return
            }
            logger.fine("Buffering $pcType ICE candidate (remote description not yet set)")
            buffer.add(candidateJson)
            return
        }

        val target = pc ?: run {
            logger.warning("addIceCandidate called for $pcType but PC is null")
            return
        }

        try {
            val data = IceCandidateData.fromJson(candidateJson)
            val candidate = IceCandidate(data.sdpMid, data.sdpMLineIndex, data.candidate)
            target.addIceCandidate(candidate)
        } catch (e: Exception) {
            logger.log(Level.WARNING, "Failed to parse $pcType ICE candidate: $candidateJson", e)
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

    private fun createSubscriberObserver() = object : PeerConnection.Observer {
        override fun onIceCandidate(candidate: IceCandidate) {
            val data = IceCandidateData(
                candidate = candidate.sdp,
                sdpMLineIndex = candidate.sdpMLineIndex,
                sdpMid = candidate.sdpMid
            )
            onSubscriberIceCandidate?.invoke(data.toJson())
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
            logger.info("Subscriber signaling state: $state")
        }

        override fun onIceConnectionChange(state: PeerConnection.IceConnectionState?) {
            logger.info("Subscriber ICE connection state: $state")
            subscriberIceState.value = state
            when (state) {
                PeerConnection.IceConnectionState.FAILED ->
                    onError?.invoke("Voice subscriber connection failed (ICE)")
                PeerConnection.IceConnectionState.DISCONNECTED ->
                    logger.warning("Subscriber ICE disconnected, may recover")
                else -> {}
            }
        }

        override fun onIceConnectionReceivingChange(receiving: Boolean) {
            logger.info("Subscriber ICE connection receiving: $receiving")
        }

        override fun onIceGatheringChange(state: PeerConnection.IceGatheringState?) {
            logger.info("Subscriber ICE gathering state: $state")
        }

        override fun onIceCandidatesRemoved(candidates: Array<out IceCandidate>?) {
            logger.info("Subscriber ICE candidates removed: ${candidates?.size ?: 0}")
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
            logger.info("Subscriber renegotiation needed")
        }
    }

    private fun createPublisherObserver() = object : PeerConnection.Observer {
        override fun onIceCandidate(candidate: IceCandidate) {
            val data = IceCandidateData(
                candidate = candidate.sdp,
                sdpMLineIndex = candidate.sdpMLineIndex,
                sdpMid = candidate.sdpMid
            )
            onPublisherIceCandidate?.invoke(data.toJson())
        }

        override fun onTrack(transceiver: RtpTransceiver) {
            // Publisher PC is send-only — incoming tracks are unexpected.
            logger.info("Publisher onTrack ignored (publisher is send-only)")
        }

        override fun onSignalingChange(state: PeerConnection.SignalingState?) {
            logger.info("Publisher signaling state: $state")
        }

        override fun onIceConnectionChange(state: PeerConnection.IceConnectionState?) {
            logger.info("Publisher ICE connection state: $state")
            publisherIceState.value = state
            when (state) {
                PeerConnection.IceConnectionState.FAILED ->
                    onError?.invoke("Voice publisher connection failed (ICE)")
                PeerConnection.IceConnectionState.DISCONNECTED ->
                    logger.warning("Publisher ICE disconnected, may recover")
                else -> {}
            }
        }

        override fun onIceConnectionReceivingChange(receiving: Boolean) {}

        override fun onIceGatheringChange(state: PeerConnection.IceGatheringState?) {}

        override fun onIceCandidatesRemoved(candidates: Array<out IceCandidate>?) {}

        override fun onAddStream(stream: MediaStream?) {}

        override fun onRemoveStream(stream: MediaStream?) {}

        override fun onDataChannel(channel: DataChannel?) {}

        override fun onRenegotiationNeeded() {
            logger.info("Publisher renegotiation needed")
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
