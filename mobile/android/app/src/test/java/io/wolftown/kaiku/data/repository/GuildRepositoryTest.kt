package io.wolftown.kaiku.data.repository

import io.mockk.coEvery
import io.mockk.coVerify
import io.mockk.mockk
import io.wolftown.kaiku.data.api.ChannelApi
import io.wolftown.kaiku.data.api.GuildApi
import io.wolftown.kaiku.domain.model.Channel
import io.wolftown.kaiku.domain.model.ChannelType
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.test.TestScope
import kotlinx.coroutines.test.advanceTimeBy
import kotlinx.coroutines.test.runCurrent
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Test

@OptIn(ExperimentalCoroutinesApi::class)
class GuildRepositoryTest {

    private val guildApi: GuildApi = mockk(relaxed = true)
    private val channelApi: ChannelApi = mockk(relaxed = true)

    private fun TestScope.repository() = GuildRepository(guildApi, channelApi, backgroundScope)

    private fun channel(id: String, unread: Int = 0, last: String? = null) = Channel(
        id = id,
        name = id,
        channelType = ChannelType.TEXT,
        unreadCount = unread,
        lastMessageId = last,
    )

    private suspend fun GuildRepository.load(vararg list: Channel) {
        coEvery { channelApi.getChannels("g1") } returns list.toList()
        loadChannels("g1")
    }

    @Test
    fun `leaving a channel re-enables unread`() = runTest {
        val repository = repository()
        repository.load(channel("a"))
        repository.setActiveChannel("a")
        repository.onMessageReceived("a")
        assertEquals(0, repository.channels.value.single().unreadCount)

        repository.clearActiveChannel("a")
        repository.onMessageReceived("a")

        assertEquals(1, repository.channels.value.single().unreadCount)
    }

    @Test
    fun `clearing a channel that is no longer active keeps the new one active`() = runTest {
        val repository = repository()
        repository.load(channel("a"), channel("b"))
        repository.setActiveChannel("a")
        repository.setActiveChannel("b")

        repository.clearActiveChannel("a")
        repository.onMessageReceived("b")

        assertEquals(0, repository.channels.value.single { it.id == "b" }.unreadCount)
    }

    @Test
    fun `opening a channel posts read with the last message id`() = runTest {
        val repository = repository()
        repository.load(channel("a", unread = 3, last = "m9"))

        repository.setActiveChannel("a")
        runCurrent()

        coVerify(exactly = 1) { channelApi.markAsRead("a", "m9") }
        assertEquals(0, repository.channels.value.single().unreadCount)
    }

    @Test
    fun `messages while open are marked read once per second at most`() = runTest {
        val repository = repository()
        repository.load(channel("a"))
        repository.setActiveChannel("a")
        runCurrent()

        repository.onActiveChannelMessage("a", "m1")
        advanceTimeBy(300)
        repository.onActiveChannelMessage("a", "m2")
        advanceTimeBy(300)
        repository.onActiveChannelMessage("a", "m3")
        advanceTimeBy(300)
        runCurrent()
        coVerify(exactly = 0) { channelApi.markAsRead("a", any()) }

        advanceTimeBy(101)
        runCurrent()
        coVerify(exactly = 1) { channelApi.markAsRead("a", any()) }
        coVerify(exactly = 1) { channelApi.markAsRead("a", "m3") }

        repository.onActiveChannelMessage("a", "m4")
        advanceTimeBy(1_001)
        runCurrent()
        coVerify(exactly = 1) { channelApi.markAsRead("a", "m4") }
    }

    @Test
    fun `messages in a channel that is not open are not marked read`() = runTest {
        val repository = repository()
        repository.load(channel("a"))

        repository.onActiveChannelMessage("a", "m1")
        advanceTimeBy(2_000)
        runCurrent()

        coVerify(exactly = 0) { channelApi.markAsRead(any(), any()) }
    }
}
