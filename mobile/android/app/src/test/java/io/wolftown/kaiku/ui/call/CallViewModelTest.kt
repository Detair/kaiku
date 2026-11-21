package io.wolftown.kaiku.ui.call

import io.mockk.every
import io.mockk.mockk
import io.mockk.verify
import io.wolftown.kaiku.data.call.CallController
import io.wolftown.kaiku.data.call.CallDirectory
import io.wolftown.kaiku.data.call.CallState
import io.wolftown.kaiku.data.call.ShownCall
import io.wolftown.kaiku.data.repository.VoiceRepository
import io.wolftown.kaiku.data.voice.AudioRoute
import io.wolftown.kaiku.data.voice.AudioRouteManager
import io.wolftown.kaiku.data.voice.VoiceLinkState
import io.wolftown.kaiku.data.voice.VoiceSessionInfo
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.launch
import kotlinx.coroutines.test.StandardTestDispatcher
import kotlinx.coroutines.test.resetMain
import kotlinx.coroutines.test.runCurrent
import kotlinx.coroutines.test.runTest
import kotlinx.coroutines.test.setMain
import org.junit.After
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test

/** The call screen's Speaker switch acts only while the call's own voice session has the sound. */
@OptIn(ExperimentalCoroutinesApi::class)
class CallViewModelTest {
    private val dispatcher = StandardTestDispatcher()
    private val state = MutableStateFlow<CallState>(CallState.Outgoing("dm-1", "c-1", 0L, voice = true, voiceId = 1L))
    private val current = MutableStateFlow<VoiceSessionInfo?>(null)
    private val controller: CallController = mockk(relaxed = true) {
        every { state } returns this@CallViewModelTest.state
        every { shown() } answers { ShownCall("me", this@CallViewModelTest.state.value) }
    }
    private val voice: VoiceRepository = mockk(relaxed = true) {
        every { linkState } returns MutableStateFlow(VoiceLinkState.Stable)
        every { current } returns this@CallViewModelTest.current
    }
    private val audio: AudioRouteManager = mockk(relaxed = true) {
        every { currentRoute } returns MutableStateFlow(AudioRoute.Earpiece)
        every { availableRoutes } returns MutableStateFlow(setOf(AudioRoute.Earpiece, AudioRoute.Speaker))
    }
    private val directory = object : CallDirectory {
        override fun nameOf(channelId: String, userId: String): String? = null
        override fun titleOf(channelId: String): String? = "Anna"
    }

    @Before fun setUp() = Dispatchers.setMain(dispatcher)
    @After fun tearDown() = Dispatchers.resetMain()

    @Test
    fun `speaker acts only once the call's own voice session has the sound`() = runTest(dispatcher) {
        val vm = CallViewModel(controller, voice, audio, directory) { 0L }
        val watching = backgroundScope.launch { vm.ui.collect {} }
        // Ringing out; the call's session is open but has not taken the sound yet.
        current.value = VoiceSessionInfo(1L, "dm-1", callTag = 1L, hasAudio = false)
        runCurrent()
        assertFalse(vm.ui.value.speakerEnabled)
        vm.toggleSpeaker()
        verify(exactly = 0) { audio.switchRoute(any()) }

        // It has the sound: Speaker works, also while still ringing out.
        current.value = VoiceSessionInfo(1L, "dm-1", callTag = 1L, hasAudio = true)
        runCurrent()
        assertTrue(vm.ui.value.speakerEnabled)
        vm.toggleSpeaker()
        verify(exactly = 1) { audio.switchRoute(AudioRoute.Speaker) }

        // A voice channel's session with the sound is not the call's.
        current.value = VoiceSessionInfo(2L, "guild-1", callTag = null, hasAudio = true)
        runCurrent()
        assertFalse(vm.ui.value.speakerEnabled)
        watching.cancel()
    }
}
