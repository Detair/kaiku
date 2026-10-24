package io.wolftown.kaiku.data.repository

import android.net.Uri
import io.ktor.http.HttpStatusCode
import io.mockk.coEvery
import io.mockk.coVerify
import io.mockk.every
import io.mockk.mockk
import io.wolftown.kaiku.data.KaikuJson
import io.wolftown.kaiku.data.api.ApiException
import io.wolftown.kaiku.data.api.MessageApi
import io.wolftown.kaiku.data.api.UploadLimits
import io.wolftown.kaiku.data.local.AuthState
import io.wolftown.kaiku.data.ws.KaikuWebSocket
import io.wolftown.kaiku.data.ws.ServerEvent
import io.wolftown.kaiku.data.ws.WallClock
import io.wolftown.kaiku.domain.model.Message
import io.wolftown.kaiku.domain.model.User
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.test.UnconfinedTestDispatcher
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import kotlinx.serialization.json.jsonObject
import org.junit.Test

@OptIn(ExperimentalCoroutinesApi::class)
class ChatRepositoryAttachmentTest {

    private val api = mockk<MessageApi>(relaxed = true)
    private val uri = mockk<Uri>()
    private val sent = Message(
        id = "m1", channelId = "c1",
        author = User(id = "me", username = "me", displayName = "Me"),
        content = "", createdAt = "2026-10-24T10:00:00Z",
    )

    private class FakeReader(
        var size: Long? = 10,
        val data: ByteArray = ByteArray(10),
        val mime: String? = "image/jpeg",
        val openFails: Boolean = false,
    ) : AttachmentReader {
        var openCalls = 0
        override fun size(uri: Uri) = size
        override fun displayName(uri: Uri) = "cat.jpg"
        override fun mimeType(uri: Uri) = mime
        override fun open(uri: Uri): java.io.InputStream? {
            openCalls++
            return if (openFails) null else data.inputStream()
        }
    }

    private val events = MutableSharedFlow<ServerEvent>(extraBufferCapacity = 8)

    /** Stands in for the real API: reads the stream the way Ktor would. */
    private fun streamingApi(onRead: ((ByteArray) -> Unit)? = null) {
        coEvery { api.uploadMessageWithFile(any(), any(), any(), any(), any(), any()) } coAnswers {
            val open = arg<() -> java.io.InputStream>(4)
            onRead?.invoke(open().use { it.readBytes() })
            sent
        }
    }

    private fun kotlinx.coroutines.test.TestScope.repository(reader: AttachmentReader): ChatRepository {
        val webSocket = mockk<KaikuWebSocket>(relaxed = true)
        every { webSocket.events } returns events
        val authState = mockk<AuthState>(relaxed = true)
        every { authState.currentUserId } returns MutableStateFlow<String?>("me")
        coEvery { api.getUploadLimits() } returns UploadLimits(maxUploadSize = 100)
        val dispatcher = UnconfinedTestDispatcher(testScheduler)
        return ChatRepository(
            api, webSocket, KaikuJson, mockk<GuildRepository>(relaxed = true), authState,
            CoroutineScope(SupervisorJob() + dispatcher),
            WallClock { 0L }, reader, dispatcher,
        )
    }

    private fun anyUpload() = api.uploadMessageWithFile(any(), any(), any(), any(), any(), any())

    @Test
    fun `oversized file rejected before upload`() = runTest {
        val reader = FakeReader(size = 101)
        val result = repository(reader).sendAttachment("c1", uri, null)

        assertTrue(result.exceptionOrNull() is ChatRepository.AttachmentException.TooLarge)
        assertEquals(0, reader.openCalls)
        coVerify(exactly = 0) { anyUpload() }
    }

    @Test
    fun `unknown size is capped while streaming`() = runTest {
        val reader = FakeReader(size = null, data = ByteArray(500))
        coEvery { anyUpload() } coAnswers {
            arg<() -> java.io.InputStream>(4)().use { it.readBytes() }
            sent
        }
        val result = repository(reader).sendAttachment("c1", uri, null)

        assertTrue(result.exceptionOrNull() is ChatRepository.AttachmentException.TooLarge)
    }

