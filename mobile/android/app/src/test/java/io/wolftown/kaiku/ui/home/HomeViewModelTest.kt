package io.wolftown.kaiku.ui.home

import app.cash.turbine.test
import io.mockk.*
import io.wolftown.kaiku.R
import io.wolftown.kaiku.data.local.TokenStorage
import io.wolftown.kaiku.data.repository.GuildRepository
import io.wolftown.kaiku.data.ws.KaikuWebSocket
import io.wolftown.kaiku.domain.model.Channel
import io.wolftown.kaiku.domain.model.ChannelType
import io.wolftown.kaiku.domain.model.Guild
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.test.*
import org.junit.After
import org.junit.Assert.*
import org.junit.Before
import org.junit.Test

@OptIn(ExperimentalCoroutinesApi::class)
class HomeViewModelTest {

    private lateinit var guildRepository: GuildRepository
    private lateinit var webSocket: KaikuWebSocket
    private lateinit var tokenStorage: TokenStorage
    private lateinit var viewModel: HomeViewModel

    private val testDispatcher = StandardTestDispatcher()

    private val guildsFlow = MutableStateFlow<List<Guild>>(emptyList())
    private val selectedGuildIdFlow = MutableStateFlow<String?>(null)
    private val channelsFlow = MutableStateFlow<List<Channel>>(emptyList())

    private val sampleGuilds = listOf(
        Guild(id = "guild-1", name = "Guild One", memberCount = 10),
        Guild(id = "guild-2", name = "Guild Two", memberCount = 5)
    )

    private val sampleChannels = listOf(
        Channel(id = "ch-1", name = "general", channelType = ChannelType.TEXT, position = 2),
        Channel(id = "ch-2", name = "voice-lobby", channelType = ChannelType.VOICE, position = 1),
        Channel(id = "ch-3", name = "announcements", channelType = ChannelType.TEXT, categoryId = "cat-1", position = 0)
    )

    @Before
    fun setUp() {
        Dispatchers.setMain(testDispatcher)
        guildRepository = mockk(relaxed = true)
        webSocket = mockk(relaxed = true)
        tokenStorage = mockk(relaxed = true)
        every { guildRepository.guilds } returns guildsFlow
        every { guildRepository.selectedGuildId } returns selectedGuildIdFlow
        every { guildRepository.channels } returns channelsFlow
        every { tokenStorage.getServerUrl() } returns "https://example.com"
    }

    @After
    fun tearDown() {
        Dispatchers.resetMain()
    }

    // ========================================================================
    // 1. Loads guilds on init
    // ========================================================================

    @Test
    fun `loads guilds on init`() = runTest {
        coEvery { guildRepository.loadGuilds() } coAnswers {
            guildsFlow.value = sampleGuilds
        }

        viewModel = HomeViewModel(guildRepository, webSocket, tokenStorage)
        advanceUntilIdle()

        coVerify { guildRepository.loadGuilds() }
        assertEquals(sampleGuilds, viewModel.guilds.value)
    }

    // ========================================================================
    // 2. Selecting guild loads channels
    // ========================================================================

    @Test
    fun `selecting guild loads channels`() = runTest {
        coEvery { guildRepository.loadGuilds() } coAnswers {
            guildsFlow.value = sampleGuilds
        }
        coEvery { guildRepository.loadChannels("guild-1") } coAnswers {
            channelsFlow.value = sampleChannels
        }
        every { guildRepository.selectGuild("guild-1") } answers {
            selectedGuildIdFlow.value = "guild-1"
        }

        viewModel = HomeViewModel(guildRepository, webSocket, tokenStorage)
        advanceUntilIdle()

        viewModel.onGuildSelected("guild-1")
        advanceUntilIdle()

        verify { guildRepository.selectGuild("guild-1") }
        coVerify { guildRepository.loadChannels("guild-1") }
        // ViewModel sorts channels by position
        assertEquals(sampleChannels.sortedBy { it.position }, viewModel.channels.value)
    }

    // ========================================================================
    // 3. Channels sorted by position
    // ========================================================================

    @Test
    fun `channels are sorted by position`() = runTest {
        val unsortedChannels = listOf(
            Channel(id = "ch-a", name = "zeta", channelType = ChannelType.TEXT, position = 3),
            Channel(id = "ch-b", name = "alpha", channelType = ChannelType.TEXT, position = 1),
            Channel(id = "ch-c", name = "beta", channelType = ChannelType.TEXT, position = 2)
        )

        coEvery { guildRepository.loadGuilds() } just Runs
        coEvery { guildRepository.loadChannels("guild-1") } coAnswers {
            channelsFlow.value = unsortedChannels
        }
        every { guildRepository.selectGuild("guild-1") } answers {
            selectedGuildIdFlow.value = "guild-1"
        }

        viewModel = HomeViewModel(guildRepository, webSocket, tokenStorage)
        advanceUntilIdle()

        viewModel.onGuildSelected("guild-1")
        advanceUntilIdle()

        val channels = viewModel.channels.value
        assertEquals(3, channels.size)
        assertEquals("ch-b", channels[0].id) // position 1
        assertEquals("ch-c", channels[1].id) // position 2
        assertEquals("ch-a", channels[2].id) // position 3
    }

