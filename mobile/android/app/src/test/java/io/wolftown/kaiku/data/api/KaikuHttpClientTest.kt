package io.wolftown.kaiku.data.api

import io.ktor.client.engine.mock.*
import io.ktor.client.request.*
import io.ktor.http.*
import io.mockk.*
import io.wolftown.kaiku.data.KaikuJson
import io.wolftown.kaiku.data.auth.RefreshResult
import io.wolftown.kaiku.data.local.AuthState
import io.wolftown.kaiku.data.local.TokenStorage
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.async
import kotlinx.coroutines.test.runTest
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import java.io.IOException
import java.util.concurrent.atomic.AtomicInteger
import org.junit.Assert.*
import org.junit.Before
import org.junit.Test

class KaikuHttpClientTest {

    private lateinit var tokenStorage: TokenStorage
    private lateinit var authState: AuthState

    @Before
    fun setUp() {
        tokenStorage = mockk(relaxed = true)
        authState = mockk(relaxed = true)
        every { tokenStorage.getServerUrl() } returns "https://kaiku.example.com"
    }

    private fun createClient(mockEngine: MockEngine): KaikuHttpClient {
        return KaikuHttpClient.forTesting(tokenStorage, authState, mockEngine)
    }

    // ========================================================================
    // Bearer token included when available
    // ========================================================================

    @Test
    fun `requests include Bearer token when token is available`() = runTest {
        every { tokenStorage.getAccessToken() } returns "test-access-token"

        val mockEngine = MockEngine { request ->
            assertEquals(
                "Bearer test-access-token",
                request.headers[HttpHeaders.Authorization]
            )
            respond(
                content = """{"status": "ok"}""",
                status = HttpStatusCode.OK,
                headers = headersOf(HttpHeaders.ContentType, "application/json")
            )
        }

        val client = createClient(mockEngine)

        val response = client.activeClient().get("/api/test")
        assertEquals(HttpStatusCode.OK, response.status)
    }

    // ========================================================================
    // No Authorization header when no token
    // ========================================================================

    @Test
    fun `requests omit Authorization header when no token`() = runTest {
        every { tokenStorage.getAccessToken() } returns null

        val mockEngine = MockEngine { request ->
            assertNull(
                "Authorization header should not be present",
                request.headers[HttpHeaders.Authorization]
            )
            respond(
                content = """{"status": "ok"}""",
                status = HttpStatusCode.OK,
                headers = headersOf(HttpHeaders.ContentType, "application/json")
            )
        }

        val client = createClient(mockEngine)

        val response = client.activeClient().get("/api/test")
        assertEquals(HttpStatusCode.OK, response.status)
    }

    // ========================================================================
    // 401 triggers refresh and retries with new token
    // ========================================================================

    @Test
    fun `on 401 triggers token refresh and retries with new token`() = runTest {
        var requestCount = 0
        // Simulate mutable token state
        var currentAccessToken: String? = "expired-token"

        every { tokenStorage.getAccessToken() } answers { currentAccessToken }
        every { tokenStorage.getRefreshToken() } returns "valid-refresh-token"
        every { tokenStorage.getUserId() } returns "user-123"
        every {
            tokenStorage.saveTokens(any(), any(), any(), any())
        } answers {
            currentAccessToken = firstArg()
        }

        val mockEngine = MockEngine { request ->
            requestCount++
            when {
                // Refresh request
                request.url.encodedPath == "/auth/refresh" -> {
                    respond(
                        content = """{
                            "access_token": "new-access-token",
                            "refresh_token": "new-refresh-token",
                            "expires_in": 900,
                            "token_type": "Bearer",
                            "setup_required": false
                        }""",
                        status = HttpStatusCode.OK,
                        headers = headersOf(HttpHeaders.ContentType, "application/json")
                    )
                }
                // First request to /api/test -> 401
                requestCount == 1 -> {
                    respond(
                        content = """{"error": "unauthorized"}""",
                        status = HttpStatusCode.Unauthorized,
                        headers = headersOf(HttpHeaders.ContentType, "application/json")
                    )
                }
                // Retry request to /api/test -> 200
                else -> {
                    assertEquals(
                        "Bearer new-access-token",
                        request.headers[HttpHeaders.Authorization]
                    )
                    respond(
                        content = """{"status": "ok"}""",
                        status = HttpStatusCode.OK,
                        headers = headersOf(HttpHeaders.ContentType, "application/json")
                    )
                }
            }
        }

        val client = createClient(mockEngine)

        val response = client.activeClient().get("/api/test")
        assertEquals(HttpStatusCode.OK, response.status)

        // Verify tokens were saved
        verify {
            tokenStorage.saveTokens(
                accessToken = "new-access-token",
                refreshToken = "new-refresh-token",
                expiresIn = 900,
                userId = "user-123"
            )
        }
    }

