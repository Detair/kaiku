package io.wolftown.kaiku.data.api

import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.client.engine.mock.toByteArray
import io.ktor.http.HttpHeaders
import io.ktor.http.HttpStatusCode
import io.mockk.every
import io.mockk.mockk
import io.wolftown.kaiku.data.local.TokenStorage
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * `POST /auth/logout` for a captured session uses only the tokens passed in:
 * never the stored ones, never a refresh (C25 fallbacks).
 */
class AuthApiLogoutTest {

    private data class Sent(val host: String, val authorization: String?, val body: String)

    private val tokenStorage: TokenStorage = mockk(relaxed = true) {
        every { getServerUrl() } returns "https://kaiku.example.com"
        // Another session's tokens are stored; they must never be sent.
        every { getAccessToken() } returns "stored-access"
        every { getRefreshToken() } returns "stored-refresh"
    }

    private fun api(status: (Int) -> HttpStatusCode, sent: MutableList<Sent>): AuthApiImpl {
        val engine = MockEngine { request ->
            sent += Sent(
                request.url.host,
                request.headers[HttpHeaders.Authorization],
                String(request.body.toByteArray()),
            )
            respond("", status(sent.size))
        }
        return AuthApiImpl(KaikuHttpClient.forTesting(tokenStorage, mockk(relaxed = true), engine).activeClient())
    }

    @Test
    fun `first attempt sends the captured bearer and refresh token`() = runTest {
        val sent = mutableListOf<Sent>()

        api({ HttpStatusCode.NoContent }, sent)
            .logoutSession("https://old.example.com", "old-access", "old-refresh")

        assertEquals(1, sent.size)
        assertEquals("old.example.com", sent[0].host)
        assertEquals("Bearer old-access", sent[0].authorization)
        assertTrue(sent[0].body.contains("\"refresh_token\":\"old-refresh\""))
    }

    @Test
    fun `stale body token is retried bearer-only`() = runTest {
        val sent = mutableListOf<Sent>()

        api({ n -> if (n == 1) HttpStatusCode.Unauthorized else HttpStatusCode.NoContent }, sent)
            .logoutSession("https://kaiku.example.com", "captured-access", "rotated-away-refresh")

        assertEquals(2, sent.size)
        assertEquals("Bearer captured-access", sent[1].authorization)
        assertTrue(!sent[1].body.contains("refresh_token"))
    }

    @Test
    fun `expired captured access falls back to the body token alone`() = runTest {
        val sent = mutableListOf<Sent>()

        api({ n -> if (n < 3) HttpStatusCode.Unauthorized else HttpStatusCode.NoContent }, sent)
            .logoutSession("https://kaiku.example.com", "expired-access", "captured-refresh")

        assertEquals(3, sent.size)
        assertNull(sent[2].authorization)
        assertTrue(sent[2].body.contains("\"refresh_token\":\"captured-refresh\""))
        // No refresh request, and nothing stored was ever sent.
        assertTrue(sent.none { it.authorization?.contains("stored") == true || it.body.contains("stored") })
    }

    @Test
    fun `without a refresh token only the bearer request is sent`() = runTest {
        val sent = mutableListOf<Sent>()

        api({ HttpStatusCode.Unauthorized }, sent)
            .logoutSession("https://kaiku.example.com", "captured-access", null)

        assertEquals(1, sent.size)
        assertEquals("Bearer captured-access", sent[0].authorization)
    }
}
