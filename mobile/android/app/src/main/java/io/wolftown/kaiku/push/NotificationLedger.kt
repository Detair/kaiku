package io.wolftown.kaiku.push

import android.content.SharedPreferences
import java.time.Instant
import java.time.format.DateTimeParseException

/**
 * Remembers, per channel, the newest message the user has been notified about
 * or has seen, so the next wake fetches only what came after it. Stores ids and
 * timestamps only, never message text.
 */
interface NotificationLedger {
    fun lastSeen(channelId: String): String?

    /** `created_at` of [lastSeen] when known (lets the latest-page fallback skip old messages). */
    fun lastSeenCreatedAt(channelId: String): String? = null

    /**
     * Records [messageId] as seen. With [createdAt] the entry only moves forward:
     * an older position than the stored one is ignored.
     */
    fun markSeen(channelId: String, messageId: String, createdAt: String? = null)

    fun clear(channelId: String)
}

/** Ordering of message positions by `(created_at, id)`, the server's catch-up order. */
internal object MessagePosition {
    fun parse(createdAt: String?): Instant? =
        try {
            createdAt?.let(Instant::parse)
        } catch (_: DateTimeParseException) {
            null
        }

    /** True if `(at, id)` is strictly after `(refAt, refId)`; null when a timestamp is unparseable. */
    fun isAfter(at: String?, id: String, refAt: String?, refId: String): Boolean? {
        val a = parse(at) ?: return null
        val b = parse(refAt) ?: return null
        val cmp = a.compareTo(b)
        return if (cmp != 0) cmp > 0 else id > refId
    }
}

/**
 * [NotificationLedger] in plain SharedPreferences (`notif_ledger`). Entries are
 * scoped to the signed-in user ([currentUserId]), so a different account on the
 * same phone never inherits another account's positions.
 */
class PrefsNotificationLedger(
    private val prefs: SharedPreferences,
    private val currentUserId: () -> String?,
) : NotificationLedger {

    private fun idKey(channelId: String) = "${currentUserId().orEmpty()}|$channelId|id"
    private fun atKey(channelId: String) = "${currentUserId().orEmpty()}|$channelId|at"

    override fun lastSeen(channelId: String): String? = prefs.getString(idKey(channelId), null)

    override fun lastSeenCreatedAt(channelId: String): String? = prefs.getString(atKey(channelId), null)

    @Synchronized
    override fun markSeen(channelId: String, messageId: String, createdAt: String?) {
        val storedId = lastSeen(channelId)
        if (storedId == messageId && (createdAt == null || lastSeenCreatedAt(channelId) != null)) return
        if (createdAt != null && storedId != null) {
            val storedAt = lastSeenCreatedAt(channelId)
            if (MessagePosition.isAfter(createdAt, messageId, storedAt, storedId) == false) return
        }
        prefs.edit()
            .putString(idKey(channelId), messageId)
            .apply { if (createdAt != null) putString(atKey(channelId), createdAt) else remove(atKey(channelId)) }
            .apply()
    }

    @Synchronized
    override fun clear(channelId: String) {
        prefs.edit().remove(idKey(channelId)).remove(atKey(channelId)).apply()
    }
}
