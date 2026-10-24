package io.wolftown.kaiku.push

import android.content.SharedPreferences
import java.time.Instant
import java.time.format.DateTimeParseException

/**
 * Remembers, per account and channel, the newest message the user has been
 * notified about or has seen, so the next wake fetches only what came after
 * it. Stores ids and timestamps only, never message text.
 *
 * Every call names the account ([userId]) explicitly: a caller binds its
 * reads and writes to the session it started from, never to whoever is
 * signed in by the time the write happens.
 */
interface NotificationLedger {
    fun lastSeen(userId: String, channelId: String): String?

    /** `created_at` of [lastSeen] when known. */
    fun lastSeenCreatedAt(userId: String, channelId: String): String?

    /**
     * Records [messageId] as seen. The position only moves forward: with a
     * [createdAt] an older position than the stored one is ignored, and
     * without one a stored timestamped position is never replaced (its order
     * is unknown, and message ids are random, not time-ordered).
     */
    fun markSeen(userId: String, channelId: String, messageId: String, createdAt: String?)

    fun clear(userId: String, channelId: String)
}

/** Ordering of message positions by `(created_at, id)`, the server's catch-up order. */
internal object MessagePosition {
    fun parse(createdAt: String?): Instant? =
        try {
            createdAt?.let(Instant::parse)
        } catch (_: DateTimeParseException) {
            null
        }

    /** True if `(at, id)` is strictly after `(refAt, refId)`; null when a timestamp is missing or unparseable. */
    fun isAfter(at: String?, id: String, refAt: String?, refId: String): Boolean? {
        val a = parse(at) ?: return null
        val b = parse(refAt) ?: return null
        val cmp = a.compareTo(b)
        return if (cmp != 0) cmp > 0 else id > refId
    }
}

/** [NotificationLedger] in plain SharedPreferences (`notif_ledger`), keyed by account and channel. */
class PrefsNotificationLedger(
    private val prefs: SharedPreferences,
) : NotificationLedger {

    private fun idKey(userId: String, channelId: String) = "$userId|$channelId|id"
    private fun atKey(userId: String, channelId: String) = "$userId|$channelId|at"

    override fun lastSeen(userId: String, channelId: String): String? =
        prefs.getString(idKey(userId, channelId), null)

    override fun lastSeenCreatedAt(userId: String, channelId: String): String? =
        prefs.getString(atKey(userId, channelId), null)

    @Synchronized
    override fun markSeen(userId: String, channelId: String, messageId: String, createdAt: String?) {
        if (!shouldReplace(userId, channelId, messageId, createdAt)) return

        val newAt = createdAt?.takeIf { MessagePosition.parse(it) != null }
        val editor = prefs.edit()
        editor.putString(idKey(userId, channelId), messageId)
        if (newAt != null) {
            editor.putString(atKey(userId, channelId), newAt)
        } else {
            editor.remove(atKey(userId, channelId))
        }
        editor.apply()
    }

    private fun shouldReplace(userId: String, channelId: String, messageId: String, createdAt: String?): Boolean {
        val storedId = lastSeen(userId, channelId) ?: return true
        val storedAt = lastSeenCreatedAt(userId, channelId)
        val storedKnown = MessagePosition.parse(storedAt) != null
        val newKnown = MessagePosition.parse(createdAt) != null
        return when {
            // Same message: only worth writing to add a missing timestamp.
            storedId == messageId -> newKnown && !storedKnown
            // Never trade a known position for one of unknown order.
            !newKnown -> !storedKnown
            !storedKnown -> true
            else -> MessagePosition.isAfter(createdAt, messageId, storedAt, storedId) == true
        }
    }

    @Synchronized
    override fun clear(userId: String, channelId: String) {
        val editor = prefs.edit()
        editor.remove(idKey(userId, channelId))
        editor.remove(atKey(userId, channelId))
        editor.apply()
    }
}