    // ========================================================================
    // Refresh failure (401) triggers logout
    // ========================================================================

    @Test
    fun `on refresh failure 401 calls authState setLoggedOut`() = runTest {
        every { tokenStorage.getAccessToken() } returns "expired-token"
        every { tokenStorage.getRefreshToken() } returns "invalid-refresh-token"

        val mockEngine = MockEngine { request ->
            when {
                request.url.encodedPath == "/auth/refresh" -> {
                    respond(
                        content = """{"error": "invalid refresh token"}""",
                        status = HttpStatusCode.Unauthorized,
                        headers = headersOf(HttpHeaders.ContentType, "application/json")
                    )
                }
                else -> {
                    respond(
                        content = """{"error": "unauthorized"}""",
                        status = HttpStatusCode.Unauthorized,
                        headers = headersOf(HttpHeaders.ContentType, "application/json")
                    )
                }
            }
        }

        val client = createClient(mockEngine)

        val response = client.activeClient().get("/api/test")
        assertEquals(HttpStatusCode.Unauthorized, response.status)

        verify { authState.setLoggedOut() }
    }

    // ========================================================================
    // Refresh failure (403) triggers logout
    // ========================================================================

    @Test
    fun `on refresh failure 403 calls authState setLoggedOut`() = runTest {
        every { tokenStorage.getAccessToken() } returns "expired-token"
        every { tokenStorage.getRefreshToken() } returns "revoked-refresh-token"

        val mockEngine = MockEngine { request ->
            when {
                request.url.encodedPath == "/auth/refresh" -> {
                    respond(
                        content = """{"error": "forbidden"}""",
                        status = HttpStatusCode.Forbidden,
                        headers = headersOf(HttpHeaders.ContentType, "application/json")
                    )
                }
                else -> {
                    respond(
                        content = """{"error": "unauthorized"}""",
                        status = HttpStatusCode.Unauthorized,
                        headers = headersOf(HttpHeaders.ContentType, "application/json")
                    )
                }
            }
        }

        val client = createClient(mockEngine)

        val response = client.activeClient().get("/api/test")
        assertEquals(HttpStatusCode.Unauthorized, response.status)

        verify { authState.setLoggedOut() }
    }

    // ========================================================================
    // No refresh token available -- skip refresh, propagate 401
    // ========================================================================

    @Test
    fun `on 401 without refresh token skips refresh and propagates 401`() = runTest {
        every { tokenStorage.getAccessToken() } returns "expired-token"
        every { tokenStorage.getRefreshToken() } returns null

        val mockEngine = MockEngine {
            respond(
                content = """{"error": "unauthorized"}""",
                status = HttpStatusCode.Unauthorized,
                headers = headersOf(HttpHeaders.ContentType, "application/json")
            )
        }

        val client = createClient(mockEngine)

        val response = client.activeClient().get("/api/test")
        assertEquals(HttpStatusCode.Unauthorized, response.status)

        // Should still logout since we got a 401 with no refresh token
        verify { authState.setLoggedOut() }
        verify { tokenStorage.clear() }
    }

    // ========================================================================
    // Concurrent requests share single refresh (mutex)
    // ========================================================================

