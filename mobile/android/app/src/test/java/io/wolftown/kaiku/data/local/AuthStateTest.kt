package io.wolftown.kaiku.data.local

import app.cash.turbine.test
import io.mockk.every
import io.mockk.mockk
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.test.UnconfinedTestDispatcher
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test

@OptIn(ExperimentalCoroutinesApi::class)
class AuthStateTest {

    private lateinit var authState: AuthState

    @Before
    fun setUp() {
        // UnconfinedTestDispatcher runs stateIn continuations synchronously on
        // the calling thread, so `.value` reflects setLoggedIn/setLoggedOut
        // immediately. See app/src/test/AGENTS.md ("inject the scope").
        authState = AuthState(CoroutineScope(SupervisorJob() + UnconfinedTestDispatcher()))
    }

    // ========================================================================
    // setLoggedIn
    // ========================================================================

    @Test
    fun `setLoggedIn updates isLoggedIn to true and sets currentUserId`() = runTest {
        authState.isLoggedIn.test {
            assertEquals(false, awaitItem()) // initial

            authState.setLoggedIn("user-123")

            assertEquals(true, awaitItem())
        }

        // Verify userId is also set
        assertEquals("user-123", authState.currentUserId.value)
    }

    @Test
    fun `setLoggedIn updates currentUserId`() = runTest {
        authState.currentUserId.test {
            assertNull(awaitItem()) // initial

            authState.setLoggedIn("user-456")

            assertEquals("user-456", awaitItem())
        }
    }

    // ========================================================================
    // setLoggedOut
    // ========================================================================

    @Test
    fun `setLoggedOut updates isLoggedIn to false`() = runTest {
        authState.setLoggedIn("user-123")

        authState.isLoggedIn.test {
            assertEquals(true, awaitItem()) // currently logged in

            authState.setLoggedOut()

            assertEquals(false, awaitItem())
        }
    }

    @Test
    fun `setLoggedOut sets currentUserId to null`() = runTest {
        authState.setLoggedIn("user-123")

        authState.currentUserId.test {
            assertEquals("user-123", awaitItem()) // currently set

            authState.setLoggedOut()

            assertNull(awaitItem())
        }
    }

    // ========================================================================
    // initialize with valid tokens
    // ========================================================================

    @Test
    fun `initialize with valid tokens sets logged in state`() = runTest {
        val tokenStorage = mockk<TokenStorage>()
        every { tokenStorage.getAccessToken() } returns "valid-token"
        every { tokenStorage.getUserId() } returns "user-789"
        every { tokenStorage.isAccessTokenExpired() } returns false
        every { tokenStorage.getRefreshToken() } returns "refresh-token"

        authState.initialize(tokenStorage)

        assertTrue(authState.isLoggedIn.value)
        assertEquals("user-789", authState.currentUserId.value)
    }

    // ========================================================================
    // initialize with no tokens
    // ========================================================================

    @Test
    fun `initialize with no tokens sets logged out state`() = runTest {
        val tokenStorage = mockk<TokenStorage>()
        every { tokenStorage.getAccessToken() } returns null
        every { tokenStorage.getUserId() } returns null
        every { tokenStorage.isAccessTokenExpired() } returns true
        every { tokenStorage.getRefreshToken() } returns null

        authState.initialize(tokenStorage)

        assertFalse(authState.isLoggedIn.value)
        assertNull(authState.currentUserId.value)
    }

    @Test
    fun `initialize with expired token but valid refresh token stays logged in`() = runTest {
        val tokenStorage = mockk<TokenStorage>()
        every { tokenStorage.getAccessToken() } returns "expired-token"
        every { tokenStorage.getUserId() } returns "user-789"
        every { tokenStorage.isAccessTokenExpired() } returns true
        every { tokenStorage.getRefreshToken() } returns "valid-refresh"

        authState.initialize(tokenStorage)

        assertTrue(authState.isLoggedIn.value)
        assertEquals("user-789", authState.currentUserId.value)
    }

    @Test
    fun `initialize with expired token and no refresh token sets logged out state`() = runTest {
        val tokenStorage = mockk<TokenStorage>()
        every { tokenStorage.getAccessToken() } returns "expired-token"
        every { tokenStorage.getUserId() } returns "user-789"
        every { tokenStorage.isAccessTokenExpired() } returns true
        every { tokenStorage.getRefreshToken() } returns null

        authState.initialize(tokenStorage)

        assertFalse(authState.isLoggedIn.value)
        assertNull(authState.currentUserId.value)
    }

    @Test
    fun `initialize with token but no userId sets logged out state`() = runTest {
        val tokenStorage = mockk<TokenStorage>()
        every { tokenStorage.getAccessToken() } returns "valid-token"
        every { tokenStorage.getUserId() } returns null
        every { tokenStorage.isAccessTokenExpired() } returns false
        every { tokenStorage.getRefreshToken() } returns "refresh-token"

        authState.initialize(tokenStorage)

        assertFalse(authState.isLoggedIn.value)
        assertNull(authState.currentUserId.value)
    }
}
