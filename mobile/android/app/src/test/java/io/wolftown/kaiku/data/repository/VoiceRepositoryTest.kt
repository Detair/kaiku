package io.wolftown.kaiku.data.repository

import android.content.Context
import app.cash.turbine.test
import io.mockk.coEvery
import io.mockk.coVerify
import io.mockk.every
import io.mockk.mockk
import io.mockk.mockkObject
import io.mockk.slot
import io.mockk.unmockkObject
import io.mockk.verify
import io.wolftown.kaiku.data.voice.AudioRouteManager
import io.wolftown.kaiku.data.voice.VoiceServiceEvents
import io.wolftown.kaiku.data.voice.WebRtcManager
import io.wolftown.kaiku.data.ws.ClientEvent
import io.wolftown.kaiku.data.ws.KaikuWebSocket
import io.wolftown.kaiku.data.ws.ServerEvent
import io.wolftown.kaiku.service.VoiceCallService
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.test.StandardTestDispatcher
import kotlinx.coroutines.test.advanceUntilIdle
import kotlinx.coroutines.test.resetMain
import kotlinx.coroutines.test.runTest
import kotlinx.coroutines.test.setMain
import org.junit.After
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test

/**
 * Unit tests for [VoiceRepository]'s dual-PC signaling wiring.
 *
 * Verifies:
 * - Publisher offer / ICE candidate callbacks produce correct WS sends with
 *   `pcType` labels.
 * - Incoming `VoicePublisherAnswer` / `VoiceSubscriberOffer` / `VoiceIceCandidate`
 *   events route to the matching `WebRtcManager` method.
 * - The repository's `_isConnected` state follows `WebRtcManager.voiceIceConnected`.
 *
 * Out of scope:
 * - Error propagation (`onError` → `_error`); covered by Phase 2 workstream B.2.
 * - Screen share / participant state.
 * - Cleanup race conditions on `iceConnectedJob.cancel()`.
 */
@OptIn(ExperimentalCoroutinesApi::class)
class VoiceRepositoryTest {

    private lateinit var webRtcManager: WebRtcManager
    private lateinit var webSocket: KaikuWebSocket
    private lateinit var audioRouteManager: AudioRouteManager
    private lateinit var voiceServiceEvents: VoiceServiceEvents
    private lateinit var context: Context
    private lateinit var repository: VoiceRepository

    private val testDispatcher = StandardTestDispatcher()

    // Mutable backing flows for the WebSocket event stream and the publisher-
    // / subscriber-side ICE-connected StateFlow. Tests drive these to simulate
    // server events and dual-PC state transitions.
    private val wsEvents = MutableSharedFlow<ServerEvent>(extraBufferCapacity = 8)
    private val voiceIceConnected = MutableStateFlow(false)

    // Callback slots that capture what VoiceRepository wires into WebRtcManager.
    private val onPublisherOfferSlot = slot<(String) -> Unit>()
    private val onPublisherIceSlot = slot<(String) -> Unit>()
    private val onSubscriberAnswerSlot = slot<(String) -> Unit>()
    private val onSubscriberIceSlot = slot<(String) -> Unit>()

    @Before
    fun setUp() {
        Dispatchers.setMain(testDispatcher)
        webRtcManager = mockk(relaxed = true)
        webSocket = mockk(relaxed = true)
        audioRouteManager = mockk(relaxed = true)
        voiceServiceEvents = mockk(relaxed = true)
        context = mockk(relaxed = true)

        // VoiceRepository.joinChannel calls VoiceCallService.start(...), which constructs
        // an Android Intent and would throw "Method putExtra in android.content.Intent not
        // mocked" under the default JVM test target. Stub the companion to a no-op.
        mockkObject(VoiceCallService.Companion)
        every { VoiceCallService.start(any(), any(), any()) } answers {}
        every { VoiceCallService.stop(any()) } answers {}

        every { webSocket.events } returns wsEvents
        every { webRtcManager.voiceIceConnected } returns voiceIceConnected
        every { voiceServiceEvents.events } returns MutableSharedFlow()

        // Capture the callbacks so tests can invoke them directly.
        every { webRtcManager.onPublisherOffer = capture(onPublisherOfferSlot) } answers {}
        every { webRtcManager.onPublisherIceCandidate = capture(onPublisherIceSlot) } answers {}
        every { webRtcManager.onSubscriberAnswer = capture(onSubscriberAnswerSlot) } answers {}
        every { webRtcManager.onSubscriberIceCandidate = capture(onSubscriberIceSlot) } answers {}

        repository = VoiceRepository(
            webRtcManager = webRtcManager,
            webSocket = webSocket,
            audioRouteManager = audioRouteManager,
            voiceServiceEvents = voiceServiceEvents,
            context = context,
        )
    }

