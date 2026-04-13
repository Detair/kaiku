package io.wolftown.kaiku.ui.voice

import androidx.lifecycle.SavedStateHandle
import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import dagger.hilt.android.lifecycle.HiltViewModel
import io.wolftown.kaiku.data.repository.GuildRepository
import io.wolftown.kaiku.data.repository.VoiceRepository
import io.wolftown.kaiku.data.voice.AudioRoute
import io.wolftown.kaiku.data.voice.AudioRouteManager
import io.wolftown.kaiku.data.voice.WebRtcManager
import io.wolftown.kaiku.data.ws.ScreenShareInfo
import io.wolftown.kaiku.data.ws.VoiceParticipant
import kotlinx.coroutines.NonCancellable
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharingStarted
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.flow.stateIn
import org.webrtc.EglBase
import org.webrtc.VideoTrack
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import java.util.logging.Level
import java.util.logging.Logger
import kotlin.coroutines.cancellation.CancellationException
import javax.inject.Inject

/**
 * ViewModel for the voice channel screen.
 *
 * Exposes voice state (participants, mute, connection, screen shares, audio route)
 * and provides actions (join, leave, mute toggle, route switch).
 */
@HiltViewModel
class VoiceViewModel @Inject constructor(
    private val voiceRepository: VoiceRepository,
    private val guildRepository: GuildRepository,
    private val audioRouteManager: AudioRouteManager,
    private val webRtcManager: WebRtcManager,
    savedStateHandle: SavedStateHandle
) : ViewModel() {

    companion object {
        private val logger = Logger.getLogger("VoiceViewModel")
    }

    private val channelId: String = savedStateHandle["channelId"]!!

    val channelName: StateFlow<String> = guildRepository.channels
        .map { channels -> channels.find { it.id == channelId }?.name ?: channelId }
        .stateIn(viewModelScope, SharingStarted.WhileSubscribed(5000), channelId)

    val participants: StateFlow<List<VoiceParticipant>> = voiceRepository.participants
    val isMuted: StateFlow<Boolean> = voiceRepository.isMuted
    val isConnected: StateFlow<Boolean> = voiceRepository.isConnected
    val screenShares: StateFlow<List<ScreenShareInfo>> = voiceRepository.screenShares
    val remoteVideoTracks: StateFlow<Map<String, VideoTrack>> = webRtcManager.remoteVideoTracks
    val layerPreferences: StateFlow<Map<String, String>> = voiceRepository.layerPreferences
    val currentRoute: StateFlow<AudioRoute> = audioRouteManager.currentRoute
    val availableRoutes: StateFlow<Set<AudioRoute>> = audioRouteManager.availableRoutes

    private val _error = MutableStateFlow<String?>(null)
    val error: StateFlow<String?> = _error.asStateFlow()

    /** Shared EGL context for video rendering. */
    val eglContext: EglBase.Context = webRtcManager.eglBase.eglBaseContext

    init {
        onJoin()
    }

    /** Joins the voice channel. Called automatically on init. */
    fun onJoin() {
        viewModelScope.launch {
            try {
                voiceRepository.joinChannel(channelId)
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                logger.log(Level.WARNING, "Failed to join voice channel: $channelId", e)
                _error.value = "Failed to join voice channel: ${e.message}"
            }
        }
    }

    /** Leaves the voice channel explicitly. */
    fun onLeave() {
        viewModelScope.launch {
            try {
                voiceRepository.leaveChannel()
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                logger.log(Level.WARNING, "Failed to leave voice channel", e)
            }
        }
    }

    /** Toggles the local microphone mute state. */
    fun onToggleMute() {
        voiceRepository.toggleMute()
    }

    /** Sets the preferred simulcast layer for a screen share stream. */
    fun onSetLayerPreference(streamId: String, layer: String) {
        voiceRepository.setLayerPreference(streamId, layer)
    }

    /** Switches the audio output route (speaker, earpiece, bluetooth, wired headset). */
    fun onSwitchAudioRoute(route: AudioRoute) {
        audioRouteManager.switchRoute(route)
    }

    fun clearError() {
        _error.value = null
    }

    public override fun onCleared() {
        // Use NonCancellable because viewModelScope is being cancelled during onCleared
        viewModelScope.launch(NonCancellable) {
            try {
                voiceRepository.leaveChannel()
            } catch (e: Exception) {
                logger.log(Level.WARNING, "Failed to leave voice channel on clear", e)
            }
        }
        super.onCleared()
    }
}
