package io.wolftown.kaiku.data.keydevice

import io.wolftown.kaiku.approval.SoftwareApprovalKeyStore
import io.wolftown.kaiku.approval.UvClass
import io.wolftown.kaiku.approval.UvDecision
import io.wolftown.kaiku.approval.UvPolicy
import io.wolftown.kaiku.approval.keyId
import io.wolftown.kaiku.data.local.AuthState
import io.wolftown.kaiku.testutil.FakeBootClock
import io.wolftown.kaiku.testutil.InMemoryKeyDeviceStore
import kotlinx.coroutines.test.TestScope
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import java.time.Duration

/**
 * "The server cannot be asked" is never one failed request: it is noted with the record, counted
 * by separate occasions, and counts as lasting only after several of them over at least a day of
 * the phone's running time (setting the clock changes nothing, a reboot never lengthens it).
 */
class ServerSilenceTest {

    private val clock = FakeBootClock(boot = 7, elapsedMs = 0)
    private val t0 = 0L
    private val keyStore = SoftwareApprovalKeyStore().apply { generate(ByteArray(32), UvPolicy.choose(30, true, true) as UvDecision.Use) }
    private val store = InMemoryKeyDeviceStore()
    private val device = LocalKeyDevice("kd1", "u1", "https://chat.example.com", UvClass.PER_USE, "e1", null)

    private fun TestScope.phone(phase: KeyDevicePhase = KeyDevicePhase.ACTIVE): Pair<ServerSilence, KeyDeviceRepository> {
        store.stored = StoredKeyDevice(device, phase, keyId = keyStore.keyId())
        val repo = KeyDeviceRepository(store, keyStore, AuthState(backgroundScope), backgroundScope)
        return ServerSilence(repo, clock) to repo
    }

    private fun at(hours: Long, minutes: Long = 0) {
        clock.elapsedMs = t0 + Duration.ofHours(hours).plus(Duration.ofMinutes(minutes)).toMillis()
    }

    private fun now() = clock.now()

    @Test fun `one failed question is noted and is not a lasting silence, however late it comes`() = runTest {
        val (s, repo) = phone()
        at(24 * 365)
        val noted = s.noteUnanswered(repo.record.value!!)
        assertEquals(Unanswered(now(), 1, Duration.ZERO), noted.unanswered)
        assertEquals(noted.unanswered, store.stored!!.unanswered)
        assertFalse(s.lasting(noted))
    }

    @Test fun `failed questions close together are one occasion`() = runTest {
        val (s, repo) = phone()
        val rec = repo.record.value!!
        s.noteUnanswered(rec)
        val writes = store.writes.size
        for (m in listOf(1L, 10, 30, 59)) {
            at(0, m)
            assertEquals(1, s.noteUnanswered(rec).unanswered!!.occasions)
        }
        // Nothing new to note: nothing is written.
        assertEquals(writes, store.writes.size)
        at(1)
        assertEquals(Unanswered(now(), 2, Duration.ofHours(1)), s.noteUnanswered(rec).unanswered)
        // The gap is counted from the last occasion that was counted.
        at(1, 59)
        assertEquals(2, s.noteUnanswered(rec).unanswered!!.occasions)
        at(2)
        assertEquals(3, s.noteUnanswered(rec).unanswered!!.occasions)
    }

    @Test fun `the silence lasts once there were three occasions and the latest is a day after the first`() = runTest {
        val (s, repo) = phone()
        val rec = repo.record.value!!
        s.noteUnanswered(rec)
        // Many occasions within one day: not yet.
        for (h in 1L..23) {
            at(h)
            assertFalse("after $h h", s.lasting(s.noteUnanswered(rec)))
        }
        at(23, 59)
        assertFalse(s.lasting(s.noteUnanswered(rec)))
        at(24)
        assertTrue(s.lasting(s.noteUnanswered(rec)))
    }

