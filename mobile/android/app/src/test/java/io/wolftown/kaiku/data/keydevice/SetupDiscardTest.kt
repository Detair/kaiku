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
import java.time.Instant

/**
 * The person's way out of a setup that never finished: offered only long after the setup's own
 * time, never for an active key, and it forgets only the record it was offered for.
 */
class SetupDiscardTest {

    private val t0: Instant = Instant.parse("2026-11-20T09:00:00Z")
    private val keyStore = SoftwareApprovalKeyStore().apply { generate(ByteArray(32), UvPolicy.choose(30, true, true) as UvDecision.Use) }
    private val store = InMemoryKeyDeviceStore()
    private val device = LocalKeyDevice(null, "u1", "https://chat.example.com", UvClass.PER_USE, "e1", null)

    private fun TestScope.phone(rec: StoredKeyDevice): Pair<SetupDiscard, KeyDeviceRepository> {
        store.stored = rec.copy(keyId = keyStore.keyId())
        val repo = KeyDeviceRepository(store, keyStore, AuthState(backgroundScope), backgroundScope)
        return SetupDiscard(repo) to repo
    }

    @Test fun `a claim may be discarded 90 minutes after it was made, not before`() = runTest {
        for (phase in listOf(KeyDevicePhase.CLAIMING, KeyDevicePhase.CLAIMED)) {
            val (d, repo) = phone(StoredKeyDevice(device, phase, since = t0))
            val rec = repo.record.value!!
            assertEquals(t0.plus(SetupDiscard.CLAIM_AFTER), d.due(rec))
            assertFalse(d.offered(rec, t0))
            assertFalse(d.offered(rec, t0.plus(SetupDiscard.CLAIM_AFTER)))
            assertEquals(Change.Mismatch, d.discard(rec, t0.plus(SetupDiscard.CLAIM_AFTER)))
            assertTrue(keyStore.hasKey())
            assertTrue(d.offered(rec, t0.plus(SetupDiscard.CLAIM_AFTER).plusSeconds(1)))
        }
    }

    @Test fun `a waiting key device may be discarded a day after it should have been ready`() = runTest {
        val readyAt = t0.plusSeconds(24 * 3600)
        val (d, repo) = phone(StoredKeyDevice(device, KeyDevicePhase.PENDING, readyAt = readyAt, since = t0))
        val rec = repo.record.value!!
        assertEquals(readyAt.plus(SetupDiscard.PENDING_AFTER), d.due(rec))
        assertFalse(d.offered(rec, readyAt.plusSeconds(3600)))
        assertTrue(d.offered(rec, readyAt.plus(SetupDiscard.PENDING_AFTER).plusSeconds(1)))
    }

    @Test fun `an active key is never offered and never discarded`() = runTest {
        val (d, repo) = phone(StoredKeyDevice(device.copy(keyDeviceId = "kd1"), KeyDevicePhase.ACTIVE, since = t0))
        val rec = repo.record.value!!
        val farLater = t0.plusSeconds(10L * 365 * 24 * 3600)
        assertNull(d.due(rec))
        assertFalse(d.offered(rec, farLater))
        assertEquals(Change.Mismatch, d.discard(rec, farLater))
        assertEquals(rec, d.noteCouldNotAsk(rec, farLater))
        assertEquals(KeyDevicePhase.ACTIVE, repo.phase())
        assertTrue(keyStore.hasKey())
        assertTrue(store.writes.isEmpty())
    }

    @Test fun `a record without a time gets one when its server first cannot be asked, and only then`() = runTest {
        val (d, repo) = phone(StoredKeyDevice(device, KeyDevicePhase.CLAIMED))
        val rec = repo.record.value!!
        assertNull(d.due(rec))
        assertFalse(d.offered(rec, t0.plusSeconds(10L * 365 * 24 * 3600)))
        val noted = d.noteCouldNotAsk(rec, t0)
        assertEquals(t0, noted.since)
        assertEquals(t0, store.stored!!.since)
        // The next time it cannot be asked does not move the time.
        assertEquals(t0, d.noteCouldNotAsk(noted, t0.plusSeconds(3600)).since)
        assertEquals(t0, store.stored!!.since)
        assertTrue(d.offered(noted, t0.plus(SetupDiscard.CLAIM_AFTER).plusSeconds(1)))
    }

    @Test fun `discarding forgets the record and its key, only if it is still the stored one`() = runTest {
        val (d, repo) = phone(StoredKeyDevice(device, KeyDevicePhase.CLAIMED, since = t0, attemptId = "a1"))
        val rec = repo.record.value!!
        val late = t0.plus(SetupDiscard.CLAIM_AFTER).plusSeconds(1)
        assertEquals(Change.Mismatch, d.discard(rec.copy(attemptId = "a2"), late))
        assertTrue(keyStore.hasKey())
        assertEquals(Change.Done(null), d.discard(rec, late))
        assertNull(repo.record.value)
        assertFalse(keyStore.hasKey())
        assertEquals(listOf("clear"), store.writes)
    }

    @Test fun `while the store cannot be read nothing is discarded`() = runTest {
        val (d, repo) = phone(StoredKeyDevice(device, KeyDevicePhase.CLAIMED, since = t0))
        val rec = repo.record.value!!
        store.unreadable = true
        repo.reload()
        assertEquals(Change.Unreadable, d.discard(rec, t0.plus(SetupDiscard.CLAIM_AFTER).plusSeconds(1)))
        assertTrue(keyStore.hasKey())
    }
}
