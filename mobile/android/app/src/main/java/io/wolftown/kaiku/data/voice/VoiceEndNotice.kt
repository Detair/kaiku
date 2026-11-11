package io.wolftown.kaiku.data.voice

import androidx.annotation.StringRes
import io.wolftown.kaiku.R

/**
 * Why a voice call ended without the user leaving, shown as one line of text.
 * [canRejoin] decides whether the "Rejoin" action is offered.
 */
sealed interface VoiceEndNotice {
    val channelId: String

    @get:StringRes
    val message: Int

    val canRejoin: Boolean

    /** C32: three rejoin attempts failed. */
    data class RejoinFailed(override val channelId: String) : VoiceEndNotice {
        override val message: Int get() = R.string.voice_rejoin_failed
        override val canRejoin: Boolean get() = true
    }
}
