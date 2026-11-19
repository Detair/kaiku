package io.wolftown.kaiku.ui.call

import android.os.PowerManager

/**
 * The proximity wake lock of the call screen: the display goes off while the
 * phone is at the ear and comes back when it is taken away
 * (`PROXIMITY_SCREEN_OFF_WAKE_LOCK`, API 21+, needs `WAKE_LOCK`). Devices
 * without a proximity sensor do nothing. Acquire and release are idempotent.
 */
class ProximityLock(power: PowerManager?) {
    private val lock: PowerManager.WakeLock? = power
        ?.takeIf { it.isWakeLockLevelSupported(PowerManager.PROXIMITY_SCREEN_OFF_WAKE_LOCK) }
        ?.newWakeLock(PowerManager.PROXIMITY_SCREEN_OFF_WAKE_LOCK, "kaiku:call-proximity")
        ?.apply { setReferenceCounted(false) }

    fun acquire() {
        val l = lock ?: return
        // Bounded, in case a release is ever missed; a call screen re-acquires on every change.
        if (!l.isHeld) l.acquire(MAX_HOLD_MS)
    }

    fun release() {
        val l = lock ?: return
        // Wait for the phone to leave the ear before the display comes back on.
        if (l.isHeld) l.release(PowerManager.RELEASE_FLAG_WAIT_FOR_NO_PROXIMITY)
    }

    private companion object {
        const val MAX_HOLD_MS = 4L * 60 * 60 * 1000
    }
}
