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
        later.release { s, _ -> out += s }
        assertEquals(listOf(WakeSignal(WakeKind.DM, "c1", 1), WakeSignal(WakeKind.MENTION, "c2", 2)), out)
        // Once.
        later.release { s, _ -> out += s }
        assertEquals(2, out.size)
    }

    @Test fun `one wake per channel is kept, with the higher count, and no call`() {
        val deferral = WakeDeferral(prefs)
        deferral.hold(WakeSignal(WakeKind.DM, "c1", 3))
        deferral.hold(WakeSignal(WakeKind.DM, "c1", 1))
        deferral.hold(WakeSignal(WakeKind.CALL, "c9", 1))
        val out = mutableListOf<WakeSignal>()
        deferral.release { s, _ -> out += s }
        assertEquals(listOf(WakeSignal(WakeKind.DM, "c1", 3)), out)
    }

    @Test fun `nothing kept is handed out after a clear`() {
        val deferral = WakeDeferral(prefs)
        deferral.hold(WakeSignal(WakeKind.DM, "c1", 1))
        deferral.clear()
        val out = mutableListOf<WakeSignal>()
        deferral.release { s, _ -> out += s }
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
        assertTrue(enqueue.indexOf("deferral.release(::enqueueHeld)") < enqueue.indexOf("enqueueNow(signal)"))
        val activity = java.io.File(root, "ui/MainActivity.kt").readText()
        assertTrue(activity.substringAfter("authState.initialize(tokenStorage)").substringBefore("setContent {")
            .contains("wakeDeferral.release(wakeEnqueuer::enqueueHeld)"))
    }

    /** A wake kept with its account (in a new process too) goes out with that account. */
    @Test fun `a wake kept with its account keeps that account`() {
        val owner = WakeOwner("u1", "https://chat.example.com/a|b")
        WakeDeferral(prefs).hold(WakeSignal(WakeKind.DM, "c1", 2), owner)
        WakeDeferral(prefs).hold(WakeSignal(WakeKind.MENTION, "c2", 1))
        val out = mutableListOf<Pair<WakeSignal, WakeOwner?>>()
        WakeDeferral(prefs).release { s, o -> out += s to o }
        assertEquals(listOf(WakeSignal(WakeKind.DM, "c1", 2) to owner, WakeSignal(WakeKind.MENTION, "c2", 1) to null), out)
    }

    /**
     * A wake kept with its account goes out only into that account's session on that server; a
     * wake kept without one goes to whoever is signed in.
     */
    @Test fun `a held wake is released only into its own account's session`() {
        var user: String? = "u2"
        var server: String? = "https://chat.example.com"
        val tokens: io.wolftown.kaiku.data.local.TokenStorage = io.mockk.mockk {
            io.mockk.every { getUserId() } answers { user }
            io.mockk.every { getServerUrl() } answers { server }
        }
        val sent = mutableListOf<Pair<WakeSignal, WakeOwner>>()
        val enqueuer = WorkManagerWakeEnqueuer(io.mockk.mockk(relaxed = true), tokens, { false }, WakeDeferral(prefs)) { s, o -> sent += s to o }
        val ownerA = WakeOwner("u1", "https://chat.example.com")
        val wake = WakeSignal(WakeKind.DM, "c1", 1)
        // Another person is signed in.
        enqueuer.enqueueHeld(wake, ownerA)
        assertTrue(sent.isEmpty())
        // The same person on another server.
        user = "u1"; server = "https://other.example.org"
        enqueuer.enqueueHeld(wake, ownerA)
        assertTrue(sent.isEmpty())
        // Its own account and server.
        server = "https://chat.example.com"
        enqueuer.enqueueHeld(wake, ownerA)
        assertEquals(listOf(wake to ownerA), sent)
        // Without a kept account: the account signed in now.
        user = "u2"
        enqueuer.enqueueHeld(wake, null)
        assertEquals(wake to WakeOwner("u2", "https://chat.example.com"), sent.last())
    }

    /** Wakes for one channel kept for different accounts (or for none) never take each other's account. */
    @Test fun `a channel's wakes keep their own accounts in every order`() {
        val a = WakeOwner("u1", "https://chat.example.com")
        val b = WakeOwner("u2", "https://chat.example.com")
        fun held(vararg wakes: Pair<Int, WakeOwner?>): Set<Pair<Int, WakeOwner?>> {
            val d = WakeDeferral(InMemorySharedPreferences())
            wakes.forEach { (count, owner) -> d.hold(WakeSignal(WakeKind.DM, "c1", count), owner) }
            val out = mutableSetOf<Pair<Int, WakeOwner?>>()
            d.release { s, o -> out += s.count to o }
            return out
        }
        // Owned, then without an account.
        assertEquals(setOf(3 to a, 1 to null), held(3 to a, 1 to null))
        // Without an account, then owned.
        assertEquals(setOf(1 to null, 3 to a), held(1 to null, 3 to a))
        // Two accounts.
        assertEquals(setOf(3 to a, 2 to b), held(3 to a, 2 to b))
        // The same account twice: one wake, the higher count.
        assertEquals(setOf(3 to a), held(3 to a, 1 to a))
    }
}
