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
        val data: ByteArray? = ByteArray(10),
    ) : AttachmentReader {
        var readCalls = 0
        var lastMax = -1L
        override fun size(uri: Uri) = size
        override fun displayName(uri: Uri) = "cat.jpg"
        override fun mimeType(uri: Uri) = "image/jpeg"
        override fun read(uri: Uri, maxBytes: Long): ByteArray? {
            readCalls++
            lastMax = maxBytes
            return data?.copyOf(minOf(data.size.toLong(), maxBytes).toInt())
        }
    }

    private fun kotlinx.coroutines.test.TestScope.repository(reader: AttachmentReader): ChatRepository {
        val webSocket = mockk<KaikuWebSocket>(relaxed = true)
        every { webSocket.events } returns MutableSharedFlow<ServerEvent>()
        val authState = mockk<AuthState>(relaxed = true)
        every { authState.currentUserId } returns MutableStateFlow<String?>("me")
        coEvery { api.getUploadLimits() } returns UploadLimits(maxUploadSize = 100)
        return ChatRepository(
            api, webSocket, KaikuJson, mockk<GuildRepository>(relaxed = true), authState,
            CoroutineScope(SupervisorJob() + UnconfinedTestDispatcher(testScheduler)),
            WallClock { 0L }, reader,
        )
    }

    @Test
    fun `oversized file rejected before upload`() = runTest {
        val reader = FakeReader(size = 101)
        val result = repository(reader).sendAttachment("c1", uri, null)

        assertTrue(result.exceptionOrNull() is ChatRepository.AttachmentException.TooLarge)
        assertEquals(0, reader.readCalls)
        coVerify(exactly = 0) { api.uploadMessageWithFile(any(), any(), any(), any(), any()) }
    }

    @Test
    fun `unknown size is read with a cap of limit plus one`() = runTest {
        val reader = FakeReader(size = null, data = ByteArray(500))
        val result = repository(reader).sendAttachment("c1", uri, null)

        assertEquals(101L, reader.lastMax)
        assertTrue(result.exceptionOrNull() is ChatRepository.AttachmentException.TooLarge)
        coVerify(exactly = 0) { api.uploadMessageWithFile(any(), any(), any(), any(), any()) }
    }

    @Test
    fun `successful upload merges the message once`() = runTest {
        coEvery { api.uploadMessageWithFile("c1", any(), "cat.jpg", "image/jpeg", "hi") } returns sent
        val repo = repository(FakeReader())

        val result = repo.sendAttachment("c1", uri, "hi")

        assertEquals(sent, result.getOrNull())
        assertEquals(listOf(sent), repo.getMessages("c1").value)
    }

    @Test
    fun `server 413 maps to too large and other errors to failed`() = runTest {
        val repo = repository(FakeReader())
        coEvery { api.uploadMessageWithFile(any(), any(), any(), any(), any()) } throws
            ApiException(HttpStatusCode.PayloadTooLarge, "x")
        assertTrue(repo.sendAttachment("c1", uri, null).exceptionOrNull()
            is ChatRepository.AttachmentException.TooLarge)

        coEvery { api.uploadMessageWithFile(any(), any(), any(), any(), any()) } throws
            ApiException(HttpStatusCode.BadRequest, "x")
        assertTrue(repo.sendAttachment("c1", uri, null).exceptionOrNull()
            is ChatRepository.AttachmentException.Failed)

        coEvery { api.uploadMessageWithFile(any(), any(), any(), any(), any()) } throws
            java.io.IOException("net")
        assertTrue(repo.sendAttachment("c1", uri, null).exceptionOrNull()
            is ChatRepository.AttachmentException.Failed)
    }

    @Test
    fun `limits are fetched once per session`() = runTest {
        coEvery { api.uploadMessageWithFile(any(), any(), any(), any(), any()) } returns sent
        val repo = repository(FakeReader())

        repo.sendAttachment("c1", uri, null)
        repo.sendAttachment("c1", uri, null)

        coVerify(exactly = 1) { api.getUploadLimits() }
    }
}
