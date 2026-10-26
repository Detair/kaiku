package io.wolftown.kaiku.push

import android.content.SharedPreferences

/** The call a channel's call notification currently stands for. */
data class ShownCall(val callId: String, val missed: Boolean)

/**
 * Per account and DM channel, the current call (ids only): recorded when its
 * ringing wake is accepted ([CallWakeIntake]), before any notification is
 * posted, so a fast `ended` still finds it. An `ended` wake acts only on that call.
 */
interface CallNotificationStore {
    fun shown(userId: String, channelId: String): ShownCall?

    fun setShown(userId: String, channelId: String, call: ShownCall)

    fun clear(userId: String, channelId: String)

    /** Forgets every call of [userId] (logout, account change). */
    fun clearUser(userId: String)

    /** Forgets every call of every account (no session stored at all). */
    fun clearAll()
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

    @Synchronized
    override fun clearUser(userId: String) {
        val prefix = "$userId|"
        val editor = prefs.edit()
        prefs.all.keys.filter { it.startsWith(prefix) }.forEach { editor.remove(it) }
        editor.apply()
    }

    @Synchronized
    override fun clearAll() {
        prefs.edit().clear().apply()
    }

    private companion object {
        const val CALL_ID = "call_id"
        const val PHASE = "call_phase"
        const val PHASE_RINGING = "ringing"
        const val PHASE_MISSED = "missed"
    }
}
