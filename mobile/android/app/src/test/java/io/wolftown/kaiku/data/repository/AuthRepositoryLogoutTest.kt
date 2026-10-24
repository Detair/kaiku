package io.wolftown.kaiku.data.repository

import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.client.engine.mock.toByteArray
import io.ktor.http.HttpStatusCode
import io.mockk.coEvery
import io.mockk.coVerify
import io.mockk.every
import io.mockk.mockk
import io.mockk.verify
import io.wolftown.kaiku.data.api.AuthApi
import io.wolftown.kaiku.data.api.AuthApiImpl
import io.wolftown.kaiku.data.api.KaikuHttpClient
import io.wolftown.kaiku.data.local.AuthSession
import io.wolftown.kaiku.data.local.AuthState
import io.wolftown.kaiku.data.local.TokenStorage
import io.wolftown.kaiku.data.session.SessionTeardown
import io.wolftown.kaiku.data.session.SessionTeardownRunner
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.awaitCancellation
import kotlinx.coroutines.launch
import kotlinx.coroutines.test.advanceUntilIdle
import kotlinx.coroutines.test.runCurrent
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/** Logout (Task 19): teardowns, then the server logout, then a clear bound to the captured session. */
class AuthRepositoryLogoutTest {

    private val log = mutableListOf<String>()
    private var generation = 1L

    private val tokenStorage: TokenStorage = mockk(relaxed = true) {
        every { getServerUrl() } returns "https://kaiku.example.com"
        every { getAccessToken() } returns ACCESS
        every { getRefreshToken() } returns REFRESH
        every { getSessionGeneration() } answers { generation }
        every { clearIfGeneration(any()) } answers {
            val cleared = firstArg<Long>() == generation
            if (cleared) {
                generation++
                log += "clear"
            }
            cleared
        }
    }

    private val authApi: AuthApi = mockk {
        coEvery { logout(any()) } answers { log += "server-logout" }
        coEvery { logoutSession(any(), any(), any()) } answers { log += "server-logout-old-session" }
    }

    private class Step(
        override val order: Int,
        private val log: MutableList<String>,
        private val action: suspend () -> Unit = {},
    ) : SessionTeardown {
        override suspend fun onLogout() {
            log += "teardown-$order"
            action()
        }
    }

    private companion object {
        const val ACCESS = "fixture-access-token"
        const val REFRESH = "fixture-refresh-token"
    }

    @Test
    fun `logout runs teardowns in order before server logout and token clear`() = runTest {
        val authState = AuthState(backgroundScope)
        authState.setLoggedIn("user-1")
        // Registered out of order: the runner sorts by order.
        val runner = SessionTeardownRunner(
            setOf(Step(30, log), Step(10, log), Step(20, log)),
            authState,
            backgroundScope,
        )
        val repository = AuthRepository(authApi, tokenStorage, authState, runner)

        repository.logout()

        assertEquals(
            listOf("teardown-10", "teardown-20", "teardown-30", "server-logout", "clear"),
            log,
        )
        assertEquals(AuthSession.LoggedOut, authState.session.value)
    }

    @Test
    fun `failing teardown does not block logout`() = runTest {
        val authState = AuthState(backgroundScope)
        authState.setLoggedIn("user-1")
        val runner = SessionTeardownRunner(
            setOf(
                Step(10, log) { throw IllegalStateException("boom") },
                Step(20, log) { awaitCancellation() }, // hangs until its timeout
                Step(30, log),
            ),
            authState,
            backgroundScope,
        )
        val repository = AuthRepository(authApi, tokenStorage, authState, runner)

        repository.logout()

        assertEquals(
            listOf("teardown-10", "teardown-20", "teardown-30", "server-logout", "clear"),
            log,
        )
        assertTrue(testScheduler.currentTime >= SessionTeardownRunner.STEP_TIMEOUT_MS)
        assertEquals(AuthSession.LoggedOut, authState.session.value)
    }

