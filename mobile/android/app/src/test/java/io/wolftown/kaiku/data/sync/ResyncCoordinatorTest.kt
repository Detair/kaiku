package io.wolftown.kaiku.data.sync

import io.mockk.coEvery
import io.mockk.coVerify
import io.mockk.every
import io.mockk.mockk
import io.mockk.verify
import io.wolftown.kaiku.data.repository.ChatRepository
import io.wolftown.kaiku.data.repository.DmRepository
import io.wolftown.kaiku.data.repository.GuildRepository
import io.wolftown.kaiku.data.ws.KaikuWebSocket
import io.wolftown.kaiku.data.ws.ServerEvent
import io.wolftown.kaiku.data.ws.WsJson
import io.wolftown.kaiku.domain.model.DmConversation
import io.wolftown.kaiku.testutil.Golden
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.test.TestScope
import kotlinx.coroutines.test.advanceTimeBy
import kotlinx.coroutines.test.runCurrent
import kotlinx.coroutines.test.runTest
import org.junit.Test

@OptIn(ExperimentalCoroutinesApi::class)
class ResyncCoordinatorTest {

    private val events = MutableSharedFlow<ServerEvent>(extraBufferCapacity = 64)
    private val webSocket: KaikuWebSocket = mockk(relaxed = true) {
        every { events } returns this@ResyncCoordinatorTest.events
    }
    private val dmList = MutableStateFlow(
        listOf(DmConversation(id = "dm-1"), DmConversation(id = "dm-2"))
    )
    private val chat: ChatRepository = mockk(relaxed = true) {
        every { resubscribeAll() } returns setOf("a")
        every { isDmChannelTracked(any()) } returns false
        every { setDmChannels(any()) } returns emptySet()
    }
    private val guilds: GuildRepository = mockk(relaxed = true)
    private val dms: DmRepository = mockk(relaxed = true) {
        every { conversations } returns dmList
    }

    private fun TestScope.startCoordinator(): ResyncCoordinator =
        ResyncCoordinator(webSocket, chat, guilds, dms, backgroundScope).also { it.start() }

    private suspend fun TestScope.emit(event: ServerEvent) {
        events.emit(event)
        runCurrent()
    }

    private val ready = ServerEvent.Ready(userId = "user-1", serverTime = "2026-03-12T10:03:00.250Z")

    @Test
    fun `ready resubscribes and catches up only after subscribed`() = runTest {
        startCoordinator()

        emit(ready)

        verify(exactly = 1) { chat.resubscribeAll() }
        coVerify(exactly = 0) { chat.catchUp(any()) }

        emit(ServerEvent.Subscribed("a"))

        coVerify(exactly = 1) { chat.catchUp("a") }
    }

    @Test
    fun `subscribed for a channel that was not resubscribed does not catch up`() = runTest {
        startCoordinator()
        emit(ready)

        emit(ServerEvent.Subscribed("other"))

        coVerify(exactly = 0) { chat.catchUp(any()) }
    }

    @Test
    fun `catch up falls back when no subscribed ack arrives`() = runTest {
        startCoordinator()
        emit(ready)

        advanceTimeBy(ResyncCoordinator.SUBSCRIBED_TIMEOUT_MS - 1)
        coVerify(exactly = 0) { chat.catchUp(any()) }

        advanceTimeBy(2)
        runCurrent()
        coVerify(exactly = 1) { chat.catchUp("a") }

        // A late ack does not catch up a second time.
        emit(ServerEvent.Subscribed("a"))
        coVerify(exactly = 1) { chat.catchUp("a") }
    }

    @Test
    fun `ack of an older socket is ignored after a new ready`() = runTest {
        startCoordinator()
        emit(ready)
        every { chat.resubscribeAll() } returns emptySet() // the screen closed meanwhile
        emit(ready)

        emit(ServerEvent.Subscribed("a"))
        advanceTimeBy(ResyncCoordinator.SUBSCRIBED_TIMEOUT_MS + 1)
        runCurrent()

        coVerify(exactly = 0) { chat.catchUp("a") }
    }

    @Test
    fun `ready reloads channels and dms and subscribes every dm channel`() = runTest {
        startCoordinator()

        emit(ready)

        coVerify(exactly = 1) { guilds.reloadSelectedGuildChannels() }
        coVerify(exactly = 1) { dms.loadDms() }
        verify(exactly = 1) { chat.setDmChannels(listOf("dm-1", "dm-2")) }
    }

    @Test
    fun `newly subscribed dm channel catches up after its ack`() = runTest {
        startCoordinator()
        emit(ready)

        emit(ServerEvent.Subscribed("dm-2"))

        coVerify(exactly = 1) { chat.catchUp("dm-2") }
    }

    @Test
    fun `one failing step does not abort resync`() = runTest {
        coEvery { chat.catchUp(any()) } throws IllegalStateException("boom")
        coEvery { guilds.reloadSelectedGuildChannels() } throws IllegalStateException("boom")
        startCoordinator()

        emit(ready)
        emit(ServerEvent.Subscribed("a"))

        coVerify(exactly = 1) { chat.catchUp("a") }
        coVerify(exactly = 1) { dms.loadDms() }
        verify(exactly = 1) { chat.setDmChannels(listOf("dm-1", "dm-2")) }

        // The coordinator keeps working on the next reconnect.
        emit(ready)
        verify(exactly = 2) { chat.resubscribeAll() }
    }

    @Test
    fun `dm_created tracks the new dm as a background subscription`() = runTest {
        startCoordinator()

        emit(WsJson.decodeFromString<ServerEvent>(Golden.load("ws_dm_created.json")))

        verify(exactly = 1) { chat.trackDmChannel("00000000-0000-4000-8000-00000000d101") }
    }
}
