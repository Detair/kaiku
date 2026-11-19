package io.wolftown.kaiku.ui.call

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import dagger.hilt.android.lifecycle.HiltViewModel
import io.wolftown.kaiku.data.call.CallController
import io.wolftown.kaiku.data.call.CallDirectory
import io.wolftown.kaiku.data.call.CallState
import io.wolftown.kaiku.data.repository.VoiceRepository
import io.wolftown.kaiku.data.voice.AudioRoute
import io.wolftown.kaiku.data.voice.AudioRouteManager
import io.wolftown.kaiku.data.voice.ElapsedClock
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharingStarted
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.combine
import kotlinx.coroutines.flow.flow
import kotlinx.coroutines.flow.stateIn
import javax.inject.Inject

/** State holder of the call screen: maps the call to [CallUi] and forwards the buttons. */
@HiltViewModel
class CallViewModel @Inject constructor(
    private val controller: CallController,
    private val voice: VoiceRepository,
    private val audio: AudioRouteManager,
    private val directory: CallDirectory,
    private val clock: ElapsedClock,
) : ViewModel() {

    private val ticks = flow {
        while (true) {
            emit(clock.nowMs())
            delay(TICK_MS)
        }
    }

    val ui: StateFlow<CallUi> = combine(
        controller.state,
        voice.linkState,
        voice.isMuted,
        audio.currentRoute,
        ticks,
    ) { state, link, muted, route, now ->
        CallUiMapper.map(state, link, muted, route, now, directory::titleOf)
    }.stateIn(viewModelScope, SharingStarted.WhileSubscribed(STOP_TIMEOUT_MS), CallUi(CallPhase.None))

    private val _micNeeded = MutableStateFlow(false)

    /** The microphone permission was refused when answering or calling: say so, with "Open settings". */
    val micNeeded: StateFlow<Boolean> = _micNeeded.asStateFlow()

    /**
     * Accept the ring in [channelId] (null: the one shown), once the
     * microphone permission is granted; without it nothing is sent and the
     * screen says why. A ring that came with the screen's intent may not be
     * applied yet; the controller handles events in order.
     */
    fun accept(micGranted: Boolean, channelId: String? = null) {
        val target = channelId ?: (controller.state.value as? CallState.Incoming)?.channelId ?: return
        if (!micGranted) {
            _micNeeded.value = true
            return
        }
        _micNeeded.value = false
        controller.accept(target)
    }

    fun decline() {
        val incoming = controller.state.value as? CallState.Incoming ?: return
        controller.decline(incoming.channelId)
    }

    fun hangUp() = controller.hangUp()

    fun dismissEnded() = controller.dismissEnded()

    fun toggleMute() = voice.toggleMute()

    fun toggleSpeaker() {
        val on = audio.currentRoute.value != AudioRoute.Speaker
        audio.switchRoute(CallUiMapper.speakerTarget(on, audio.availableRoutes.value))
    }

    private companion object {
        const val TICK_MS = 1_000L
        const val STOP_TIMEOUT_MS = 5_000L
    }
}
