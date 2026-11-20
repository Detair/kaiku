package io.wolftown.kaiku.data.keydevice

import io.wolftown.kaiku.approval.KeyId
import io.wolftown.kaiku.approval.SoftwareApprovalKeyStore
import io.wolftown.kaiku.approval.UvClass
import io.wolftown.kaiku.approval.UvDecision
import io.wolftown.kaiku.approval.UvPolicy
import io.wolftown.kaiku.approval.keyId
import io.wolftown.kaiku.data.local.AuthState
import io.wolftown.kaiku.testutil.InMemoryKeyDeviceStore
import io.wolftown.kaiku.testutil.markActive
import io.wolftown.kaiku.testutil.markPending
import io.wolftown.kaiku.testutil.saveClaimed
import kotlinx.coroutines.test.TestScope
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import java.time.Instant

/** The storage contract: every change names what it expects, and "unreadable" is not "absent". */
class KeyDeviceRepositoryTest {

    private val store = InMemoryKeyDeviceStore()
    private val use = UvPolicy.choose(30, true, true) as UvDecision.Use
    private val keyStore = SoftwareApprovalKeyStore().apply { generate(ByteArray(32), use) }
    private val device = LocalKeyDevice(
        keyDeviceId = null, userId = "u1", origin = "https://chat.example.com", uvClass = UvClass.PER_USE,
        enrollmentId = "e1", claimSecret = "claim-secret", accountName = "Oma",
    )

    private fun TestScope.repo(authState: AuthState = AuthState(backgroundScope)) = KeyDeviceRepository(store, keyStore, authState, backgroundScope)

    private fun claiming(attempt: String = "a1") = StoredKeyDevice(device, KeyDevicePhase.CLAIMING, keyId = keyStore.keyId(), attemptId = attempt)

    @Test fun `pending state survives a new repository instance`() = runTest {
        val authState = AuthState(backgroundScope)
        val readyAt = Instant.parse("2026-11-04T12:00:00Z")
        repo(authState).markPending(device, readyAt)

        val again = repo(authState)
        assertEquals(KeyDeviceState.Pending(readyAt, "e1"), again.current())
        assertEquals("claim-secret", again.local()!!.claimSecret)
        assertEquals("https://chat.example.com", again.pinnedOrigin())
    }

    @Test fun `lapse on a key device keeps the key and push`() = runTest {
        val authState = AuthState(backgroundScope)
        val repo = repo(authState)
        repo.saveClaimed(device)
        authState.setLoggedIn("u1")
        repo.markActive("u1", "kd1")
        assertEquals(KeyDeviceState.Active("u1"), repo.current())
        assertTrue(repo.keepsPushOnSignOut())

        // A refresh ended the session (SESSION_EXPIRED): tokens go, the key stays.
        authState.setLoggedOut()

        assertEquals(KeyDeviceState.Lapsed("u1"), repo.current())
        assertTrue(keyStore.hasKey())
        assertEquals("kd1", repo.local()!!.keyDeviceId)
        assertTrue(repo.keepsPushOnSignOut())
    }

    @Test fun `lapse without a key logs out as today`() = runTest {
        val authState = AuthState(backgroundScope)
        val repo = repo(authState)
        authState.setLoggedIn("u1")
        authState.setLoggedOut()
        assertEquals(KeyDeviceState.None, repo.current())
        assertFalse(repo.keepsPushOnSignOut())
        assertNull(repo.pinnedOrigin())
    }

    @Test fun `a record of an earlier version takes the key in the alias`() = runTest {
        store.stored = StoredKeyDevice(device, KeyDevicePhase.ACTIVE)
        assertEquals(keyStore.keyId(), repo().record.value!!.keyId)
    }

    @Test fun `the first record is written only where none is stored, for the key in the alias`() = runTest {
        val repo = repo()
        // Another key than the alias holds: the record would name a key the phone does not have.
        assertEquals(Change.Mismatch, repo.create(claiming().copy(keyId = KeyId.of(byteArrayOf(1)))))
        assertEquals(Change.Mismatch, repo.create(claiming().copy(keyId = null)))
        assertNull(store.stored)
        val mine = claiming()
        assertEquals(Change.Done(mine), repo.create(mine))
        // A second setup's record never replaces it.
        assertEquals(Change.Mismatch, repo.create(claiming("a2")))
        assertEquals(mine, store.stored)
    }

