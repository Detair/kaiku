package io.wolftown.kaiku.push

import io.wolftown.kaiku.testutil.InMemorySharedPreferences
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

/** A message wake that came while the key storage could not be read is kept, not lost. */
class WakeDeferralTest {

    private val prefs = InMemorySharedPreferences()

    @Test fun `a wake kept while the storage could not be read goes out once it can`() {
        val deferral = WakeDeferral(prefs)
        deferral.hold(WakeSignal(WakeKind.DM, "c1", 1))
        deferral.hold(WakeSignal(WakeKind.MENTION, "c2", 2))
        // Kept across a new process.
        val later = WakeDeferral(prefs)
        val out = mutableListOf<WakeSignal>()
        later.release { out += it }
        assertEquals(listOf(WakeSignal(WakeKind.DM, "c1", 1), WakeSignal(WakeKind.MENTION, "c2", 2)), out)
        // Once.
        later.release { out += it }
        assertEquals(2, out.size)
    }

    @Test fun `one wake per channel is kept, with the higher count, and no call`() {
        val deferral = WakeDeferral(prefs)
        deferral.hold(WakeSignal(WakeKind.DM, "c1", 3))
        deferral.hold(WakeSignal(WakeKind.DM, "c1", 1))
        deferral.hold(WakeSignal(WakeKind.CALL, "c9", 1))
        val out = mutableListOf<WakeSignal>()
        deferral.release { out += it }
        assertEquals(listOf(WakeSignal(WakeKind.DM, "c1", 3)), out)
    }

    @Test fun `nothing kept is handed out after a clear`() {
        val deferral = WakeDeferral(prefs)
        deferral.hold(WakeSignal(WakeKind.DM, "c1", 1))
        deferral.clear()
        val out = mutableListOf<WakeSignal>()
        deferral.release { out += it }
        assertTrue(out.isEmpty())
    }

    /** The queue keeps wakes while the storage cannot be read, and hands them out first once it can. */
    @Test fun `the wake queue keeps wakes until the storage can be read`() {
        val root = generateSequence(java.io.File("").absoluteFile) { it.parentFile }
            .map { java.io.File(it, "src/main/java/io/wolftown/kaiku") }
            .first { it.isDirectory }
        val src = java.io.File(root, "push/WakeScheduler.kt").readText()
        val enqueue = src.substringAfter("override fun enqueue(signal: WakeSignal) {").substringBefore("private fun enqueueNow")
        assertTrue(enqueue.contains("if (keyStorage.unreadable()) return deferral.hold(signal)"))
        assertTrue(enqueue.indexOf("deferral.release(::enqueueNow)") < enqueue.indexOf("enqueueNow(signal)"))
        val activity = java.io.File(root, "ui/MainActivity.kt").readText()
        assertTrue(activity.substringAfter("authState.initialize(tokenStorage)").substringBefore("setContent {")
            .contains("wakeDeferral.release(wakeEnqueuer::enqueue)"))
    }
}
