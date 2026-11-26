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
import java.time.Instant

/**
 * The person may discard one kind of setup only: a claim that needs a code and never got its
 * answer. And only as the last step: its own time long past, its server silent for long.
 */
class SetupDiscardTest {

    private val t0: Instant = Instant.parse("2026-11-20T09:00:00Z")
    /** The phone's running time; the claims below were recorded at its start ([written]). */
    private val clock = FakeBootClock(boot = 3, elapsedMs = 0)
    private val written = BootTime(3, 0)
    private var now: Instant
        get() = t0.plusMillis(clock.elapsedMs)
        set(v) {
            clock.elapsedMs = Duration.between(t0, v).toMillis()
        }
    private val keyStore = SoftwareApprovalKeyStore().apply { generate(ByteArray(32), UvPolicy.choose(30, true, true) as UvDecision.Use) }
    private val store = InMemoryKeyDeviceStore()
    private val device = LocalKeyDevice(null, "u1", "https://chat.example.com", UvClass.PER_USE, "e1", null)
    /** Silent on three occasions over more than a day. */
    private val silentForLong = Unanswered(BootTime(3, Duration.ofHours(25).toMillis()), 3, Duration.ofHours(25))

    private fun TestScope.phone(rec: StoredKeyDevice): Pair<SetupDiscard, KeyDeviceRepository> {
        store.stored = rec.copy(keyId = keyStore.keyId())
        val repo = KeyDeviceRepository(store, keyStore, AuthState(backgroundScope), backgroundScope)
        return SetupDiscard(repo, ServerSilence(repo, clock)) to repo
    }

    private fun lostCodeClaim(unanswered: Unanswered? = silentForLong) =
        StoredKeyDevice(device, KeyDevicePhase.CLAIMING, since = t0, codeClaim = true, unanswered = unanswered, attemptId = "a1", written = written)

    @Test fun `a claim that needs a code and never got its answer may be discarded once all of it holds`() = runTest {
        now = t0.plus(Duration.ofHours(26))
        val (d, repo) = phone(lostCodeClaim())
        val rec = repo.record.value!!
        assertTrue(d.canNeverFinish(rec))
        assertTrue(d.offered(rec))
        assertEquals(Change.Done(null), d.discard(rec))
        assertNull(repo.record.value)
        assertFalse(keyStore.hasKey())
        assertEquals(listOf("clear"), store.writes)
    }

    @Test fun `a record whose code was shown, a waiting key device and an active key are never discarded`() = runTest {
        now = t0.plus(Duration.ofDays(400))
        for (rec in listOf(
            StoredKeyDevice(device.copy(claimSecret = "s"), KeyDevicePhase.CLAIMED, since = t0, codeClaim = true, unanswered = silentForLong),
            StoredKeyDevice(device, KeyDevicePhase.PENDING, readyAt = t0, since = t0, codeClaim = true, unanswered = silentForLong),
            StoredKeyDevice(device.copy(keyDeviceId = "kd1"), KeyDevicePhase.ACTIVE, since = t0, codeClaim = true, unanswered = silentForLong),
        )) {
            val (d, repo) = phone(rec)
            val stored = repo.record.value!!
            assertFalse(rec.phase.name, d.canNeverFinish(stored))
            assertFalse(rec.phase.name, d.offered(stored))
            assertEquals(rec.phase.name, Change.Mismatch, d.discard(stored))
            assertEquals(rec.phase.name, rec.phase, repo.phase())
            assertTrue(rec.phase.name, keyStore.hasKey())
            assertTrue(rec.phase.name, store.writes.isEmpty())
        }
    }

    /** The app's own registration: its claim completes by itself, so the server may hold this key. */
    @Test fun `a claim that completes by itself is never discarded, nor one that does not say what it is`() = runTest {
        now = t0.plus(Duration.ofDays(400))
        for (rec in listOf(
            lostCodeClaim().copy(codeClaim = false),
            // Written before records said which kind of claim they are.
            StoredKeyDevice(device, KeyDevicePhase.CLAIMING, unanswered = silentForLong),
        )) {
            val (d, repo) = phone(rec)
            val stored = repo.record.value!!
            assertFalse(d.canNeverFinish(stored))
            assertFalse(d.offered(stored))
            assertEquals(Change.Mismatch, d.discard(stored))
            assertTrue(keyStore.hasKey())
        }
    }

    @Test fun `it is not offered while the server's silence has not lasted`() = runTest {
        now = t0.plus(Duration.ofDays(400))
        for (u in listOf(
            null,
            Unanswered(written, 1, Duration.ZERO),
            // Many occasions within a day.
            Unanswered(written, 20, Duration.ofHours(23)),
            // Two occasions, far apart.
            Unanswered(written, 2, Duration.ofDays(300)),
        )) {
            val (d, repo) = phone(lostCodeClaim(u))
            val stored = repo.record.value!!
            assertFalse(u.toString(), d.offered(stored))
            assertEquals(u.toString(), Change.Mismatch, d.discard(stored))
            assertTrue(u.toString(), keyStore.hasKey())
        }
    }

    @Test fun `it is not offered before the claim's own time is long past`() = runTest {
        // The notes say "silent for long", the claim is 90 minutes old at most by the phone's running time.
        val (d, repo) = phone(lostCodeClaim())
        val stored = repo.record.value!!
        now = t0.plus(SetupDiscard.CLAIM_AFTER)
        assertFalse(d.offered(stored))
        assertEquals(Change.Mismatch, d.discard(stored))
        now = t0.plus(SetupDiscard.CLAIM_AFTER).plusSeconds(1)
        assertTrue(d.offered(stored))
        // A claim that does not say when it was made (by the running time) is not offered at all.
        val (d2, repo2) = phone(lostCodeClaim().copy(written = null))
        assertFalse(d2.offered(repo2.record.value!!))
    }

    @Test fun `discarding goes by what is stored, not by what the caller holds`() = runTest {
        now = t0.plus(Duration.ofHours(26))
        // Stored: the server has answered meanwhile (no note). The caller still holds the older copy.
        val (d, repo) = phone(lostCodeClaim(unanswered = null))
        val stale = repo.record.value!!.copy(unanswered = silentForLong)
        assertEquals(Change.Mismatch, d.discard(stale))
        assertTrue(keyStore.hasKey())
        // Another record than the stored one.
        assertEquals(Change.Mismatch, d.discard(stale.copy(attemptId = "a2")))
        // The store cannot be read.
        store.unreadable = true
        repo.reload()
        assertEquals(Change.Unreadable, d.discard(stale))
        assertTrue(keyStore.hasKey())
    }

    /** Setting the phone's clock forward does not make a claim old: only its running time does. */
    @Test fun `a clock set forward does not bring the offer early`() = runTest {
        val (d, repo) = phone(lostCodeClaim().copy(since = t0.minus(Duration.ofDays(30))))
        clock.elapsedMs = Duration.ofMinutes(30).toMillis()
        assertFalse(d.offered(repo.record.value!!))
        // After a reboot only the new boot's running time counts.
        clock.reboot(Duration.ofMinutes(20))
        assertFalse(d.offered(repo.record.value!!))
        clock.advance(Duration.ofMinutes(71))
        assertTrue(d.offered(repo.record.value!!))
    }
}
