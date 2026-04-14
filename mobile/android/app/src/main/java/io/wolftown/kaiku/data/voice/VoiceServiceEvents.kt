package io.wolftown.kaiku.data.voice

import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.SharedFlow
import kotlinx.coroutines.flow.asSharedFlow
import java.util.logging.Logger
import javax.inject.Inject
import javax.inject.Singleton

sealed class VoiceServiceEvent {
    data object MuteToggle : VoiceServiceEvent()
    data object Disconnect : VoiceServiceEvent()
}

@Singleton
class VoiceServiceEvents @Inject constructor() {
    companion object {
        private val logger = Logger.getLogger("VoiceServiceEvents")
    }

    private val _events = MutableSharedFlow<VoiceServiceEvent>(extraBufferCapacity = 5)
    val events: SharedFlow<VoiceServiceEvent> = _events.asSharedFlow()

    fun emit(event: VoiceServiceEvent) {
        val emitted = _events.tryEmit(event)
        if (!emitted) {
            logger.warning("VoiceServiceEvent dropped (buffer full): ${event::class.simpleName}")
        }
    }
}