    // ========================================================================
    // 4. Loading state transitions
    // ========================================================================

    @Test
    fun `loading state transitions correctly`() = runTest {
        coEvery { guildRepository.loadGuilds() } coAnswers {
            guildsFlow.value = sampleGuilds
        }

        viewModel = HomeViewModel(guildRepository, webSocket, tokenStorage)

        viewModel.isLoading.test {
            // Initial loading state should be true (loading started in init)
            val first = awaitItem()
            if (first) {
                // Loading is true, wait for it to become false
                val second = awaitItem()
                assertFalse("Expected loading to be false after guilds loaded", second)
            } else {
                // It could already be false if dispatch was fast; advance and check
                testScheduler.advanceUntilIdle()
                // No more items expected since it's already false
            }
        }
    }

    // ========================================================================
    // 5. Error state on API failure
    // ========================================================================

    @Test
    fun `error state on API failure`() = runTest {
        coEvery { guildRepository.loadGuilds() } throws RuntimeException("Network error")

        viewModel = HomeViewModel(guildRepository, webSocket, tokenStorage)
        advanceUntilIdle()

        assertNotNull(viewModel.error.value)
        assertEquals(R.string.home_servers_load_failed, viewModel.error.value)
        assertFalse(viewModel.isLoading.value)
    }

    // ========================================================================
    // 6. Selected guild is exposed correctly
    // ========================================================================

    @Test
    fun `selected guild is exposed correctly`() = runTest {
        coEvery { guildRepository.loadGuilds() } coAnswers {
            guildsFlow.value = sampleGuilds
        }
        coEvery { guildRepository.loadChannels(any()) } just Runs
        every { guildRepository.selectGuild("guild-2") } answers {
            selectedGuildIdFlow.value = "guild-2"
        }

        viewModel = HomeViewModel(guildRepository, webSocket, tokenStorage)
        advanceUntilIdle()

        assertNull(viewModel.selectedGuild.value)

        viewModel.onGuildSelected("guild-2")
        advanceUntilIdle()

        assertEquals(sampleGuilds[1], viewModel.selectedGuild.value)
    }

    // ========================================================================
    // 7. Refresh reloads guilds
    // ========================================================================

    @Test
    fun `refresh reloads guilds`() = runTest {
        coEvery { guildRepository.loadGuilds() } coAnswers {
            guildsFlow.value = sampleGuilds
        }

        viewModel = HomeViewModel(guildRepository, webSocket, tokenStorage)
        advanceUntilIdle()

        viewModel.refresh()
        advanceUntilIdle()

        // loadGuilds should have been called twice: once in init, once in refresh
        coVerify(exactly = 2) { guildRepository.loadGuilds() }
    }

    // ========================================================================
    // 8. Error is cleared on successful refresh
    // ========================================================================

    @Test
    fun `error is cleared on successful refresh`() = runTest {
        var callCount = 0
        coEvery { guildRepository.loadGuilds() } coAnswers {
            callCount++
            if (callCount == 1) {
                throw RuntimeException("Network error")
            } else {
                guildsFlow.value = sampleGuilds
            }
        }

        viewModel = HomeViewModel(guildRepository, webSocket, tokenStorage)
        advanceUntilIdle()

        assertNotNull(viewModel.error.value)

        viewModel.refresh()
        advanceUntilIdle()

        assertNull(viewModel.error.value)
        assertEquals(sampleGuilds, viewModel.guilds.value)
    }

    // ========================================================================
    // 9. Unknown channel types are hidden (C5)
    // ========================================================================

    @Test
    fun `unknown channel types are hidden from channel list`() = runTest {
        coEvery { guildRepository.loadGuilds() } just Runs
        viewModel = HomeViewModel(guildRepository, webSocket, tokenStorage)
        advanceUntilIdle()

        channelsFlow.value = listOf(
            Channel(id = "t", name = "general", channelType = ChannelType.TEXT, position = 0),
            Channel(id = "f", name = "ideas", channelType = ChannelType.UNKNOWN, position = 1),
            Channel(id = "v", name = "lounge", channelType = ChannelType.VOICE, position = 2),
            Channel(id = "d", name = "dm", channelType = ChannelType.DM, position = 3),
        )
        advanceUntilIdle()

        assertEquals(listOf("t", "v"), viewModel.channels.value.map { it.id })
    }

    // ========================================================================
    // 10. Channel load error is shown when guilds exist
    // ========================================================================

    @Test
    fun `channel load error is exposed when guilds exist`() = runTest {
        guildsFlow.value = sampleGuilds
        coEvery { guildRepository.loadGuilds() } just Runs
        coEvery { guildRepository.loadChannels("guild-1") } throws RuntimeException("boom")

        viewModel = HomeViewModel(guildRepository, webSocket, tokenStorage)
        advanceUntilIdle()
        viewModel.onGuildSelected("guild-1")
        advanceUntilIdle()

        assertEquals(R.string.home_channels_load_failed, viewModel.error.value)
        assertTrue(viewModel.guilds.value.isNotEmpty())
    }
}
