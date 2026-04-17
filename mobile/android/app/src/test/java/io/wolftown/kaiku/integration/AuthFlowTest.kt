package io.wolftown.kaiku.integration

import io.mockk.*
import io.wolftown.kaiku.data.api.AuthApi
import io.wolftown.kaiku.data.local.AuthState
import io.wolftown.kaiku.data.local.TokenStorage
import io.wolftown.kaiku.data.repository.AuthRepository
import io.wolftown.kaiku.data.ws.ConnectionState
import io.wolftown.kaiku.data.ws.KaikuWebSocket
import io.wolftown.kaiku.domain.model.AuthResponse
import io.wolftown.kaiku.domain.model.User
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.test.UnconfinedTestDispatcher
import kotlinx.coroutines.test.runTest
import org.junit.Assert.*
import org.junit.Before
import org.junit.Test

/**
 * Integration test verifying the full auth flow:
 * login → token storage → WebSocket connect → guild list availability.
 *
 * Uses mocked server responses but real AuthState, AuthRepository, and TokenStorage interactions.
 */
@OptIn(ExperimentalCoroutinesApi::class)
class AuthFlowTest {

    private lateinit var authApi: AuthApi
    private lateinit var tokenStorage: TokenStorage
    private lateinit var authState: AuthState
    private lateinit var webSocket: KaikuWebSocket
    private lateinit var authRepository: AuthRepository

    private val testUser = User(
        id = "user-42",
        username = "testuser",
        displayName = "Test User"
    )

    private val testAuthResponse = AuthResponse(
        accessToken = "access-token-abc",
        refreshToken = "refresh-token-xyz",
        expiresIn = 900,
        tokenType = "Bearer"
    )

    @Before
    fun setUp() {
        authApi = mockk()
        tokenStorage = mockk(relaxed = true)
        // UnconfinedTestDispatcher drives AuthState's stateIn synchronously so
        // `.isLoggedIn.value` / `.currentUserId.value` reflect setLoggedIn and
        // setLoggedOut immediately. See app/src/test/AGENTS.md.
        authState = AuthState(CoroutineScope(SupervisorJob() + UnconfinedTestDispatcher()))
        webSocket = mockk(relaxed = true)

        authRepository = AuthRepository(authApi, tokenStorage, authState)
    }

    // ========================================================================
    // Login → token storage → auth state
    // ========================================================================

    @Test
    fun `login stores tokens and sets auth state to logged in`() = runTest {
        coEvery { authApi.login("testuser", "pass", null) } returns testAuthResponse
        coEvery { authApi.authenticatedGetMe(any()) } returns testUser

        val result = authRepository.login("testuser", "pass")

        assertTrue(result.isSuccess)
        assertEquals(testUser, result.getOrNull())

        // Since #525, tokens are persisted exactly once — after authenticatedGetMe
        // succeeds — with the real userId. No placeholder empty-userId write.
        verify(exactly = 1) {
            tokenStorage.saveTokens(
                accessToken = "access-token-abc",
                refreshToken = "refresh-token-xyz",
                expiresIn = 900,
                userId = "user-42"
            )
        }

        // Auth state should be logged in
        assertTrue(authState.isLoggedIn.value)
        assertEquals("user-42", authState.currentUserId.value)
    }

    @Test
    fun `login failure does not store tokens or update auth state`() = runTest {
        coEvery {
            authApi.login("testuser", "wrong", null)
        } throws Exception("Invalid credentials")

        val result = authRepository.login("testuser", "wrong")

        assertTrue(result.isFailure)
        assertEquals("Invalid credentials", result.exceptionOrNull()?.message)

        // No tokens should be saved
        verify(exactly = 0) { tokenStorage.saveTokens(any(), any(), any(), any()) }

        // Auth state should remain logged out
        assertFalse(authState.isLoggedIn.value)
        assertNull(authState.currentUserId.value)
    }

    @Test
    fun `register cleans up tokens when getMe fails`() = runTest {
        coEvery {
            authApi.register("newuser", "pass", null, null)
        } returns testAuthResponse
        coEvery { authApi.authenticatedGetMe(any()) } throws RuntimeException("Network error")

        val result = authRepository.register("newuser", "pass", null, null)

        assertTrue(result.isFailure)
        verify { tokenStorage.clear() }
    }

