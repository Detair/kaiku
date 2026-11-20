package io.wolftown.kaiku.data.api

import io.ktor.client.engine.okhttp.OkHttp
import io.mockk.every
import io.mockk.mockk
import io.wolftown.kaiku.data.local.AuthState
import io.wolftown.kaiku.data.local.TokenStorage
import kotlinx.coroutines.test.runTest
import okhttp3.mockwebserver.MockResponse
import okhttp3.mockwebserver.MockWebServer
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Test

/**
 * A request that is sent once and is answered 401 was not processed: after the token was renewed
 * it is sent again, as a new request. Through the app's own HTTP client (its token renewal), on
 * the OkHttp engine with the guard against repeats, at a local test server.
 */
class OneShotRefreshTest {

    private val server = MockWebServer().apply { start() }

    @After fun tearDown() = server.shutdown()

    @Test fun `a single-use request answered 401 is sent again after the token was renewed`() = runTest {
        var access: String? = "expired-access"
        val tokens: TokenStorage = mockk(relaxed = true) {
            every { getServerUrl() } returns server.url("/").toString().trimEnd('/')
            every { getAccessToken() } answers { access }
            every { getRefreshToken() } returns "refresh"
            every { getUserId() } returns "u1"
            every { getSessionGeneration() } returns 1L
            every { saveTokensIfRefreshTokenIs(any(), any(), any(), any(), any()) } answers { access = secondArg(); true }
        }
        val authState: AuthState = mockk(relaxed = true)
        val engine = OkHttp.create { config { addNetworkInterceptor(OneShotRequests()) } }
        val client = KaikuHttpClient.forTesting(tokens, authState, engine).activeClient()
        server.enqueue(MockResponse().setResponseCode(401).setBody("""{"error":"TOKEN_EXPIRED"}"""))
        server.enqueue(MockResponse().setResponseCode(200).setHeader("Content-Type", "application/json").setBody(
            """{"access_token":"new-access","refresh_token":"new-refresh","expires_in":900,"token_type":"Bearer"}""",
        ))
        server.enqueue(MockResponse().setResponseCode(200).setHeader("Content-Type", "application/json").setBody("""{"state":"approved"}"""))

        val answer = ApprovalApiImpl(client).answer("c1", "approve", "", "sig")

        assertEquals("approved", answer.state)
        assertEquals(3, server.requestCount)
        val first = server.takeRequest()
        server.takeRequest() // the renewal
        val again = server.takeRequest()
        assertEquals("/api/approvals/c1/answer", again.path)
        assertEquals("Bearer new-access", again.getHeader("Authorization"))
        assertEquals("Bearer expired-access", first.getHeader("Authorization"))
    }
}
