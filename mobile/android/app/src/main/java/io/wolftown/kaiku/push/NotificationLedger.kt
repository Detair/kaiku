package io.wolftown.kaiku.push

import android.content.SharedPreferences
import io.wolftown.kaiku.domain.model.Message
import java.time.Instant
import java.time.format.DateTimeParseException

/**
 * Per account and channel, two persisted positions (ids and timestamps only,
 * never message text):
 *
 * - the **notified/seen watermark**: the newest message the user was notified
 *   about or saw here; the next wake fetches only what came after it;
 * - the **read cursor** ([ReadCursor]): how far the channel was read, also on
 *   other devices, as far as known here; an extra local filter next to the
 *   server's unread count.
 *
 * Every call names the account ([userId]) explicitly: a caller binds its
 * reads and writes to the session it started from.
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

    /**
     * Records a read up to [messageId] (from a read-sync event). With its
     * [createdAt] the read is exact and only moves forward; without one only
     * the id is kept, as a boundary for the next fetch.
     */
    fun markRead(userId: String, channelId: String, messageId: String, createdAt: String?)

    fun readCursor(userId: String, channelId: String): ReadCursor

    fun clear(userId: String, channelId: String)
}

/**
 * How far a channel was read, as known locally. An extra filter only: the
 * server's unread count decides whether anything notifies at all (C20).
 *
 * - [id]/[at]: an exact read position (message id and its `created_at`).
 * - [receivedId]: a read up to a message whose `created_at` is not known
 *   here; it is the boundary when the fetched list contains it, otherwise it
 *   filters nothing. No time-based guesses.
 */
data class ReadCursor(
    val id: String? = null,
    val at: String? = null,
    val receivedId: String? = null,
) {
    /** The messages of [sorted] (oldest first, as fetched) that are not read. */
    fun unread(sorted: List<Message>): List<Message> {
        val boundary = receivedId?.let { rid -> sorted.indexOfFirst { it.id == rid } } ?: -1
        return sorted.filterIndexed { index, m ->
            val exactRead = id != null && MessagePosition.isAtOrBefore(m.createdAt.ifEmpty { null }, m.id, at, id)
            !exactRead && index > boundary
        }
    }

    /** The message of [sorted] that resolves [receivedId], if fetched. */
    fun resolve(sorted: List<Message>): Message? =
        receivedId?.let { rid -> sorted.firstOrNull { it.id == rid && it.createdAt.isNotEmpty() } }
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

    /**
     * True if `(at, id)` is at or before `(refAt, refId)`. Without [refId]
     * everything up to and including [refAt] counts. False when a timestamp
     * is missing or unparseable.
     */
    fun isAtOrBefore(at: String?, id: String, refAt: String?, refId: String?): Boolean {
        val a = parse(at) ?: return false
        val b = parse(refAt) ?: return false
        val cmp = a.compareTo(b)
        return when {
            cmp != 0 -> cmp < 0
            refId == null -> true
            else -> id <= refId
        }
    }
}

/** [NotificationLedger] in plain SharedPreferences (`notif_ledger`), keyed by account and channel. */
class PrefsNotificationLedger(
    private val prefs: SharedPreferences,
) : NotificationLedger {

    private fun key(userId: String, channelId: String, field: String) = "$userId|$channelId|$field"

    private fun get(userId: String, channelId: String, field: String): String? =
        prefs.getString(key(userId, channelId, field), null)

    override fun lastSeen(userId: String, channelId: String): String? = get(userId, channelId, SEEN_ID)

    override fun lastSeenCreatedAt(userId: String, channelId: String): String? = get(userId, channelId, SEEN_AT)

    @Synchronized
    override fun markSeen(userId: String, channelId: String, messageId: String, createdAt: String?) {
        val storedId = lastSeen(userId, channelId)
        val storedAt = lastSeenCreatedAt(userId, channelId)
        if (!advances(storedId, storedAt, messageId, createdAt)) return
        write(userId, channelId, SEEN_ID to messageId, SEEN_AT to createdAt?.takeIf { MessagePosition.parse(it) != null })
    }

    @Synchronized
    override fun markRead(userId: String, channelId: String, messageId: String, createdAt: String?) {
        if (MessagePosition.parse(createdAt) == null) {
            write(userId, channelId, RCV_ID to messageId)
            return
        }
        val storedId = get(userId, channelId, READ_ID)
        val storedAt = get(userId, channelId, READ_AT)
        if (advances(storedId, storedAt, messageId, createdAt)) {
            write(userId, channelId, READ_ID to messageId, READ_AT to createdAt)
        }
        // The id-only read of this very message is now resolved.
        if (get(userId, channelId, RCV_ID) == messageId) write(userId, channelId, RCV_ID to null)
    }

    override fun readCursor(userId: String, channelId: String): ReadCursor = ReadCursor(
        id = get(userId, channelId, READ_ID),
        at = get(userId, channelId, READ_AT),
        receivedId = get(userId, channelId, RCV_ID),
    )

    @Synchronized
    override fun clear(userId: String, channelId: String) {
        val editor = prefs.edit()
        for (field in ALL_FIELDS) editor.remove(key(userId, channelId, field))
        editor.apply()
    }

    /** Forward-only rule for an `(id, created_at)` position. */
    private fun advances(storedId: String?, storedAt: String?, newId: String, newAt: String?): Boolean {
        if (storedId == null) return true
        val storedKnown = MessagePosition.parse(storedAt) != null
        val newKnown = MessagePosition.parse(newAt) != null
        return when {
            // Same message: only worth writing to add a missing timestamp.
            storedId == newId -> newKnown && !storedKnown
            // Never trade a known position for one of unknown order.
            !newKnown -> !storedKnown
            !storedKnown -> true
            else -> MessagePosition.isAfter(newAt, newId, storedAt, storedId) == true
        }
    }

    /** Writes the given fields; a null value removes the field. */
    private fun write(userId: String, channelId: String, vararg fields: Pair<String, String?>) {
        val editor = prefs.edit()
        for ((field, value) in fields) {
            if (value != null) {
                editor.putString(key(userId, channelId, field), value)
            } else {
                editor.remove(key(userId, channelId, field))
            }
        }
        editor.apply()
    }

    private companion object {
        const val SEEN_ID = "id"
        const val SEEN_AT = "at"
        const val READ_ID = "read_id"
        const val READ_AT = "read_at"
        const val RCV_ID = "rcv_id"
        val ALL_FIELDS = listOf(SEEN_ID, SEEN_AT, READ_ID, READ_AT, RCV_ID)
    }
}
