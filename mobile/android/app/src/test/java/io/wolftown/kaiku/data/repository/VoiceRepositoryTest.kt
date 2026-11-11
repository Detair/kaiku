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
import io.wolftown.kaiku.R
import io.wolftown.kaiku.data.voice.AudioRouteManager
import io.wolftown.kaiku.data.voice.IceTrouble
import io.wolftown.kaiku.data.voice.MicPermissionChecker
import io.wolftown.kaiku.data.voice.MissingMicPermissionException
import io.wolftown.kaiku.data.voice.VoiceEndNotice
import io.wolftown.kaiku.data.voice.VoiceLinkState
import io.wolftown.kaiku.data.voice.VoiceServiceEvent
import io.wolftown.kaiku.data.voice.VoiceServiceEvents
import io.wolftown.kaiku.data.voice.WebRtcManager
import io.wolftown.kaiku.data.ws.ClientEvent
import io.wolftown.kaiku.data.ws.ConnectionState
import io.wolftown.kaiku.data.ws.KaikuWebSocket
import io.wolftown.kaiku.data.ws.VoiceRemovedReason
import io.wolftown.kaiku.data.ws.ServerEvent
import io.wolftown.kaiku.service.VoiceCallService
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.awaitCancellation
import kotlinx.coroutines.cancel
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.test.StandardTestDispatcher
import kotlinx.coroutines.test.advanceTimeBy
import kotlinx.coroutines.test.advanceUntilIdle
import kotlinx.coroutines.test.runCurrent
import kotlinx.coroutines.test.resetMain
import kotlinx.coroutines.test.runTest
import kotlinx.coroutines.test.setMain
import org.junit.After
import org.junit.Assert.assertEquals
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
    private lateinit var micPermissionChecker: MicPermissionChecker
    private lateinit var context: Context
    private lateinit var repository: VoiceRepository

    // Main is set to this dispatcher, so runTest shares its scheduler and
    // advanceUntilIdle() drives every coroutine the repository launches.
    private val testDispatcher = StandardTestDispatcher()
    private lateinit var voiceScope: CoroutineScope

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

    private val connectionState = MutableStateFlow(ConnectionState.Connected)
    private var iceTrouble: ((IceTrouble) -> Unit)? = null

    @Before
    fun setUp() {
        Dispatchers.setMain(testDispatcher)
        voiceScope = CoroutineScope(SupervisorJob() + testDispatcher)
        webRtcManager = mockk(relaxed = true)
        webSocket = mockk(relaxed = true)
        audioRouteManager = mockk(relaxed = true)
        voiceServiceEvents = mockk(relaxed = true)
        micPermissionChecker = mockk()
        every { micPermissionChecker.isGranted() } returns true
        context = mockk(relaxed = true)

        // VoiceRepository.joinChannel calls VoiceCallService.start(...), which constructs
        // an Android Intent and would throw "Method putExtra in android.content.Intent not
        // mocked" under the default JVM test target. Stub the companion to a no-op.
        mockkObject(VoiceCallService.Companion)
        every { VoiceCallService.start(any(), any(), any()) } answers {}
        every { VoiceCallService.stop(any()) } answers {}

        every { webSocket.events } returns wsEvents
        every { webSocket.connectionState } returns connectionState
        every { webSocket.send(any()) } returns true
        every { webRtcManager.onIceTrouble = any() } answers { iceTrouble = firstArg() }
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
            micPermissionChecker = micPermissionChecker,
            context = context,
            voiceScope = voiceScope,
        )
    }

    @After
    fun tearDown() {
        voiceScope.cancel()
        unmockkObject(VoiceCallService.Companion)
        Dispatchers.resetMain()
    }

    @Test
    fun `join without mic permission fails fast`() = runTest {
        every { micPermissionChecker.isGranted() } returns false

        try {
            repository.joinChannel("ch-1")
            org.junit.Assert.fail("expected MissingMicPermissionException")
        } catch (_: MissingMicPermissionException) {
            // expected
        }

        coVerify(exactly = 0) { webRtcManager.initialize() }
        assertTrue(repository.currentChannelId.value == null)
    }

    @Test
    fun `service start failure leaves channel and sets error`() = runTest {
        val serviceEvents = MutableSharedFlow<VoiceServiceEvent>(extraBufferCapacity = 5)
        every { voiceServiceEvents.events } returns serviceEvents

        repository.joinChannel("ch-1")
        // The collector subscribes UNDISPATCHED inside joinChannel.
        assertEquals(1, serviceEvents.subscriptionCount.value)

        serviceEvents.emit(VoiceServiceEvent.StartFailed)
        advanceUntilIdle()

        assertEquals(R.string.voice_start_failed, repository.error.value)
        assertTrue(repository.currentChannelId.value == null)
        verify { webSocket.send(ClientEvent.VoiceLeave("ch-1")) }
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

        assertEquals(1, wsEvents.subscriptionCount.value)
        wsEvents.emit(ServerEvent.VoicePublisherAnswer(channelId = "ch-1", sdp = "v=0\r\no=answer\r\n"))
        advanceUntilIdle()

        coVerify { webRtcManager.handlePublisherAnswer(match { it.startsWith("v=0") }) }
    }

    @Test
    fun `VoiceSubscriberOffer event routes to WebRtcManager handleSubscriberOffer`() = runTest {
        repository.joinChannel("ch-1")
        advanceUntilIdle()

        assertEquals(1, wsEvents.subscriptionCount.value)
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

    // -- Join owned by the repository (Task 19) -------------------------------

    @Test
    fun `startJoin without mic permission sets the string error`() = runTest {
        every { micPermissionChecker.isGranted() } returns false

        repository.startJoin("ch-1")
        advanceUntilIdle()

        assertEquals(R.string.voice_mic_permission_required, repository.error.value)
        assertEquals(null, repository.currentChannelId.value)
    }

    @Test
    fun `leave during join cancels the join and cleans up`() = runTest {
        coEvery { webRtcManager.createSubscriberPeerConnection() } coAnswers { awaitCancellation() }

        repository.startJoin("ch-1")
        advanceUntilIdle()
        assertEquals("ch-1", repository.currentChannelId.value)

        repository.leaveChannel()
        advanceUntilIdle()

        assertEquals(null, repository.currentChannelId.value)
        verify { webSocket.send(ClientEvent.VoiceLeave("ch-1")) }
        verify(exactly = 0) { webSocket.send(ClientEvent.VoiceJoin("ch-1")) }
        verify(atLeast = 1) { webRtcManager.closePeerConnections() }
        verify(atLeast = 1) { VoiceCallService.stop(any()) }
    }

    @Test
    fun `leave clears the channel before VoiceLeave is sent`() = runTest {
        repository.joinChannel("ch-1")
        var channelAtSend: String? = "unset"
        every { webSocket.send(ClientEvent.VoiceLeave("ch-1")) } answers {
            channelAtSend = repository.currentChannelId.value
            true
        }

        repository.leaveChannel()

        assertEquals(null, channelAtSend)
    }

    // -- Rejoin after a lost connection (C32) ---------------------------------

    private suspend fun kotlinx.coroutines.test.TestScope.reconnect() {
        connectionState.value = ConnectionState.Disconnected
        runCurrent()
        connectionState.value = ConnectionState.Connected
        wsEvents.emit(ServerEvent.Ready(userId = "me"))
        runCurrent()
    }

    private fun verifyJoins(count: Int) =
        verify(exactly = count) { webSocket.send(ClientEvent.VoiceJoin("ch-1")) }

    @Test
    fun `socket loss shows reconnecting without tearing anything down`() = runTest {
        repository.joinChannel("ch-1")
        advanceUntilIdle()

        connectionState.value = ConnectionState.Disconnected
        runCurrent()

        assertEquals(VoiceLinkState.Interrupted, repository.linkState.value)
        assertEquals("ch-1", repository.currentChannelId.value)
        verify(exactly = 0) { VoiceCallService.stop(any()) }
        verify(exactly = 0) { webRtcManager.closePeerConnections() }
    }

    @Test
    fun `Ready after a lost socket rejoins after 1 s with fresh peer connections`() = runTest {
        repository.joinChannel("ch-1")
        advanceUntilIdle()
        verifyJoins(1)

        reconnect()
        assertEquals(VoiceLinkState.Rejoining(1), repository.linkState.value)
        advanceTimeBy(999); runCurrent()
        verifyJoins(1)
        advanceTimeBy(1); runCurrent()

        verifyJoins(2)
        verify(exactly = 1) { webRtcManager.closePeerConnections() }
        coVerify(exactly = 2) { webRtcManager.createSubscriberPeerConnection() }
        coVerify(exactly = 2) { webRtcManager.createPublisherOffer() }

        // Both peer connections connect: back to stable, service untouched.
        voiceIceConnected.value = true
        runCurrent()
        assertEquals(VoiceLinkState.Stable, repository.linkState.value)
        assertEquals("ch-1", repository.currentChannelId.value)
        verify(exactly = 0) { VoiceCallService.stop(any()) }
        verify(exactly = 1) { VoiceCallService.start(any(), any(), any()) }
    }

    @Test
    fun `mute state is restored after a rejoin`() = runTest {
        repository.joinChannel("ch-1")
        advanceUntilIdle()
        repository.toggleMute()
        verify(exactly = 1) { webSocket.send(ClientEvent.VoiceMute("ch-1")) }

        reconnect()
        advanceTimeBy(1_000); runCurrent()

        verify(exactly = 2) { webSocket.send(ClientEvent.VoiceMute("ch-1")) }
        assertTrue(repository.isMuted.value)
    }

    @Test
    fun `not muted means no mute event after a rejoin`() = runTest {
        repository.joinChannel("ch-1")
        advanceUntilIdle()

        reconnect()
        advanceTimeBy(1_000); runCurrent()

        verify(exactly = 0) { webSocket.send(ClientEvent.VoiceMute("ch-1")) }
    }

    @Test
    fun `own leave cancels a pending rejoin`() = runTest {
        repository.joinChannel("ch-1")
        advanceUntilIdle()
        reconnect()
        advanceTimeBy(500); runCurrent()

        repository.leaveChannel()
        advanceTimeBy(60_000); runCurrent()

        verifyJoins(1)
        assertEquals(VoiceLinkState.Stable, repository.linkState.value)
        assertEquals(null, repository.endNotice.value)
    }

    @Test
    fun `a Ready without a voice session does nothing`() = runTest {
        advanceUntilIdle()
        reconnect()
        advanceTimeBy(60_000); runCurrent()

        verify(exactly = 0) { webSocket.send(ofType<ClientEvent.VoiceJoin>()) }
    }

    @Test
    fun `three failed attempts clear the voice state stop the service and offer rejoin`() = runTest {
        repository.joinChannel("ch-1")
        advanceUntilIdle()
        reconnect()

        // 1 s + 10 s timeout, 3 s + 10 s, 9 s + 10 s
        advanceTimeBy(43_000); runCurrent()

        verifyJoins(4)
        assertEquals(null, repository.currentChannelId.value)
        assertEquals(VoiceEndNotice.RejoinFailed("ch-1"), repository.endNotice.value)
        assertEquals(VoiceLinkState.Stable, repository.linkState.value)
        verify(atLeast = 1) { VoiceCallService.stop(any()) }
        verify { webSocket.send(ClientEvent.VoiceLeave("ch-1")) }

        // Nothing re-arms by itself.
        advanceTimeBy(60_000); runCurrent()
        verifyJoins(4)
    }

    @Test
    fun `rejoin action after giving up joins the channel again`() = runTest {
        repository.joinChannel("ch-1")
        advanceUntilIdle()
        reconnect()
        advanceTimeBy(43_000); runCurrent()

        repository.rejoin()
        advanceUntilIdle()

        assertEquals("ch-1", repository.currentChannelId.value)
        assertEquals(null, repository.endNotice.value)
    }

    @Test
    fun `server still holding the old peer gets one leave and join`() = runTest {
        repository.joinChannel("ch-1")
        advanceUntilIdle()
        reconnect()
        advanceTimeBy(1_000); runCurrent()
        verifyJoins(2)

        wsEvents.emit(ServerEvent.VoiceError(code = "VOICE_ERROR", message = "Already in voice channel"))
        runCurrent()

        verify(exactly = 1) { webSocket.send(ClientEvent.VoiceLeave("ch-1")) }
        verifyJoins(3)
        // The error belongs to the attempt, not to the user.
        assertEquals(null, repository.error.value)

        // A second "already joined" in the same attempt fails it (no loop).
        wsEvents.emit(ServerEvent.VoiceError(code = "VOICE_ERROR", message = "Already in voice channel"))
        runCurrent()
        verifyJoins(3)
        verify(exactly = 1) { webSocket.send(ClientEvent.VoiceLeave("ch-1")) }
    }

    @Test
    fun `ice disconnected only shows reconnecting`() = runTest {
        repository.joinChannel("ch-1")
        advanceUntilIdle()

        iceTrouble!!.invoke(IceTrouble.Disconnected)
        assertEquals(VoiceLinkState.Interrupted, repository.linkState.value)
        advanceTimeBy(60_000); runCurrent()

        verifyJoins(1)
        verify(exactly = 0) { webRtcManager.closePeerConnections() }

        voiceIceConnected.value = true
        runCurrent()
        assertEquals(VoiceLinkState.Stable, repository.linkState.value)
    }

    @Test
    fun `ice failed on a live socket rejoins like a lost socket`() = runTest {
        repository.joinChannel("ch-1")
        advanceUntilIdle()

        iceTrouble!!.invoke(IceTrouble.Failed)
        advanceTimeBy(1_000); runCurrent()

        verifyJoins(2)
        verify(exactly = 1) { webRtcManager.closePeerConnections() }
    }

    // -- Forced leave (C31) ---------------------------------------------------

    @Test
    fun `voice_removed tears down clears state stops the service and says why`() = runTest {
        repository.joinChannel("ch-1")
        advanceUntilIdle()

        wsEvents.emit(ServerEvent.VoiceRemoved(channelId = "ch-1", reason = "banned"))
        advanceUntilIdle()

        assertEquals(null, repository.currentChannelId.value)
        assertEquals(
            VoiceEndNotice.Removed("ch-1", VoiceRemovedReason.Banned),
            repository.endNotice.value
        )
        assertFalse(repository.endNotice.value!!.canRejoin)
        verify(atLeast = 1) { webRtcManager.closePeerConnections() }
        verify(atLeast = 1) { VoiceCallService.stop(any()) }
        // The server already removed the peer: no voice_leave.
        verify(exactly = 0) { webSocket.send(ClientEvent.VoiceLeave("ch-1")) }
    }

    @Test
    fun `no rejoin after voice_removed even when the socket reconnects`() = runTest {
        repository.joinChannel("ch-1")
        advanceUntilIdle()

        wsEvents.emit(ServerEvent.VoiceRemoved(channelId = "ch-1", reason = "kicked"))
        advanceUntilIdle()
        connectionState.value = ConnectionState.Disconnected
        runCurrent()
        connectionState.value = ConnectionState.Connected
        wsEvents.emit(ServerEvent.Ready(userId = "me"))
        advanceTimeBy(60_000); runCurrent()

        verifyJoins(1)
        assertEquals(null, repository.currentChannelId.value)
        assertEquals(VoiceLinkState.Stable, repository.linkState.value)
    }

    @Test
    fun `voice_removed while a rejoin is pending cancels it`() = runTest {
        repository.joinChannel("ch-1")
        advanceUntilIdle()
        reconnect()
        advanceTimeBy(500); runCurrent()

        wsEvents.emit(ServerEvent.VoiceRemoved(channelId = "ch-1", reason = "permission_lost"))
        advanceTimeBy(60_000); runCurrent()

        verifyJoins(1)
        assertEquals(
            VoiceEndNotice.Removed("ch-1", VoiceRemovedReason.PermissionLost),
            repository.endNotice.value
        )
    }

    @Test
    fun `voice_removed with an unknown reason ends the call like kicked`() = runTest {
        repository.joinChannel("ch-1")
        advanceUntilIdle()

        wsEvents.emit(ServerEvent.VoiceRemoved(channelId = "ch-1", reason = "from_the_future"))
        advanceUntilIdle()

        assertEquals(
            VoiceEndNotice.Removed("ch-1", VoiceRemovedReason.Kicked),
            repository.endNotice.value
        )
    }

    @Test
    fun `voice_removed without a channel id applies to the current call`() = runTest {
        repository.joinChannel("ch-1")
        advanceUntilIdle()

        wsEvents.emit(ServerEvent.VoiceRemoved(channelId = null, reason = "channel_deleted"))
        advanceUntilIdle()

        assertEquals(null, repository.currentChannelId.value)
        assertEquals(
            VoiceEndNotice.Removed("ch-1", VoiceRemovedReason.ChannelDeleted),
            repository.endNotice.value
        )
    }

    @Test
    fun `voice_removed for another channel is ignored`() = runTest {
        repository.joinChannel("ch-1")
        advanceUntilIdle()

        wsEvents.emit(ServerEvent.VoiceRemoved(channelId = "ch-2", reason = "kicked"))
        advanceUntilIdle()

        assertEquals("ch-1", repository.currentChannelId.value)
        assertEquals(null, repository.endNotice.value)
    }

    @Test
    fun `every removal reason has its own line of text`() {
        val texts = VoiceRemovedReason.entries.map { VoiceEndNotice.Removed("c", it).message }
        assertEquals(VoiceRemovedReason.entries.size, texts.toSet().size)
    }

    @Test
    fun `a new join clears the removal notice`() = runTest {
        repository.joinChannel("ch-1")
        advanceUntilIdle()
        wsEvents.emit(ServerEvent.VoiceRemoved(channelId = "ch-1", reason = "kicked"))
        advanceUntilIdle()

        repository.joinChannel("ch-1")

        assertEquals(null, repository.endNotice.value)
    }
}