    @Test
    fun `logout cancelled mid-teardown still runs the remaining steps`() = runTest {
        val authState = AuthState(backgroundScope)
        authState.setLoggedIn("user-1")
        val firstStepRunning = CompletableDeferred<Unit>()
        val releaseFirstStep = CompletableDeferred<Unit>()
        val runner = SessionTeardownRunner(
            setOf(
                Step(10, log) {
                    firstStepRunning.complete(Unit)
                    releaseFirstStep.await()
                },
                Step(20, log),
                Step(30, log),
            ),
            authState,
            backgroundScope,
        )
        val repository = AuthRepository(authApi, tokenStorage, authState, runner)

        // The settings ViewModel's scope dies while the logout is in step 10.
        val caller = launch { repository.logout() }
        firstStepRunning.await()
        caller.cancel()
        releaseFirstStep.complete(Unit)
        advanceUntilIdle()

        assertEquals(
            listOf("teardown-10", "teardown-20", "teardown-30", "server-logout", "clear"),
            log,
        )
        assertEquals(AuthSession.LoggedOut, authState.session.value)
    }

    @Test
    fun `logout does not clear a newer session`() = runTest {
        val authState = AuthState(backgroundScope)
        authState.setLoggedIn("user-1")
        val runner = SessionTeardownRunner(
            setOf(
                Step(10, log) {
                    // A new login (same user) lands while the old session is torn down.
                    generation++
                    authState.setLoggedIn("user-1")
                },
            ),
            authState,
            backgroundScope,
        )
        val repository = AuthRepository(authApi, tokenStorage, authState, runner)

        repository.logout()

        // Only the old session is ended server-side, with its own captured tokens.
        assertEquals(listOf("teardown-10", "server-logout-old-session"), log)
        coVerify(exactly = 1) { authApi.logoutSession("https://kaiku.example.com", ACCESS, REFRESH) }
        coVerify(exactly = 0) { authApi.logout(any()) }
        verify(exactly = 0) { tokenStorage.clear() }
        assertTrue(authState.session.value is AuthSession.LoggedIn)
    }

    @Test
    fun `server logout failure still clears the session`() = runTest {
        val authState = AuthState(backgroundScope)
        authState.setLoggedIn("user-1")
        coEvery { authApi.logout(any()) } throws java.io.IOException("offline")
        val repository = AuthRepository(
            authApi, tokenStorage, authState,
            SessionTeardownRunner(setOf(Step(10, log)), authState, backgroundScope),
        )

        repository.logout()

        assertEquals(listOf("teardown-10", "clear"), log)
        assertEquals(AuthSession.LoggedOut, authState.session.value)
    }

    @Test
    fun `explicit logout tears down once although it ends in LoggedOut`() = runTest {
        val authState = AuthState(backgroundScope)
        authState.setLoggedIn("user-1")
        val runner = SessionTeardownRunner(setOf(Step(10, log)), authState, backgroundScope)
        runner.start()
        runCurrent()
        val repository = AuthRepository(authApi, tokenStorage, authState, runner)

        repository.logout()
        advanceUntilIdle()

        assertEquals(listOf("teardown-10", "server-logout", "clear"), log)
    }

    @Test
    fun `logout sends the refresh token`() = runTest {
        var path: String? = null
        var body: String? = null
        val engine = MockEngine { request ->
            path = request.url.encodedPath
            body = String(request.body.toByteArray())
            respond(content = "", status = HttpStatusCode.NoContent)
        }
        val authState = AuthState(backgroundScope)
        authState.setLoggedIn("user-1")
        val repository = AuthRepository(
            AuthApiImpl(KaikuHttpClient.forTesting(tokenStorage, authState, engine).activeClient()),
            tokenStorage,
            authState,
            SessionTeardownRunner(emptySet(), authState, backgroundScope),
        )

        repository.logout()

        assertEquals("/auth/logout", path)
        assertTrue(body.orEmpty(), body.orEmpty().contains("\"refresh_token\":\"$REFRESH\""))
        assertEquals(listOf("clear"), log)
    }

    @Test
    fun `logout without a refresh token sends no body`() = runTest {
        every { tokenStorage.getRefreshToken() } returns null
        var body: String? = null
        val engine = MockEngine { request ->
            body = String(request.body.toByteArray())
            respond(content = "", status = HttpStatusCode.NoContent)
        }
        val authState = AuthState(backgroundScope)
        val api = AuthApiImpl(KaikuHttpClient.forTesting(tokenStorage, authState, engine).activeClient())

        api.logout(null)

        assertFalse(body.orEmpty().contains("refresh_token"))
    }
}
