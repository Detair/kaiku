package io.wolftown.kaiku.data.keydevice

import java.time.Duration
import javax.inject.Inject

/**
 * What the phone notes, with the key record, about its server not answering: so that "the server
 * cannot be asked" is never one failed request (no connection at one start, a server that is
 * restarting), but something that has lasted.
 *
 * A question counts as not answered when nothing came back that the server itself said: no
 * connection, a timeout, or a page that is not the server's (no error code of its own). Any answer
 * of the server itself, also a refusal or "too many requests", takes all notes back ([noteAnswered]).
 *
 * Failed questions less than [OCCASION_GAP] apart are one occasion. The silence is [lasting] once
 * there were [OCCASIONS] separate occasions and at least [LASTING] lies between the first and the
 * latest of them. All by the phone's running time ([BootClock]), which setting the clock does not
 * change and a reboot never lengthens; the notes are written through [KeyDeviceRepository.update].
 */
@javax.inject.Singleton
class ServerSilence @Inject constructor(
    private val keyDevices: KeyDeviceRepository,
    val clock: BootClock,
) {
    companion object {
        /** Failed questions closer together than this are one occasion. */
        val OCCASION_GAP: Duration = Duration.ofHours(1)

        /** How many separate occasions the silence must have been met on. */
        const val OCCASIONS = 3

        /** How long the running time from the first of them to the latest must be. */
        val LASTING: Duration = Duration.ofHours(24)
    }

    /**
     * The server answered, but taking the notes back did not reach storage: until it does, the
     * stored notes are about questions the server has since answered, and never count as lasting.
     */
    @Volatile
    private var answeredUnsaved = false

    private fun after(u: Unanswered?): Unanswered {
        if (u == null) return Unanswered(clock.now(), 1, Duration.ZERO)
        val gap = clock.since(u.last)
        return if (gap >= OCCASION_GAP) {
            Unanswered(clock.now(), (u.occasions + 1).coerceAtMost(1_000_000), u.span.plus(gap))
        } else {
            u
        }
    }

    /** A question about [record] got no answer. Returns the record as stored afterwards. */
    fun noteUnanswered(record: StoredKeyDevice): StoredKeyDevice {
        val stored = keyDevices.record.value?.takeIf { it sameAs record } ?: return record
        // After an answer whose reset was not stored, the count starts anew from this question.
        val reset = answeredUnsaved
        val next = after(if (reset) null else stored.unanswered)
        if (next == stored.unanswered) return stored
        val done = keyDevices.update(stored) { it.copy(unanswered = next) } as? Change.Done
        if (done != null && reset) answeredUnsaved = false
        return done?.record ?: stored
    }

    /** The server of [record] answered: nothing is noted any more. Returns the record as stored afterwards. */
    fun noteAnswered(record: StoredKeyDevice): StoredKeyDevice {
        val stored = keyDevices.record.value?.takeIf { it sameAs record } ?: return record
        if (stored.unanswered == null) {
            answeredUnsaved = false
            return stored
        }
        val done = keyDevices.update(stored) { it.copy(unanswered = null) } as? Change.Done
        answeredUnsaved = done == null
        return done?.record ?: stored
    }

    /** The server of [record] has stayed silent on [OCCASIONS] separate occasions over at least [LASTING]. */
    fun lasting(record: StoredKeyDevice): Boolean {
        if (answeredUnsaved) return false
        val u = record.unanswered ?: return false
        return u.occasions >= OCCASIONS && u.span >= LASTING
    }
}
