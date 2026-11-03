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
import io.wolftown.kaiku.data.api.LoginAttemptApiImpl
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
import io.wolftown.kaiku.push.PushRegistrar

/** A collected sign-in attempt only persists (or clears) for its own sign-in and server. */
@OptIn(ExperimentalCoroutinesApi::class)
class AuthRepositoryLoginRaceTest {

    private var serverUrl: String? = "https://a.example.com"

    private val authApi: AuthApi = mockk()
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

    @Test
    fun `server change during login stores nothing`() = runTest {
        coEvery { authApi.authenticatedGetMe(any(), any()) } returns
            User(id = "user-a", username = "anna", displayName = "Anna")

        val result = repository.completeAttempt("https://a.example.com", "pw") {
            serverUrl = "https://b.example.com" // the user switched servers meanwhile
            response("access-a")
        }

        assertTrue(result.isFailure)
        verify(exactly = 0) { tokenStorage.saveTokens(any(), any(), any(), any()) }
        assertEquals(AuthSession.LoggedOut, authState.session.value)
    }

    @Test
    fun `stale login failing after a newer login keeps the newer session`() = runTest {
        coEvery { authApi.authenticatedGetMe("access-new", any()) } returns
            User(id = "user-new", username = "new", displayName = "New")

        val result = repository.completeAttempt("https://a.example.com", "pw") {
            // A newer sign-in completes while the old collect is still in flight...
            assertTrue(repository.completeAttempt("https://a.example.com", "pw") { response("access-new") }.isSuccess)
            // ...then the old one fails.
            throw IOException("timeout")
        }

        assertTrue(result.isFailure)
        verify(exactly = 1) { tokenStorage.saveTokens("access-new", "refresh-access-new", 900, "user-new") }
        verify(exactly = 0) { tokenStorage.clear() }
        assertEquals("user-new", (authState.session.value as AuthSession.LoggedIn).userId)
    }

    @Test
    fun `server switch during login sends the token nowhere else and clears nothing`() = runTest {
        every { tokenStorage.getAccessToken() } returns null
        val requests = mutableListOf<Triple<String, String, String?>>() // host, path, Authorization
        val engine = MockEngine { request ->
            requests += Triple(request.url.host, request.url.encodedPath, request.headers[HttpHeaders.Authorization])
            val json = headersOf(HttpHeaders.ContentType, "application/json")
            if (request.url.encodedPath == "/auth/login-attempts/a-1/collect") {
                serverUrl = "https://b.example.com" // switched while the collect is in flight
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
        val client = KaikuHttpClient.forTesting(tokenStorage, authState, engine).activeClient()
        val attempts = LoginAttemptApiImpl(client)
        val realRepository = AuthRepository(
            AuthApiImpl(client),
            tokenStorage,
            authState,
            mockk<PushRegistrar>(relaxed = true),
            noTeardown(authState)
        )

        val result = realRepository.completeAttempt("https://a.example.com", null) { attempts.collect("a-1", "verifier") }

        assertTrue(result.isFailure)
        // The minted session never reaches B: A gets the collect, then the logout that ends it there.
        // (How many logout tries run depends on the bounded best-effort timeout, which runs on the
        // test's virtual clock while the mock engine answers on real threads: at least one.)
        assertTrue(requests.all { it.first == "a.example.com" })
        assertEquals("/auth/login-attempts/a-1/collect", requests.first().second)
        val logouts = requests.drop(1)
        assertTrue(logouts.isNotEmpty() && logouts.all { it.second == "/auth/logout" })
        assertEquals("Bearer access-a", logouts.first().third)
        verify(exactly = 0) { tokenStorage.clear() }
        verify(exactly = 0) { tokenStorage.saveTokens(any(), any(), any(), any()) }
        assertEquals(AuthSession.LoggedOut, authState.session.value)
    }
}
