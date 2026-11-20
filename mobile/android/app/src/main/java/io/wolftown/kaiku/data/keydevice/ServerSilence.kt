package io.wolftown.kaiku.data.keydevice

import java.time.Duration
import java.time.Instant
import javax.inject.Inject

/**
 * What the phone notes, with the key record, about its server not answering: so that "the server
 * cannot be asked" is never one failed request (no connection at one start, a server that is
 * restarting), but something that has lasted.
 *
 * A question counts as not answered when nothing came back that the server itself said: no
 * connection, a timeout, a server fault (5xx), "too many requests", a page that is not the
 * server's, or "no key device" without the server's time for a claim that is counted by it. Any
 * answer of the server itself, also a refusal, takes all notes back ([noteAnswered]).
 *
 * Failed questions less than [OCCASION_GAP] apart are one occasion. The silence is [lasting] once
 * there were [OCCASIONS] separate occasions and the latest of them is at least [LASTING] after the
 * first. All by the phone's own clock; the notes are written through [KeyDeviceRepository.update].
 */
class ServerSilence internal constructor(
    private val keyDevices: KeyDeviceRepository,
    private val clock: () -> Instant,
) {
    @Inject
    constructor(keyDevices: KeyDeviceRepository) : this(keyDevices, Instant::now)

    companion object {
        /** Failed questions closer together than this are one occasion. */
        val OCCASION_GAP: Duration = Duration.ofHours(1)

        /** How many separate occasions the silence must have been met on. */
        const val OCCASIONS = 3

        /** How long after the first of them the latest must be. */
        val LASTING: Duration = Duration.ofHours(24)
    }

    fun now(): Instant = clock()

    private fun after(u: Unanswered?, at: Instant): Unanswered = when {
        // Nothing noted yet, or the phone's clock was put back behind the first note: counted from now.
        u == null || at.isBefore(u.since) -> Unanswered(at, at, 1)
        Duration.between(u.last, at) >= OCCASION_GAP -> u.copy(last = at, occasions = (u.occasions + 1).coerceAtMost(1_000_000))
        else -> u
    }

    /** A question about [record] got no answer. Returns the record as stored afterwards. */
    fun noteUnanswered(record: StoredKeyDevice): StoredKeyDevice {
        val stored = keyDevices.record.value?.takeIf { it sameAs record } ?: return record
        val at = clock()
        if (after(stored.unanswered, at) == stored.unanswered) return stored
        return (keyDevices.update(stored) { it.copy(unanswered = after(it.unanswered, at)) } as? Change.Done)?.record ?: stored
    }

    /** The server of [record] answered: nothing is noted any more. Returns the record as stored afterwards. */
    fun noteAnswered(record: StoredKeyDevice): StoredKeyDevice {
        val stored = keyDevices.record.value?.takeIf { it sameAs record } ?: return record
        if (stored.unanswered == null) return stored
        return (keyDevices.update(stored) { it.copy(unanswered = null) } as? Change.Done)?.record ?: stored
    }

    /** The server of [record] has stayed silent on [OCCASIONS] separate occasions over at least [LASTING]. */
    fun lasting(record: StoredKeyDevice): Boolean {
        val u = record.unanswered ?: return false
        return u.occasions >= OCCASIONS && Duration.between(u.since, u.last) >= LASTING
    }
}
