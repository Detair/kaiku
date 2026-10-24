package io.wolftown.kaiku.data.repository

import io.mockk.coEvery
import io.mockk.coVerify
import io.mockk.coVerifyOrder
import io.mockk.mockk
import io.mockk.verify
import io.wolftown.kaiku.data.api.AuthApi
import io.wolftown.kaiku.data.local.AuthState
import io.wolftown.kaiku.data.local.TokenStorage
import io.wolftown.kaiku.push.PushRegistrar
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.awaitCancellation
import kotlinx.coroutines.test.currentTime
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Test
import java.io.IOException

/** Logout removes this device's push registration first, without ever blocking. */
@OptIn(ExperimentalCoroutinesApi::class)
class AuthRepositoryTest {

    private val authApi: AuthApi = mockk(relaxed = true)
    private val tokenStorage: TokenStorage = mockk(relaxed = true)
    private val authState: AuthState = mockk(relaxed = true)
    private val registrar: PushRegistrar = mockk(relaxed = true)
    private val repository = AuthRepository(authApi, tokenStorage, authState, registrar)

    @Test
    fun `logout deletes device subscription before tokens are cleared`() = runTest {
        repository.logout()

        coVerifyOrder {
            registrar.unregisterForLogout()
            authApi.logout()
            tokenStorage.clear()
            authState.setLoggedOut()
        }
    }

    @Test
    fun `logout still completes when unregister fails`() = runTest {
        coEvery { registrar.unregisterForLogout() } throws IOException("offline")

        repository.logout()

        coVerify(exactly = 1) { authApi.logout() }
        verify(exactly = 1) { tokenStorage.clear() }
        verify(exactly = 1) { authState.setLoggedOut() }
    }

    @Test
    fun `logout does not wait for a hanging unregister`() = runTest {
        coEvery { registrar.unregisterForLogout() } coAnswers { awaitCancellation() }

        repository.logout()

        assertEquals(AuthRepository.PUSH_UNREGISTER_TIMEOUT_MS, currentTime)
        coVerify(exactly = 1) { authApi.logout() }
        verify(exactly = 1) { tokenStorage.clear() }
    }
}
