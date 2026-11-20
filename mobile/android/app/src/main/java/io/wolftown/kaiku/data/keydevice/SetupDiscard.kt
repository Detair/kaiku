package io.wolftown.kaiku.data.keydevice

import java.time.Duration
import javax.inject.Inject

/**
 * The one case in which the person may delete a key the phone made for a setup: a claim that needs
 * a code and never got its answer (the record is still `CLAIMING`, so no code of it was ever
 * shown). Nothing can come of such a claim, whatever the server holds. A record whose code was
 * shown, a waiting key device and an active key are never discarded: the server may have made, or
 * may still make, such a key a key device.
 *
 * Even for that one case this is the person's decision, not a server answer, and the last step:
 * it is offered only when the claim's own time is long past by the phone's clock and the server
 * has stayed silent for long ([ServerSilence.lasting]: several separate occasions over at least a
 * day, never a single failed request), and the screen asks before it happens.
 */
class SetupDiscard @Inject constructor(private val keyDevices: KeyDeviceRepository, val silence: ServerSilence) {

    companion object {
        /** A claim lives 20 minutes at most (its challenge, then its code); "long past" is well after. */
        val CLAIM_AFTER: Duration = Duration.ofMinutes(90)
    }

    /** Whether [record] is of the one kind that can be discarded at all. */
    fun canNeverFinish(record: StoredKeyDevice): Boolean = record.phase == KeyDevicePhase.CLAIMING && record.codeClaim

    /** Whether discarding [record] is offered now. */
    fun offered(record: StoredKeyDevice): Boolean = canNeverFinish(record) &&
        record.since?.plus(CLAIM_AFTER)?.let(silence.now()::isAfter) == true &&
        silence.lasting(record)

    /**
     * Forgets [expected] and its key if it is still the stored record and discarding it is offered
     * now, by what is stored (the notes about its server are read from storage, not from [expected]).
     */
    fun discard(expected: StoredKeyDevice): Change {
        val stored = keyDevices.record.value?.takeIf { it sameAs expected }
            ?: return if (keyDevices.unreadable()) Change.Unreadable else Change.Mismatch
        return if (!offered(stored)) Change.Mismatch else keyDevices.forget(stored, Forget.DISCARDED_BY_PERSON)
    }
}
