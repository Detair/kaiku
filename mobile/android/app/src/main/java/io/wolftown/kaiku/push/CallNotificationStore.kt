package io.wolftown.kaiku.push

import android.content.SharedPreferences

/** The call a channel's call notification currently stands for. */
data class ShownCall(val callId: String, val missed: Boolean)

/**
 * Per account and DM channel, the call id of the call notification last
 * posted there (ids only). An `ended` wake acts only on that call.
 */
interface CallNotificationStore {
    fun shown(userId: String, channelId: String): ShownCall?

    fun setShown(userId: String, channelId: String, call: ShownCall)

    fun clear(userId: String, channelId: String)
}

/** [CallNotificationStore] in plain SharedPreferences (`notif_calls`), keyed like [PrefsNotificationLedger]. */
class PrefsCallNotificationStore(
    private val prefs: SharedPreferences,
) : CallNotificationStore {

    private fun key(userId: String, channelId: String, field: String) = "$userId|$channelId|$field"

    @Synchronized
    override fun shown(userId: String, channelId: String): ShownCall? {
        val callId = prefs.getString(key(userId, channelId, CALL_ID), null) ?: return null
        val missed = prefs.getString(key(userId, channelId, PHASE), null) == PHASE_MISSED
        return ShownCall(callId, missed)
    }

    @Synchronized
    override fun setShown(userId: String, channelId: String, call: ShownCall) {
        prefs.edit()
            .putString(key(userId, channelId, CALL_ID), call.callId)
            .putString(key(userId, channelId, PHASE), if (call.missed) PHASE_MISSED else PHASE_RINGING)
            .apply()
    }

    @Synchronized
    override fun clear(userId: String, channelId: String) {
        prefs.edit()
            .remove(key(userId, channelId, CALL_ID))
            .remove(key(userId, channelId, PHASE))
            .apply()
    }

    private companion object {
        const val CALL_ID = "call_id"
        const val PHASE = "call_phase"
        const val PHASE_RINGING = "ringing"
        const val PHASE_MISSED = "missed"
    }
}
