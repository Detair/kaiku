package io.wolftown.kaiku.ui.voice

import android.Manifest
import android.os.Build

/** What the voice screen should do next, given the current microphone permission state. */
sealed interface VoicePermissionDecision {
    /** Microphone access is granted: join the channel. */
    data object Join : VoicePermissionDecision

    /** Ask the system for the runtime permissions. */
    data object Request : VoicePermissionDecision

    /** Explain why the microphone is needed, then request again. */
    data object ShowRationale : VoicePermissionDecision

    /** Permanently denied: only the app settings screen can grant it now. */
    data object OpenSettings : VoicePermissionDecision
}

/** Runtime permission rules for joining a voice channel. */
object VoicePermissions {

    /**
     * Permissions to request in one batch for [sdkInt].
     *
     * Only RECORD_AUDIO gates joining. Bluetooth (API 31+) and notifications
     * (API 33+) are optional: voice still works if the user declines them.
     */
    fun toRequest(sdkInt: Int): List<String> = buildList {
        add(Manifest.permission.RECORD_AUDIO)
        if (sdkInt >= Build.VERSION_CODES.S) add(Manifest.permission.BLUETOOTH_CONNECT)
        if (sdkInt >= Build.VERSION_CODES.TIRAMISU) add(Manifest.permission.POST_NOTIFICATIONS)
    }

    fun decide(
        micGranted: Boolean,
        shouldShowRationale: Boolean,
        askedBefore: Boolean
    ): VoicePermissionDecision = when {
        micGranted -> VoicePermissionDecision.Join
        shouldShowRationale -> VoicePermissionDecision.ShowRationale
        askedBefore -> VoicePermissionDecision.OpenSettings
        else -> VoicePermissionDecision.Request
    }
}
