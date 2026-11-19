package io.wolftown.kaiku.data.repository

import io.mockk.coEvery
import io.mockk.every
import io.mockk.mockk
import io.wolftown.kaiku.data.KaikuJson
import io.wolftown.kaiku.data.api.DmApi
import io.wolftown.kaiku.data.local.AuthState
import io.wolftown.kaiku.data.ws.KaikuWebSocket
import io.wolftown.kaiku.data.ws.ServerEvent
import io.wolftown.kaiku.domain.AttachmentKind
import io.wolftown.kaiku.domain.model.Attachment
import io.wolftown.kaiku.domain.model.DmConversation
import io.wolftown.kaiku.domain.model.LastMessagePreview
import io.wolftown.kaiku.domain.model.Message
import io.wolftown.kaiku.domain.model.User
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.test.TestScope
import kotlinx.coroutines.test.runCurrent
import kotlinx.coroutines.test.runTest
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.encodeToJsonElement
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

/** The chat list follows the last activity and previews attachments. */
@OptIn(ExperimentalCoroutinesApi::class)
class DmRepositoryOrderTest {
    private val events = MutableSharedFlow<ServerEvent>(extraBufferCapacity = 16)
    private val webSocket: KaikuWebSocket = mockk(relaxed = true) { every { events } returns this@DmRepositoryOrderTest.events }
    private val api: DmApi = mockk(relaxed = true)
    private val authState: AuthState = mockk(relaxed = true) { every { currentUserId } returns MutableStateFlow<String?>("me") }

    private fun TestScope.repository() = DmRepository(api, webSocket, KaikuJson, authState, backgroundScope)

    private fun dm(id: String, at: String, unread: Int = 0) =
        DmConversation(id = id, unreadCount = unread, lastMessage = LastMessagePreview("$id-last", "x", createdAt = at))

    private fun message(channel: String, id: String, author: String, at: String, content: String = "hi", attachments: List<Attachment> = emptyList()) =
        ServerEvent.MessageNew(
            channel,
            KaikuJson.encodeToJsonElement(
                Message(id = id, channelId = channel, author = User(author, author, author.uppercase()), content = content,
                    attachments = attachments, createdAt = at)
            ) as JsonObject,
        )

    private suspend fun TestScope.loaded(): DmRepository {
        coEvery { api.getDms() } returns listOf(dm("anna", "2026-11-19T10:00:00Z"), dm("bert", "2026-11-18T10:00:00Z", unread = 1))
        return repository().also { it.loadDms() }
    }

    @Test
    fun `an incoming message moves its conversation to the top and counts as unread`() = runTest {
        val repo = loaded()
        events.emit(message("bert", "b2", "bert", "2026-11-19T11:00:00Z"))
        runCurrent()
        assertEquals(listOf("bert", "anna"), repo.conversations.value.map { it.id })
        assertEquals(2, repo.conversations.value.first().unreadCount)
    }

    @Test
    // The order is what this test proves; "no unread for an own message" is older behaviour it keeps.
    fun `a message I sent moves the conversation up without adding unread`() = runTest {
        val repo = loaded()
        events.emit(message("bert", "b2", "me", "2026-11-19T11:00:00Z"))
        runCurrent()
        val top = repo.conversations.value.first()
        assertEquals("bert", top.id)
        assertEquals(1, top.unreadCount)
        assertEquals("me", top.lastMessage?.userId)
    }

    @Test
    fun `a message with the same time as the top conversation still moves its conversation to the top`() = runTest {
        val repo = loaded()
        events.emit(message("bert", "b2", "bert", "2026-11-19T10:00:00Z"))
        runCurrent()
        assertEquals(listOf("bert", "anna"), repo.conversations.value.map { it.id })
    }

    @Test
    fun `a photo without text is previewed as a photo`() = runTest {
        val repo = loaded()
        events.emit(message("anna", "a2", "anna", "2026-11-19T12:00:00Z", content = "",
            attachments = listOf(Attachment("f", "IMG_1.jpg", "image/jpeg", 10, "/f"))))
        runCurrent()
        assertEquals(AttachmentKind.PHOTO, repo.conversations.value.first().lastMessage?.attachmentKind)
    }

    @Test
    fun `an encrypted message is marked so its ciphertext is never shown`() = runTest {
        val repo = loaded()
        events.emit(
            ServerEvent.MessageNew(
                "anna",
                KaikuJson.encodeToJsonElement(
                    Message(id = "a3", channelId = "anna", author = User("anna", "anna", "Anna"), content = "c2lnbmFs",
                        encrypted = true, createdAt = "2026-11-19T12:00:00Z")
                ) as JsonObject,
            )
        )
        runCurrent()
        assertTrue(repo.conversations.value.first().lastMessage!!.encrypted)
    }

    @Test
    fun `the server list is shown newest first even if it arrives in another order`() = runTest {
        coEvery { api.getDms() } returns listOf(dm("old", "2026-11-01T10:00:00Z"), dm("new", "2026-11-19T10:00:00Z"))
        val repo = repository()
        repo.loadDms()
        assertEquals(listOf("new", "old"), repo.conversations.value.map { it.id })
    }
}
