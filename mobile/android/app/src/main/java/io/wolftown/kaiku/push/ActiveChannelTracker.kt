package io.wolftown.kaiku.push

import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import javax.inject.Inject
import javax.inject.Singleton

/**
 * The chat screen currently resumed in the foreground, if any. Wakes for this
 * channel never post a notification (the user is looking at it).
 */
@Singleton
class ActiveChannelTracker @Inject constructor() {
    private val _visibleChannelId = MutableStateFlow<String?>(null)
    val visibleChannelId: StateFlow<String?> = _visibleChannelId.asStateFlow()

    fun onVisible(channelId: String) {
        _visibleChannelId.value = channelId
    }

    /** Clears only if [channelId] is still the visible one (a newer screen may have resumed first). */
    fun onHidden(channelId: String) {
        _visibleChannelId.compareAndSet(channelId, null)
    }
}
