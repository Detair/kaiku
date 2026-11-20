package io.wolftown.kaiku.ui.call

import androidx.annotation.StringRes
import io.wolftown.kaiku.R
import io.wolftown.kaiku.data.call.CallEndReason
import io.wolftown.kaiku.data.call.CallRef
import io.wolftown.kaiku.data.call.CallState
import io.wolftown.kaiku.data.voice.AudioRoute
import io.wolftown.kaiku.data.voice.VoiceLinkState
import io.wolftown.kaiku.data.ws.VoiceRemovedReason

/** What the call screen shows. */
enum class CallPhase { None, Ringing, Calling, Connecting, Connected, Reconnecting, Ended }

/**
 * The call screen's state: who ([name], null when unknown), the line under
 * it ([status]), the running time once connected, and the controls.
 */
data class CallUi(
    val phase: CallPhase,
    val name: String? = null,
    @StringRes val status: Int? = null,
    val durationSecs: Long? = null,
    val muted: Boolean = false,
    val speaker: Boolean = false,
    /** The sound goes to the earpiece (the phone is held to the ear). */
    val earpiece: Boolean = false,
    /** The call these controls are for: a tap acts on this call and on no other. */
    val call: CallRef? = null,
    /**
     * Speaker can be switched: the call's own voice session has the phone's sound (also while
     * ringing out). Before that the route belongs to whatever else is on, and the call's
     * session sets its own when it takes the sound.
     */
    val speakerEnabled: Boolean = false,
)

/** Pure mapping from the call, the voice link and the audio route to [CallUi]. */
object CallUiMapper {
    fun map(
        state: CallState,
        link: VoiceLinkState,
        route: AudioRoute,
        nowMs: Long,
        titleOf: (String) -> String?,
        /** The call's own voice session has the phone's sound. */
        callHasAudio: Boolean = false,
    ): CallUi {
        // The call's own mute: what the switch shows is what this call's microphone does (or
        // will do once it is on), whatever another voice session is doing meanwhile.
        val muted = when (state) {
            is CallState.Outgoing -> state.muted
            is CallState.Connecting -> state.muted
            is CallState.Connected -> state.muted
            else -> false
        }
        val speaker = route == AudioRoute.Speaker
        val earpiece = route == AudioRoute.Earpiece
        return mapWithoutRoute(state, link, muted, speaker, nowMs, titleOf).let {
            if (it.phase == CallPhase.None || it.phase == CallPhase.Ringing || it.phase == CallPhase.Ended) it else it.copy(earpiece = earpiece)
        }.copy(speakerEnabled = callHasAudio && state.usesVoice())
    }

    private fun mapWithoutRoute(
        state: CallState,
        link: VoiceLinkState,
        muted: Boolean,
        speaker: Boolean,
        nowMs: Long,
        titleOf: (String) -> String?,
    ): CallUi {
        return when (state) {
            CallState.Idle -> CallUi(CallPhase.None)
            is CallState.Incoming ->
                CallUi(CallPhase.Ringing, state.callerName ?: titleOf(state.channelId), R.string.voice_call_incoming)
            is CallState.Outgoing ->
                CallUi(CallPhase.Calling, titleOf(state.channelId), R.string.voice_call_calling, muted = muted, speaker = speaker)
            is CallState.Connecting ->
                CallUi(CallPhase.Connecting, titleOf(state.channelId), R.string.voice_call_connecting, muted = muted, speaker = speaker)
            is CallState.Connected -> {
                val duration = ((nowMs - state.sinceMs) / 1000).coerceAtLeast(0)
                when (link) {
                    VoiceLinkState.Connecting ->
                        CallUi(CallPhase.Connecting, titleOf(state.channelId), R.string.voice_call_connecting, null, muted, speaker)
                    VoiceLinkState.Interrupted, is VoiceLinkState.Rejoining ->
                        CallUi(CallPhase.Reconnecting, titleOf(state.channelId), R.string.voice_call_reconnecting, duration, muted, speaker)
                    VoiceLinkState.Stable ->
                        CallUi(CallPhase.Connected, titleOf(state.channelId), R.string.voice_call_in_call, duration, muted, speaker)
                }
            }
            is CallState.Ended -> CallUi(CallPhase.Ended, titleOf(state.channelId), endedText(state.reason, state.removed))
        }
    }

    @StringRes
    fun endedText(reason: CallEndReason, removed: VoiceRemovedReason?): Int = when (reason) {
        CallEndReason.Cancelled -> R.string.voice_call_ended_cancelled
        CallEndReason.Declined -> R.string.voice_call_ended_declined
        CallEndReason.NoAnswer -> R.string.voice_call_ended_no_answer
        CallEndReason.Ended, CallEndReason.Left -> R.string.voice_call_ended_default
        CallEndReason.StartFailed -> R.string.voice_call_start_failed
        CallEndReason.Blocked -> R.string.voice_call_blocked
        CallEndReason.Failed -> R.string.voice_call_failed
        CallEndReason.Gone -> R.string.voice_call_gone
        CallEndReason.ConnectionLost -> R.string.voice_call_cut_off
        CallEndReason.Removed ->
            if (removed == VoiceRemovedReason.SessionReplaced) R.string.voice_call_taken_elsewhere else R.string.voice_call_removed
    }

    /**
     * Where "Speaker" sends the sound: on → the loudspeaker; off → a connected
     * headset (Bluetooth first, as the voice layer prefers), else the earpiece.
     */
    fun speakerTarget(on: Boolean, available: Set<AudioRoute>): AudioRoute = when {
        on -> AudioRoute.Speaker
        AudioRoute.Bluetooth in available -> AudioRoute.Bluetooth
        AudioRoute.WiredHeadset in available -> AudioRoute.WiredHeadset
        else -> AudioRoute.Earpiece
    }

    /** "m:ss", or "h:mm:ss" from one hour on (digits only, no words to translate). */
    fun formatDuration(secs: Long): String {
        val h = secs / 3600
        val m = (secs % 3600) / 60
        val s = secs % 60
        return if (h > 0) "%d:%02d:%02d".format(h, m, s) else "%d:%02d".format(m, s)
    }
}

/** A state in which the call has (or is getting) a voice session of its own. */
private fun CallState.usesVoice(): Boolean =
    this is CallState.Outgoing || this is CallState.Connecting || this is CallState.Connected
