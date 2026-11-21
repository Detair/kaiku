package io.wolftown.kaiku.data.voice

import androidx.annotation.StringRes
import io.wolftown.kaiku.R

/**
 * The one line a voice screen, the overlay and the ongoing-call notification
 * show for the link, or null while it is connected.
 */
@StringRes
fun VoiceLinkState.statusText(): Int? = when (this) {
    VoiceLinkState.Stable -> null
    VoiceLinkState.Connecting -> R.string.voice_call_connecting
    VoiceLinkState.Interrupted, is VoiceLinkState.Rejoining -> R.string.voice_call_reconnecting
}
