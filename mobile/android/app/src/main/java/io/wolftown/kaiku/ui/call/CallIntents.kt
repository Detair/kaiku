package io.wolftown.kaiku.ui.call

import android.content.Context
import android.content.Intent

/**
 * The intents of the call screen ([CallActivity]) and of the Decline action
 * ([CallActionReceiver]). A ring known only from its notification (the app
 * was not running) travels in the extras; [parse] reads them back.
 */
object CallIntents {
    const val EXTRA_USER_ID = "io.wolftown.kaiku.call.USER_ID"
    const val EXTRA_CHANNEL_ID = "io.wolftown.kaiku.call.CHANNEL_ID"
    const val EXTRA_CALL_ID = "io.wolftown.kaiku.call.CALL_ID"
    const val EXTRA_CALLER_ID = "io.wolftown.kaiku.call.CALLER_ID"
    const val EXTRA_CALLER_NAME = "io.wolftown.kaiku.call.CALLER_NAME"
    const val EXTRA_ACTION = "io.wolftown.kaiku.call.ACTION"

    /** Open the screen (full-screen intent, a tap). */
    const val ACTION_SHOW = "show"

    /** Open the screen and answer (the Accept action: an activity, see [CallActivity]). */
    const val ACTION_ACCEPT = "accept"

    /** Broadcast action of Decline. */
    const val ACTION_DECLINE = "io.wolftown.kaiku.call.DECLINE"

    /** A ring as a notification carries it, for the account [userId]. */
    data class Ring(
        val userId: String,
        val channelId: String,
        val callId: String,
        val callerId: String,
        val callerName: String?,
    )

    data class Parsed(val ring: Ring?, val action: String)

    /** Reads what [screen] or [decline] put in; [get] is the intent's string extra. */
    fun parse(get: (String) -> String?): Parsed {
        val ring = run {
            val userId = get(EXTRA_USER_ID) ?: return@run null
            val channelId = get(EXTRA_CHANNEL_ID) ?: return@run null
            val callId = get(EXTRA_CALL_ID) ?: return@run null
            val callerId = get(EXTRA_CALLER_ID) ?: return@run null
            Ring(userId, channelId, callId, callerId, get(EXTRA_CALLER_NAME)?.takeIf { it.isNotBlank() })
        }
        return Parsed(ring, get(EXTRA_ACTION) ?: ACTION_SHOW)
    }

    /** The call screen, optionally for [ring] and with [action]. */
    fun screen(context: Context, ring: Ring? = null, action: String = ACTION_SHOW): Intent =
        Intent(context, CallActivity::class.java)
            .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
            .putExtra(EXTRA_ACTION, action)
            .putRing(ring)

    /** The Decline action of [ring]'s notification. */
    fun decline(context: Context, ring: Ring): Intent =
        Intent(context, CallActionReceiver::class.java)
            .setAction(ACTION_DECLINE)
            .putRing(ring)

    private fun Intent.putRing(ring: Ring?): Intent {
        if (ring == null) return this
        return putExtra(EXTRA_USER_ID, ring.userId)
            .putExtra(EXTRA_CHANNEL_ID, ring.channelId)
            .putExtra(EXTRA_CALL_ID, ring.callId)
            .putExtra(EXTRA_CALLER_ID, ring.callerId)
            .putExtra(EXTRA_CALLER_NAME, ring.callerName)
    }
}

/** How an incoming call is presented. */
enum class IncomingCallStyle {
    /**
     * `CallStyle` with a full-screen intent: over the lock screen and, when
     * the screen is off, full-screen with the ringtone.
     */
    CallStyleFullScreen,

    /**
     * A high-priority heads-up (category CALL) with Accept and Decline as
     * actions. `CallStyle` is not used here: the system accepts it only with
     * a full-screen intent or from a foreground service.
     */
    HeadsUpWithActions,
}

object IncomingCallPresentation {
    /** API 34: the user may take the full-screen permission away. */
    private const val API_FULL_SCREEN_CHECK = 34

    /**
     * Whether full-screen intents may be used: below API 34 the permission
     * is granted at install time; from API 34 `NotificationManager.canUseFullScreenIntent()`
     * says ([check]).
     */
    fun canUseFullScreenIntent(sdkInt: Int, check: () -> Boolean): Boolean =
        sdkInt < API_FULL_SCREEN_CHECK || check()

    fun choose(canUseFullScreenIntent: Boolean): IncomingCallStyle =
        if (canUseFullScreenIntent) IncomingCallStyle.CallStyleFullScreen else IncomingCallStyle.HeadsUpWithActions
}
