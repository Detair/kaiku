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
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * A device-key session (C26) is installed only if nothing changed since the
 * sign-in started: no logout, no other server, the same running session.
 * A session it does not install is ended on the server.
 */
@OptIn(ExperimentalCoroutinesApi::class)
class AuthRepositoryDeviceSessionTest {

    private val origin = "https://chat.example.com"
    private var serverUrl: String? = origin
    private val authApi: AuthApi = mockk(relaxed = true)
    private val tokenStorage: TokenStorage = mockk(relaxed = true) {
        every { getServerUrl() } answers { serverUrl }
        every { saveServerUrl(any()) } answers { serverUrl = firstArg() }
    }
    private val authState = AuthState(CoroutineScope(SupervisorJob() + UnconfinedTestDispatcher()))
    private val repository = AuthRepository(authApi, tokenStorage, authState, mockk<PushRegistrar>(relaxed = true), noTeardown(authState))
    private val auth = AuthResponse(accessToken = "device-access", refreshToken = "device-refresh", expiresIn = 900, tokenType = "Bearer")
    private val alice = User(id = "u1", username = "alice", displayName = "Alice")

    @Test fun `a device session is installed when nothing changed`() = runTest {
        coEvery { authApi.authenticatedGetMe("device-access", origin) } returns alice
        val start = repository.beginDeviceSignIn(origin)!!

        assertTrue(repository.installDeviceSession(auth, "u1", start).isSuccess)

        verify { tokenStorage.saveTokens("device-access", "device-refresh", 900, "u1") }
        assertEquals("u1", (authState.session.value as AuthSession.LoggedIn).userId)
    }

    @Test fun `a device session landing after a logout installs nothing and is ended`() = runTest {
        val start = repository.beginDeviceSignIn(origin)!!
        coEvery { authApi.authenticatedGetMe("device-access", origin) } coAnswers {
            repository.logout() // the person signed out while the profile was loading
            alice
        }

        assertTrue(repository.installDeviceSession(auth, "u1", start).isFailure)

        verify(exactly = 0) { tokenStorage.saveTokens("device-access", any(), any(), any()) }
        coVerify { authApi.logoutSession(origin, "device-access", "device-refresh") }
        assertEquals(AuthSession.LoggedOut, authState.session.value)
    }

    @Test fun `a renewal landing after a sign-out does not bring the session back`() = runTest {
        authState.setLoggedIn("u1")
        val start = repository.beginDeviceSignIn(origin)!!
        coEvery { authApi.authenticatedGetMe("device-access", origin) } coAnswers {
            repository.logout()
            alice
        }

        assertTrue(repository.installDeviceSession(auth, "u1", start).isFailure)

        verify(exactly = 0) { tokenStorage.saveTokens("device-access", any(), any(), any()) }
        coVerify { authApi.logoutSession(origin, "device-access", "device-refresh") }
        assertEquals(AuthSession.LoggedOut, authState.session.value)
    }

    @Test fun `a server change during the device sign-in stores nothing`() = runTest {
        val start = repository.beginDeviceSignIn(origin)!!
        coEvery { authApi.authenticatedGetMe("device-access", origin) } coAnswers {
            serverUrl = "https://other.example.org"
            alice
        }

        assertTrue(repository.installDeviceSession(auth, "u1", start).isFailure)

        verify(exactly = 0) { tokenStorage.saveTokens("device-access", any(), any(), any()) }
        coVerify { authApi.logoutSession(origin, "device-access", "device-refresh") }
    }

    @Test fun `a renewal over another account's session is refused and the new session ended`() = runTest {
        authState.setLoggedIn("someone-else")
        val running = authState.session.value
        val start = repository.beginDeviceSignIn(origin)!!
        coEvery { authApi.authenticatedGetMe("device-access", origin) } returns alice

        assertTrue(repository.installDeviceSession(auth, "u1", start).exceptionOrNull() is AlreadySignedInException)

        verify(exactly = 0) { tokenStorage.saveTokens("device-access", any(), any(), any()) }
        coVerify { authApi.logoutSession(origin, "device-access", "device-refresh") }
        assertEquals(running, authState.session.value)
    }

    @Test fun `the key's server is used when the app lost its saved server`() = runTest {
        serverUrl = null // a damaged session store was reset; the key's metadata survived

        val start = repository.beginDeviceSignIn(origin)

        assertNotNull(start)
        assertEquals(origin, serverUrl)
        assertEquals(origin, start!!.serverUrl)
    }

    @Test fun `a running session on another server refuses the device sign-in`() = runTest {
        serverUrl = "https://other.example.org"
        authState.setLoggedIn("u1")

        assertNull(repository.beginDeviceSignIn(origin))
        assertEquals("https://other.example.org", serverUrl)
    }
}
