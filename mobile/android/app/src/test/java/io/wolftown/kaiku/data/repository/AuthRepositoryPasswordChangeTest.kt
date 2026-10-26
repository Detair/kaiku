package io.wolftown.kaiku.data.repository

import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.client.engine.mock.toByteArray
import io.ktor.http.HttpHeaders
import io.ktor.http.HttpStatusCode
import io.ktor.http.headersOf
import io.mockk.every
import io.mockk.mockk
import io.mockk.verify
import io.wolftown.kaiku.data.KaikuJson
import io.wolftown.kaiku.data.api.ApiException
import io.wolftown.kaiku.data.api.AuthApiImpl
import io.wolftown.kaiku.data.api.KaikuHttpClient
import io.wolftown.kaiku.data.local.AuthSession
import io.wolftown.kaiku.data.local.AuthState
import io.wolftown.kaiku.data.local.TokenStorage
import io.wolftown.kaiku.testutil.Golden
import io.wolftown.kaiku.ui.auth.PasswordChangeRequiredException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.test.UnconfinedTestDispatcher
import kotlinx.coroutines.test.runTest
import kotlinx.serialization.json.boolean
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import kotlin.test.assertIs
import io.wolftown.kaiku.push.PushRegistrar

/**
 * C4 forced password change in [AuthRepository], over the real [AuthApiImpl]
 * and [KaikuHttpClient] with a MockEngine server.
 */
@OptIn(ExperimentalCoroutinesApi::class)
class AuthRepositoryPasswordChangeTest {

    private data class Recorded(
        val host: String,
        val path: String,
        val authorization: String?,
        val body: String
    )

    /** Configured server URL and stored access token, changeable by a test. */
    private var serverUrl: String? = "https://kaiku.example.com"
    private var storedAccessToken: String? = null

    private val tokenStorage: TokenStorage = mockk(relaxed = true) {
        every { getServerUrl() } answers { serverUrl }
        // Nothing is stored while a change is pending, unless a test says otherwise.
        every { getAccessToken() } answers { storedAccessToken }
        every { getRefreshToken() } returns null
    }

    // UnconfinedTestDispatcher drives AuthState's stateIn synchronously (see app/src/test/AGENTS.md).
    private val authState = AuthState(CoroutineScope(SupervisorJob() + UnconfinedTestDispatcher()))

    private val requests = mutableListOf<Recorded>()

    /** Status and body of `POST /auth/me/password`. */
    private var passwordStatus = HttpStatusCode.OK
    private var passwordBody = """{"success":true,"message":"Password updated successfully.","revoked_count":0}"""

    /** Runs inside the server while a request is in flight (e.g. the user cancels meanwhile). */
    private var duringLoginRequest: () -> Unit = {}
    private var duringPasswordRequest: () -> Unit = {}
    private var duringMeRequest: () -> Unit = {}

    private val engine = MockEngine { request ->
        val path = request.url.encodedPath
        requests += Recorded(
            host = request.url.host,
            path = path,
            authorization = request.headers[HttpHeaders.Authorization],
            body = String(request.body.toByteArray())
        )
        val json = headersOf(HttpHeaders.ContentType, "application/json")
        when (path) {
            "/auth/login" -> {
                duringLoginRequest()
                respond(Golden.load("auth_response_password_change.json"), HttpStatusCode.OK, json)
            }
            "/auth/me/password" -> {
                duringPasswordRequest()
                respond(passwordBody, passwordStatus, json)
            }
            "/auth/me" -> {
                duringMeRequest()
                respond(USER_JSON, HttpStatusCode.OK, json)
            }
            else -> respond("""{"error":"NOT_FOUND"}""", HttpStatusCode.NotFound, json)
        }
    }

    private val repository = AuthRepository(
        AuthApiImpl(KaikuHttpClient.forTesting(tokenStorage, authState, engine).activeClient()),
        tokenStorage,
        authState,
        mockk<PushRegistrar>(relaxed = true)
    )

    private companion object {
        const val TEMP_ACCESS = "fixture-temporary-access-token"
        const val TEMP_REFRESH = "fixture-temporary-refresh-token"
        const val USER_JSON = """{
            "id": "user-42",
            "username": "anna",
            "display_name": "Anna",
            "email": null,
            "avatar_url": null,
            "status": "online",
            "mfa_enabled": false,
            "password_change_required": false
        }"""
    }

    @Test
    fun `login requiring password change persists no tokens`() = runTest {
        val result = repository.login("anna", "temp-pass-1")

        assertIs<PasswordChangeRequiredException>(result.exceptionOrNull())
        assertTrue(repository.hasPendingPasswordChange())
        assertEquals(listOf("/auth/login"), requests.map { it.path }) // no GET /auth/me
        verify(exactly = 0) { tokenStorage.saveTokens(any(), any(), any(), any()) }
        assertEquals(AuthSession.LoggedOut, authState.session.value)
    }