    @Test fun `a record is replaced only by who holds that very record`() = runTest {
        val repo = repo()
        val mine = claiming()
        repo.create(mine)
        val claimed = mine.copy(phase = KeyDevicePhase.CLAIMED)
        // Stale expectations: another setup, another phase, another enrolment, another key.
        for (stale in listOf(
            mine.copy(attemptId = "a2"), claimed, mine.copy(device = device.copy(enrollmentId = "e2")),
            mine.copy(keyId = "other"), mine.copy(device = device.copy(origin = "https://other.example.com")),
            mine.copy(device = device.copy(keyDeviceId = "kd9")), mine.copy(device = device.copy(userId = "u2")),
        )) {
            assertEquals(stale.toString(), Change.Mismatch, repo.replace(stale, claimed))
            assertEquals(stale.toString(), Change.Mismatch, repo.forget(stale, Forget.SERVER_ANSWER))
        }
        assertEquals(mine, store.stored)
        assertTrue(keyStore.hasKey())
        assertEquals(Change.Done(claimed), repo.replace(mine, claimed))
        // The record it replaced is no longer the stored one.
        assertEquals(Change.Mismatch, repo.replace(mine, claimed))
    }

    @Test fun `times kept beside the record do not make it another record`() = runTest {
        val repo = repo()
        val mine = claiming().copy(phase = KeyDevicePhase.ACTIVE)
        store.stored = mine
        repo.reload()
        assertTrue(repo.markRenewalPrompted(mine, Instant.parse("2026-11-20T10:00:00Z")).done)
        assertEquals(Instant.parse("2026-11-20T10:00:00Z"), repo.lastRenewalPromptAt())
        // Held from before the time was noted: still that very record.
        assertEquals(Change.Done(null), repo.forget(mine, Forget.SERVER_ANSWER))
    }

    @Test fun `a record never changes its server, account, key or setup`() = runTest {
        val repo = repo()
        val mine = claiming()
        repo.create(mine)
        for (next in listOf(
            mine.copy(device = device.copy(origin = "https://other.example.com")), mine.copy(device = device.copy(userId = "u2")),
            mine.copy(keyId = "other"), mine.copy(attemptId = "a2"),
        )) {
            assertTrue(runCatching { repo.replace(mine, next) }.isFailure)
        }
        assertEquals(mine, store.stored)
    }

    @Test fun `forgetting clears the record and deletes its key`() = runTest {
        val repo = repo()
        val mine = claiming()
        repo.create(mine)
        assertEquals(Change.Done(null), repo.forget(mine, Forget.SERVER_ANSWER))
        assertEquals(KeyDeviceState.None, repo.current())
        assertFalse(keyStore.hasKey())
        assertNull(store.stored)
        assertEquals(listOf("save:CLAIMING", "clear"), store.writes)
    }

    @Test fun `forgetting a record leaves a key in the alias that is not that record's`() = runTest {
        store.stored = StoredKeyDevice(device, KeyDevicePhase.ACTIVE, keyId = "the-records-key")
        val repo = repo()
        val other = keyStore.keyId()
        assertEquals(Change.Done(null), repo.forget(store.stored!!, Forget.SERVER_ANSWER))
        assertEquals(other, keyStore.keyId())
    }

    @Test fun `a write that does not reach storage changes nothing, here or in memory`() = runTest {
        val repo = repo()
        store.failWrites = true
        assertEquals(Change.WriteFailed, repo.create(claiming()))
        assertNull(repo.record.value)
        store.failWrites = false
        val mine = claiming()
        repo.create(mine)
        store.failWrites = true
        assertEquals(Change.WriteFailed, repo.replace(mine, mine.copy(phase = KeyDevicePhase.CLAIMED)))
        assertEquals(mine, repo.record.value)
        // A clear that fails forgets nothing and deletes no key.
        assertEquals(Change.WriteFailed, repo.forget(mine, Forget.SERVER_ANSWER))
        assertEquals(mine, repo.record.value)
        assertTrue(keyStore.hasKey())
    }

