package io.wolftown.kaiku.push

import androidx.annotation.StringRes
import io.wolftown.kaiku.R
import io.wolftown.kaiku.navigation.DeepLinks

/** What a DM call notification shows. */
enum class CallKind {
    /** The call is ringing: a heads-up pointing to the PC. */
    INCOMING,

    /** The call ended unanswered (no answer or cancelled by the caller). */
    MISSED,
}

/**
 * One call notification for account [userId] in DM [channelId]. [callerName]
 * is null when it could not be resolved (the generic text is shown then).
 */
data class CallNotification(
    val userId: String,
    val channelId: String,
    val callId: String,
    val kind: CallKind,
    val callerName: String?,
)

/**
 * Posts and clears DM call notifications. They are keyed by channel, apart
 * from the channel's message notification, so a call never merges into the
 * conversation and a read never clears a call. Logout or an account change
 * removes them with [NotificationSink.cancelAll].
 */
interface CallNotificationSink {
    /**
     * Posts (or replaces) the channel's call notification [prepare] returns,
     * or nothing when it returns null. [prepare] runs in the same critical
     * section as the post and as [NotificationSink.cancelAll], also when
     * notifications are disabled.
     */
    fun showCall(prepare: () -> CallNotification?): ShowResult

    fun cancelCall(channelId: String)
}

/** The localized text of a call notification. */
object CallNotificationText {
    /** String resource and, when it has a `%1$s` placeholder, the caller name. */
    data class Text(@StringRes val res: Int, val name: String?)

    fun of(n: CallNotification): Text {
        val name = n.callerName?.takeIf { it.isNotBlank() }
        return when (n.kind) {
            CallKind.INCOMING ->
                if (name != null) Text(R.string.notifications_call_incoming, name)
                else Text(R.string.notifications_call_incoming_unknown, null)
            CallKind.MISSED ->
                if (name != null) Text(R.string.notifications_call_missed, name)
                else Text(R.string.notifications_call_missed_unknown, null)
        }
    }
}

/**
 * Where tapping a notification leads: the channel deep link, tagged with the
 * account the notification was posted for (so another account never opens it).
 */
data class NotificationTapTarget(val uri: String, val userId: String, val requestCode: Int)

object NotificationTaps {
    fun message(channelId: String, userId: String) =
        NotificationTapTarget(DeepLinks.channelUri(channelId), userId, channelId.hashCode())

    /** Own request code, so the call's PendingIntent never replaces the message one. */
    fun call(n: CallNotification) =
        NotificationTapTarget(DeepLinks.channelUri(n.channelId), n.userId, "call-${n.channelId}".hashCode())
}