    @After
    fun tearDown() {
        unmockkObject(VoiceCallService.Companion)
        Dispatchers.resetMain()
    }

    @Test
    fun `joinChannel sends VoicePublisherOffer when onPublisherOffer fires`() = runTest {
        repository.joinChannel("ch-1")
        advanceUntilIdle()

        // Invoke the captured publisher-offer callback as if WebRtcManager produced an SDP.
        onPublisherOfferSlot.captured.invoke("v=0\r\no=- 1 IN IP4 0.0.0.0\r\n")

        verify { webSocket.send(match<ClientEvent.VoicePublisherOffer> { event ->
            event.channelId == "ch-1" && event.sdp.startsWith("v=0")
        }) }
    }

    @Test
    fun `joinChannel sends VoiceIceCandidate with pcType=publisher for publisher ICE`() = runTest {
        repository.joinChannel("ch-1")
        advanceUntilIdle()

        onPublisherIceSlot.captured.invoke("""{"candidate":"c0","sdpMLineIndex":0,"sdpMid":"0"}""")

        verify { webSocket.send(match<ClientEvent.VoiceIceCandidate> { event ->
            event.channelId == "ch-1" && event.pcType == "publisher" && event.candidate.contains("c0")
        }) }
    }

    @Test
    fun `joinChannel sends VoiceIceCandidate with pcType=subscriber for subscriber ICE`() = runTest {
        repository.joinChannel("ch-1")
        advanceUntilIdle()

        onSubscriberIceSlot.captured.invoke("""{"candidate":"c1","sdpMLineIndex":0,"sdpMid":"0"}""")

        verify { webSocket.send(match<ClientEvent.VoiceIceCandidate> { event ->
            event.channelId == "ch-1" && event.pcType == "subscriber" && event.candidate.contains("c1")
        }) }
    }

    @Test
    fun `VoicePublisherAnswer event routes to WebRtcManager handlePublisherAnswer`() = runTest {
        repository.joinChannel("ch-1")
        advanceUntilIdle()

        wsEvents.emit(ServerEvent.VoicePublisherAnswer(channelId = "ch-1", sdp = "v=0\r\no=answer\r\n"))
        advanceUntilIdle()

        coVerify { webRtcManager.handlePublisherAnswer(match { it.startsWith("v=0") }) }
    }

    @Test
    fun `VoiceSubscriberOffer event routes to WebRtcManager handleSubscriberOffer`() = runTest {
        repository.joinChannel("ch-1")
        advanceUntilIdle()

        wsEvents.emit(ServerEvent.VoiceSubscriberOffer(channelId = "ch-1", sdp = "v=0\r\no=offer\r\n"))
        advanceUntilIdle()

        coVerify { webRtcManager.handleSubscriberOffer(match { it.startsWith("v=0") }) }
    }

    @Test
    fun `_isConnected follows WebRtcManager voiceIceConnected StateFlow`() = runTest {
        repository.joinChannel("ch-1")
        advanceUntilIdle()

        repository.isConnected.test {
            // Initial: both PCs not yet connected.
            assertFalse(awaitItem())

            voiceIceConnected.value = true
            advanceUntilIdle()
            assertTrue(awaitItem())

            voiceIceConnected.value = false
            advanceUntilIdle()
            assertFalse(awaitItem())

            cancelAndIgnoreRemainingEvents()
        }
    }
}
