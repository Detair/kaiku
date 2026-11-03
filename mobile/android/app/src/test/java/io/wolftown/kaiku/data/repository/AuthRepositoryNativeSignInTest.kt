package io.wolftown.kaiku.data.repository

import io.mockk.coEvery
import io.mockk.coVerify
import io.mockk.every
import io.mockk.mockk
import io.mockk.verify
import io.wolftown.kaiku.data.api.AuthApi
import io.wolftown.kaiku.data.local.AuthSession
import io.wolftown.kaiku.data.local.AuthState
import io.wolftown.kaiku.data.local.TokenStorage
import io.wolftown.kaiku.domain.model.AuthResponse
import io.wolftown.kaiku.domain.model.User
import io.wolftown.kaiku.push.PushRegistrar
import io.wolftown.kaiku.testutil.noTeardown
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.test.UnconfinedTestDispatcher
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * A passkey sign-in ([AuthRepository.installNativeSignIn]) that is not
 * installed ends the session the server already minted for it, on the
 * server that minted it (L2b review, deferred to LP).
 */
@OptIn(ExperimentalCoroutinesApi::class)
class AuthRepositoryNativeSignInTest {

    private val serverA = "https://a.example.com"
    private var serverUrl: String? = serverA

    private val authApi: AuthApi = mockk(relaxed = true)
    private val tokenStorage: TokenStorage = mockk(relaxed = true) {
        every { getServerUrl() } answers { serverUrl }
    }

    // UnconfinedTestDispatcher drives AuthState's stateIn synchronously (see app/src/test/AGENTS.md).
    private val authState = AuthState(CoroutineScope(SupervisorJob() + UnconfinedTestDispatcher()))
    private val repository = AuthRepository(authApi, tokenStorage, authState, mockk<PushRegistrar>(relaxed = true), noTeardown(authState))

    private fun response(access: String) = AuthResponse(
        accessToken = access,
        refreshToken = "refresh-$access",
        expiresIn = 900,
        tokenType = "Bearer"
    )

    private fun user(id: String) = User(id = id, username = id, displayName = id)

    @Test
    fun `a passkey result superseded by another sign-in ends its unused session`() = runTest {
        coEvery { authApi.login("anna", "pw", null) } returns response("access-pw")
        coEvery { authApi.authenticatedGetMe("access-pw", any()) } returns user("user-a")
        coEvery { authApi.authenticatedGetMe("access-pk", any()) } coAnswers {
            // A password sign-in completes while the passkey result is checked.
            assertTrue(repository.login("anna", "pw").isSuccess)
            user("user-a")
        }

        val result = repository.installNativeSignIn(response("access-pk"), serverA)

        assertTrue(result.isFailure)
        coVerify(exactly = 1) { authApi.logoutSession(serverA, "access-pk", "refresh-access-pk") }
        verify(exactly = 0) { tokenStorage.saveTokens("access-pk", any(), any(), any()) }
        assertEquals("user-a", (authState.session.value as AuthSession.LoggedIn).userId)
    }

    @Test
    fun `a passkey result after another sign-in installed ends its session`() = runTest {
        authState.setLoggedIn("user-qr")

        val result = repository.installNativeSignIn(response("access-pk"), serverA)

        assertTrue(result.exceptionOrNull() is AlreadySignedInException)
        coVerify(exactly = 1) { authApi.logoutSession(serverA, "access-pk", "refresh-access-pk") }
        coVerify(exactly = 0) { authApi.authenticatedGetMe("access-pk", any()) }
        verify(exactly = 0) { tokenStorage.saveTokens(any(), any(), any(), any()) }
    }

    @Test
    fun `a passkey result after a server switch is ended on its own server only`() = runTest {
        serverUrl = "https://b.example.com"

        val result = repository.installNativeSignIn(response("access-pk"), serverA)

        assertTrue(result.isFailure)
        coVerify(exactly = 1) { authApi.logoutSession(serverA, "access-pk", "refresh-access-pk") }
        coVerify(exactly = 0) { authApi.logoutSession("https://b.example.com", any(), any()) }
        coVerify(exactly = 0) { authApi.authenticatedGetMe(any(), any()) }
        assertEquals(AuthSession.LoggedOut, authState.session.value)
    }

    @Test
    fun `an installed passkey sign-in ends nothing`() = runTest {
        coEvery { authApi.authenticatedGetMe("access-pk", serverA) } returns user("user-a")

        val result = repository.installNativeSignIn(response("access-pk"), serverA)

        assertTrue(result.isSuccess)
        coVerify(exactly = 0) { authApi.logoutSession(any(), any(), any()) }
        verify(exactly = 1) { tokenStorage.saveTokens("access-pk", "refresh-access-pk", 900, "user-a") }
    }
}
