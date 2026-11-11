package io.wolftown.kaiku.data.voice

import androidx.annotation.StringRes
import io.wolftown.kaiku.R
import io.wolftown.kaiku.data.ws.VoiceRemovedReason

/**
 * Why a voice call ended without the user leaving, shown as one line of text.
 * [canRejoin] decides whether the "Rejoin" action is offered.
 */
sealed interface VoiceEndNotice {
    val channelId: String

    @get:StringRes
    val message: Int

    val canRejoin: Boolean

    /**
     * C32: the rejoin gave up (three failed attempts, or no connection for
     * 60 s). [wasMuted] is the user's last mute choice; "Rejoin" restores it.
     */
    data class RejoinFailed(
        override val channelId: String,
        val wasMuted: Boolean = false,
    ) : VoiceEndNotice {
        override val message: Int get() = R.string.voice_call_rejoin_failed
        override val canRejoin: Boolean get() = true
    }

    /** C31: the server removed us; never rejoined automatically, so no action. */
    data class Removed(
        override val channelId: String,
        val reason: VoiceRemovedReason,
    ) : VoiceEndNotice {
        override val message: Int
            get() = when (reason) {
                VoiceRemovedReason.Kicked -> R.string.voice_removed_kicked
                VoiceRemovedReason.Banned -> R.string.voice_removed_banned
                VoiceRemovedReason.PermissionLost -> R.string.voice_removed_permission_lost
                VoiceRemovedReason.ChannelDeleted -> R.string.voice_removed_channel_deleted
                VoiceRemovedReason.SessionReplaced -> R.string.voice_removed_session_replaced
            }
        override val canRejoin: Boolean get() = false
    }
}
