package io.wolftown.kaiku.ui.channel

import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.collectLatest
import kotlinx.coroutines.flow.receiveAsFlow

/**
 * Text for a polite live region (AND-B-11, review W2). A live region speaks
 * only when its text changes, so "New message from Anna" twice in a row would
 * be spoken once. Every [announce] therefore empties the region first, then
 * sets the text, and empties it again after [holdMs]: each call is its own
 * change, also when the words repeat.
 */
internal class LiveAnnouncer(
    private val gapMs: Long = GAP_MS,
    private val holdMs: Long = HOLD_MS,
) {
    private val requests = Channel<String>(Channel.CONFLATED)
    private val _text = MutableStateFlow("")

    /** What the live region holds now ("" between announcements). */
    val text: StateFlow<String> = _text.asStateFlow()

    fun announce(message: String) {
        requests.trySend(message)
    }

    /** Runs while the screen is shown (LaunchedEffect). */
    suspend fun run() {
        requests.receiveAsFlow().collectLatest { message ->
            _text.value = ""
            delay(gapMs)
            _text.value = message
            delay(holdMs)
            _text.value = ""
        }
    }

    private companion object {
        const val GAP_MS = 100L
        const val HOLD_MS = 4_000L
    }
}