    @Test fun `a failing Keystore delete still forgets the record`() = runTest {
        val repo = repo()
        val mine = claiming()
        repo.create(mine)
        keyStore.failDelete = true
        assertEquals(Change.Done(null), repo.forget(mine, Forget.SERVER_ANSWER))
        assertNull(repo.local())
    }

    @Test fun `while the store cannot be read nothing is written, forgotten, made or deleted`() = runTest {
        store.stored = StoredKeyDevice(device, KeyDevicePhase.ACTIVE, keyId = keyStore.keyId())
        store.unreadable = true
        val repo = repo()
        val key = keyStore.keyId()!!
        assertTrue(repo.unreadable())
        assertNull(repo.record.value)
        assertEquals(KeyDeviceState.None, repo.current())
        assertEquals(Change.Unreadable, repo.create(claiming()))
        assertEquals(Change.Unreadable, repo.replace(store.stored!!, store.stored!!.copy(phase = KeyDevicePhase.PENDING)))
        assertEquals(Change.Unreadable, repo.forget(store.stored!!, Forget.SERVER_ANSWER))
        assertEquals(Change.Unreadable, repo.markRenewalPrompted(store.stored!!, Instant.now()))
        // The key is not an unclaimed left-over just because its record cannot be read.
        assertEquals(NewKey.Unreadable, repo.generateKey(ByteArray(32), use))
        assertFalse(repo.deleteUnclaimedKey(key))
        assertEquals(key, keyStore.keyId())
        assertTrue(store.writes.isEmpty())
        assertTrue(keyStore.deleteAsked.isEmpty())

        store.unreadable = false
        assertTrue(repo.reload())
        assertEquals(store.stored, repo.record.value)
    }

    @Test fun `a key is made only while no record is stored, and an unclaimed left-over goes first`() = runTest {
        val repo = repo()
        val leftOver = keyStore.keyId()
        val made = repo.generateKey(ByteArray(32), use) as NewKey.Made
        assertEquals(listOf(leftOver), keyStore.deleteAsked)
        assertEquals(made.key.keyId, keyStore.keyId())
        repo.create(claiming())
        assertEquals(NewKey.RecordExists, repo.generateKey(ByteArray(32), use))
        assertEquals(made.key.keyId, keyStore.keyId())
    }

    @Test fun `an unclaimed key is deleted only by its id and only without a record`() = runTest {
        val repo = repo()
        val mine = keyStore.keyId()!!
        assertFalse(repo.deleteUnclaimedKey("another-key"))
        assertTrue(keyStore.hasKey())
        repo.create(claiming())
        assertFalse(repo.deleteUnclaimedKey(mine))
        assertTrue(keyStore.hasKey())
        repo.forget(repo.record.value!!, Forget.SERVER_ANSWER)
        keyStore.generate(ByteArray(32), use)
        assertTrue(repo.deleteUnclaimedKey(keyStore.keyId()!!))
        assertFalse(keyStore.hasKey())
    }

    @Test fun `a key that cannot be handed back is not left in the alias`() {
        var alias = false
        val failed = runCatching {
            io.wolftown.kaiku.approval.newKeyOrNone(deleteAlias = { alias = false }) {
                alias = true // the key pair was created
                throw java.security.KeyStoreException("no certificate for the new key")
            }
        }
        assertTrue(failed.exceptionOrNull() is java.security.KeyStoreException)
        assertFalse(alias)
    }

    // ── The install id of a record ────────────────────────────────────────────────────────────

    @Test fun `a record keeps the install id it names, whatever a later write says`() = runTest {
        val repo = repo()
        val first = (repo.create(claiming().copy(installId = "install-1")) as Change.Done).record!!
        val claimed = (repo.replace(first, first.copy(phase = KeyDevicePhase.CLAIMED, installId = "install-2")) as Change.Done).record!!
        assertEquals("install-1", claimed.installId)
        val later = (repo.update(claimed) { it.copy(installId = null, sessionCap = Instant.parse("2027-01-01T00:00:00Z")) } as Change.Done).record!!
        assertEquals("install-1", later.installId)
        assertEquals("install-1", store.stored!!.installId)
        // Named once, it is not taken from the app again.
        assertEquals(Change.Done(later), repo.bindInstallId("install-3"))
        assertEquals("install-1", store.stored!!.installId)
    }

