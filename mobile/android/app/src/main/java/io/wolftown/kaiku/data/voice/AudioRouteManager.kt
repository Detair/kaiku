package io.wolftown.kaiku.data.voice

import android.bluetooth.BluetoothHeadset
import android.bluetooth.BluetoothProfile
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.media.AudioAttributes
import android.media.AudioFocusRequest
import android.media.AudioDeviceInfo
import android.media.AudioManager
import android.os.Build
import androidx.annotation.RequiresApi
import dagger.hilt.android.qualifiers.ApplicationContext
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch
import java.io.Closeable
import java.util.logging.Logger
import javax.inject.Inject
import javax.inject.Singleton

/**
 * Audio output route for voice calls.
 */
enum class AudioRoute {
    /** Device speaker (loudspeaker). */
    Speaker,

    /** Earpiece (phone held to ear). */
    Earpiece,

    /** Bluetooth headset/earbuds. */
    Bluetooth,

    /** Wired headset (3.5mm or USB-C audio). */
    WiredHeadset
}

/**
 * Picks the route used when a call starts: Bluetooth > wired headset > speaker.
 *
 * The earpiece is never the default: family voice is group voice (as on Discord),
 * so the phone is not held to the ear. It stays selectable manually.
 */
internal fun selectDefaultRoute(available: Set<AudioRoute>): AudioRoute = when {
    AudioRoute.Bluetooth in available -> AudioRoute.Bluetooth
    AudioRoute.WiredHeadset in available -> AudioRoute.WiredHeadset
    else -> AudioRoute.Speaker
}

/**
 * Manages Android audio routing for voice calls.
 *
 * Handles:
 * - Audio focus acquisition/release for voice communication
 * - Route detection (speaker, earpiece, bluetooth, wired headset)
 * - Route switching
 * - Broadcast receivers for headset plug/unplug and Bluetooth state changes
 */
