package io.wolftown.kaiku.data.keydevice

import io.wolftown.kaiku.approval.SoftwareApprovalKeyStore
import io.wolftown.kaiku.approval.UvClass
import io.wolftown.kaiku.approval.UvDecision
import io.wolftown.kaiku.approval.UvPolicy
import io.wolftown.kaiku.approval.keyId
import io.wolftown.kaiku.data.local.AuthState
import io.wolftown.kaiku.testutil.InMemoryKeyDeviceStore
import kotlinx.coroutines.test.TestScope
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import java.time.Duration
import java.time.Instant

/**
 * "The server cannot be asked" is never one failed request: it is noted with the record, counted
 * by separate occasions, and counts as lasting only after several of them over at least a day.
 */
class ServerSilenceTest {

    private val t0: Instant = Instant.parse("2026-11-20T09:00:00Z")
    private var now: Instant = t0
    private val keyStore = SoftwareApprovalKeyStore().apply { generate(ByteArray(32), UvPolicy.choose(30, true, true) as UvDecision.Use) }
    private val store = InMemoryKeyDeviceStore()
    private val device = LocalKeyDevice("kd1", "u1", "https://chat.example.com", UvClass.PER_USE, "e1", null)

    private fun TestScope.phone(phase: KeyDevicePhase = KeyDevicePhase.ACTIVE): Pair<ServerSilence, KeyDeviceRepository> {
        store.stored = StoredKeyDevice(device, phase, keyId = keyStore.keyId())
        val repo = KeyDeviceRepository(store, keyStore, AuthState(backgroundScope), backgroundScope)
        return ServerSilence(repo) { now } to repo
    }

    private fun at(hours: Long, minutes: Long = 0) {
        now = t0.plus(Duration.ofHours(hours)).plus(Duration.ofMinutes(minutes))
    }

    @Test fun `one failed question is noted and is not a lasting silence, however late it comes`() = runTest {
        val (s, repo) = phone()
        at(24 * 365)
        val noted = s.noteUnanswered(repo.record.value!!)
        assertEquals(Unanswered(now, now, 1), noted.unanswered)
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
        assertEquals(Unanswered(t0, now, 2), s.noteUnanswered(rec).unanswered)
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
        assertEquals(Unanswered(now, now, 1), again.unanswered)
        assertFalse(s.lasting(again))
    }

    @Test fun `a clock put back behind the first note starts the count from there`() = runTest {
        val (s, repo) = phone()
        val rec = repo.record.value!!
        at(48)
        s.noteUnanswered(rec)
        at(0)
        assertEquals(Unanswered(t0, t0, 1), s.noteUnanswered(rec).unanswered)
        // Put back, but not behind the first note: not a new occasion.
        at(5)
        s.noteUnanswered(rec)
        at(4)
        assertEquals(2, s.noteUnanswered(rec).unanswered!!.occasions)
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
