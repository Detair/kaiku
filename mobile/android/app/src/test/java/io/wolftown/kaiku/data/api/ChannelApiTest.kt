package io.wolftown.kaiku.data.api

import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.client.engine.mock.toByteArray
import io.ktor.http.HttpMethod
import io.ktor.http.HttpStatusCode
import io.mockk.every
import io.mockk.mockk
import io.wolftown.kaiku.data.local.AuthState
import io.wolftown.kaiku.data.local.TokenStorage
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class ChannelApiTest {

    private val tokenStorage: TokenStorage = mockk(relaxed = true)
    private val authState: AuthState = mockk(relaxed = true)

    init {
        every { tokenStorage.getServerUrl() } returns "https://kaiku.example.com"
    }

    @Test
    fun `mark read sends last_read_message_id`() = runTest {
        var method: HttpMethod? = null
        var path: String? = null
        var body: String? = null
        val engine = MockEngine { request ->
            method = request.method
            path = request.url.encodedPath
            body = String(request.body.toByteArray())
            respond(content = "", status = HttpStatusCode.OK)
        }
        val client = KaikuHttpClient.forTesting(tokenStorage, authState, engine)

        ChannelApiImpl(client.activeClient()).markAsRead("ch-1", "m1")

        assertEquals(HttpMethod.Post, method)
        assertEquals("/api/channels/ch-1/read", path)
        assertTrue(body.orEmpty(), body.orEmpty().contains("\"last_read_message_id\":\"m1\""))
    }
}
