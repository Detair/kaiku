package io.wolftown.kaiku.data.api

import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.client.engine.mock.toByteArray
import io.ktor.http.HttpHeaders
import io.ktor.http.HttpStatusCode
import io.ktor.http.headersOf
import io.mockk.every
import io.mockk.mockk
import io.wolftown.kaiku.data.local.AuthState
import io.wolftown.kaiku.data.local.TokenStorage
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/** `POST /auth/logout` falls back to bearer-only (C25) when the body token went stale. */
class AuthApiLogoutTest {

    private data class LogoutRequest(val authorization: String?, val body: String)

    @Test
    fun `stale body token after a refresh retries logout bearer-only`() = runTest {
        var accessToken = "expired-access"
        var refreshToken = "old-refresh"
        val tokenStorage: TokenStorage = mockk(relaxed = true) {
            every { getServerUrl() } returns "https://kaiku.example.com"
            every { getAccessToken() } answers { accessToken }
            every { getRefreshToken() } answers { refreshToken }
            every { getUserId() } returns "user-1"
            every { saveTokensIfRefreshTokenIs(any(), any(), any(), any(), any()) } answers {
                accessToken = secondArg()
                refreshToken = thirdArg<String?>() ?: refreshToken
                true
            }
            every { clearIfRefreshTokenIs(any()) } returns true
        }
        val authState: AuthState = mockk(relaxed = true)
        val logouts = mutableListOf<LogoutRequest>()
        val engine = MockEngine { request ->
            when (request.url.encodedPath) {
                "/auth/refresh" -> respond(
                    content = """{"access_token":"new-access","refresh_token":"new-refresh",
                        "expires_in":900,"token_type":"Bearer"}""",
                    status = HttpStatusCode.OK,
                    headers = headersOf(HttpHeaders.ContentType, "application/json"),
                )
                "/auth/logout" -> {
                    val body = String(request.body.toByteArray())
                    logouts += LogoutRequest(request.headers[HttpHeaders.Authorization], body)
                    when {
                        // 1st: expired bearer -> 401, the client refreshes and replays.
                        logouts.size == 1 -> respond("", HttpStatusCode.Unauthorized)
                        // 2nd: the replay still carries the rotated-away refresh token.
                        body.contains("old-refresh") -> respond("", HttpStatusCode.Unauthorized)
                        else -> respond("", HttpStatusCode.NoContent)
                    }
                }
                else -> respond("", HttpStatusCode.NotFound)
            }
        }
        val api = AuthApiImpl(KaikuHttpClient.forTesting(tokenStorage, authState, engine).activeClient())

        api.logout("old-refresh")

        assertEquals(3, logouts.size)
        assertEquals("Bearer new-access", logouts[1].authorization)
        assertTrue(logouts[1].body.contains("old-refresh"))
        // The retry is bearer-only with the rotated access token.
        assertEquals("Bearer new-access", logouts[2].authorization)
        assertFalse(logouts[2].body.contains("refresh_token"))
    }

    @Test
    fun `logout of an ended session uses its own bearer and skips the interceptor`() = runTest {
        val tokenStorage: TokenStorage = mockk(relaxed = true) {
            every { getServerUrl() } returns "https://kaiku.example.com"
            every { getAccessToken() } returns "newer-session-access"
        }
        var request: LogoutRequest? = null
        var host: String? = null
        val engine = MockEngine { r ->
            host = r.url.host
            request = LogoutRequest(r.headers[HttpHeaders.Authorization], String(r.body.toByteArray()))
            respond("", HttpStatusCode.NoContent)
        }
        val api = AuthApiImpl(
            KaikuHttpClient.forTesting(tokenStorage, mockk(relaxed = true), engine).activeClient()
        )

        api.logoutSession("https://old.example.com", "old-access", "old-refresh")

        assertEquals("old.example.com", host)
        assertEquals("Bearer old-access", request?.authorization)
        assertTrue(request?.body.orEmpty().contains("\"refresh_token\":\"old-refresh\""))
    }
}
