package io.wolftown.kaiku.push

import android.content.SharedPreferences

/** Where the current call of a channel stands. */
enum class CallPhase {
    /** Accepted ringing wake (its notification may or may not be posted yet). */
    RINGING,

    /** Accepted ended wake whose work has not finished yet; no ringing of this call is accepted any more. */
    ENDING,

    /** Shown as a missed call. */
    MISSED,

    /** Ended without a missed call (declined, or everyone left): a short-lived tombstone. */
    ENDED,
}

/**
 * The current call of a channel: [atMs] is the receive time of the ringing
 * ([CallPhase.RINGING]) or ended ([CallPhase.ENDING]) wake, or the time it was
 * missed or ended (wall clock).
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

    /**
     * Compare-and-set: writes [call] only if [shown] still returns [expected]
     * (null: nothing stored), atomically with the other writes. Returns whether it wrote.
     */
    fun setShownIf(userId: String, channelId: String, expected: ShownCall?, call: ShownCall): Boolean

    fun clear(userId: String, channelId: String)

    /** Removes the entry only if [shown] still returns [expected]. Returns whether it removed it. */
    fun clearIf(userId: String, channelId: String, expected: ShownCall): Boolean

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
    override fun setShownIf(userId: String, channelId: String, expected: ShownCall?, call: ShownCall): Boolean {
        if (shown(userId, channelId) != expected) return false
        setShown(userId, channelId, call)
        return true
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
    override fun clearIf(userId: String, channelId: String, expected: ShownCall): Boolean {
        if (shown(userId, channelId) != expected) return false
        clear(userId, channelId)
        return true
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
     * A stored [CallPhase.RINGING] or [CallPhase.ENDING] older than this counts
     * as absent: a call whose `ended` never came must not block the next one.
     */
    const val TRANSIENT_MAX_AGE_MS = 2 * 60 * 1000L

    /** [stored], or null when it is a stale ringing or ending entry. */
    fun live(stored: ShownCall?, nowMs: Long): ShownCall? = stored?.takeUnless {
        (it.phase == CallPhase.RINGING || it.phase == CallPhase.ENDING) && nowMs - it.atMs > TRANSIENT_MAX_AGE_MS
    }

    /**
     * A ringing applies to a new call, or repeats the current one while it is
     * still ringing; a call that is ending, missed or ended never rings again.
     */
    fun ringingApplies(stored: ShownCall?, call: CallInfo, nowMs: Long): Boolean {
        val current = live(stored, nowMs)
        return current == null || current.callId != call.callId || current.phase == CallPhase.RINGING
    }

    /**
     * An ended with a known reason applies to the ringing (or already ending)
     * call it names, or when no call is stored at all (the phone missed the
     * ringing). Another stored call, a missed call or a tombstone make it ignored.
     */
    fun endedApplies(stored: ShownCall?, call: CallInfo, nowMs: Long): Boolean {
        if (call.reason == null || call.reason == CallEndReason.UNKNOWN) return false
        val current = live(stored, nowMs) ?: return true
        return current.callId == call.callId &&
            (current.phase == CallPhase.RINGING || current.phase == CallPhase.ENDING)
    }
}