    @Test fun `two occasions are not several, however far apart`() = runTest {
        val (s, repo) = phone()
        val rec = repo.record.value!!
        s.noteUnanswered(rec)
        at(24 * 30)
        val second = s.noteUnanswered(rec)
        assertEquals(2, second.unanswered!!.occasions)
        assertFalse(s.lasting(second))
        // The third, an hour later, makes it several over more than a day.
        at(24 * 30 + 1)
        assertTrue(s.lasting(s.noteUnanswered(rec)))
    }

    @Test fun `an answer takes every note back, and the count starts anew`() = runTest {
        val (s, repo) = phone()
        val rec = repo.record.value!!
        s.noteUnanswered(rec)
        at(12)
        s.noteUnanswered(rec)
        at(25)
        assertTrue(s.lasting(s.noteUnanswered(rec)))
        val answered = s.noteAnswered(rec)
        assertNull(answered.unanswered)
        assertNull(store.stored!!.unanswered)
        assertFalse(s.lasting(answered))
        // Nothing to take back: nothing is written.
        val writes = store.writes.size
        s.noteAnswered(rec)
        assertEquals(writes, store.writes.size)
        at(26)
        val again = s.noteUnanswered(rec)
        assertEquals(Unanswered(now(), 1, Duration.ZERO), again.unanswered)
        assertFalse(s.lasting(again))
    }

    /** A reboot between two failed questions counts only the running time of the new boot. */
    @Test fun `a reboot never makes the silence longer than it was`() = runTest {
        val (s, repo) = phone()
        val rec = repo.record.value!!
        at(0)
        s.noteUnanswered(rec)
        // Rebooted after 30 hours, running for 2 hours now: 2 hours count.
        clock.reboot(Duration.ofHours(2))
        val second = s.noteUnanswered(rec).unanswered!!
        assertEquals(2, second.occasions)
        assertEquals(Duration.ofHours(2), second.span)
        // Rebooted again, running for 30 minutes: less than an occasion apart, nothing new.
        clock.reboot(Duration.ofMinutes(30))
        assertEquals(second, s.noteUnanswered(rec).unanswered)
        clock.advance(Duration.ofHours(23))
        val third = s.noteUnanswered(rec)
        assertEquals(3, third.unanswered!!.occasions)
        assertTrue(s.lasting(third))
    }

    /** When the phone cannot tell its boots apart, only the time this process saw counts. */
    @Test fun `without a boot count only the running time seen by the app counts`() = runTest {
        val (s, repo) = phone()
        val rec = repo.record.value!!
        clock.boot = null
        clock.elapsedMs = Duration.ofDays(30).toMillis()
        s.noteUnanswered(rec)
        // A new process: its running time since boot says nothing about when the note was made.
        clock.elapsedMs += Duration.ofHours(1).toMillis()
        val next = s.noteUnanswered(rec).unanswered!!
        assertEquals(1, next.occasions)
    }

    @Test fun `notes are kept only with the very record they are about`() = runTest {
        val (s, repo) = phone()
        val other = repo.record.value!!.copy(attemptId = "another")
        assertEquals(other, s.noteUnanswered(other))
        assertEquals(other, s.noteAnswered(other))
        assertNull(store.stored!!.unanswered)
        assertTrue(store.writes.isEmpty())
        // The store cannot be read: nothing is noted.
        val rec = repo.record.value!!
        store.unreadable = true
        repo.reload()
        assertEquals(rec, s.noteUnanswered(rec))
    }

    @Test fun `a note does not change which record it is, nor its phase or key`() = runTest {
        val (s, repo) = phone(KeyDevicePhase.PENDING)
        val rec = repo.record.value!!
        val noted = s.noteUnanswered(rec)
        assertTrue(noted sameAs rec)
        assertEquals(rec.copy(unanswered = noted.unanswered), noted)
        assertTrue(keyStore.hasKey())
    }
}
