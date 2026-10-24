package io.wolftown.kaiku.data.api

import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.client.engine.mock.toByteArray
import io.ktor.http.HttpHeaders
import io.ktor.http.HttpMethod
import io.ktor.http.HttpStatusCode
import io.ktor.http.headersOf
import io.mockk.every
import io.mockk.mockk
import io.wolftown.kaiku.data.local.AuthState
import io.wolftown.kaiku.data.local.TokenStorage
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class MessageApiUploadTest {

    private val tokenStorage: TokenStorage = mockk(relaxed = true)
    private val authState: AuthState = mockk(relaxed = true)
    private val json = headersOf(HttpHeaders.ContentType, "application/json")

    private val messageJson = """
        {"id":"m1","channel_id":"c1","author":{"id":"u1","username":"a","display_name":"A"},
         "content":"hi","created_at":"2026-10-24T10:00:00Z"}
    """.trimIndent()

    init {
        every { tokenStorage.getServerUrl() } returns "https://kaiku.example.com"
    }

    @Test
    fun `upload sends multipart with file and content`() = runTest {
        var method: HttpMethod? = null
        var path: String? = null
        var contentType: String? = null
        var body = ""
        val engine = MockEngine { request ->
            method = request.method
            path = request.url.encodedPath
            contentType = request.body.contentType?.toString()
            body = String(request.body.toByteArray(), Charsets.ISO_8859_1)
            respond(messageJson, HttpStatusCode.Created, json)
        }
        val client = KaikuHttpClient.forTesting(tokenStorage, authState, engine)

        val message = MessageApiImpl(client.activeClient())
            .uploadMessageWithFile("c1", "PHOTO".toByteArray(), "cat.jpg", "image/jpeg", "look")

        assertEquals("m1", message.id)
        assertEquals(HttpMethod.Post, method)
        assertEquals("/api/messages/channel/c1/upload", path)
        assertTrue(contentType.orEmpty(), contentType.orEmpty().startsWith("multipart/form-data"))
        assertTrue(body, body.contains("name=file"))
        assertTrue(body, body.contains("filename=\"cat.jpg\""))
        assertTrue(body, body.contains("Content-Type: image/jpeg"))
        assertTrue(body, body.contains("PHOTO"))
        assertTrue(body, body.contains("name=content"))
        assertTrue(body, body.contains("look"))
    }

    @Test
    fun `upload without caption omits content field`() = runTest {
        var body = ""
        val engine = MockEngine { request ->
            body = String(request.body.toByteArray(), Charsets.ISO_8859_1)
            respond(messageJson, HttpStatusCode.Created, json)
        }
        val client = KaikuHttpClient.forTesting(tokenStorage, authState, engine)

        MessageApiImpl(client.activeClient())
            .uploadMessageWithFile("c1", byteArrayOf(1), "a.png", "image/png", null)

        assertTrue(body, !body.contains("name=content"))
    }

    @Test
    fun `upload limits are read from the config endpoint`() = runTest {
        var path: String? = null
        val engine = MockEngine { request ->
            path = request.url.encodedPath
            respond(
                """{"max_avatar_size":1,"max_emoji_size":2,"max_upload_size":3}""",
                HttpStatusCode.OK, json
            )
        }
        val client = KaikuHttpClient.forTesting(tokenStorage, authState, engine)

        val limits = MessageApiImpl(client.activeClient()).getUploadLimits()

        assertEquals("/api/config/upload-limits", path)
        assertEquals(3L, limits.maxUploadSize)
    }

    @Test
    fun `payload too large becomes ApiException with status`() = runTest {
        val engine = MockEngine {
            respond(
                """{"error":"FILE_TOO_LARGE","message":"File too large"}""",
                HttpStatusCode.PayloadTooLarge, json
            )
        }
        val client = KaikuHttpClient.forTesting(tokenStorage, authState, engine)

        val e = runCatching {
            MessageApiImpl(client.activeClient())
                .uploadMessageWithFile("c1", byteArrayOf(1), "a.png", "image/png", null)
        }.exceptionOrNull() as ApiException

        assertEquals(HttpStatusCode.PayloadTooLarge, e.status)
        assertEquals("FILE_TOO_LARGE", e.code)
    }
}
