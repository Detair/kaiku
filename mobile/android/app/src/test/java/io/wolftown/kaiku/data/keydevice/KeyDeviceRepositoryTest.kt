package io.wolftown.kaiku.data.keydevice

import io.wolftown.kaiku.approval.SoftwareApprovalKeyStore
import io.wolftown.kaiku.approval.UvClass
import io.wolftown.kaiku.approval.UvDecision
import io.wolftown.kaiku.approval.UvPolicy
import io.wolftown.kaiku.data.local.AuthState
import io.wolftown.kaiku.testutil.InMemoryKeyDeviceStore
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import java.time.Instant

class KeyDeviceRepositoryTest {

    private val store = InMemoryKeyDeviceStore()
    private val keyStore = SoftwareApprovalKeyStore().apply {
        generate(ByteArray(32), UvPolicy.choose(30, true, true) as UvDecision.Use)
    }
    private val device = LocalKeyDevice(
        keyDeviceId = null, userId = "u1", origin = "https://chat.example.com", uvClass = UvClass.PER_USE,
        enrollmentId = "e1", claimSecret = "claim-secret", accountName = "Oma",
    )

    @Test fun `pending state survives a new repository instance`() = runTest {
        val authState = AuthState(backgroundScope)
        val readyAt = Instant.parse("2026-11-04T12:00:00Z")
        KeyDeviceRepository(store, keyStore, authState, backgroundScope).markPending(device, readyAt)

        val again = KeyDeviceRepository(store, keyStore, authState, backgroundScope)
        assertEquals(KeyDeviceState.Pending(readyAt, "e1"), again.current())
        assertEquals("claim-secret", again.local()!!.claimSecret)
        assertEquals("https://chat.example.com", again.pinnedOrigin())
    }

    @Test fun `lapse on a key device keeps the key and push`() = runTest {
        val authState = AuthState(backgroundScope)
        val repo = KeyDeviceRepository(store, keyStore, authState, backgroundScope)
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
        val repo = KeyDeviceRepository(store, keyStore, authState, backgroundScope)
        authState.setLoggedIn("u1")
        authState.setLoggedOut()
        assertEquals(KeyDeviceState.None, repo.current())
        assertFalse(repo.keepsPushOnSignOut())
        assertNull(repo.pinnedOrigin())
    }

    @Test fun `forget deletes the key and the metadata`() = runTest {
        val repo = KeyDeviceRepository(store, keyStore, AuthState(backgroundScope), backgroundScope)
        repo.markPending(device, Instant.parse("2026-11-04T12:00:00Z"))
        repo.forget()
        assertEquals(KeyDeviceState.None, repo.current())
        assertFalse(keyStore.hasKey())
        assertNull(store.stored)
    }
}
