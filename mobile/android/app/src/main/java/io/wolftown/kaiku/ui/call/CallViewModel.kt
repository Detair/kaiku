package io.wolftown.kaiku.ui.call

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import dagger.hilt.android.lifecycle.HiltViewModel
import io.wolftown.kaiku.data.call.CallController
import io.wolftown.kaiku.data.call.CallDirectory
import io.wolftown.kaiku.data.call.CallInput
import io.wolftown.kaiku.data.call.CallRef
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
    ) { _, link, muted, route, now ->
        // The call and the account it is for, read together: the controls name exactly this call.
        val shown = controller.shown()
        CallUiMapper.map(shown.state, link, muted, route, now, directory::titleOf).copy(call = shown.ref())
    }.stateIn(viewModelScope, SharingStarted.WhileSubscribed(STOP_TIMEOUT_MS), CallUi(CallPhase.None))

    private val _micNeeded = MutableStateFlow(false)

    /** The microphone permission was refused when answering or calling: say so, with "Open settings". */
    val micNeeded: StateFlow<Boolean> = _micNeeded.asStateFlow()

    /** The ring an Accept is waiting for (unlock, permission dialog); kept across a rotation. */
    private var pendingAccept: CallRef? = null

    /** Accept of [call] has to wait for the microphone permission: its answer is for this call only. */
    fun acceptNeedsPermission(call: CallRef) {
        pendingAccept = call
    }

    /** The permission dialog (or the unlock) answered. Without a waiting Accept nothing is answered. */
    fun permissionResult(granted: Boolean) {
        val call = pendingAccept ?: return
        pendingAccept = null
        accept(call, granted)
    }

    /**
     * Accept the ring [call], once the microphone permission is granted;
     * without it nothing is sent and the screen says why. If that ring is
     * over by now (or another call rings there), the call owner drops it.
     */
    fun accept(call: CallRef, micGranted: Boolean) {
        if (!micGranted) {
            _micNeeded.value = true
            return
        }
        _micNeeded.value = false
        controller.submit(CallInput.Accept(call))
    }

    fun decline(call: CallRef?) {
        controller.submit(CallInput.Decline(call ?: return))
    }

    fun hangUp(call: CallRef?) {
        controller.submit(CallInput.HangUp(call ?: return))
    }

    fun dismissEnded(call: CallRef?) {
        call ?: return
        controller.submit(CallInput.DismissEnded(call.account, call.channelId))
    }

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
