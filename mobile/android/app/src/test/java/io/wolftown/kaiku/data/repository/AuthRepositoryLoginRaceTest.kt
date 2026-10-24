package io.wolftown.kaiku.data.repository

import io.wolftown.kaiku.testutil.noTeardown
import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.http.HttpHeaders
import io.ktor.http.HttpStatusCode
import io.ktor.http.headersOf
import io.mockk.coEvery
import io.mockk.every
import io.mockk.mockk
import io.mockk.verify
import io.wolftown.kaiku.data.api.AuthApi
import io.wolftown.kaiku.data.api.AuthApiImpl
import io.wolftown.kaiku.data.api.KaikuHttpClient
import io.wolftown.kaiku.data.local.AuthSession
import io.wolftown.kaiku.data.local.AuthState
import io.wolftown.kaiku.data.local.TokenStorage
import io.wolftown.kaiku.domain.model.AuthResponse
import io.wolftown.kaiku.domain.model.User
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.test.UnconfinedTestDispatcher
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import java.io.IOException

/** A login only persists (or clears) for its own attempt and server. */
@OptIn(ExperimentalCoroutinesApi::class)
class AuthRepositoryLoginRaceTest {

    private var serverUrl: String? = "https://a.example.com"

    private val authApi: AuthApi = mockk()
    private val tokenStorage: TokenStorage = mockk(relaxed = true) {
        every { getServerUrl() } answers { serverUrl }
    }

    // UnconfinedTestDispatcher drives AuthState's stateIn synchronously (see app/src/test/AGENTS.md).
    private val authState = AuthState(CoroutineScope(SupervisorJob() + UnconfinedTestDispatcher()))
    private val repository = AuthRepository(authApi, tokenStorage, authState, noTeardown(authState))

    private fun response(access: String) = AuthResponse(
        accessToken = access,
        refreshToken = "refresh-$access",
        expiresIn = 900,
        tokenType = "Bearer"
    )

    @Test
    fun `server change during login stores nothing`() = runTest {
        coEvery { authApi.login("anna", "pw", null) } coAnswers {
            serverUrl = "https://b.example.com" // the user switched servers meanwhile
            response("access-a")
        }
        coEvery { authApi.authenticatedGetMe(any(), any()) } returns
            User(id = "user-a", username = "anna", displayName = "Anna")

        val result = repository.login("anna", "pw")

        assertTrue(result.isFailure)
        verify(exactly = 0) { tokenStorage.saveTokens(any(), any(), any(), any()) }
        assertEquals(AuthSession.LoggedOut, authState.session.value)
    }

    @Test
    fun `stale login failing after a newer login keeps the newer session`() = runTest {
        coEvery { authApi.login("new", "pw", null) } returns response("access-new")
        coEvery { authApi.authenticatedGetMe("access-new", any()) } returns
            User(id = "user-new", username = "new", displayName = "New")
        coEvery { authApi.login("old", "pw", null) } coAnswers {
            // A newer login completes while the old request is still in flight...
            assertTrue(repository.login("new", "pw").isSuccess)
            // ...then the old one fails.
            throw IOException("timeout")
        }

        val result = repository.login("old", "pw")

        assertTrue(result.isFailure)
        verify(exactly = 1) { tokenStorage.saveTokens("access-new", "refresh-access-new", 900, "user-new") }
        verify(exactly = 0) { tokenStorage.clear() }
        assertEquals(AuthSession.LoggedIn("user-new"), authState.session.value)
    }

    @Test
    fun `server switch during login sends the token nowhere else and clears nothing`() = runTest {
        every { tokenStorage.getAccessToken() } returns null
        val requests = mutableListOf<Pair<String, String?>>() // host to Authorization
        val engine = MockEngine { request ->
            requests += request.url.host to request.headers[HttpHeaders.Authorization]
            val json = headersOf(HttpHeaders.ContentType, "application/json")
            if (request.url.encodedPath == "/auth/login") {
                serverUrl = "https://b.example.com" // switched while the login is in flight
                respond(
                    """{"access_token":"access-a","refresh_token":"refresh-a","expires_in":900,
                    "token_type":"Bearer","setup_required":false,"password_change_required":false}""",
                    HttpStatusCode.OK,
                    json
                )
            } else {
                respond("""{"error":"INVALID_TOKEN"}""", HttpStatusCode.Unauthorized, json)
            }
        }
        val realRepository = AuthRepository(
            AuthApiImpl(KaikuHttpClient.forTesting(tokenStorage, authState, engine).activeClient()),
            tokenStorage,
            authState,
            noTeardown(authState)
        )

        val result = realRepository.login("anna", "pw")

        assertTrue(result.isFailure)
        assertEquals(listOf("a.example.com"), requests.map { it.first }) // only the login itself
        assertTrue(requests.none { it.second == "Bearer access-a" })
        verify(exactly = 0) { tokenStorage.clear() }
        verify(exactly = 0) { tokenStorage.saveTokens(any(), any(), any(), any()) }
        assertEquals(AuthSession.LoggedOut, authState.session.value)
    }
}
