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
    fun `qr redeem error carries the server code`() = runTest {
        val api = api(HttpStatusCode.Unauthorized, """{"error":"INVALID_TOKEN","message":"Expired"}""")
        assertEquals("INVALID_TOKEN", codeOf { api.redeemQrToken("https://kaiku.example.com", "t") })
    }

    @Test
    fun `oidc exchange error carries the server code`() = runTest {
        val api = api(HttpStatusCode.BadRequest, """{"error":"OIDC_FAILED","message":"Bad code"}""")
        assertEquals("OIDC_FAILED", codeOf { api.exchangeOidcCode("c", "s", "kaiku://auth/callback") })
    }
}