    @Test
    fun `concurrent requests during refresh do not trigger multiple refresh calls`() = runTest {
        var refreshCallCount = 0
        // Simulate mutable token state: saveTokens updates the returned value
        var currentAccessToken = "expired-token"

        every { tokenStorage.getAccessToken() } answers { currentAccessToken }
        every { tokenStorage.getRefreshToken() } returns "valid-refresh-token"
        every { tokenStorage.getUserId() } returns "user-123"
        every {
            tokenStorage.saveTokens(any(), any(), any(), any())
        } answers {
            // Simulate real token storage: update the current token
            currentAccessToken = firstArg()
        }

        val mockEngine = MockEngine { request ->
            when {
                request.url.encodedPath == "/auth/refresh" -> {
                    refreshCallCount++
                    respond(
                        content = """{
                            "access_token": "new-access-token",
                            "refresh_token": "new-refresh-token",
                            "expires_in": 900,
                            "token_type": "Bearer",
                            "setup_required": false
                        }""",
                        status = HttpStatusCode.OK,
                        headers = headersOf(HttpHeaders.ContentType, "application/json")
                    )
                }
                else -> {
                    if (request.headers[HttpHeaders.Authorization] == "Bearer expired-token") {
                        respond(
                            content = """{"error": "unauthorized"}""",
                            status = HttpStatusCode.Unauthorized,
                            headers = headersOf(HttpHeaders.ContentType, "application/json")
                        )
                    } else {
                        respond(
                            content = """{"status": "ok"}""",
                            status = HttpStatusCode.OK,
                            headers = headersOf(HttpHeaders.ContentType, "application/json")
                        )
                    }
                }
            }
        }

        val client = createClient(mockEngine)

        // Launch two sequential requests that both start with expired token.
        // The first triggers a refresh; the second should detect the token
        // was already refreshed (via the mutex double-check) and skip refresh.
        val response1 = client.activeClient().get("/api/test1")
        val response2 = client.activeClient().get("/api/test2")

        assertEquals(HttpStatusCode.OK, response1.status)
        assertEquals(HttpStatusCode.OK, response2.status)

        // Only the first request should have triggered a refresh
        assertEquals(
            "Only one refresh call should be made",
            1,
            refreshCallCount
        )
    }

    // ========================================================================
    // Base URL is set from server URL
    // ========================================================================

    @Test
    fun `base URL is set from tokenStorage serverUrl`() = runTest {
        every { tokenStorage.getAccessToken() } returns null
        every { tokenStorage.getServerUrl() } returns "https://my-server.example.com"

        val mockEngine = MockEngine { request ->
            assertEquals("my-server.example.com", request.url.host)
            assertEquals("https", request.url.protocol.name)
            respond(
                content = """{"status": "ok"}""",
                status = HttpStatusCode.OK,
                headers = headersOf(HttpHeaders.ContentType, "application/json")
            )
        }

        val client = createClient(mockEngine)

        val response = client.activeClient().get("/api/test")
        assertEquals(HttpStatusCode.OK, response.status)
    }

    // ========================================================================
    // Content-Type default is application/json
    // ========================================================================

    @Test
    fun `default content type is application json`() = runTest {
        every { tokenStorage.getAccessToken() } returns null

        val mockEngine = MockEngine { request ->
            val contentTypeHeader = request.headers[HttpHeaders.ContentType]
            assertEquals(
                "application/json",
                contentTypeHeader
            )
            respond(
                content = """{"status": "ok"}""",
                status = HttpStatusCode.OK,
                headers = headersOf(HttpHeaders.ContentType, "application/json")
            )
        }

        val client = createClient(mockEngine)

        val response = client.activeClient().get("/api/test")
        assertEquals(HttpStatusCode.OK, response.status)
    }

    // ========================================================================
    // TokenRefresher: refresh goes through the configured pipeline
    // ========================================================================

    private fun MockRequestHandleScope.refreshOk(accessToken: String = "new-access-token") = respond(
        content = """{
            "access_token": "$accessToken",
            "refresh_token": "new-refresh-token",
            "expires_in": 900,
            "token_type": "Bearer",
            "setup_required": false
        }""",
        status = HttpStatusCode.OK,
        headers = headersOf(HttpHeaders.ContentType, "application/json")
    )

    /** Mutable token state backing the relaxed TokenStorage mock. */
    private fun stubTokenState(initialAccessToken: String, expiresAtMillis: Long) {
        var currentAccessToken: String? = initialAccessToken
        every { tokenStorage.getAccessToken() } answers { currentAccessToken }
        every { tokenStorage.getAccessTokenExpiresAtMillis() } returns expiresAtMillis
        every { tokenStorage.getRefreshToken() } returns "valid-refresh-token"
        every { tokenStorage.getUserId() } returns "user-123"
        every { tokenStorage.saveTokens(any(), any(), any(), any()) } answers {
            currentAccessToken = firstArg()
        }
    }

