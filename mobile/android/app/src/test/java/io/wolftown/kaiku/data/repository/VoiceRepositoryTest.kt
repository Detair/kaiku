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
import io.wolftown.kaiku.data.voice.PublisherLink
import io.wolftown.kaiku.data.voice.SessionGuard
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
import kotlinx.coroutines.launch
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
    private val publisherLink = MutableStateFlow(PublisherLink(generation = 0L, connected = false))

    /** Generation of the fake WebRtcManager's connections; closePeerConnections bumps it. */
    private var rtcGen = 0L

    /** Number of the authenticated socket; each [reconnect] makes a new one. */
    private var authSeq = 1L

    /** The publisher of the current connections reports (dis)connected. */
    private fun setPublisher(connected: Boolean) {
        publisherLink.value = PublisherLink(rtcGen, connected)
    }

    // Callback slots that capture what VoiceRepository wires into WebRtcManager.
    private val onPublisherOfferSlot = slot<(String) -> Unit>()
    private val onPublisherIceSlot = slot<(String) -> Unit>()
    private val onSubscriberAnswerSlot = slot<(String) -> Unit>()
    private val onSubscriberIceSlot = slot<(String) -> Unit>()

    private val connectionState = MutableStateFlow(ConnectionState.Connected)
    private var iceTrouble: ((IceTrouble, Long) -> Unit)? = null

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
        every { webRtcManager.publisherLink } returns publisherLink
        every { webRtcManager.currentGeneration } answers { rtcGen }
        every { webRtcManager.closePeerConnections() } answers {
            rtcGen++
            publisherLink.value = PublisherLink(rtcGen, false)
            rtcGen
        }
        // Peer connections are installed unless the session ended (the guard says no).
        coEvery { webRtcManager.createSubscriberPeerConnection(any()) } answers { firstArg<SessionGuard>().runIfCurrent {} }
        coEvery { webRtcManager.createPublisherOffer(any()) } answers { firstArg<SessionGuard>().runIfCurrent {} }
        // voice_join goes out only on an authenticated socket (like KaikuWebSocket),
        // through the same mock send, and reports which socket.
        every { webSocket.authenticatedSocket } answers {
            if (connectionState.value == ConnectionState.Connected) authSeq else null
        }
        every { webSocket.sendIfAuthenticated(any()) } answers {
            if (connectionState.value != ConnectionState.Connected) null
            else if (webSocket.send(firstArg())) authSeq else null
        }
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
    fun `_isConnected follows WebRtcManager publisherLink StateFlow`() = runTest {
        repository.joinChannel("ch-1")
        advanceUntilIdle()

        repository.isConnected.test {
            // Initial: the publisher PC is not yet connected.
            assertFalse(awaitItem())

            setPublisher(true)
            advanceUntilIdle()
            assertTrue(awaitItem())

            setPublisher(false)
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
        coEvery { webRtcManager.createSubscriberPeerConnection(any()) } coAnswers { awaitCancellation() }

        repository.startJoin("ch-1")
        advanceUntilIdle()
        assertEquals("ch-1", repository.currentChannelId.value)

        repository.leaveChannel()
        advanceUntilIdle()

        assertEquals(null, repository.currentChannelId.value)
        // The join never sent voice_join, so there is nothing to leave.
        verify(exactly = 0) { webSocket.send(ClientEvent.VoiceLeave("ch-1")) }
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
        authSeq++
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
        coVerify(exactly = 2) { webRtcManager.createSubscriberPeerConnection(any()) }
        coVerify(exactly = 2) { webRtcManager.createPublisherOffer(any()) }

        // Both peer connections connect: back to stable, service untouched.
        setPublisher(true)
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

        // 1 s + 15 s timeout, 3 s + 15 s, 9 s + 15 s
        advanceTimeBy(58_000); runCurrent()

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
        advanceTimeBy(58_000); runCurrent()

        repository.rejoin()
        advanceUntilIdle()

        assertEquals("ch-1", repository.currentChannelId.value)
        assertEquals(null, repository.endNotice.value)
    }

    @Test
    fun `a voice_error during an attempt fails it without a leave or a snackbar`() = runTest {
        repository.joinChannel("ch-1")
        advanceUntilIdle()
        reconnect()
        advanceTimeBy(1_000); runCurrent()
        verifyJoins(2)

        wsEvents.emit(ServerEvent.VoiceError(code = "VOICE_ERROR", message = "Already in voice channel"))
        runCurrent()

        // No special "already joined" handling: the attempt just failed.
        verify(exactly = 0) { webSocket.send(ClientEvent.VoiceLeave("ch-1")) }
        verifyJoins(2)
        assertEquals(null, repository.error.value)
        advanceTimeBy(3_000); runCurrent()
        verifyJoins(3)
    }

    @Test
    fun `ice disconnected only shows reconnecting`() = runTest {
        repository.joinChannel("ch-1")
        advanceUntilIdle()

        iceTrouble!!.invoke(IceTrouble.Disconnected, rtcGen)
        assertEquals(VoiceLinkState.Interrupted, repository.linkState.value)
        advanceTimeBy(60_000); runCurrent()

        verifyJoins(1)
        verify(exactly = 0) { webRtcManager.closePeerConnections() }

        setPublisher(true)
        runCurrent()
        assertEquals(VoiceLinkState.Stable, repository.linkState.value)
    }

    @Test
    fun `ice failed on a live socket rejoins like a lost socket`() = runTest {
        repository.joinChannel("ch-1")
        advanceUntilIdle()

        iceTrouble!!.invoke(IceTrouble.Failed, rtcGen)
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
        authSeq++; wsEvents.emit(ServerEvent.Ready(userId = "me"))
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

    // -- Fix round 1: session end always wins over a rejoin attempt -----------
    //
    // The log records what reached the WebRTC layer and the socket, in order.
    // A step hook runs a leave or a removal *inside* a rejoin step, the way a
    // second thread would interleave with the attempt in production.

    private val log: MutableList<String> = java.util.Collections.synchronizedList(mutableListOf())
    private val stepHooks = mutableMapOf<String, () -> Unit>()
    private val stepCounts = mutableMapOf<String, Int>()

    private fun step(name: String) {
        val n = (stepCounts[name] ?: 0) + 1
        stepCounts[name] = n
        stepHooks.remove("$name#$n")?.invoke()
    }

    private fun recordWebRtcAndSocket() {
        every { webRtcManager.closePeerConnections() } answers {
            step("close")
            log += "close"
            rtcGen++
            publisherLink.value = PublisherLink(rtcGen, false)
            rtcGen
        }
        // Like WebRtcManager: the native create runs (a leave or removal may
        // happen meanwhile), then the guard decides whether the PC is installed
        // or closed again.
        coEvery { webRtcManager.createSubscriberPeerConnection(any()) } coAnswers {
            step("createSub")
            val installed = firstArg<SessionGuard>().runIfCurrent {}
            log += if (installed) "createSub" else "createSub-discarded"
            installed
        }
        coEvery { webRtcManager.createPublisherOffer(any()) } coAnswers {
            step("createPub")
            val installed = firstArg<SessionGuard>().runIfCurrent {}
            log += if (installed) "createPub" else "createPub-discarded"
            installed
        }
        every { webSocket.send(any()) } answers {
            val name = firstArg<ClientEvent>()::class.simpleName!!
            log += "send:$name"
            step("send:$name")
            true
        }
    }

    private fun kotlinx.coroutines.test.TestScope.leaveNow() {
        log += "LEAVE"
        backgroundScope.launch(start = kotlinx.coroutines.CoroutineStart.UNDISPATCHED) {
            repository.leaveChannel()
        }
    }

    private fun removeNow() {
        log += "REMOVED"
        repository.handleServerEventNow("ch-1", ServerEvent.VoiceRemoved(channelId = "ch-1", reason = "kicked"))
    }

    /** Joins, loses the socket, and lets the first rejoin attempt start (it stops at [hook]). */
    private suspend fun kotlinx.coroutines.test.TestScope.runAttemptWith(
        hook: String,
        muted: Boolean = false,
        end: () -> Unit,
    ) {
        recordWebRtcAndSocket()
        repository.joinChannel("ch-1")
        advanceUntilIdle()
        if (muted) repository.toggleMute()
        stepHooks[hook] = end
        reconnect()
        advanceTimeBy(1_000); runCurrent()
        advanceUntilIdle()
    }

    private fun assertSessionEndWon(marker: String) {
        val at = log.indexOf(marker)
        assertTrue("marker $marker missing: $log", at >= 0)
        val after = log.drop(at + 1)
        assertFalse("voice_join after $marker: $log", "send:VoiceJoin" in after)
        assertFalse("voice_mute after $marker: $log", "send:VoiceMute" in after)
        assertFalse("subscriber PC installed after $marker: $log", "createSub" in after)
        assertFalse("publisher PC installed after $marker: $log", "createPub" in after)
        val lastCreate = maxOf(log.lastIndexOf("createSub"), log.lastIndexOf("createPub"))
        assertTrue("a created PC was never closed: $log", log.lastIndexOf("close") > lastCreate)
        assertEquals(null, repository.currentChannelId.value)
        assertEquals(VoiceLinkState.Stable, repository.linkState.value)
        verify(atLeast = 1) { VoiceCallService.stop(any()) }
    }

    @Test
    fun `leave before the rejoin creates its peer connections wins`() = runTest {
        runAttemptWith("close#1") { leaveNow() }
        assertSessionEndWon("LEAVE")
    }

    @Test
    fun `leave while the subscriber PC is being created wins`() = runTest {
        runAttemptWith("createSub#2") { leaveNow() }
        assertSessionEndWon("LEAVE")
    }

    @Test
    fun `leave right after the rejoin sent voice_join wins`() = runTest {
        runAttemptWith("send:VoiceJoin#2", muted = true) { leaveNow() }
        assertSessionEndWon("LEAVE")
    }

    @Test
    fun `leave waits for the attempt to finish before the teardown`() = runTest {
        runAttemptWith("createSub#2") { leaveNow() }
        // The attempt's last step comes before voice_leave and the teardown.
        val leaveAt = log.indexOf("send:VoiceLeave")
        val lastAttemptStep = log.indexOfLast { it.startsWith("createSub") || it.startsWith("createPub") }
        assertTrue("teardown ran before the attempt finished: $log", lastAttemptStep < leaveAt)
    }

    @Test
    fun `voice_removed before the rejoin creates its peer connections wins`() = runTest {
        runAttemptWith("close#1") { removeNow() }
        assertSessionEndWon("REMOVED")
    }

    @Test
    fun `voice_removed while the subscriber PC is being created wins`() = runTest {
        runAttemptWith("createSub#2") { removeNow() }
        assertSessionEndWon("REMOVED")
    }

    @Test
    fun `voice_removed right after the rejoin sent voice_join wins`() = runTest {
        runAttemptWith("send:VoiceJoin#2") { removeNow() }
        assertSessionEndWon("REMOVED")
    }

    // -- Fix round 1: attempt timeout, socket loss, offline cap ---------------

    @Test
    fun `an attempt times out after 15 s and its connections are closed at once`() = runTest {
        recordWebRtcAndSocket()
        repository.joinChannel("ch-1")
        advanceUntilIdle()
        reconnect()
        advanceTimeBy(1_000); runCurrent()
        verifyJoins(2)
        val closesAtStart = log.count { it == "close" }

        advanceTimeBy(14_999); runCurrent()
        assertEquals(closesAtStart, log.count { it == "close" })
        advanceTimeBy(1); runCurrent()
        // Timed out at 16 s: closed now, not when attempt 2 starts at 19 s.
        assertEquals(closesAtStart + 1, log.count { it == "close" })
        verifyJoins(2)
        advanceTimeBy(3_000); runCurrent()
        verifyJoins(3)
    }

    @Test
    fun `socket loss during attempt 3 keeps the budget and the next Ready recovers`() = runTest {
        recordWebRtcAndSocket()
        repository.joinChannel("ch-1")
        advanceUntilIdle()
        reconnect()
        // Attempts 1 and 2 time out: 1 s + 15 s, 3 s + 15 s; attempt 3 starts at 43 s.
        advanceTimeBy(43_000); runCurrent()
        verifyJoins(4)
        assertEquals(VoiceLinkState.Rejoining(3), repository.linkState.value)
        val closes = log.count { it == "close" }

        advanceTimeBy(5_000); runCurrent()
        connectionState.value = ConnectionState.Disconnected
        runCurrent()
        // The attempt is cancelled and its connections discarded at once.
        assertEquals(closes + 1, log.count { it == "close" })

        // Its timeout (58 s) passes without a give-up.
        advanceTimeBy(20_000); runCurrent()
        assertEquals("ch-1", repository.currentChannelId.value)
        assertEquals(null, repository.endNotice.value)
        verify(exactly = 0) { VoiceCallService.stop(any()) }

        connectionState.value = ConnectionState.Connected
        authSeq++; wsEvents.emit(ServerEvent.Ready(userId = "me"))
        advanceTimeBy(9_000); runCurrent()
        verifyJoins(5)
        connectPublisher()
        runCurrent()

        assertEquals(VoiceLinkState.Stable, repository.linkState.value)
        assertEquals("ch-1", repository.currentChannelId.value)
        verify(exactly = 0) { VoiceCallService.stop(any()) }
    }

    @Test
    fun `60 s without an authenticated socket gives up like the third failure`() = runTest {
        repository.joinChannel("ch-1")
        advanceUntilIdle()

        connectionState.value = ConnectionState.Disconnected
        runCurrent()
        advanceTimeBy(59_999); runCurrent()
        assertEquals("ch-1", repository.currentChannelId.value)

        advanceTimeBy(1); runCurrent()
        assertEquals(null, repository.currentChannelId.value)
        assertEquals(VoiceEndNotice.RejoinFailed("ch-1", wasMuted = false), repository.endNotice.value)
        assertEquals(VoiceLinkState.Stable, repository.linkState.value)
        verify(atLeast = 1) { VoiceCallService.stop(any()) }
        verify(atLeast = 1) { webRtcManager.closePeerConnections() }

        // A socket that comes back later does not pull the user back in.
        connectionState.value = ConnectionState.Connected
        authSeq++; wsEvents.emit(ServerEvent.Ready(userId = "me"))
        advanceTimeBy(60_000); runCurrent()
        verifyJoins(1)
    }

    // -- Fix round 1: success criterion ---------------------------------------

    @Test
    fun `a rejoin into an empty channel succeeds on the publisher alone`() = runTest {
        repository.joinChannel("ch-1")
        advanceUntilIdle()
        reconnect()
        advanceTimeBy(1_000); runCurrent()
        verifyJoins(2)

        // Nobody else publishes, so the server sends no subscriber offer.
        connectPublisher()
        runCurrent()

        assertEquals(VoiceLinkState.Stable, repository.linkState.value)
        assertTrue(repository.isConnected.value)
        advanceTimeBy(120_000); runCurrent()
        verifyJoins(2)
        assertEquals("ch-1", repository.currentChannelId.value)
    }

    // -- Fix round 1: mute state agrees in both layers ------------------------

    private var rtcMuted = false

    /** The publisher PC reports connected; the subscriber stays new (empty room). */
    private fun connectPublisher() {
        setPublisher(true)
    }

    private fun trackRtcMute() {
        every { webRtcManager.setMuted(any()) } answers { rtcMuted = firstArg() }
        every { webRtcManager.isMuted } answers { rtcMuted }
    }

    @Test
    fun `give-up while muted resets both layers and Rejoin restores the mute`() = runTest {
        trackRtcMute()
        recordWebRtcAndSocket()
        repository.joinChannel("ch-1")
        advanceUntilIdle()
        repository.toggleMute()
        assertTrue(rtcMuted && repository.isMuted.value)

        reconnect()
        advanceTimeBy(70_000); runCurrent()
        assertEquals(null, repository.currentChannelId.value)
        // Nothing is captured after the give-up; both layers say "not muted".
        assertEquals(repository.isMuted.value, rtcMuted)
        assertFalse(rtcMuted)

        log.clear()
        repository.rejoin()
        advanceUntilIdle()

        // The user's last choice is back in both layers, before the mic track exists.
        assertTrue(repository.isMuted.value)
        assertTrue(rtcMuted)
        val joinAt = log.indexOf("send:VoiceJoin")
        val muteAt = log.indexOf("send:VoiceMute")
        assertTrue("server not told about the mute: $log", joinAt in 0 until muteAt)
    }

    @Test
    fun `kick and leave while muted leave both layers unmuted`() = runTest {
        trackRtcMute()
        repository.joinChannel("ch-1")
        advanceUntilIdle()
        repository.toggleMute()
        wsEvents.emit(ServerEvent.VoiceRemoved(channelId = "ch-1", reason = "kicked"))
        advanceUntilIdle()
        assertFalse(repository.isMuted.value)
        assertFalse(rtcMuted)

        repository.joinChannel("ch-1")
        advanceUntilIdle()
        repository.toggleMute()
        repository.leaveChannel()
        assertFalse(repository.isMuted.value)
        assertFalse(rtcMuted)
    }

    // -- Fix round 1: minor findings ------------------------------------------

    @Test
    fun `a WebRTC error during a rejoin attempt fails the attempt without a snackbar`() = runTest {
        var onError: ((String) -> Unit)? = null
        every { webRtcManager.onError = any() } answers { onError = firstArg() }
        repository.joinChannel("ch-1")
        advanceUntilIdle()
        reconnect()
        advanceTimeBy(1_000); runCurrent()
        verifyJoins(2)

        onError!!.invoke("SDP setPublisherRemoteDesc failed")
        runCurrent()

        assertEquals(null, repository.error.value)
        // The attempt failed at once: attempt 2 follows 3 s later, not after the timeout.
        advanceTimeBy(3_000); runCurrent()
        verifyJoins(3)
    }

    @Test
    fun `logout clears the end notice`() = runTest {
        repository.joinChannel("ch-1")
        advanceUntilIdle()
        reconnect()
        advanceTimeBy(70_000); runCurrent()
        assertTrue(repository.endNotice.value is VoiceEndNotice.RejoinFailed)

        // SessionTeardown.onLogout calls leaveChannel().
        repository.leaveChannel()

        assertEquals(null, repository.endNotice.value)
    }

    @Test
    fun `a Ready during the first join waits for the join before rejoining`() = runTest {
        recordWebRtcAndSocket()
        val gate = kotlinx.coroutines.CompletableDeferred<Unit>()
        coEvery { webRtcManager.createPublisherOffer(any()) } coAnswers {
            gate.await()
            log += "createPub"
            true
        }
        repository.startJoin("ch-1")
        runCurrent()
        reconnect()
        advanceTimeBy(5_000); runCurrent()
        // The join is still creating its publisher: the attempt has not touched anything.
        assertFalse("close" in log)

        gate.complete(Unit)
        runCurrent()
        advanceTimeBy(1_000); runCurrent()
        val pubAt = log.indexOf("createPub")
        assertTrue("attempt closed the join's connections: $log", pubAt in 0 until log.indexOf("close"))
    }

    // -- Fix round 2 -----------------------------------------------------------

    @Test
    fun `a success of an older publisher does not complete the current attempt`() = runTest {
        repository.joinChannel("ch-1")
        advanceUntilIdle()
        reconnect()
        advanceTimeBy(1_000); runCurrent()
        verifyJoins(2)
        val ownGeneration = rtcGen

        // A callback of the retired connection publishes CONNECTED late.
        publisherLink.value = PublisherLink(ownGeneration - 1, connected = true)
        runCurrent()
        assertEquals(VoiceLinkState.Rejoining(1), repository.linkState.value)

        // Only the attempt's own publisher completes it.
        publisherLink.value = PublisherLink(ownGeneration, connected = true)
        runCurrent()
        assertEquals(VoiceLinkState.Stable, repository.linkState.value)
        verifyJoins(2)
    }

    @Test
    fun `an ICE failure of an older connection does not fail the current attempt`() = runTest {
        repository.joinChannel("ch-1")
        advanceUntilIdle()
        reconnect()
        advanceTimeBy(1_000); runCurrent()
        verifyJoins(2)

        iceTrouble!!.invoke(IceTrouble.Failed, rtcGen - 1)
        advanceTimeBy(3_000); runCurrent()

        // Still attempt 1 (no early failure, so no attempt 2 after 3 s).
        verifyJoins(2)
        setPublisher(true)
        runCurrent()
        assertEquals(VoiceLinkState.Stable, repository.linkState.value)
    }

    @Test
    fun `a late Ready of the socket the join went out on starts no rejoin`() = runTest {
        recordWebRtcAndSocket()
        repository.joinChannel("ch-1") // on authenticated socket 1
        advanceUntilIdle()

        // Socket 1's own Ready reaches the voice collector only now; no socket loss.
        wsEvents.emit(ServerEvent.Ready(userId = "me"))
        advanceTimeBy(60_000); runCurrent()

        verifyJoins(1)
        assertFalse("close" in log)
        assertEquals(VoiceLinkState.Stable, repository.linkState.value)
        assertEquals("ch-1", repository.currentChannelId.value)
    }

    // -- Fix round 3: voice signalling waits for Ready ------------------------

    private fun verifyNoVoiceFrames() {
        verify(exactly = 0) { webSocket.send(ofType<ClientEvent.VoiceJoin>()) }
        verify(exactly = 0) { webSocket.send(ofType<ClientEvent.VoiceLeave>()) }
        verify(exactly = 0) { webSocket.send(ofType<ClientEvent.VoiceMute>()) }
        verify(exactly = 0) { webSocket.send(ofType<ClientEvent.VoiceUnmute>()) }
        verify(exactly = 0) { webSocket.send(ofType<ClientEvent.VoicePublisherOffer>()) }
        verify(exactly = 0) { webSocket.send(ofType<ClientEvent.VoiceIceCandidate>()) }
    }

    @Test
    fun `a join requested while the socket authenticates is held and sent once after Ready`() = runTest {
        // Authenticate is out, Ready not yet: a frame now would reach the server
        // with unknown timing, so nothing voice-related is sent.
        connectionState.value = ConnectionState.Connecting
        repository.startJoin("ch-1")
        advanceTimeBy(5_000); runCurrent()
        verifyNoVoiceFrames()
        assertEquals("ch-1", repository.currentChannelId.value)
        assertFalse(repository.isConnected.value) // the UI shows "connecting"
        coVerify(exactly = 0) { webRtcManager.createPublisherOffer(any()) }

        // Ready of socket 1 (its first authentication).
        connectionState.value = ConnectionState.Connected
        wsEvents.emit(ServerEvent.Ready(userId = "me"))
        runCurrent()
        verifyJoins(1)
        coVerify(exactly = 1) { webRtcManager.createPublisherOffer(any()) }

        // That Ready starts no recovery: no second join, connections untouched.
        advanceTimeBy(60_000); runCurrent()
        verifyJoins(1)
        verify(exactly = 0) { webRtcManager.closePeerConnections() }
        setPublisher(true)
        runCurrent()
        assertEquals(VoiceLinkState.Stable, repository.linkState.value)
        assertTrue(repository.isConnected.value)
    }

    @Test
    fun `a join requested without a socket is held and sent once on Ready`() = runTest {
        connectionState.value = ConnectionState.Disconnected
        repository.startJoin("ch-1")
        advanceTimeBy(5_000); runCurrent()
        connectionState.value = ConnectionState.Connecting
        advanceTimeBy(1_000); runCurrent()
        verifyNoVoiceFrames()

        connectionState.value = ConnectionState.Connected
        wsEvents.emit(ServerEvent.Ready(userId = "me"))
        advanceTimeBy(60_000); runCurrent()

        verifyJoins(1)
        verify(exactly = 0) { webRtcManager.closePeerConnections() }
        assertEquals("ch-1", repository.currentChannelId.value)
    }

    @Test
    fun `leaving while the join is held sends nothing`() = runTest {
        connectionState.value = ConnectionState.Connecting
        repository.startJoin("ch-1")
        advanceTimeBy(5_000); runCurrent()

        repository.leaveChannel()
        connectionState.value = ConnectionState.Connected
        wsEvents.emit(ServerEvent.Ready(userId = "me"))
        advanceTimeBy(60_000); runCurrent()

        verifyNoVoiceFrames()
        assertEquals(null, repository.currentChannelId.value)
        verify(atLeast = 1) { VoiceCallService.stop(any()) }
    }

    @Test
    fun `a mute toggled while the join is held goes out right after the join`() = runTest {
        recordWebRtcAndSocket()
        connectionState.value = ConnectionState.Connecting
        repository.startJoin("ch-1")
        advanceTimeBy(1_000); runCurrent()
        repository.toggleMute()
        verifyNoVoiceFrames()

        connectionState.value = ConnectionState.Connected
        wsEvents.emit(ServerEvent.Ready(userId = "me"))
        runCurrent()

        assertEquals(log.indexOf("send:VoiceJoin") + 1, log.indexOf("send:VoiceMute"))
        verify(exactly = 1) { webSocket.send(ClientEvent.VoiceMute("ch-1")) }
    }

    @Test
    fun `a full channel cannot destroy a call whose join was held for Ready`() = runTest {
        // The server would refuse any second voice_join of this user (full channel).
        var joins = 0
        every { webSocket.send(ofType<ClientEvent.VoiceJoin>()) } answers {
            if (++joins > 1) {
                wsEvents.tryEmit(ServerEvent.VoiceError(code = "VOICE_ERROR", message = "Channel is full"))
            }
            true
        }
        connectionState.value = ConnectionState.Connecting
        repository.startJoin("ch-1")
        runCurrent()
        connectionState.value = ConnectionState.Connected
        wsEvents.emit(ServerEvent.Ready(userId = "me"))
        runCurrent()
        setPublisher(true)
        advanceTimeBy(120_000); runCurrent()

        assertEquals(1, joins)
        assertEquals("ch-1", repository.currentChannelId.value)
        assertTrue(repository.isConnected.value)
        assertEquals(null, repository.endNotice.value)
        verify(exactly = 0) { webRtcManager.closePeerConnections() }
    }

    @Test
    fun `a Ready handled before the join sent voice_join on that socket starts no rejoin`() = runTest {
        // Socket 1's Ready is handled (collector thread) right before the
        // join (IO thread) sends its voice_join on that same socket.
        var first = true
        every { webSocket.sendIfAuthenticated(ClientEvent.VoiceJoin("ch-1")) } answers {
            if (first) {
                first = false
                repository.handleServerEventNow("ch-1", ServerEvent.Ready(userId = "me"))
            }
            if (webSocket.send(firstArg())) authSeq else null
        }
        repository.startJoin("ch-1")
        advanceTimeBy(60_000); runCurrent()

        // The join's voice_join went out on socket 1: nothing to rejoin.
        verifyJoins(1)
        assertEquals("ch-1", repository.currentChannelId.value)
        assertEquals(VoiceLinkState.Stable, repository.linkState.value)
    }

    @Test
    fun `a give-up does not cancel the user's join to another channel`() = runTest {
        // Socket down from the start: the 60 s offline cap ends channel 1.
        connectionState.value = ConnectionState.Connecting
        val gate = kotlinx.coroutines.CompletableDeferred<Unit>()
        var firstInit = true
        coEvery { webRtcManager.initialize() } coAnswers {
            if (firstInit) {
                firstInit = false
                // A native step that does not stop for cancellation.
                kotlinx.coroutines.withContext(kotlinx.coroutines.NonCancellable) { gate.await() }
            }
        }
        repository.startJoin("ch-1")
        runCurrent()
        advanceTimeBy(30_000); runCurrent()

        // The user taps channel 2; that join waits for join 1 to finish.
        repository.startJoin("ch-2")
        runCurrent()
        advanceTimeBy(30_000); runCurrent() // give-up of channel 1 at 60 s
        verify(atLeast = 1) { VoiceCallService.stop(any()) }

        gate.complete(Unit)
        advanceTimeBy(1_000); runCurrent()

        // Channel 1 is torn down (its join never went out: nothing to leave)
        // and the tap on channel 2 was not lost.
        assertEquals("ch-2", repository.currentChannelId.value)
        verify(exactly = 0) { webSocket.send(ClientEvent.VoiceLeave("ch-1")) }

        // Channel 2's join is held until the socket is authenticated.
        connectionState.value = ConnectionState.Connected
        wsEvents.emit(ServerEvent.Ready(userId = "me"))
        runCurrent()
        verify(exactly = 1) { webSocket.send(ClientEvent.VoiceJoin("ch-2")) }
        verify(exactly = 0) { webSocket.send(ClientEvent.VoiceJoin("ch-1")) }
    }

    @Test
    fun `a session end cannot slip between the guard check and the install`() = runTest {
        recordWebRtcAndSocket()
        repository.joinChannel("ch-1")
        advanceUntilIdle()
        var removalBlocked = false
        coEvery { webRtcManager.createSubscriberPeerConnection(any()) } coAnswers {
            firstArg<SessionGuard>().runIfCurrent {
                // voice_removed arrives on another thread while the PC is installed.
                val removal = Thread {
                    repository.handleServerEventNow(
                        "ch-1", ServerEvent.VoiceRemoved(channelId = "ch-1", reason = "kicked")
                    )
                    log += "REMOVED"
                }
                removal.start()
                removal.join(300)
                removalBlocked = removal.isAlive
                log += "createSub"
            }
        }
        reconnect()
        advanceTimeBy(1_000); runCurrent()
        // Wait for the blocked removal to go through, then for its teardown.
        val deadline = System.currentTimeMillis() + 5_000
        while ("REMOVED" !in log && System.currentTimeMillis() < deadline) Thread.sleep(5)
        Thread.sleep(50)
        advanceUntilIdle()

        // The removal waited for the install, so the teardown closed that PC,
        // and no voice_join went out after it.
        assertTrue(removalBlocked)
        assertSessionEndWon("REMOVED")
    }
}
