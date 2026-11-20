package io.wolftown.kaiku.data.keydevice

import io.mockk.every
import io.mockk.mockk
import io.wolftown.kaiku.approval.SoftwareApprovalKeyStore
import io.wolftown.kaiku.approval.UvClass
import io.wolftown.kaiku.approval.UvDecision
import io.wolftown.kaiku.approval.UvPolicy
import io.wolftown.kaiku.approval.keyId
import io.wolftown.kaiku.data.local.AuthState
import io.wolftown.kaiku.data.repository.AuthRepository
import io.wolftown.kaiku.testutil.FakeApprovalApi
import io.wolftown.kaiku.testutil.InMemoryKeyDeviceStore
import io.wolftown.kaiku.testutil.markActive
import io.wolftown.kaiku.testutil.saveClaimed
import kotlinx.coroutines.launch
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class KeyInvalidationTest {

    private val use = UvPolicy.choose(30, true, true) as UvDecision.Use
    private val keyStore = SoftwareApprovalKeyStore().apply { generate(ByteArray(32), use) }

    private fun local() = LocalKeyDevice("kd1", "u1", "https://chat.example.com", UvClass.PER_USE, "e1", null)

    @Test fun `an invalidated key is forgotten even when its screen is left during the report`() = runTest {
        val authState = AuthState(backgroundScope).apply { setLoggedIn("u1") }
        val repo = KeyDeviceRepository(InMemoryKeyDeviceStore(), keyStore, authState, backgroundScope)
        repo.saveClaimed(local())
        repo.markActive("u1", "kd1")
        val api = FakeApprovalApi().apply { reportHangs = true }
        val auth: AuthRepository = mockk { every { hasLiveSession() } returns true }
        val key = keyStore.keyId()
        val job = launch { KeyInvalidation(api, repo, auth).handle(key) }
        testScheduler.runCurrent()
        job.cancel() // the screen that found out is left
        testScheduler.advanceUntilIdle()
        assertNull(repo.local())
        assertEquals(listOf("reportInvalidated:kd1"), api.calls)
    }

    @Test fun `a signature that failed on an older key never forgets the key the phone has now`() = runTest {
        val authState = AuthState(backgroundScope)
        val repo = KeyDeviceRepository(InMemoryKeyDeviceStore(), keyStore, authState, backgroundScope)
        val old = keyStore.keyId()
        // Meanwhile that key was replaced by a new setup, which stored its claim.
        keyStore.delete()
        val now = keyStore.generate(ByteArray(32), use).keyId
        repo.saveClaimed(local())
        val api = FakeApprovalApi()
        val auth: AuthRepository = mockk { every { hasLiveSession() } returns true }

        KeyInvalidation(api, repo, auth).handle(old)

        assertNotNull(repo.local())
        assertEquals(now, keyStore.keyId())
        assertTrue(api.calls.isEmpty())
    }

    @Test fun `a record whose key is missing is forgotten`() = runTest {
        val authState = AuthState(backgroundScope)
        keyStore.delete()
        val repo = KeyDeviceRepository(InMemoryKeyDeviceStore(), keyStore, authState, backgroundScope)
        repo.saveClaimed(local())
        KeyInvalidation(FakeApprovalApi(), repo, mockk { every { hasLiveSession() } returns false }).handle(null)
        assertNull(repo.local())
    }
}
