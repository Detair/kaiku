package io.wolftown.kaiku.data.api

import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.http.HttpHeaders
import io.ktor.http.HttpStatusCode
import io.ktor.http.headersOf
import io.mockk.every
import io.mockk.mockk
import io.wolftown.kaiku.data.local.TokenStorage
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Assert.fail
import org.junit.Test

/** The server's `error` code reaches [ApiException.code], so AuthErrorMapper can match it. */
class AuthApiErrorCodeTest {

    private val tokenStorage: TokenStorage = mockk(relaxed = true) {
        every { getServerUrl() } returns "https://kaiku.example.com"
    }

    private fun api(status: HttpStatusCode, body: String): AuthApiImpl {
        val engine = MockEngine {
            respond(body, status, headersOf(HttpHeaders.ContentType, "application/json"))
        }
        return AuthApiImpl(KaikuHttpClient.forTesting(tokenStorage, mockk(relaxed = true), engine).activeClient())
    }

    private suspend fun codeOf(block: suspend () -> Unit): String? {
        try {
            block()
        } catch (e: ApiException) {
            return e.code
        }
        fail("expected ApiException")
        return null
    }

    @Test
    fun `register error carries the server code`() = runTest {
        val api = api(HttpStatusCode.Conflict, """{"error":"USER_EXISTS","message":"Username taken"}""")
        assertEquals("USER_EXISTS", codeOf { api.register("anna", "pw-long-enough") })
    }

    @Test
    fun `oidc exchange error carries the server code`() = runTest {
        val engine = MockEngine {
            respond("""{"error":"INVALID_CREDENTIALS","message":"Bad code"}""", HttpStatusCode.Unauthorized,
                headersOf(HttpHeaders.ContentType, "application/json"))
        }
        val api = LoginAttemptApiImpl(KaikuHttpClient.forTesting(tokenStorage, mockk(relaxed = true), engine).activeClient())
        // The one-time code of the callback, redeemed with the attempt's verifier (C27).
        assertEquals("INVALID_CREDENTIALS", codeOf { api.exchangeOidc("c", "verifier") })
    }

    @Test
    fun `register is a public route whose errors keep their fields`() = runTest {
        val rec = io.wolftown.kaiku.testutil.ApiRecorder(status = HttpStatusCode.TooManyRequests,
            body = """{"error":"TOO_MANY_ATTEMPTS","message":"slow down"}""", extraHeaders = mapOf("Retry-After" to "120"))
        val api = AuthApiImpl(rec.client)
        val e = try {
            api.register("anna", "pw-long-enough")
            null
        } catch (x: ApiException) {
            x
        }
        assertEquals("TOO_MANY_ATTEMPTS", e?.code)
        assertEquals(120L, e?.fields?.retryAfterSeconds)
        // Pre-session: the stored bearer of some other session never goes along.
        assertEquals(null, rec.last.headers["Authorization"])
    }
}
