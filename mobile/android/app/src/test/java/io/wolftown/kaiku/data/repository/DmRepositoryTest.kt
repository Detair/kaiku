package io.wolftown.kaiku.data.repository

import io.mockk.coEvery
import io.mockk.every
import io.mockk.mockk
import io.wolftown.kaiku.data.KaikuJson
import io.wolftown.kaiku.data.api.DmApi
import io.wolftown.kaiku.data.ws.KaikuWebSocket
import io.wolftown.kaiku.data.ws.ServerEvent
import io.wolftown.kaiku.data.ws.WsJson
import io.wolftown.kaiku.domain.model.DmConversation
import io.wolftown.kaiku.domain.model.LastMessagePreview
import io.wolftown.kaiku.domain.model.Message
import io.wolftown.kaiku.domain.model.User
import io.mockk.coVerify
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.encodeToJsonElement
import io.wolftown.kaiku.testutil.Golden
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.launch
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.test.TestScope
import kotlinx.coroutines.test.runCurrent
import kotlinx.coroutines.test.runTest
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.put
import org.junit.Assert.assertEquals
import org.junit.Test

@OptIn(ExperimentalCoroutinesApi::class)
class DmRepositoryTest {

    private companion object {
        // Ids from server/tests/integration/android_contract_fixtures.rs
        const val NEW_DM_ID = "00000000-0000-4000-8000-00000000d101"
    }

    private val events = MutableSharedFlow<ServerEvent>(extraBufferCapacity = 64)
    private val webSocket: KaikuWebSocket = mockk(relaxed = true) {
        every { events } returns this@DmRepositoryTest.events
    }
    private val api: DmApi = mockk(relaxed = true)

    private fun TestScope.repository() = DmRepository(api, webSocket, KaikuJson, backgroundScope)

    @Test
    fun `dm_created upserts conversation at top`() = runTest {
        coEvery { api.getDms() } returns listOf(
            DmConversation(id = "dm-old", unreadCount = 1),
            DmConversation(id = NEW_DM_ID, name = "stale copy"),
        )
        val repository = repository()
        repository.loadDms()

        events.emit(WsJson.decodeFromString<ServerEvent>(Golden.load("ws_dm_created.json")))
        runCurrent()

        val conversations = repository.conversations.value
        assertEquals(listOf(NEW_DM_ID, "dm-old"), conversations.map { it.id })
        val created = conversations.first()
        assertEquals("", created.name)
        assertEquals(listOf("Alice", "Bob"), created.participants.map { it.displayName })
        assertEquals(0, created.unreadCount)
    }

    @Test
    fun `dm_created with an undecodable channel is ignored`() = runTest {
        coEvery { api.getDms() } returns listOf(DmConversation(id = "dm-old"))
        val repository = repository()
        repository.loadDms()

        events.emit(ServerEvent.DmCreated(buildJsonObject { put("name", "no id") }))
        runCurrent()

        assertEquals(listOf("dm-old"), repository.conversations.value.map { it.id })
    }

    @Test
    fun `dm_created during a gated loadDms is kept`() = runTest {
        coEvery { api.getDms() } returns listOf(
            DmConversation(id = "dm-old"),
            DmConversation(id = "dm-deleted"),
        )
        val repository = repository()
        repository.loadDms()

        val gate = CompletableDeferred<List<DmConversation>>()
        coEvery { api.getDms() } coAnswers { gate.await() }
        val load = launch { repository.loadDms() }
        runCurrent()

        events.emit(WsJson.decodeFromString<ServerEvent>(Golden.load("ws_dm_created.json")))
        runCurrent()
        // The server deleted dm-deleted; dm-b is new on the server.
        gate.complete(listOf(DmConversation(id = "dm-b"), DmConversation(id = "dm-old")))
        load.join()

        assertEquals(listOf(NEW_DM_ID, "dm-b", "dm-old"), repository.conversations.value.map { it.id })
    }

    @Test
    fun `dm_created before a load started is not resurrected`() = runTest {
        coEvery { api.getDms() } returns emptyList()
        val repository = repository()
        events.emit(WsJson.decodeFromString<ServerEvent>(Golden.load("ws_dm_created.json")))
        runCurrent()
        assertEquals(listOf(NEW_DM_ID), repository.conversations.value.map { it.id })

        // The server no longer lists it (e.g. deleted): the next load drops it.
        repository.loadDms()

        assertEquals(emptyList<String>(), repository.conversations.value.map { it.id })
    }

    private fun messageEvent(channelId: String, id: String, content: String) = ServerEvent.MessageNew(
        channelId,
        KaikuJson.encodeToJsonElement(
            Message(
                id = id,
                channelId = channelId,
                author = User(id = "u2", username = "bob", displayName = "Bob"),
                content = content,
                createdAt = "2026-10-24T10:00:00Z",
            )
        ) as JsonObject,
    )

    @Test
    fun `message in an inactive DM increments unread and updates the preview`() = runTest {
        coEvery { api.getDms() } returns listOf(
            DmConversation(id = "dm-1", unreadCount = 1, lastMessage = LastMessagePreview("m0", "old")),
            DmConversation(id = "dm-2"),
        )
        val repository = repository()
        repository.loadDms()

        events.emit(messageEvent("dm-1", "m1", "hello"))
        runCurrent()

        val dm = repository.conversations.value.first { it.id == "dm-1" }
        assertEquals(2, dm.unreadCount)
        assertEquals("m1", dm.lastMessage?.id)
        assertEquals("hello", dm.lastMessage?.content)
        assertEquals(0, repository.conversations.value.first { it.id == "dm-2" }.unreadCount)
        coVerify(exactly = 0) { api.markAsRead(any(), any()) }
    }

    @Test
    fun `message in the active DM marks read`() = runTest {
        coEvery { api.getDms() } returns listOf(DmConversation(id = "dm-1"))
        val repository = repository()
        repository.loadDms()
        repository.setActiveChannel("dm-1")

        events.emit(messageEvent("dm-1", "m1", "hello"))
        runCurrent()

        val dm = repository.conversations.value.single()
        assertEquals(0, dm.unreadCount)
        assertEquals("m1", dm.lastMessage?.id)
        coVerify(exactly = 1) { api.markAsRead("dm-1", "m1") }
    }

    @Test
    fun `message in a channel that is not a DM is ignored`() = runTest {
        coEvery { api.getDms() } returns listOf(DmConversation(id = "dm-1"))
        val repository = repository()
        repository.loadDms()

        events.emit(messageEvent("guild-channel", "m1", "hello"))
        runCurrent()

        assertEquals(0, repository.conversations.value.single().unreadCount)
    }

    @Test
    fun `leaving the active DM re-enables unread`() = runTest {
        coEvery { api.getDms() } returns listOf(DmConversation(id = "dm-1"))
        val repository = repository()
        repository.loadDms()
        repository.setActiveChannel("dm-1")
        repository.clearActiveChannel("dm-1")

        events.emit(messageEvent("dm-1", "m1", "hello"))
        runCurrent()

        assertEquals(1, repository.conversations.value.single().unreadCount)
    }
}
