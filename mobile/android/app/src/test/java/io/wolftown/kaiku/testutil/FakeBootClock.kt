package io.wolftown.kaiku.testutil

import io.wolftown.kaiku.data.keydevice.BootClock
import io.wolftown.kaiku.data.keydevice.BootTime
import io.wolftown.kaiku.data.keydevice.sinceOf
import java.time.Duration

/** The phone's running time, set by the test ([elapsedMs], [boot]); a reboot is a new [boot] with a small [elapsedMs]. */
class FakeBootClock(var boot: Int? = 1, var elapsedMs: Long = 1_000_000) : BootClock {
    private val firstSeen = mutableMapOf<BootTime, Long>()

    override fun now(): BootTime = BootTime(boot, elapsedMs)

    override fun since(then: BootTime): Duration = sinceOf(then, now()) { n -> firstSeen.getOrPut(then) { n.elapsedMs } }

    fun advance(d: Duration) {
        elapsedMs += d.toMillis()
    }

    fun reboot(runningFor: Duration = Duration.ofMinutes(1)) {
        boot = (boot ?: 0) + 1
        elapsedMs = runningFor.toMillis()
    }
}
