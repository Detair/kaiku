package io.wolftown.kaiku.ui.voice

import androidx.lifecycle.SavedStateHandle
import app.cash.turbine.test
import io.mockk.*
import io.wolftown.kaiku.data.repository.GuildRepository
import io.wolftown.kaiku.data.repository.VoiceRepository
import io.wolftown.kaiku.data.voice.AudioRoute
import io.wolftown.kaiku.data.voice.AudioRouteManager
import io.wolftown.kaiku.data.voice.WebRtcManager
import io.wolftown.kaiku.data.ws.ScreenShareInfo
import io.wolftown.kaiku.data.ws.VoiceParticipant
import org.webrtc.VideoTrack
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.test.*
import org.junit.After
import org.junit.Assert.*
import org.junit.Before
import org.junit.Test

@OptIn(ExperimentalCoroutinesApi::class)
class VoiceViewModelTest {

    private lateinit var voiceRepository: VoiceRepository
    private lateinit var guildRepository: GuildRepository
    private lateinit var audioRouteManager: AudioRouteManager
    private lateinit var webRtcManager: WebRtcManager
    private lateinit var savedStateHandle: SavedStateHandle
    private lateinit var viewModel: VoiceViewModel

    private val testDispatcher = StandardTestDispatcher()

    private val currentChannelIdFlow = MutableStateFlow<String?>(null)
    private val participantsFlow = MutableStateFlow<List<VoiceParticipant>>(emptyList())
    private val isMutedFlow = MutableStateFlow(false)
    private val isConnectedFlow = MutableStateFlow(false)
    private val screenSharesFlow = MutableStateFlow<List<ScreenShareInfo>>(emptyList())
    private val layerPreferencesFlow = MutableStateFlow<Map<String, String>>(emptyMap())
    private val errorFlow = MutableStateFlow<String?>(null)
    private val remoteVideoTracksFlow = MutableStateFlow<Map<String, VideoTrack>>(emptyMap())
    private val currentRouteFlow = MutableStateFlow(AudioRoute.Speaker)
    private val availableRoutesFlow = MutableStateFlow(setOf(AudioRoute.Speaker, AudioRoute.Earpiece))

    private val sampleParticipants = listOf(
        VoiceParticipant(
            userId = "user-1",
            username = "alice",
            displayName = "Alice",
            muted = false
        ),
        VoiceParticipant(
            userId = "user-2",
            username = "bob",
            displayName = "Bob",
            muted = true
        )
    )

    @Before
    fun setUp() {
        Dispatchers.setMain(testDispatcher)
        voiceRepository = mockk(relaxed = true)
        guildRepository = mockk(relaxed = true)
        audioRouteManager = mockk(relaxed = true)
        webRtcManager = mockk(relaxed = true)
        savedStateHandle = SavedStateHandle(mapOf("channelId" to "voice-ch-1"))

        every { voiceRepository.currentChannelId } returns currentChannelIdFlow
        every { voiceRepository.participants } returns participantsFlow
        every { voiceRepository.isMuted } returns isMutedFlow
        every { voiceRepository.isConnected } returns isConnectedFlow
        every { voiceRepository.screenShares } returns screenSharesFlow
        every { voiceRepository.layerPreferences } returns layerPreferencesFlow
        every { voiceRepository.error } returns errorFlow
        every { audioRouteManager.currentRoute } returns currentRouteFlow
        every { audioRouteManager.availableRoutes } returns availableRoutesFlow
        every { webRtcManager.remoteVideoTracks } returns remoteVideoTracksFlow
    }

    @After
    fun tearDown() {
        Dispatchers.resetMain()
    }

    private fun createViewModel(): VoiceViewModel {
        return VoiceViewModel(voiceRepository, guildRepository, audioRouteManager, webRtcManager, savedStateHandle)
    }

    // ========================================================================
    // 1. Joining channel calls voiceRepository.joinChannel
    // ========================================================================

    @Test
    fun `joining channel calls voiceRepository joinChannel`() = runTest {
        viewModel = createViewModel()
        advanceUntilIdle()

        // ViewModel joins the channel on init
        coVerify { voiceRepository.joinChannel("voice-ch-1") }
    }

    // ========================================================================
    // 2. Leaving channel calls voiceRepository.leaveChannel
    // ========================================================================

