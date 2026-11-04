package io.wolftown.kaiku.data.keydevice

import io.mockk.every
import io.mockk.mockk
import io.wolftown.kaiku.approval.ApprovalKeyStore
import io.wolftown.kaiku.approval.UvClass
import io.wolftown.kaiku.data.local.AuthState
import io.wolftown.kaiku.data.repository.AuthRepository
import io.wolftown.kaiku.testutil.FakeApprovalApi
import io.wolftown.kaiku.testutil.InMemoryKeyDeviceStore
import kotlinx.coroutines.launch
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class KeyInvalidationTest {

    private fun local() = LocalKeyDevice("kd1", "u1", "https://chat.example.com", UvClass.PER_USE, "e1", null)

    @Test fun `an invalidated key is forgotten even when its screen is left during the report`() = runTest {
        val authState = AuthState(backgroundScope).apply { setLoggedIn("u1") }
        val repo = KeyDeviceRepository(InMemoryKeyDeviceStore(), mockk(relaxed = true), authState, backgroundScope)
        repo.saveClaimed(local())
        repo.markActive("u1", "kd1")
        val api = FakeApprovalApi().apply { reportHangs = true }
        val auth: AuthRepository = mockk { every { hasLiveSession() } returns true }
        val job = launch { KeyInvalidation(api, repo, auth).handle() }
        testScheduler.runCurrent()
        job.cancel() // the screen that found out is left
        testScheduler.advanceUntilIdle()
        assertNull(repo.local())
    }

    @Test fun `a failing keystore delete still forgets the record`() = runTest {
        val authState = AuthState(backgroundScope).apply { setLoggedIn("u1") }
        val keyStore: ApprovalKeyStore = mockk(relaxed = true) { every { delete() } throws java.security.KeyStoreException("busy") }
        val repo = KeyDeviceRepository(InMemoryKeyDeviceStore(), keyStore, authState, backgroundScope)
        repo.saveClaimed(local())
        val failed = runCatching { repo.forget() }.isFailure
        assertTrue(failed)
        assertNull(repo.local())
    }
}
