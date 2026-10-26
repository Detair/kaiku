package io.wolftown.kaiku.data.repository

import io.mockk.coEvery
import io.mockk.coVerify
import io.mockk.coVerifyOrder
import io.mockk.every
import io.mockk.mockk
import io.mockk.verify
import io.wolftown.kaiku.data.api.AuthApi
import io.wolftown.kaiku.data.local.AuthSession
import io.wolftown.kaiku.data.local.AuthState
import io.wolftown.kaiku.data.local.TokenStorage
import io.wolftown.kaiku.data.session.SessionTeardownRunner
import io.wolftown.kaiku.push.PushRegistrar
import io.wolftown.kaiku.testutil.appScope
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.awaitCancellation
import kotlinx.coroutines.test.TestScope
import kotlinx.coroutines.test.currentTime
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Test
import java.io.IOException

/** Logout removes this device's push registration first, without ever blocking. */
@OptIn(ExperimentalCoroutinesApi::class)
class AuthRepositoryTest {

    private val authApi: AuthApi = mockk(relaxed = true)
    private val tokenStorage: TokenStorage = mockk(relaxed = true) {
        every { getServerUrl() } returns "https://kaiku.example.com"
        every { getAccessToken() } returns "fixture-access"
        every { getRefreshToken() } returns "fixture-refresh"
        every { getSessionGeneration() } returns 1L
        every { clearIfGeneration(1L) } returns true
    }
    private val registrar: PushRegistrar = mockk(relaxed = true)

    private fun TestScope.signedIn(): Pair<AuthRepository, AuthState> {
        val authState = AuthState(backgroundScope)
        authState.setLoggedIn("user-1")
        val runner = SessionTeardownRunner(emptySet(), authState, appScope())
        return AuthRepository(authApi, tokenStorage, authState, registrar, runner) to authState
    }

    @Test
    fun `logout deletes device subscription before tokens are cleared`() = runTest {
        val (repository, authState) = signedIn()
        val sessionId = (authState.session.value as AuthSession.LoggedIn).sessionId

        repository.logout()

        coVerifyOrder {
            // Bound to the ending session: never sent with another session's token.
            registrar.unregisterForLogout(sessionId)
            authApi.logoutSession(any(), any(), any())
            tokenStorage.clearIfGeneration(1L)
        }
        assertEquals(AuthSession.LoggedOut, authState.session.value)
    }

    @Test
    fun `logout still completes when unregister fails`() = runTest {
        coEvery { registrar.unregisterForLogout(any()) } throws IOException("offline")
        val (repository, authState) = signedIn()

        repository.logout()

        coVerify(exactly = 1) { authApi.logoutSession(any(), any(), any()) }
        verify(exactly = 1) { tokenStorage.clearIfGeneration(1L) }
        assertEquals(AuthSession.LoggedOut, authState.session.value)
    }

    @Test
    fun `logout does not wait for a hanging unregister`() = runTest {
        coEvery { registrar.unregisterForLogout(any()) } coAnswers { awaitCancellation() }
        val (repository, authState) = signedIn()

        repository.logout()

        assertEquals(AuthRepository.PUSH_UNREGISTER_TIMEOUT_MS, currentTime)
        coVerify(exactly = 1) { authApi.logoutSession(any(), any(), any()) }
        verify(exactly = 1) { tokenStorage.clearIfGeneration(1L) }
        assertEquals(AuthSession.LoggedOut, authState.session.value)
    }
}
