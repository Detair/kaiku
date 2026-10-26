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
import io.wolftown.kaiku.push.PushRegistrar
import io.wolftown.kaiku.testutil.noTeardown
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

/** No sign-in path replaces a running session: the user signs out first (Task 19 final review). */
class AuthRepositoryAccountSwitchTest {

    private val pushRegistrar: PushRegistrar = mockk(relaxed = true)
    private val authApi: AuthApi = mockk(relaxed = true)
    private val tokenStorage: TokenStorage = mockk(relaxed = true) {
        every { getServerUrl() } returns "https://a.example.com"
    }

    @Test
    fun `qr redeem while signed in is refused and changes nothing`() = runTest {
        val authState = AuthState(backgroundScope)
        authState.setLoggedIn("user-a")
        val running = authState.session.value
        val repository = AuthRepository(authApi, tokenStorage, authState, pushRegistrar, noTeardown(authState))

        val result = repository.redeemQrToken("https://b.example.com", "qr-token-b")

        assertTrue(result.exceptionOrNull() is AlreadySignedInException)
        coVerify(exactly = 0) { authApi.redeemQrToken(any(), any()) }
        verify(exactly = 0) { tokenStorage.saveServerUrl(any()) }
        verify(exactly = 0) { tokenStorage.saveTokens(any(), any(), any(), any()) }
        verify(exactly = 0) { tokenStorage.clear() }
        assertEquals(running, authState.session.value)
    }

    @Test
    fun `oidc and password sign-ins while signed in are refused`() = runTest {
        val authState = AuthState(backgroundScope)
        authState.setLoggedIn("user-a")
        val running = authState.session.value as AuthSession.LoggedIn
        val repository = AuthRepository(authApi, tokenStorage, authState, pushRegistrar, noTeardown(authState))

        assertTrue(repository.completeOidcLogin("b-access", "b-refresh", 900).exceptionOrNull() is AlreadySignedInException)
        assertTrue(repository.exchangeOidcCode("code", "state").exceptionOrNull() is AlreadySignedInException)
        assertTrue(repository.login("b", "pw").exceptionOrNull() is AlreadySignedInException)
        assertTrue(repository.register("b", "pw", null, null).exceptionOrNull() is AlreadySignedInException)

        coVerify(exactly = 0) { authApi.authenticatedGetMe(any(), any()) }
        coVerify(exactly = 0) { authApi.login(any(), any(), any()) }
        verify(exactly = 0) { tokenStorage.saveTokens(any(), any(), any(), any()) }
        verify(exactly = 0) { tokenStorage.clear() }
        assertEquals(running, authState.session.value)
    }

    @Test
    fun `sign-in that loses the race to another sign-in installs nothing`() = runTest {
        val authState = AuthState(backgroundScope)
        val repository = AuthRepository(authApi, tokenStorage, authState, pushRegistrar, noTeardown(authState))
        coEvery { authApi.authenticatedGetMe(any(), any()) } coAnswers {
            authState.setLoggedIn("user-qr") // e.g. a QR redeem finished first
            io.wolftown.kaiku.domain.model.User(id = "user-oidc", username = "o", displayName = "O")
        }

        val result = repository.completeOidcLogin("oidc-access", "oidc-refresh", 900)

        assertTrue(result.exceptionOrNull() is AlreadySignedInException)
        verify(exactly = 0) { tokenStorage.saveTokens(any(), any(), any(), any()) }
        verify(exactly = 0) { tokenStorage.clear() }
        assertEquals("user-qr", (authState.session.value as AuthSession.LoggedIn).userId)
    }
}
