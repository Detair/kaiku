package io.wolftown.kaiku.data.voice

import android.os.SystemClock
import javax.inject.Inject

/**
 * Milliseconds of real elapsed time, including time the device was suspended
 * (C32 "real elapsed time"). Only differences are meaningful.
 */
fun interface ElapsedClock {
    fun nowMs(): Long

    companion object {
        /** A clock that never moves: elapsed time is then what the coroutine delays measured (tests). */
        val STOPPED = ElapsedClock { 0L }
    }
}

/** [SystemClock.elapsedRealtime]: keeps counting while the device sleeps, unlike coroutine delays. */
class AndroidElapsedClock @Inject constructor() : ElapsedClock {
    override fun nowMs(): Long = SystemClock.elapsedRealtime()
}
