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
import io.wolftown.kaiku.data.local.AuthSession
import io.wolftown.kaiku.data.local.AuthState
import io.wolftown.kaiku.data.local.TokenStorage
import kotlinx.coroutines.flow.MutableStateFlow
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
            .uploadMessageWithFile("c1", "cat.jpg", "image/jpeg", 5L, { "PHOTO".byteInputStream() }, "look")

        assertEquals("m1", message.id)
        assertEquals(HttpMethod.Post, method)
        assertEquals("/api/messages/channel/c1/upload", path)
        assertTrue(contentType.orEmpty(), contentType.orEmpty().startsWith("multipart/form-data"))
        assertTrue(body, body.contains("name=file"))
        assertTrue(body, body.contains("filename=\"cat.jpg\""))
        assertTrue(body, body.contains("Content-Type: image/jpeg"))
        // The part content, between the blank line after the headers and the next boundary.
        val filePart = body.substringAfter("name=file").substringAfter("\r\n\r\n").substringBefore("\r\n--")
        assertEquals("PHOTO", filePart)
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
            .uploadMessageWithFile("c1", "a.png", "image/png", null, { byteArrayOf(1).inputStream() }, null)

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
                .uploadMessageWithFile("c1", "a.png", "image/png", null, { byteArrayOf(1).inputStream() }, null)
        }.exceptionOrNull() as ApiException

        assertEquals(HttpStatusCode.PayloadTooLarge, e.status)
        assertEquals("FILE_TOO_LARGE", e.code)
    }

    private suspend fun bodyFor(fileName: String): String {
        var body = ""
        val engine = MockEngine { request ->
            body = String(request.body.toByteArray(), Charsets.UTF_8)
            respond(messageJson, HttpStatusCode.Created, json)
        }
        val client = KaikuHttpClient.forTesting(tokenStorage, authState, engine)
        MessageApiImpl(client.activeClient())
            .uploadMessageWithFile("c1", fileName, "image/png", 1L, { byteArrayOf(1).inputStream() }, null)
        return body
    }

    @Test
    fun `file name quotes backslashes and newlines are sanitised`() = runTest {
        val body = bodyFor("a\"b\\c\r\nd.png")
        val disposition = body.lineSequence().first { it.startsWith("Content-Disposition") }
        assertTrue(disposition, disposition.contains("filename=\"a_b_c__d.png\""))
        assertTrue(body, !body.contains("a\"b"))
        assertTrue(body, !body.contains("c\r\nd"))
    }

    @Test
    fun `non ascii file name adds RFC 5987 filename star`() = runTest {
        val body = bodyFor("Bär.png")
        assertTrue(body, body.contains("filename=\"B_r.png\""))
        assertTrue(body, body.contains("filename*=UTF-8''B%C3%A4r.png"))
    }

    @Test
    fun `upload bound to an ended session is not sent`() = runTest {
        every { authState.session } returns MutableStateFlow(AuthSession.LoggedIn("u2", sessionId = 2L))
        every { tokenStorage.getAccessToken() } returns "token-of-session-2"
        var requests = 0
        val engine = MockEngine {
            requests++
            respond(messageJson, HttpStatusCode.Created, json)
        }
        val api = MessageApiImpl(KaikuHttpClient.forTesting(tokenStorage, authState, engine).activeClient())

        val failure = runCatching {
            api.uploadMessageWithFile("c1", "a.png", "image/png", 1L, { byteArrayOf(1).inputStream() }, null, sessionId = 1L)
        }.exceptionOrNull()

        assertTrue(failure is SessionChangedException)
        assertEquals(0, requests)

        // The current session's upload goes out normally.
        api.uploadMessageWithFile("c1", "a.png", "image/png", 1L, { byteArrayOf(1).inputStream() }, null, sessionId = 2L)
        assertEquals(1, requests)
    }
}
