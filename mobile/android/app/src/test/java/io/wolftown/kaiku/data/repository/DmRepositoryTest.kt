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
import io.wolftown.kaiku.testutil.Golden
import kotlinx.coroutines.ExperimentalCoroutinesApi
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
}
