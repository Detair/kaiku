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
import kotlinx.coroutines.flow.SharingStarted
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.flow.stateIn
import org.webrtc.EglBase
import org.webrtc.VideoTrack
import java.util.logging.Logger
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

    /** True while this ViewModel's channel is the one the repository is joined to. */
    val isInChannel: StateFlow<Boolean> = voiceRepository.currentChannelId
        .map { it == channelId }
        .stateIn(viewModelScope, SharingStarted.Eagerly, false)
    val screenShares: StateFlow<List<ScreenShareInfo>> = voiceRepository.screenShares
    val remoteVideoTracks: StateFlow<Map<String, VideoTrack>> = webRtcManager.remoteVideoTracks
    val layerPreferences: StateFlow<Map<String, String>> = voiceRepository.layerPreferences
    val currentRoute: StateFlow<AudioRoute> = audioRouteManager.currentRoute
    val availableRoutes: StateFlow<Set<AudioRoute>> = audioRouteManager.availableRoutes

    /** `@StringRes` of the latest join or voice error, or null. */
    val error: StateFlow<Int?> = voiceRepository.error

    /** Shared EGL context for video rendering. */
    val eglContext: EglBase.Context by lazy { webRtcManager.eglBase.eglBaseContext }

    /**
     * Joins the voice channel. The screen calls this once microphone permission is
     * granted; it is a no-op if we are already in this channel (re-entry via overlay).
     * The join runs on the repository's scope: Back during setup keeps it going.
     */
    fun onJoin() {
        if (voiceRepository.currentChannelId.value == channelId) return
        voiceRepository.startJoin(channelId)
    }

    /**
     * Leaves the voice channel explicitly (the Leave button). Runs on the
     * repository's scope: the screen navigates back right after the tap,
     * which clears this ViewModel.
     */
    fun onLeave() {
        voiceRepository.leaveInBackground()
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
        voiceRepository.clearError()
    }

    /**
     * Leaving the voice screen (Back, another destination) keeps the call: the
     * repository owns it and the overlay bar leads back here. Only the Leave
     * button ([onLeave]), the overlay's disconnect or the notification action
     * end it.
     */
    public override fun onCleared() {
        super.onCleared()
    }
}
