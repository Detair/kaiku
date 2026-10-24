package io.wolftown.kaiku.ui.voice

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import dagger.hilt.android.lifecycle.HiltViewModel
import io.wolftown.kaiku.data.repository.GuildRepository
import io.wolftown.kaiku.data.repository.VoiceRepository
import kotlinx.coroutines.flow.SharingStarted
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.combine
import kotlinx.coroutines.flow.stateIn
import javax.inject.Inject

/** Route of the voice screen in the nav graph. */
internal const val VOICE_ROUTE = "voice/{channelId}"

/**
 * The overlay bar shows while a call is running and the voice screen is not
 * the current destination, so a call that kept running after Back can be
 * reopened. Never on top of the voice screen itself.
 */
internal fun shouldShowVoiceOverlay(voiceChannelId: String?, currentRoute: String?): Boolean =
    voiceChannelId != null && currentRoute != VOICE_ROUTE

/**
 * Thin ViewModel for the [VoiceOverlay] bar.
 *
 * Provides voice state (current channel, its name, mute status) to the
 * navigation scaffold so the overlay persists across screen navigation.
 */
@HiltViewModel
class VoiceOverlayViewModel @Inject constructor(
    private val voiceRepository: VoiceRepository,
    guildRepository: GuildRepository,
) : ViewModel() {

    val currentChannelId: StateFlow<String?> = voiceRepository.currentChannelId
    val isMuted: StateFlow<Boolean> = voiceRepository.isMuted

    /** Name of the channel the call runs in, or null if it is not in the loaded channel list. */
    val channelName: StateFlow<String?> =
        combine(voiceRepository.currentChannelId, guildRepository.channels) { id, channels ->
            id?.let { channelId -> channels.find { it.id == channelId }?.name }
        }.stateIn(viewModelScope, SharingStarted.WhileSubscribed(5_000), null)

    fun onToggleMute() {
        voiceRepository.toggleMute()
    }

    fun onDisconnect() {
        voiceRepository.leaveInBackground()
    }
}
