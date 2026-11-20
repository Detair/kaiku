package io.wolftown.kaiku.data.repository

import io.mockk.coEvery
import io.mockk.coVerify
import io.mockk.every
import io.mockk.mockk
import io.mockk.verify
import io.wolftown.kaiku.approval.keyId
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
import kotlinx.coroutines.launch
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import java.io.IOException

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
        every { saveSession(any(), any(), any(), any(), any()) } answers {
            if (saveFails) return@answers false
            firstArg<String?>()?.let { serverUrl = it }
            savedAccess = secondArg()
            true
        }
    }
    private var saveFails = false
    private var savedAccess: String? = null
    private val authState = AuthState(CoroutineScope(SupervisorJob() + UnconfinedTestDispatcher()))
    private val repository = AuthRepository(authApi, tokenStorage, authState, mockk<PushRegistrar>(relaxed = true), noTeardown(authState))
    private val auth = AuthResponse(accessToken = "device-access", refreshToken = "device-refresh", expiresIn = 900, tokenType = "Bearer")
    private val alice = User(id = "u1", username = "alice", displayName = "Alice")

    @Test fun `a device session is installed when nothing changed`() = runTest {
        coEvery { authApi.authenticatedGetMe("device-access", origin) } returns alice
        val start = repository.beginDeviceSignIn(origin)!!

        assertTrue(repository.installDeviceSession(auth, "u1", start).isSuccess)

        verify { tokenStorage.saveSession(any(), "device-access", "device-refresh", 900, "u1") }
        assertEquals("u1", (authState.session.value as AuthSession.LoggedIn).userId)
    }

    /** The address and the session are one write: when it does not land, nothing is signed in. */
    @Test fun `a device session whose write fails is not signed in and is ended`() = runTest {
        serverUrl = null
        saveFails = true
        coEvery { authApi.authenticatedGetMe("device-access", origin) } returns alice
        val start = repository.beginDeviceSignIn(origin)!!

        assertTrue(repository.installDeviceSession(auth, "u1", start).isFailure)

        assertEquals(AuthSession.LoggedOut, authState.session.value)
        assertNull(serverUrl)
        assertNull(savedAccess)
        verify(exactly = 0) { tokenStorage.saveTokens(any(), any(), any(), any()) }
        verify(exactly = 0) { tokenStorage.saveServerUrl(any()) }
        coVerify { authApi.logoutSession(origin, "device-access", "device-refresh") }
    }

    @Test fun `a device session landing after a logout installs nothing and is ended`() = runTest {
        val start = repository.beginDeviceSignIn(origin)!!
        coEvery { authApi.authenticatedGetMe("device-access", origin) } coAnswers {
            repository.logout() // the person signed out while the profile was loading
            alice
        }

        assertTrue(repository.installDeviceSession(auth, "u1", start).isFailure)

        verify(exactly = 0) { tokenStorage.saveSession(any(), "device-access", any(), any(), any()) }
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

        verify(exactly = 0) { tokenStorage.saveSession(any(), "device-access", any(), any(), any()) }
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

        verify(exactly = 0) { tokenStorage.saveSession(any(), "device-access", any(), any(), any()) }
        coVerify { authApi.logoutSession(origin, "device-access", "device-refresh") }
    }

    @Test fun `a renewal over another account's session is refused and the new session ended`() = runTest {
        authState.setLoggedIn("someone-else")
        val running = authState.session.value
        val start = repository.beginDeviceSignIn(origin)!!
        coEvery { authApi.authenticatedGetMe("device-access", origin) } returns alice

        assertTrue(repository.installDeviceSession(auth, "u1", start).exceptionOrNull() is AlreadySignedInException)

        verify(exactly = 0) { tokenStorage.saveSession(any(), "device-access", any(), any(), any()) }
        coVerify { authApi.logoutSession(origin, "device-access", "device-refresh") }
        assertEquals(running, authState.session.value)
    }

    @Test fun `the key's server is used when the app lost its saved server`() = runTest {
        serverUrl = null // a damaged session store was reset; the key's metadata survived

        val start = repository.beginDeviceSignIn(origin)

        assertNotNull(start)
        assertEquals(origin, start!!.serverUrl)
        // The app's address follows the key only once its signature was accepted and the session installed.
        assertNull(serverUrl)
        coEvery { authApi.authenticatedGetMe("device-access", origin) } returns alice
        assertTrue(repository.installDeviceSession(auth, "u1", start).isSuccess)
        assertEquals(origin, serverUrl)
    }

    /**
     * A sign-in whose signature is refused (a key the server does not hold for this phone) leaves
     * the app's address as it was: the sign-in form later still points at the server it pointed at.
     */
    @Test fun `a sign-in that never installs leaves the app's address as it was`() = runTest {
        serverUrl = "https://typed.example.net"
        val start = repository.beginDeviceSignIn(origin)
        assertNotNull(start)
        assertEquals(origin, start!!.serverUrl)
        // The signature is refused: nothing is installed, nothing written.
        assertEquals("https://typed.example.net", serverUrl)
        verify(exactly = 0) { tokenStorage.saveServerUrl(any()) }
    }

    @Test fun `a running session on another server refuses the device sign-in`() = runTest {
        serverUrl = "https://other.example.org"
        authState.setLoggedIn("u1")

        assertNull(repository.beginDeviceSignIn(origin))
        assertEquals("https://other.example.org", serverUrl)
    }

    @Test fun `a device sign-in supersedes a sign-in still in flight and is never wiped by it`() = runTest {
        coEvery { authApi.authenticatedGetMe("device-access", origin) } returns alice
        val result = repository.completeAttempt(origin, null) {
            // While the attempt is collected, the key signs in (e.g. from a notification).
            val start = repository.beginDeviceSignIn(origin)!!
            assertTrue(repository.installDeviceSession(auth, "u1", start).isSuccess)
            throw java.io.IOException("the collect failed late")
        }
        assertTrue(result.isFailure)
        verify(exactly = 0) { tokenStorage.clear() }
        assertEquals("u1", (authState.session.value as AuthSession.LoggedIn).userId)
    }

    @Test fun `two overlapping device sign-ins never both install`() = runTest {
        coEvery { authApi.authenticatedGetMe(any(), origin) } returns alice
        val first = repository.beginDeviceSignIn(origin)!!
        val second = repository.beginDeviceSignIn(origin)!!
        assertTrue(repository.installDeviceSession(auth.copy(accessToken = "second"), "u1", second).isSuccess)
        assertTrue(repository.installDeviceSession(auth, "u1", first).isFailure)
        coVerify { authApi.logoutSession(origin, "device-access", "device-refresh") }
    }

    @Test fun `a collected session that is not installed is ended on its server`() = runTest {
        val result = repository.completeAttempt(origin, null) {
            serverUrl = "https://other.example.org" // the person changed the server meanwhile
            AuthResponse(accessToken = "attempt-access", refreshToken = "attempt-refresh", expiresIn = 900, tokenType = "Bearer")
        }
        assertTrue(result.isFailure)
        coVerify { authApi.logoutSession(origin, "attempt-access", "attempt-refresh") }
        verify(exactly = 0) { tokenStorage.saveTokens("attempt-access", any(), any(), any()) }
    }

    @Test fun `ending the replaced session survives a cancelled caller`() = runTest {
        authState.setLoggedIn("u1")
        every { tokenStorage.getAccessToken() } returns "old-access"
        coEvery { authApi.authenticatedGetMe("device-access", origin) } returns alice
        val started = kotlinx.coroutines.CompletableDeferred<Unit>()
        coEvery { authApi.logoutSession(origin, "old-access", null) } coAnswers {
            started.complete(Unit)
            kotlinx.coroutines.delay(1_000)
        }
        val start = repository.beginDeviceSignIn(origin)!!
        val job = backgroundScope.launch { repository.installDeviceSession(auth, "u1", start) }
        started.await()
        job.cancel()
        testScheduler.advanceUntilIdle()
        verify { tokenStorage.saveSession(any(), "device-access", "device-refresh", 900, "u1") }
        coVerify(exactly = 1) { authApi.logoutSession(origin, "old-access", null) }
        // The installed session itself is never ended by the cancellation.
        coVerify(exactly = 0) { authApi.logoutSession(origin, "device-access", any()) }
    }

    private val attemptAuth = AuthResponse(
        accessToken = "attempt-access", refreshToken = "attempt-refresh", expiresIn = 900, tokenType = "Bearer",
    )

    @Test fun `a collected session whose profile load fails is ended`() = runTest {
        coEvery { authApi.authenticatedGetMe("attempt-access", origin) } throws IOException("offline")

        val result = repository.completeAttempt(origin, null) { attemptAuth }

        assertTrue(result.exceptionOrNull() is IOException)
        coVerify(exactly = 1) { authApi.logoutSession(origin, "attempt-access", "attempt-refresh") }
        verify(exactly = 0) { tokenStorage.saveTokens("attempt-access", any(), any(), any()) }
    }

    @Test fun `a collected session cancelled while its profile loads is ended`() = runTest {
        val loading = kotlinx.coroutines.CompletableDeferred<Unit>()
        coEvery { authApi.authenticatedGetMe("attempt-access", origin) } coAnswers {
            loading.complete(Unit)
            kotlinx.coroutines.awaitCancellation()
        }

        val job = launch { repository.completeAttempt(origin, null) { attemptAuth } }
        loading.await()
        job.cancel()
        job.join()

        coVerify(exactly = 1) { authApi.logoutSession(origin, "attempt-access", "attempt-refresh") }
        verify(exactly = 0) { tokenStorage.saveTokens("attempt-access", any(), any(), any()) }
    }

    @Test fun `a collect that fails mints nothing and ends nothing`() = runTest {
        val result = repository.completeAttempt(origin, null) { throw IOException("offline") }

        assertTrue(result.isFailure)
        coVerify(exactly = 0) { authApi.logoutSession(any(), any(), any()) }
    }

    @Test fun `a device session whose profile load fails is ended`() = runTest {
        coEvery { authApi.authenticatedGetMe("device-access", origin) } throws IOException("offline")
        val start = repository.beginDeviceSignIn(origin)!!

        assertTrue(repository.installDeviceSession(auth, "u1", start).exceptionOrNull() is IOException)

        coVerify(exactly = 1) { authApi.logoutSession(origin, "device-access", "device-refresh") }
        verify(exactly = 0) { tokenStorage.saveSession(any(), "device-access", any(), any(), any()) }
    }

    @Test fun `a device sign-in cancelled while its profile loads ends its session`() = runTest {
        val loading = kotlinx.coroutines.CompletableDeferred<Unit>()
        coEvery { authApi.authenticatedGetMe("device-access", origin) } coAnswers {
            loading.complete(Unit)
            kotlinx.coroutines.awaitCancellation()
        }
        val start = repository.beginDeviceSignIn(origin)!!

        val job = launch { repository.installDeviceSession(auth, "u1", start) }
        loading.await()
        job.cancel()
        job.join()

        coVerify(exactly = 1) { authApi.logoutSession(origin, "device-access", "device-refresh") }
        verify(exactly = 0) { tokenStorage.saveSession(any(), "device-access", any(), any(), any()) }
        assertEquals(AuthSession.LoggedOut, authState.session.value)
    }

    /**
     * A setup creates its key record for the session it checked: a sign-in that would install a
     * session meanwhile waits until the record is made (and is then judged with it).
     */
    @Test fun `a session is not installed while a key record is being created`() = runTest {
        val f = io.wolftown.kaiku.testutil.KeyDeviceFixture(backgroundScope, active = false)
        coEvery { authApi.authenticatedGetMe("attempt-access", origin) } returns alice
        val finished = java.util.concurrent.CountDownLatch(1)
        var installedDuringCreate = true
        var install: Thread? = null
        val record = io.wolftown.kaiku.data.keydevice.StoredKeyDevice(
            io.wolftown.kaiku.data.keydevice.LocalKeyDevice(null, "u1", origin, io.wolftown.kaiku.approval.UvClass.PER_USE, "e1", null, "Alice"),
            io.wolftown.kaiku.data.keydevice.KeyDevicePhase.CLAIMING,
            keyId = f.keyStore.keyId(),
        )
        val created = f.repo.create(record) {
            // The check runs; a sign-in completes on another thread right now.
            install = Thread {
                kotlinx.coroutines.runBlocking { repository.completeAttempt(origin, null) { attemptAuth } }
                finished.countDown()
            }.also { it.start() }
            installedDuringCreate = finished.await(1, java.util.concurrent.TimeUnit.SECONDS)
            false
        }
        install!!.join(10_000)
        assertTrue(created is io.wolftown.kaiku.data.keydevice.Change.Done)
        assertTrue("the sign-in installed while the record was being created", !installedDuringCreate)
        assertEquals(0L, finished.count)
    }
}