    @Test
    fun `refresh request is sent to configured server host`() = runTest {
        stubTokenState("expired-token", expiresAtMillis = 0L)
        var refreshRequests = 0

        val mockEngine = MockEngine { request ->
            refreshRequests++
            assertEquals(URLProtocol.HTTPS, request.url.protocol)
            assertEquals("kaiku.example.com", request.url.host)
            assertEquals("/auth/refresh", request.url.encodedPath)
            assertEquals(HttpMethod.Post, request.method)
            assertNull(
                "Refresh request must not carry the access token",
                request.headers[HttpHeaders.Authorization]
            )
            val body = KaikuJson.parseToJsonElement(
                request.body.toByteArray().decodeToString()
            ).jsonObject
            assertEquals("valid-refresh-token", body["refresh_token"]?.jsonPrimitive?.content)
            refreshOk()
        }

        val client = createClient(mockEngine)

        assertEquals(RefreshResult.SUCCESS, client.forceRefresh())
        assertEquals(1, refreshRequests)
    }

    @Test
    fun `ensureFresh skips network when token valid long enough`() = runTest {
        stubTokenState("valid-token", expiresAtMillis = System.currentTimeMillis() + 10 * 60_000L)
        var requestCount = 0

        val mockEngine = MockEngine {
            requestCount++
            refreshOk()
        }

        val client = createClient(mockEngine)

        assertEquals(RefreshResult.SUCCESS, client.ensureFresh())
        assertEquals(0, requestCount)
        verify(exactly = 0) { tokenStorage.saveTokens(any(), any(), any(), any()) }
    }

    @Test
    fun `ensureFresh refreshes when token expires within lead time`() = runTest {
        stubTokenState("almost-expired-token", expiresAtMillis = System.currentTimeMillis() + 30_000L)
        var refreshRequests = 0

        val mockEngine = MockEngine { request ->
            assertEquals("/auth/refresh", request.url.encodedPath)
            refreshRequests++
            refreshOk()
        }

        val client = createClient(mockEngine)

        assertEquals(RefreshResult.SUCCESS, client.ensureFresh(minValidityMs = 60_000))
        assertEquals(1, refreshRequests)
        verify {
            tokenStorage.saveTokens(
                accessToken = "new-access-token",
                refreshToken = "new-refresh-token",
                expiresIn = 900,
                userId = "user-123"
            )
        }
    }

    @Test
    fun `forceRefresh with rejected refresh token logs out`() = runTest {
        stubTokenState("expired-token", expiresAtMillis = 0L)

        val mockEngine = MockEngine {
            respond(
                content = """{"error": "invalid refresh token"}""",
                status = HttpStatusCode.Unauthorized,
                headers = headersOf(HttpHeaders.ContentType, "application/json")
            )
        }

        val client = createClient(mockEngine)

        assertEquals(RefreshResult.AUTH_REJECTED, client.forceRefresh())
        verify { authState.setLoggedOut() }
        verify { tokenStorage.clear() }
    }

    @Test
    fun `forceRefresh network failure keeps session`() = runTest {
        stubTokenState("expired-token", expiresAtMillis = 0L)

        val mockEngine = MockEngine { throw IOException("network unreachable") }

        val client = createClient(mockEngine)

        assertEquals(RefreshResult.NETWORK_ERROR, client.forceRefresh())
        verify(exactly = 0) { authState.setLoggedOut() }
        verify(exactly = 0) { tokenStorage.clear() }
    }

    @Test
    fun `concurrent 401s trigger a single refresh`() = runTest {
        stubTokenState("expired-token", expiresAtMillis = 0L)
        val refreshRequests = AtomicInteger(0)
        val expiredArrivals = AtomicInteger(0)
        val bothSentWithExpiredToken = CompletableDeferred<Unit>()

        val mockEngine = MockEngine { request ->
            when {
                request.url.encodedPath == "/auth/refresh" -> {
                    refreshRequests.incrementAndGet()
                    refreshOk()
                }
                request.headers[HttpHeaders.Authorization] == "Bearer expired-token" -> {
                    // Hold both 401s until both requests were sent with the old token,
                    // so they genuinely race for the refresh.
                    if (expiredArrivals.incrementAndGet() == 2) {
                        bothSentWithExpiredToken.complete(Unit)
                    }
                    bothSentWithExpiredToken.await()
                    respond(
                        content = """{"error": "unauthorized"}""",
                        status = HttpStatusCode.Unauthorized,
                        headers = headersOf(HttpHeaders.ContentType, "application/json")
                    )
                }
                else -> {
                    assertEquals(
                        "Bearer new-access-token",
                        request.headers[HttpHeaders.Authorization]
                    )
                    respond(
                        content = """{"status": "ok"}""",
                        status = HttpStatusCode.OK,
                        headers = headersOf(HttpHeaders.ContentType, "application/json")
                    )
                }
            }
        }

        val client = createClient(mockEngine)

        val first = async { client.activeClient().get("/api/test1") }
        val second = async { client.activeClient().get("/api/test2") }

        assertEquals(HttpStatusCode.OK, first.await().status)
        assertEquals(HttpStatusCode.OK, second.await().status)
        assertEquals("Exactly one refresh request", 1, refreshRequests.get())
    }