    @Test
    fun `leaving channel calls voiceRepository leaveChannel`() = runTest {
        viewModel = createViewModel()
        advanceUntilIdle()

        viewModel.onLeave()
        advanceUntilIdle()

        coVerify { voiceRepository.leaveChannel() }
    }

    // ========================================================================
    // 3. Toggle mute updates state
    // ========================================================================

    @Test
    fun `toggle mute calls voiceRepository toggleMute`() = runTest {
        viewModel = createViewModel()
        advanceUntilIdle()

        viewModel.onToggleMute()
        advanceUntilIdle()

        verify { voiceRepository.toggleMute() }
    }

    @Test
    fun `isMuted state reflects repository`() = runTest {
        viewModel = createViewModel()
        advanceUntilIdle()

        assertFalse(viewModel.isMuted.value)

        isMutedFlow.value = true
        advanceUntilIdle()

        assertTrue(viewModel.isMuted.value)
    }

    // ========================================================================
    // 4. Participants state reflects repository
    // ========================================================================

    @Test
    fun `participants state reflects repository`() = runTest {
        viewModel = createViewModel()
        advanceUntilIdle()

        assertTrue(viewModel.participants.value.isEmpty())

        participantsFlow.value = sampleParticipants
        advanceUntilIdle()

        assertEquals(sampleParticipants, viewModel.participants.value)
        assertEquals(2, viewModel.participants.value.size)
        assertEquals("alice", viewModel.participants.value[0].username)
    }

    // ========================================================================
    // 5. Disconnect on ViewModel cleared
    // ========================================================================

    @Test
    fun `disconnect on ViewModel cleared`() = runTest {
        viewModel = createViewModel()
        advanceUntilIdle()

        viewModel.onCleared()
        advanceUntilIdle()

        coVerify { voiceRepository.leaveChannel() }
    }

    // ========================================================================
    // Additional: isConnected state reflects repository
    // ========================================================================

    @Test
    fun `isConnected state reflects repository`() = runTest {
        viewModel = createViewModel()
        advanceUntilIdle()

        assertFalse(viewModel.isConnected.value)

        isConnectedFlow.value = true
        advanceUntilIdle()

        assertTrue(viewModel.isConnected.value)
    }

    // ========================================================================
    // Additional: screenShares state reflects repository
    // ========================================================================

    @Test
    fun `screenShares state reflects repository`() = runTest {
        viewModel = createViewModel()
        advanceUntilIdle()

        assertTrue(viewModel.screenShares.value.isEmpty())

        val shares = listOf(
            ScreenShareInfo(
                streamId = "stream-1",
                userId = "user-1",
                username = "alice",
                sourceLabel = "Screen",
                hasAudio = true
            )
        )
        screenSharesFlow.value = shares
        advanceUntilIdle()

        assertEquals(shares, viewModel.screenShares.value)
    }

    // ========================================================================
    // Additional: audio route switching
    // ========================================================================

    @Test
    fun `switching audio route calls audioRouteManager`() = runTest {
        viewModel = createViewModel()
        advanceUntilIdle()

        viewModel.onSwitchAudioRoute(AudioRoute.Bluetooth)

        verify { audioRouteManager.switchRoute(AudioRoute.Bluetooth) }
    }

    @Test
    fun `current audio route reflects audioRouteManager`() = runTest {
        viewModel = createViewModel()
        advanceUntilIdle()

        assertEquals(AudioRoute.Speaker, viewModel.currentRoute.value)

        currentRouteFlow.value = AudioRoute.Bluetooth
        advanceUntilIdle()

        assertEquals(AudioRoute.Bluetooth, viewModel.currentRoute.value)
    }

    @Test
    fun `available routes reflect audioRouteManager`() = runTest {
        viewModel = createViewModel()
        advanceUntilIdle()

        assertEquals(setOf(AudioRoute.Speaker, AudioRoute.Earpiece), viewModel.availableRoutes.value)

        availableRoutesFlow.value = setOf(AudioRoute.Speaker, AudioRoute.Earpiece, AudioRoute.Bluetooth)
        advanceUntilIdle()

        assertTrue(viewModel.availableRoutes.value.contains(AudioRoute.Bluetooth))
    }
}