    @Test fun `a record from before it named its install id takes the app's, once`() = runTest {
        val repo = repo()
        repo.saveClaimed(device)
        assertNull(repo.record.value!!.installId)
        assertTrue(repo.bindInstallId("install-1").done)
        assertEquals("install-1", repo.record.value!!.installId)
        assertEquals("install-1", store.stored!!.installId)
        repo.bindInstallId("install-2")
        assertEquals("install-1", repo.record.value!!.installId)
        // It is still the same record to whoever held it before.
        assertTrue(repo.update(StoredKeyDevice(device, KeyDevicePhase.CLAIMED, keyId = keyStore.keyId())) { it }.done)
    }

    @Test fun `no install id is taken where there is no record, the store cannot be read, or the write fails`() = runTest {
        val repo = repo()
        assertEquals(Change.Mismatch, repo.bindInstallId("install-1"))
        assertTrue(store.writes.isEmpty())
        repo.saveClaimed(device)
        store.failWrites = true
        assertEquals(Change.WriteFailed, repo.bindInstallId("install-1"))
        assertNull(repo.record.value!!.installId)
        store.failWrites = false
        store.unreadable = true
        repo.reload()
        assertEquals(Change.Unreadable, repo.bindInstallId("install-1"))
        assertNull(store.stored!!.installId)
    }

    // ── The person's own decision ─────────────────────────────────────────────────────────────

    @Test fun `a person's decision never forgets an active key, a waiting one, or a record whose code was shown`() = runTest {
        for (phase in listOf(KeyDevicePhase.ACTIVE, KeyDevicePhase.PENDING, KeyDevicePhase.CLAIMED)) {
            store.stored = null
            val repo = repo()
            val first = (repo.create(claiming().copy(codeClaim = true)) as Change.Done).record!!
            val rec = (repo.replace(first, first.copy(phase = phase)) as Change.Done).record!!
            assertEquals(phase.name, Change.Mismatch, repo.forget(rec, Forget.DISCARDED_BY_PERSON))
            assertEquals(phase.name, phase, repo.phase())
            assertTrue(phase.name, keyStore.hasKey())
            assertFalse(phase.name, store.writes.contains("clear"))
        }
    }

    @Test fun `a person's decision never forgets a claim that completes by itself`() = runTest {
        val repo = repo()
        // The app's own registration, or a record that does not say which kind of claim it is.
        val rec = (repo.create(claiming()) as Change.Done).record!!
        assertFalse(rec.codeClaim)
        assertEquals(Change.Mismatch, repo.forget(rec, Forget.DISCARDED_BY_PERSON))
        assertEquals(KeyDevicePhase.CLAIMING, repo.phase())
        assertTrue(keyStore.hasKey())
        // A server answer still can (the answer table decides that, not this rule).
        assertTrue(repo.forget(rec, Forget.SERVER_ANSWER).done)
    }

    @Test fun `a person's decision forgets a claim that needs a code and has no answer, only if it is the stored one`() = runTest {
        val repo = repo()
        val rec = (repo.create(claiming().copy(codeClaim = true)) as Change.Done).record!!
        assertEquals(Change.Mismatch, repo.forget(rec.copy(attemptId = "another"), Forget.DISCARDED_BY_PERSON))
        assertTrue(keyStore.hasKey())
        assertTrue(repo.forget(rec, Forget.DISCARDED_BY_PERSON).done)
        assertNull(repo.record.value)
        assertFalse(keyStore.hasKey())
    }

    @Test fun `what is noted about the server's silence is kept beside the record and does not change which record it is`() = runTest {
        val repo = repo()
        val rec = (repo.create(claiming()) as Change.Done).record!!
        val t = BootTime(1, 0)
        val noted = (repo.update(rec) { it.copy(unanswered = Unanswered(t, 1, java.time.Duration.ZERO)) } as Change.Done).record!!
        assertTrue(noted sameAs rec)
        assertEquals(Unanswered(t, 1, java.time.Duration.ZERO), store.stored!!.unanswered)
        // The holder of the older copy still names the stored record.
        assertTrue(repo.update(rec) { it.copy(unanswered = null) }.done)
        assertNull(store.stored!!.unanswered)
    }
}
