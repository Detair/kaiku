package io.wolftown.kaiku.data.repository

import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.client.engine.mock.toByteArray
import io.ktor.http.HttpHeaders
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
import kotlinx.coroutines.async
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
        coEvery { logoutSession(any(), any(), any()) } answers { log += "server-logout" }
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
        assertEquals(listOf("teardown-10", "server-logout"), log)
        coVerify(exactly = 1) { authApi.logoutSession("https://kaiku.example.com", ACCESS, REFRESH) }
        verify(exactly = 0) { tokenStorage.clear() }
        assertTrue(authState.session.value is AuthSession.LoggedIn)
    }

    @Test
    fun `server logout failure still clears the session`() = runTest {
        val authState = AuthState(backgroundScope)
        authState.setLoggedIn("user-1")
        coEvery { authApi.logoutSession(any(), any(), any()) } throws java.io.IOException("offline")
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
        var authorization: String? = null
        val engine = MockEngine { request ->
            path = request.url.encodedPath
            body = String(request.body.toByteArray())
            authorization = request.headers[HttpHeaders.Authorization]
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
        assertEquals("Bearer $ACCESS", authorization)
        assertTrue(body.orEmpty(), body.orEmpty().contains("\"refresh_token\":\"$REFRESH\""))
        assertEquals(listOf("clear"), log)
    }

    @Test
    fun `a login landing during the logout call never sees its tokens sent`() = runTest {
        val sentAuthorizations = mutableListOf<String?>()
        val sentBodies = mutableListOf<String>()
        var accessToken = ACCESS
        var refreshToken = REFRESH
        every { tokenStorage.getAccessToken() } answers { accessToken }
        every { tokenStorage.getRefreshToken() } answers { refreshToken }
        val engine = MockEngine { request ->
            sentAuthorizations += request.headers[HttpHeaders.Authorization]
            sentBodies += String(request.body.toByteArray())
            if (sentAuthorizations.size == 1) {
                // Login B lands while the first logout request is in flight.
                generation++
                accessToken = "session-b-access"
                refreshToken = "session-b-refresh"
            }
            // Refuse every attempt, so all retries are sent.
            respond(content = "", status = HttpStatusCode.Unauthorized)
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

        assertEquals(3, sentAuthorizations.size) // body+bearer, bearer only, body only
        assertTrue(sentAuthorizations.none { it?.contains("session-b") == true })
        assertTrue(sentBodies.none { it.contains("session-b") })
        assertEquals(listOf("Bearer $ACCESS", "Bearer $ACCESS", null), sentAuthorizations)
        assertTrue(log.isEmpty()) // the newer session's tokens are not cleared
    }

    @Test
    fun `login waits until the previous session's teardown has finished`() = runTest {
        val authState = AuthState(backgroundScope)
        authState.setLoggedIn("user-a")
        val sessionA = authState.session.value as AuthSession.LoggedIn
        val release = CompletableDeferred<Unit>()
        val sessionAtLastStep = mutableListOf<AuthSession>()
        val runner = SessionTeardownRunner(
            setOf(
                Step(10, log) { release.await() },
                Step(20, log) { sessionAtLastStep += authState.session.value },
            ),
            authState,
            backgroundScope,
        )
        coEvery { authApi.login("b", "pw", null) } returns io.wolftown.kaiku.domain.model.AuthResponse(
            accessToken = "session-b-access",
            refreshToken = "session-b-refresh",
            expiresIn = 900,
            tokenType = "Bearer",
        )
        coEvery { authApi.authenticatedGetMe(any(), any()) } returns
            io.wolftown.kaiku.domain.model.User(id = "user-b", username = "b", displayName = "B")
        every { tokenStorage.saveTokens(any(), any(), any(), any()) } answers { generation++ }
        val repository = AuthRepository(authApi, tokenStorage, authState, runner)

        val logout = launch { repository.logout() }
        runCurrent() // session a's teardown is in step 10
        val login = async { repository.login("b", "pw") }
        runCurrent()
        assertTrue(authState.session.value == sessionA) // login b is waiting

        release.complete(Unit)
        advanceUntilIdle()

        assertTrue(login.await().isSuccess)
        logout.join()
        // Every step of a ran, and before b was logged in.
        assertEquals(listOf("teardown-10", "teardown-20"), log.filter { it.startsWith("teardown") })
        assertEquals(1, sessionAtLastStep.size)
        assertFalse(sessionAtLastStep.single() is AuthSession.LoggedIn &&
            (sessionAtLastStep.single() as AuthSession.LoggedIn).userId == "user-b")
        assertEquals("user-b", (authState.session.value as AuthSession.LoggedIn).userId)
    }
}