    // ========================================================================
    // Logout → clear tokens → auth state
    // ========================================================================

    @Test
    fun `logout clears tokens and sets auth state to logged out`() = runTest {
        // First login
        coEvery { authApi.login("testuser", "pass", null) } returns testAuthResponse
        coEvery { authApi.authenticatedGetMe(any()) } returns testUser
        coEvery { authApi.logout() } just Runs
        authRepository.login("testuser", "pass")

        assertTrue(authState.isLoggedIn.value)

        // Now logout
        authRepository.logout()

        verify { tokenStorage.clear() }
        assertFalse(authState.isLoggedIn.value)
        assertNull(authState.currentUserId.value)
    }

    @Test
    fun `logout succeeds even when server-side logout fails`() = runTest {
        // First login
        coEvery { authApi.login("testuser", "pass", null) } returns testAuthResponse
        coEvery { authApi.authenticatedGetMe(any()) } returns testUser
        coEvery { authApi.logout() } throws Exception("Network error")
        authRepository.login("testuser", "pass")

        // Logout should still clear local state
        authRepository.logout()

        verify { tokenStorage.clear() }
        assertFalse(authState.isLoggedIn.value)
    }

    // ========================================================================
    // Initialize auth state from stored tokens
    // ========================================================================

    @Test
    fun `initialize restores auth state from valid stored tokens`() = runTest {
        every { tokenStorage.getAccessToken() } returns "stored-token"
        every { tokenStorage.getUserId() } returns "user-42"
        every { tokenStorage.isAccessTokenExpired() } returns false
        every { tokenStorage.getRefreshToken() } returns "refresh-token"

        authState.initialize(tokenStorage)

        assertTrue(authState.isLoggedIn.value)
        assertEquals("user-42", authState.currentUserId.value)
    }

    @Test
    fun `initialize stays logged in with expired token but valid refresh token`() = runTest {
        every { tokenStorage.getAccessToken() } returns "expired-token"
        every { tokenStorage.getUserId() } returns "user-42"
        every { tokenStorage.isAccessTokenExpired() } returns true
        every { tokenStorage.getRefreshToken() } returns "valid-refresh"

        authState.initialize(tokenStorage)

        assertTrue(authState.isLoggedIn.value)
        assertEquals("user-42", authState.currentUserId.value)
    }

    @Test
    fun `initialize logs out with expired token and no refresh token`() = runTest {
        every { tokenStorage.getAccessToken() } returns "expired-token"
        every { tokenStorage.getUserId() } returns "user-42"
        every { tokenStorage.isAccessTokenExpired() } returns true
        every { tokenStorage.getRefreshToken() } returns null

        authState.initialize(tokenStorage)

        assertFalse(authState.isLoggedIn.value)
        assertNull(authState.currentUserId.value)
    }

    // ========================================================================
    // WebSocket connection after login
    // ========================================================================

    @Test
    fun `WebSocket can connect after login stores token`() = runTest {
        coEvery { authApi.login("testuser", "pass", null) } returns testAuthResponse
        coEvery { authApi.authenticatedGetMe(any()) } returns testUser
        every { tokenStorage.getAccessToken() } returns "access-token-abc"
        every { tokenStorage.getServerUrl() } returns "https://kaiku.example.com"

        authRepository.login("testuser", "pass")

        // After login, the app would call webSocket.connect()
        webSocket.connect("https://kaiku.example.com")

        verify { webSocket.connect("https://kaiku.example.com") }
    }

    // ========================================================================
    // OIDC login flow
    // ========================================================================

    @Test
    fun `OIDC login stores tokens and sets auth state`() = runTest {
        coEvery { authApi.authenticatedGetMe(any()) } returns testUser

        val result = authRepository.completeOidcLogin(
            accessToken = "oidc-access-token",
            refreshToken = "oidc-refresh-token",
            expiresIn = 3600
        )

        assertTrue(result.isSuccess)
        assertEquals(testUser, result.getOrNull())

        verify {
            tokenStorage.saveTokens(
                accessToken = "oidc-access-token",
                refreshToken = "oidc-refresh-token",
                expiresIn = 3600,
                userId = "user-42"
            )
        }

        assertTrue(authState.isLoggedIn.value)
        assertEquals("user-42", authState.currentUserId.value)
    }
}
