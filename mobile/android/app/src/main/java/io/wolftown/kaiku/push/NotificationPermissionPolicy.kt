package io.wolftown.kaiku.push

enum class NotificationPermissionDecision { GRANTED, ASK_WITH_RATIONALE, OPEN_SETTINGS }

/** Decides how to get notification permission; pure so it can be unit tested. */
object NotificationPermissionPolicy {
    private const val TIRAMISU = 33

    fun decide(
        sdkInt: Int,
        notificationsEnabled: Boolean,
        askedBefore: Boolean,
        shouldShowRationale: Boolean,
    ): NotificationPermissionDecision = when {
        notificationsEnabled -> NotificationPermissionDecision.GRANTED
        // Before Android 13 there is no runtime permission to request.
        sdkInt < TIRAMISU -> NotificationPermissionDecision.OPEN_SETTINGS
        !askedBefore || shouldShowRationale -> NotificationPermissionDecision.ASK_WITH_RATIONALE
        else -> NotificationPermissionDecision.OPEN_SETTINGS
    }
}
