package io.wolftown.kaiku.data.keydevice

import android.content.Context
import android.os.SystemClock
import android.provider.Settings
import dagger.hilt.android.qualifiers.ApplicationContext
import java.time.Duration
import javax.inject.Inject

/**
 * A moment by the phone's own running time: which boot of the phone ([boot]; null when Android
 * does not say) and how long the phone had been running then ([elapsedMs]). Not changed by
 * setting the phone's clock.
 */
data class BootTime(val boot: Int?, val elapsedMs: Long)

/**
 * The phone's running time, for "how long ago" questions that must not be shortened by setting
 * the clock. [since] never says more time has passed than really has: after a reboot it counts
 * from the start of the new boot, and when the boot cannot be told it counts from the first time
 * this process looked.
 */
interface BootClock {
    fun now(): BootTime

    /** How long ago [then] was, never more than really passed. */
    fun since(then: BootTime): Duration
}

/** [BootClock] from `elapsedRealtime` and the boot count Android keeps. */
class AndroidBootClock internal constructor(
    private val bootCount: () -> Int?,
    private val elapsedMs: () -> Long,
) : BootClock {

    @Inject
    constructor(@ApplicationContext context: Context) : this(
        bootCount = {
            try {
                Settings.Global.getInt(context.contentResolver, Settings.Global.BOOT_COUNT)
            } catch (_: Exception) {
                null
            }
        },
        elapsedMs = SystemClock::elapsedRealtime,
    )

    /** Moments this process first saw, for times whose boot cannot be told. */
    private val firstSeen = java.util.concurrent.ConcurrentHashMap<BootTime, Long>()

    override fun now(): BootTime = BootTime(bootCount(), elapsedMs())

    override fun since(then: BootTime): Duration = sinceOf(then, now()) { n -> firstSeen.getOrPut(then) { n.elapsedMs } }
}

/**
 * The rule of [BootClock.since], on its own: the same boot counts the difference; another boot
 * counts from the start of the current one; when either boot is not known only the time since
 * [firstSeen] (the first look of this process, same boot by construction) counts.
 */
internal fun sinceOf(then: BootTime, now: BootTime, firstSeen: (BootTime) -> Long): Duration {
    val ms = when {
        then.boot != null && now.boot != null && then.boot == now.boot && now.elapsedMs >= then.elapsedMs -> now.elapsedMs - then.elapsedMs
        then.boot != null && now.boot != null && then.boot != now.boot -> now.elapsedMs
        else -> (now.elapsedMs - firstSeen(now)).coerceAtLeast(0)
    }
    return Duration.ofMillis(ms)
}