    @Test
    fun `successful upload streams the file and merges the message once`() = runTest {
        var received: ByteArray? = null
        streamingApi { received = it }
        val repo = repository(FakeReader(data = ByteArray(10) { it.toByte() }))

        val result = repo.sendAttachment("c1", uri, "hi")

        assertEquals(sent, result.getOrNull())
        assertEquals(ByteArray(10) { it.toByte() }.toList(), received?.toList())
        assertEquals(listOf(sent), repo.getMessages("c1").value)
    }

    @Test
    fun `websocket echo of the uploaded message is deduplicated`() = runTest {
        streamingApi()
        val repo = repository(FakeReader())
        repo.sendAttachment("c1", uri, null)

        events.emit(ServerEvent.MessageNew("c1", KaikuJson.encodeToJsonElement(Message.serializer(), sent).jsonObject))

        assertEquals(listOf(sent), repo.getMessages("c1").value)
    }

    @Test
    fun `unsupported mime rejected before opening the file`() = runTest {
        val reader = FakeReader(mime = "image/heic")
        val result = repository(reader).sendAttachment("c1", uri, null)

        assertTrue(result.exceptionOrNull() is ChatRepository.AttachmentException.UnsupportedType)
        assertEquals(0, reader.openCalls)
    }

    @Test
    fun `unknown mime is rejected`() = runTest {
        val result = repository(FakeReader(mime = null)).sendAttachment("c1", uri, null)
        assertTrue(result.exceptionOrNull() is ChatRepository.AttachmentException.UnsupportedType)
    }

    @Test
    fun `file that cannot be opened maps to failed`() = runTest {
        val result = repository(FakeReader(openFails = true)).sendAttachment("c1", uri, null)
        assertTrue(result.exceptionOrNull() is ChatRepository.AttachmentException.Failed)
        coVerify(exactly = 0) { anyUpload() }
    }

    @Test
    fun `out of memory maps to failed`() = runTest {
        coEvery { anyUpload() } throws OutOfMemoryError()
        val result = repository(FakeReader()).sendAttachment("c1", uri, null)
        assertTrue(result.exceptionOrNull() is ChatRepository.AttachmentException.Failed)
    }

    @Test
    fun `server errors map to the matching exception`() = runTest {
        val repo = repository(FakeReader())
        coEvery { anyUpload() } throws ApiException(HttpStatusCode.PayloadTooLarge, "x")
        assertTrue(repo.sendAttachment("c1", uri, null).exceptionOrNull()
            is ChatRepository.AttachmentException.TooLarge)

        coEvery { anyUpload() } throws ApiException(HttpStatusCode.UnsupportedMediaType, "x", "INVALID_MIME_TYPE")
        assertTrue(repo.sendAttachment("c1", uri, null).exceptionOrNull()
            is ChatRepository.AttachmentException.UnsupportedType)

        coEvery { anyUpload() } throws ApiException(HttpStatusCode.BadRequest, "x")
        assertTrue(repo.sendAttachment("c1", uri, null).exceptionOrNull()
            is ChatRepository.AttachmentException.Failed)

        coEvery { anyUpload() } throws java.io.IOException("net")
        assertTrue(repo.sendAttachment("c1", uri, null).exceptionOrNull()
            is ChatRepository.AttachmentException.Failed)
    }

    @Test
    fun `limits are fetched once per session and dropped after a 413`() = runTest {
        streamingApi()
        val repo = repository(FakeReader())

        repo.sendAttachment("c1", uri, null)
        repo.sendAttachment("c1", uri, null)
        coVerify(exactly = 1) { api.getUploadLimits() }

        coEvery { anyUpload() } throws ApiException(HttpStatusCode.PayloadTooLarge, "x")
        repo.sendAttachment("c1", uri, null)
        repo.sendAttachment("c1", uri, null)
        coVerify(exactly = 2) { api.getUploadLimits() }
    }
}
