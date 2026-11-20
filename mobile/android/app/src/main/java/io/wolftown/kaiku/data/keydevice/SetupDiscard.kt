package io.wolftown.kaiku.data.keydevice

import java.time.Duration
import java.time.Instant
import javax.inject.Inject

/**
 * The way out of a setup that was never finished and whose server cannot be asked any more (it is
 * gone, cannot be reached from this phone, or its answers carry no time): the person may discard it.
 * This is the person's decision, not a server answer, so it is offered only when the setup's own
 * time is long past by the phone's clock, only for a record that is not an active key, and the
 * screen asks before it happens. If the setup did finish on the server, the server then lists a key
 * device this phone no longer has; an admin removes it there.
 */
class SetupDiscard @Inject constructor(private val keyDevices: KeyDeviceRepository) {

    companion object {
        /** A claim lives 20 minutes at most (its challenge, then its code); discarding is offered well after. */
        val CLAIM_AFTER: Duration = Duration.ofMinutes(90)

        /** A waiting key device is ready at its time; discarding is offered a day after that. */
        val PENDING_AFTER: Duration = Duration.ofHours(24)
    }

    /** The phone's own time after which [record] may be discarded; null: not at all, or no time is known yet. */
    fun due(record: StoredKeyDevice): Instant? = when (record.phase) {
        KeyDevicePhase.ACTIVE -> null
        KeyDevicePhase.PENDING -> (record.readyAt ?: record.since)?.plus(PENDING_AFTER)
        KeyDevicePhase.CLAIMING, KeyDevicePhase.CLAIMED -> record.since?.plus(CLAIM_AFTER)
    }

    fun offered(record: StoredKeyDevice, now: Instant): Boolean = due(record)?.let(now::isAfter) == true

    /**
     * For a record that carries no time of its own (written by an earlier version): the moment its
     * server first could not be asked is noted, and the time until discarding counts from there.
     * Returns the record as stored afterwards.
     */
    fun noteCouldNotAsk(record: StoredKeyDevice, now: Instant): StoredKeyDevice {
        if (record.phase == KeyDevicePhase.ACTIVE || due(record) != null) return record
        return (keyDevices.update(record) { if (it.since == null) it.copy(since = now) else it } as? Change.Done)?.record ?: record
    }

    /** Forgets [expected] and its key if it may be discarded now and is still the stored record. */
    fun discard(expected: StoredKeyDevice, now: Instant): Change =
        if (!offered(expected, now)) Change.Mismatch else keyDevices.forget(expected, Forget.DISCARDED_BY_PERSON)
}
