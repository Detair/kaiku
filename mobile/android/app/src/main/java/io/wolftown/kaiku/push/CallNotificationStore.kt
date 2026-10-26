package io.wolftown.kaiku.push

import android.content.SharedPreferences

/** Where the current call of a channel stands. */
enum class CallPhase {
    /** Accepted ringing wake (its notification may or may not be posted yet). */
    RINGING,

    /** Shown as a missed call. */
    MISSED,

    /** Ended without a missed call (declined, or everyone left): a short-lived tombstone. */
    ENDED,
}

/**
 * The current call of a channel: [atMs] is the receive time of the ringing
 * wake ([CallPhase.RINGING]) or the time it was missed or ended (wall clock).
 */
data class ShownCall(val callId: String, val phase: CallPhase, val atMs: Long = 0L)

/**
 * Per account and DM channel, the current call (ids and times only):
 * recorded when its ringing wake is accepted ([CallWakeIntake]), before any
 * notification is posted, so a fast `ended` still finds it. An `ended` wake
 * acts only on that call (or on none stored at all, see [CallRules]).
 */
interface CallNotificationStore {
    /** The current call, or null; an expired [CallPhase.ENDED] tombstone reads as null. */
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
    private val now: () -> Long = System::currentTimeMillis,
) : CallNotificationStore {

    companion object {
        /** A tombstone outlives the ended push's 1 h TTL, so no late duplicate re-rings. */
        const val TOMBSTONE_TTL_MS = 60 * 60 * 1000L

        private const val CALL_ID = "call_id"
        private const val PHASE = "call_phase"
        private const val AT = "call_at"
    }

    private fun key(userId: String, channelId: String, field: String) = "$userId|$channelId|$field"

    @Synchronized
    override fun shown(userId: String, channelId: String): ShownCall? {
        val callId = prefs.getString(key(userId, channelId, CALL_ID), null) ?: return null
        val phaseName = prefs.getString(key(userId, channelId, PHASE), null)
        val phase = CallPhase.entries.firstOrNull { it.name == phaseName } ?: return null
        val at = prefs.getString(key(userId, channelId, AT), null)?.toLongOrNull() ?: 0L
        if (phase == CallPhase.ENDED && now() - at > TOMBSTONE_TTL_MS) return null
        return ShownCall(callId, phase, at)
    }

    @Synchronized
    override fun setShown(userId: String, channelId: String, call: ShownCall) {
        prefs.edit()
            .putString(key(userId, channelId, CALL_ID), call.callId)
            .putString(key(userId, channelId, PHASE), call.phase.name)
            .putString(key(userId, channelId, AT), call.atMs.toString())
            .apply()
    }

    @Synchronized
    override fun clear(userId: String, channelId: String) {
        prefs.edit()
            .remove(key(userId, channelId, CALL_ID))
            .remove(key(userId, channelId, PHASE))
            .remove(key(userId, channelId, AT))
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
}

/** Which call wakes still apply to the stored call of a channel (shared by intake and work). */
internal object CallRules {
    /**
     * A ringing applies to a new call, or repeats the current one while it is
     * still ringing; a call already missed or ended never rings again.
     */
    fun ringingApplies(stored: ShownCall?, call: CallInfo): Boolean =
        stored == null || stored.callId != call.callId || stored.phase == CallPhase.RINGING

    /**
     * An ended with a known reason applies to the ringing call it names, or
     * when no call is stored at all (the phone missed the ringing). Another
     * stored call, a missed call or a tombstone make it ignored.
     */
    fun endedApplies(stored: ShownCall?, call: CallInfo): Boolean {
        if (call.reason == null || call.reason == CallEndReason.UNKNOWN) return false
        return stored == null || (stored.callId == call.callId && stored.phase == CallPhase.RINGING)
    }
}