    @Test
    fun `complete password change posts with bearer then logs in`() = runTest {
        repository.login("anna", "temp-pass-1")

        val result = repository.completePasswordChange("my-own-password")

        assertEquals("user-42", result.getOrThrow().id)
        assertEquals(listOf("/auth/login", "/auth/me/password", "/auth/me"), requests.map { it.path })

        val change = requests[1]
        assertEquals("kaiku.example.com", change.host)
        assertEquals("kaiku.example.com", requests[2].host)
        assertEquals("Bearer $TEMP_ACCESS", change.authorization)
        val body = KaikuJson.parseToJsonElement(change.body).jsonObject
        assertEquals("temp-pass-1", body.getValue("current_password").jsonPrimitive.content)
        assertEquals("my-own-password", body.getValue("new_password").jsonPrimitive.content)
        assertFalse(body.getValue("revoke_others").jsonPrimitive.boolean)

        assertEquals("Bearer $TEMP_ACCESS", requests[2].authorization)
        verify(exactly = 1) {
            tokenStorage.saveTokens(
                accessToken = TEMP_ACCESS,
                refreshToken = TEMP_REFRESH,
                expiresIn = 900,
                userId = "user-42"
            )
        }
        assertEquals(AuthSession.LoggedIn("user-42"), authState.session.value)
        assertFalse(repository.hasPendingPasswordChange())
    }

    @Test
    fun `complete without pending state fails`() = runTest {
        val result = repository.completePasswordChange("my-own-password")

        assertIs<IllegalStateException>(result.exceptionOrNull())
        assertTrue(requests.isEmpty())
        verify(exactly = 0) { tokenStorage.saveTokens(any(), any(), any(), any()) }
    }

    @Test
    fun `cancel clears pending state`() = runTest {
        repository.login("anna", "temp-pass-1")
        assertTrue(repository.hasPendingPasswordChange())

        repository.cancelPasswordChange()

        assertFalse(repository.hasPendingPasswordChange())
        assertIs<IllegalStateException>(repository.completePasswordChange("my-own-password").exceptionOrNull())
        assertEquals(listOf("/auth/login"), requests.map { it.path })
    }

    @Test
    fun `cancel while the password change is in flight does not log in`() = runTest {
        repository.login("anna", "temp-pass-1")
        duringPasswordRequest = { repository.cancelPasswordChange() }

        val result = repository.completePasswordChange("my-own-password")

        assertIs<IllegalStateException>(result.exceptionOrNull())
        assertEquals(listOf("/auth/login", "/auth/me/password"), requests.map { it.path })
        verify(exactly = 0) { tokenStorage.saveTokens(any(), any(), any(), any()) }
        assertEquals(AuthSession.LoggedOut, authState.session.value)
        assertFalse(repository.hasPendingPasswordChange())
    }

    @Test
    fun `cancel during getMe does not persist the session`() = runTest {
        repository.login("anna", "temp-pass-1")
        // The pending state is cleared while GET /auth/me is in flight.
        duringMeRequest = { repository.cancelPasswordChange() }

        val result = repository.completePasswordChange("my-own-password")

        assertIs<IllegalStateException>(result.exceptionOrNull())
        verify(exactly = 0) { tokenStorage.saveTokens(any(), any(), any(), any()) }
        assertEquals(AuthSession.LoggedOut, authState.session.value)
    }

    @Test
    fun `server change while pending sends nothing and fails`() = runTest {
        repository.login("anna", "temp-pass-1")
        serverUrl = "https://other.example.com"

        val result = repository.completePasswordChange("my-own-password")

        assertIs<IllegalStateException>(result.exceptionOrNull())
        assertEquals(listOf("/auth/login"), requests.map { it.path }) // neither host got the passwords
        assertFalse(repository.hasPendingPasswordChange())
        verify(exactly = 0) { tokenStorage.saveTokens(any(), any(), any(), any()) }
    }

    @Test
    fun `explicit bearer survives a different stored token`() = runTest {
        repository.login("anna", "temp-pass-1")
        // E.g. another session got stored meanwhile: the interceptor must not swap tokens.
        storedAccessToken = "other-stored-token"

        repository.completePasswordChange("my-own-password")

        assertEquals("Bearer $TEMP_ACCESS", requests.single { it.path == "/auth/me/password" }.authorization)
        assertEquals("Bearer $TEMP_ACCESS", requests.single { it.path == "/auth/me" }.authorization)
    }

    @Test
    fun `cancel while the login is in flight installs no pending change`() = runTest {
        duringLoginRequest = { repository.cancelPasswordChange() }

        val result = repository.login("anna", "temp-pass-1")

        assertIs<IllegalStateException>(result.exceptionOrNull())
        assertFalse(repository.hasPendingPasswordChange())
        verify(exactly = 0) { tokenStorage.saveTokens(any(), any(), any(), any()) }
        assertEquals(AuthSession.LoggedOut, authState.session.value)
    }

    @Test
    fun `rejected password change keeps pending state and persists nothing`() = runTest {
        passwordStatus = HttpStatusCode.BadRequest
        passwordBody = """{"error":"VALIDATION_ERROR","message":"new_password: must differ"}"""
        repository.login("anna", "temp-pass-1")

        val error = repository.completePasswordChange("temp-pass-1").exceptionOrNull()

        assertIs<ApiException>(error)
        assertEquals("VALIDATION_ERROR", error.code)
        assertTrue(repository.hasPendingPasswordChange())
        verify(exactly = 0) { tokenStorage.saveTokens(any(), any(), any(), any()) }
        assertEquals(AuthSession.LoggedOut, authState.session.value)
    }
}