@Singleton
class AudioRouteManager @Inject constructor(
    @ApplicationContext private val context: Context
) : Closeable {
    companion object {
        private val logger = Logger.getLogger("AudioRouteManager")

        private val WIRED_TYPES = setOf(
            AudioDeviceInfo.TYPE_WIRED_HEADSET,
            AudioDeviceInfo.TYPE_WIRED_HEADPHONES,
            AudioDeviceInfo.TYPE_USB_HEADSET
        )
    }

    private val audioManager: AudioManager =
        context.getSystemService(Context.AUDIO_SERVICE) as AudioManager

    private val _currentRoute = MutableStateFlow(AudioRoute.Speaker)
    /** The currently active audio route. */
    val currentRoute: StateFlow<AudioRoute> = _currentRoute.asStateFlow()

    private val _availableRoutes = MutableStateFlow(setOf(AudioRoute.Speaker, AudioRoute.Earpiece))
    /** The set of currently available audio routes. */
    val availableRoutes: StateFlow<Set<AudioRoute>> = _availableRoutes.asStateFlow()

    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main)

    private var audioFocusRequest: AudioFocusRequest? = null
    private var headsetReceiver: BroadcastReceiver? = null
    private var bluetoothReceiver: BroadcastReceiver? = null
    private var scoTimeoutJob: Job? = null

    private val scoReceiver = object : BroadcastReceiver() {
        override fun onReceive(context: Context, intent: Intent) {
            val state = intent.getIntExtra(AudioManager.EXTRA_SCO_AUDIO_STATE, -1)
            when (state) {
                AudioManager.SCO_AUDIO_STATE_CONNECTED -> {
                    audioManager.isBluetoothScoOn = true
                    _currentRoute.value = AudioRoute.Bluetooth
                    scoTimeoutJob?.cancel()
                }
                AudioManager.SCO_AUDIO_STATE_ERROR,
                AudioManager.SCO_AUDIO_STATE_DISCONNECTED -> {
                    audioManager.isBluetoothScoOn = false
                    _currentRoute.value = AudioRoute.Speaker
                    scoTimeoutJob?.cancel()
                }
            }
        }
    }

    /**
     * Requests audio focus for voice communication.
     *
     * Sets the audio mode to [AudioManager.MODE_IN_COMMUNICATION] and
     * registers broadcast receivers for headset/Bluetooth state changes.
     */
    fun requestAudioFocus() {
        val attributes = AudioAttributes.Builder()
            .setUsage(AudioAttributes.USAGE_VOICE_COMMUNICATION)
            .setContentType(AudioAttributes.CONTENT_TYPE_SPEECH)
            .build()

        val focusRequest = AudioFocusRequest.Builder(AudioManager.AUDIOFOCUS_GAIN)
            .setAudioAttributes(attributes)
            .setAcceptsDelayedFocusGain(false)
            .setOnAudioFocusChangeListener { focusChange ->
                when (focusChange) {
                    AudioManager.AUDIOFOCUS_LOSS,
                    AudioManager.AUDIOFOCUS_LOSS_TRANSIENT -> {
                        logger.info("Audio focus lost: $focusChange")
                    }
                    AudioManager.AUDIOFOCUS_GAIN -> {
                        logger.info("Audio focus gained")
                    }
                }
            }
            .build()

        audioFocusRequest = focusRequest
        audioManager.requestAudioFocus(focusRequest)
        audioManager.mode = AudioManager.MODE_IN_COMMUNICATION

        registerReceivers()
        // Apply the default immediately so the UI and the real output agree.
        detectAvailableRoutes(forceDefault = true)

        logger.info("Audio focus requested, mode set to MODE_IN_COMMUNICATION")
    }

    /**
     * Releases audio focus and restores the audio mode to normal.
     *
     * Unregisters broadcast receivers.
     */
    fun abandonAudioFocus() {
        audioFocusRequest?.let { audioManager.abandonAudioFocusRequest(it) }
        audioFocusRequest = null
        audioManager.mode = AudioManager.MODE_NORMAL
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
            audioManager.clearCommunicationDevice()
        } else {
            audioManager.isSpeakerphoneOn = false
        }
        stopBluetoothSco()

        unregisterReceivers()

        logger.info("Audio focus abandoned, mode restored to MODE_NORMAL")
    }

    override fun close() {
        unregisterReceivers()
        scope.cancel()
    }

    /**
     * Switches the audio output to the specified [route].
     *
     * Only routes present in [availableRoutes] will have an effect.
     */
    fun switchRoute(route: AudioRoute) {
        if (route !in _availableRoutes.value) {
            logger.warning("Attempted to switch to unavailable route: $route")
            return
        }

        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
            if (!switchRouteModern(route)) {
                logger.warning("No communication device for route $route")
                return
            }
        } else {
            switchRouteLegacy(route)
        }

        logger.info("Switched audio route to $route")
    }

    /**
     * Detects currently available audio routes and updates [availableRoutes].
     *
     * Speaker and Earpiece are always available. Bluetooth (an actual SCO output
     * device) and WiredHeadset depend on connected hardware.
     *
     * @param forceDefault apply [selectDefaultRoute] even if it equals the current
     *   route (call start). Otherwise only react to changes: fall back if the
     *   current route disappeared, or move to a newly connected Bluetooth/wired
     *   device.
     */
    fun detectAvailableRoutes(forceDefault: Boolean = false) {
        val previous = _availableRoutes.value
        val routes = mutableSetOf(AudioRoute.Speaker, AudioRoute.Earpiece)
        val outputs = audioManager.getDevices(AudioManager.GET_DEVICES_OUTPUTS)

        if (outputs.any { it.type in WIRED_TYPES }) {
            routes.add(AudioRoute.WiredHeadset)
        }
        if (outputs.any { it.type == AudioDeviceInfo.TYPE_BLUETOOTH_SCO }) {
            routes.add(AudioRoute.Bluetooth)
        }

        _availableRoutes.value = routes

        val current = _currentRoute.value
        val default = selectDefaultRoute(routes)
        val newlyConnected = (routes - previous).any {
            it == AudioRoute.Bluetooth || it == AudioRoute.WiredHeadset
        }
        val target = when {
            forceDefault -> default
            current !in routes -> default
            newlyConnected -> default
            else -> current
        }

        if (forceDefault || target != current) {
            switchRoute(target)
        }

        logger.info("Available routes: $routes")
    }

    // -- Internal -------------------------------------------------------------

    private fun switchRouteLegacy(route: AudioRoute) {
        when (route) {
            AudioRoute.Speaker -> {
                audioManager.isSpeakerphoneOn = true
                stopBluetoothSco()
                _currentRoute.value = route
            }
            AudioRoute.Earpiece -> {
                audioManager.isSpeakerphoneOn = false
                stopBluetoothSco()
                _currentRoute.value = route
            }
            AudioRoute.Bluetooth -> {
                // Route update is deferred — the scoReceiver will set
                // _currentRoute once SCO_AUDIO_STATE_CONNECTED arrives.
                audioManager.isSpeakerphoneOn = false
                startBluetoothSco()
            }
            AudioRoute.WiredHeadset -> {
                audioManager.isSpeakerphoneOn = false
                stopBluetoothSco()
                _currentRoute.value = route
            }
        }
    }

    /** API 31+: select the communication device directly. Returns false if none matches. */
    @RequiresApi(Build.VERSION_CODES.S)
    private fun switchRouteModern(route: AudioRoute): Boolean {
        val wanted: Set<Int> = when (route) {
            AudioRoute.Speaker -> setOf(AudioDeviceInfo.TYPE_BUILTIN_SPEAKER)
            AudioRoute.Earpiece -> setOf(AudioDeviceInfo.TYPE_BUILTIN_EARPIECE)
            AudioRoute.Bluetooth -> setOf(
                AudioDeviceInfo.TYPE_BLUETOOTH_SCO,
                AudioDeviceInfo.TYPE_BLE_HEADSET
            )
            AudioRoute.WiredHeadset -> WIRED_TYPES
        }
        val device = audioManager.availableCommunicationDevices.firstOrNull { it.type in wanted }
            ?: return false
        if (!audioManager.setCommunicationDevice(device)) return false
        _currentRoute.value = route
        return true
    }

    private fun startBluetoothSco() {
        try {
            @Suppress("DEPRECATION")
            audioManager.startBluetoothSco()

            // Wait for BroadcastReceiver to confirm SCO connected; fall back on timeout
            scoTimeoutJob?.cancel()
            scoTimeoutJob = scope.launch {
                delay(3000)
                if (_currentRoute.value != AudioRoute.Bluetooth) {
                    logger.warning("Bluetooth SCO timeout — falling back to speaker")
                    _currentRoute.value = AudioRoute.Speaker
                }
            }
        } catch (e: Exception) {
            logger.warning("Failed to start Bluetooth SCO: ${e.message}")
        }
    }

    private fun stopBluetoothSco() {
        try {
            if (audioManager.isBluetoothScoOn) {
                audioManager.isBluetoothScoOn = false
                @Suppress("DEPRECATION")
                audioManager.stopBluetoothSco()
            }
        } catch (e: Exception) {
            logger.warning("Failed to stop Bluetooth SCO: ${e.message}")
        }
    }

    private fun registerReceivers() {
        // Wired headset plug/unplug
        headsetReceiver = object : BroadcastReceiver() {
            override fun onReceive(context: Context, intent: Intent) {
                logger.info("Headset plug event: ${intent.getIntExtra("state", -1)}")
                detectAvailableRoutes()
            }
        }
        val headsetFilter = IntentFilter(Intent.ACTION_HEADSET_PLUG)
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            context.registerReceiver(
                headsetReceiver,
                headsetFilter,
                Context.RECEIVER_NOT_EXPORTED
            )
        } else {
            @Suppress("UnspecifiedRegisterReceiverFlag")
            context.registerReceiver(headsetReceiver, headsetFilter)
        }

        // Bluetooth SCO audio state
        val scoFilter = IntentFilter(AudioManager.ACTION_SCO_AUDIO_STATE_UPDATED)
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            context.registerReceiver(
                scoReceiver,
                scoFilter,
                Context.RECEIVER_NOT_EXPORTED
            )
        } else {
            @Suppress("UnspecifiedRegisterReceiverFlag")
            context.registerReceiver(scoReceiver, scoFilter)
        }

        // Bluetooth headset connection state
        bluetoothReceiver = object : BroadcastReceiver() {
            override fun onReceive(context: Context, intent: Intent) {
                val state = intent.getIntExtra(
                    BluetoothProfile.EXTRA_STATE,
                    BluetoothProfile.STATE_DISCONNECTED
                )
                logger.info("Bluetooth headset state: $state")
                detectAvailableRoutes()
            }
        }
        val btFilter = IntentFilter(BluetoothHeadset.ACTION_CONNECTION_STATE_CHANGED)
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            context.registerReceiver(
                bluetoothReceiver,
                btFilter,
                Context.RECEIVER_NOT_EXPORTED
            )
        } else {
            @Suppress("UnspecifiedRegisterReceiverFlag")
            context.registerReceiver(bluetoothReceiver, btFilter)
        }
    }

    private fun unregisterReceivers() {
        headsetReceiver?.let {
            try {
                context.unregisterReceiver(it)
            } catch (_: IllegalArgumentException) {
                // Receiver not registered
            }
        }
        headsetReceiver = null

        try {
            context.unregisterReceiver(scoReceiver)
        } catch (_: IllegalArgumentException) {
            // Receiver not registered
        }

        bluetoothReceiver?.let {
            try {
                context.unregisterReceiver(it)
            } catch (_: IllegalArgumentException) {
                // Receiver not registered
            }
        }
        bluetoothReceiver = null

        scoTimeoutJob?.cancel()
        scoTimeoutJob = null
    }
}
