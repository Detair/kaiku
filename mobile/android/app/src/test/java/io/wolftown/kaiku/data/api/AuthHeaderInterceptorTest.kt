package io.wolftown.kaiku.data.api

import io.mockk.every
import io.mockk.mockk
import io.mockk.slot
import io.wolftown.kaiku.data.local.TokenStorage
import okhttp3.Interceptor
import okhttp3.Protocol
import okhttp3.Request
import okhttp3.Response
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class AuthHeaderInterceptorTest {

    private fun tokenStorage(
        serverUrl: String? = "https://kaiku.example.com",
        token: String? = "t",
    ): TokenStorage = mockk {
        every { getServerUrl() } returns serverUrl
        every { getAccessToken() } returns token
    }

    /** Runs the interceptor on a fake chain and returns the request it forwarded. */
    private fun forwarded(
        storage: TokenStorage,
        url: String,
        configure: Request.Builder.() -> Unit = {},
    ): Request {
        val original = Request.Builder().url(url).apply(configure).build()
        val sent = slot<Request>()
        val chain = mockk<Interceptor.Chain> {
            every { request() } returns original
            every { proceed(capture(sent)) } answers {
                Response.Builder()
                    .request(sent.captured)
                    .protocol(Protocol.HTTP_1_1)
                    .code(200)
                    .message("OK")
                    .build()
            }
        }
        AuthHeaderInterceptor(storage).intercept(chain)
        return sent.captured
    }

    @Test
    fun `auth header added for server host`() {
        val sent = forwarded(tokenStorage(), "https://kaiku.example.com/api/files/a")
        assertEquals("Bearer t", sent.header("Authorization"))
    }

    @Test
    fun `auth header added when host case differs and default port is explicit`() {
        val sent = forwarded(
            tokenStorage(serverUrl = "https://Kaiku.Example.com/"),
            "https://kaiku.example.com:443/api/files/a"
        )
        assertEquals("Bearer t", sent.header("Authorization"))
    }

    @Test
    fun `auth header not added for foreign host`() {
        val sent = forwarded(tokenStorage(), "https://other.example/api/files/a")
        assertNull(sent.header("Authorization"))
    }

    @Test
    fun `auth header not added for look-alike host`() {
        val sent = forwarded(tokenStorage(), "https://kaiku.example.com.evil.example/a")
        assertNull(sent.header("Authorization"))
    }

    @Test
    fun `auth header not added for same host on another port`() {
        val sent = forwarded(tokenStorage(), "https://kaiku.example.com:8443/api/files/a")
        assertNull(sent.header("Authorization"))
    }

    @Test
    fun `auth header not added over plain http`() {
        val sent = forwarded(tokenStorage(), "http://kaiku.example.com/api/files/a")
        assertNull(sent.header("Authorization"))
    }

    @Test
    fun `auth header not added when server url is plain http`() {
        val sent = forwarded(
            tokenStorage(serverUrl = "http://kaiku.example.com"),
            "http://kaiku.example.com/api/files/a"
        )
        assertNull(sent.header("Authorization"))
    }

    @Test
    fun `auth header not added without token or server url`() {
        assertNull(
            forwarded(tokenStorage(token = null), "https://kaiku.example.com/a")
                .header("Authorization")
        )
        assertNull(
            forwarded(tokenStorage(serverUrl = null), "https://kaiku.example.com/a")
                .header("Authorization")
        )
    }

    @Test
    fun `existing authorization header is kept`() {
        val sent = forwarded(tokenStorage(), "https://kaiku.example.com/a") {
            header("Authorization", "Bearer other")
        }
        assertEquals(listOf("Bearer other"), sent.headers("Authorization"))
    }
}