    @Test
    fun `forceRefresh server error keeps session`() = runTest {
        stubTokenState("expired-token", expiresAtMillis = 0L)

        val mockEngine = MockEngine {
            respond(
                content = """{"error": "internal"}""",
                status = HttpStatusCode.ServiceUnavailable,
                headers = headersOf(HttpHeaders.ContentType, "application/json")
            )
        }

        val client = createClient(mockEngine)

        assertEquals(RefreshResult.NETWORK_ERROR, client.forceRefresh())
        verify(exactly = 0) { authState.setLoggedOut() }
        verify(exactly = 0) { tokenStorage.clear() }
        verify(exactly = 0) { tokenStorage.saveTokens(any(), any(), any(), any()) }
    }

    @Test
    fun `forceRefresh with malformed request 400 keeps session`() = runTest {
        stubTokenState("expired-token", expiresAtMillis = 0L)

        val mockEngine = MockEngine {
            respond(
                content = """{"error": "bad request"}""",
                status = HttpStatusCode.BadRequest,
                headers = headersOf(HttpHeaders.ContentType, "application/json")
            )
        }

        val client = createClient(mockEngine)

        assertEquals(RefreshResult.NETWORK_ERROR, client.forceRefresh())
        verify(exactly = 0) { authState.setLoggedOut() }
        verify(exactly = 0) { tokenStorage.clear() }
    }

    @Test
    fun `logout during in-flight refresh does not resurrect session`() = runTest {
        stubTokenState("expired-token", expiresAtMillis = 0L)
        var storedRefreshToken: String? = "valid-refresh-token"
        every { tokenStorage.getRefreshToken() } answers { storedRefreshToken }
        every { tokenStorage.clear() } answers { storedRefreshToken = null }

        val mockEngine = MockEngine {
            // User logs out while the refresh request is in flight
            tokenStorage.clear()
            refreshOk()
        }

        val client = createClient(mockEngine)

        assertEquals(RefreshResult.NO_SESSION, client.forceRefresh())
        verify(exactly = 0) { tokenStorage.saveTokens(any(), any(), any(), any()) }
    }

    @Test
    fun `new login during in-flight refresh is not clobbered`() = runTest {
        stubTokenState("expired-token", expiresAtMillis = 0L)
        var storedRefreshToken: String? = "valid-refresh-token"
        every { tokenStorage.getRefreshToken() } answers { storedRefreshToken }

        val mockEngine = MockEngine {
            // A new login stores a different refresh token while the request is in flight
            storedRefreshToken = "fresh-login-refresh-token"
            refreshOk()
        }

        val client = createClient(mockEngine)

        assertEquals(RefreshResult.SUCCESS, client.forceRefresh())
        verify(exactly = 0) { tokenStorage.saveTokens(any(), any(), any(), any()) }
        verify(exactly = 0) { authState.setLoggedOut() }
    }

    @Test
    fun `rejected refresh clears storage before logging out`() = runTest {
        stubTokenState("expired-token", expiresAtMillis = 0L)

        val mockEngine = MockEngine {
            respond(
                content = """{"error": "invalid refresh token"}""",
                status = HttpStatusCode.Unauthorized,
                headers = headersOf(HttpHeaders.ContentType, "application/json")
            )
        }

        val client = createClient(mockEngine)

        assertEquals(RefreshResult.AUTH_REJECTED, client.forceRefresh())
        verifyOrder {
            tokenStorage.clear()
            authState.setLoggedOut()
        }
    }
}
