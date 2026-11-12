package io.wolftown.kaiku.data.voice

import io.mockk.coEvery
import io.mockk.mockk
import io.mockk.verify
import io.wolftown.kaiku.data.api.IceServerConfig
import io.wolftown.kaiku.data.api.VoiceApi
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.test.TestScope
import kotlinx.coroutines.test.UnconfinedTestDispatcher
import kotlinx.coroutines.test.runCurrent
import kotlinx.coroutines.test.runTest
import kotlinx.serialization.json.Json
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertSame
import org.junit.Assert.assertTrue
import org.junit.Test
import org.webrtc.AudioTrack
import org.webrtc.PeerConnection
import org.webrtc.PeerConnectionFactory

/**
 * Unit tests for the WebRTC voice layer.
 *
 * The `org.webrtc.*` classes (PeerConnection, AudioTrack, etc.) require the
 * Android runtime and cannot be instantiated in JVM unit tests. Therefore,
 * this test class focuses on:
 *
 * - ICE candidate serialization / deserialization ([IceCandidateData])
 * - Mute state and the microphone track (against mocked tracks)
 * - Connection generations, the publisher link and the session guard
 *
 * Tests that require PeerConnection, AudioTrack, or PeerConnectionFactory
 * are documented below and should be run as instrumented (androidTest) tests.
 *
 * ### Tests requiring instrumented test environment:
 * - SDP offer/answer creation flow (needs PeerConnection)
 * - Remote track classification (AudioTrack vs VideoTrack from onTrack)
 * - Local audio track enable/disable (needs real AudioTrack instance)
 * - PeerConnectionFactory initialization
 * - ICE candidate round-trip through PeerConnection
 */
class WebRtcManagerTest {

    /**
     * Construct a [WebRtcManager] with mocks suitable for tests that exercise
     * signaling/state behavior without needing a real EGL or native PeerConnection.
     * The provider's `get()` is never invoked unless the test reads `.eglBase`.
     *
     * [voiceScope] defaults to an `UnconfinedTestDispatcher`-backed [TestScope],
     * which runs continuations synchronously on the calling thread. Tests that
     * need scheduler control (i.e., `runCurrent()` / `advanceUntilIdle()` must
     * drive the stateIn combine) should pass `backgroundScope` from a `runTest`
     * block instead.
     */
    @OptIn(ExperimentalCoroutinesApi::class)
    private fun newWebRtcManager(
        voiceScope: CoroutineScope = TestScope(UnconfinedTestDispatcher()),
        voiceApi: VoiceApi = mockk<VoiceApi>(relaxed = true),
        factoryProvider: PeerConnectionFactoryProvider = mockk<PeerConnectionFactoryProvider>(relaxed = true),
    ): WebRtcManager = WebRtcManager(
        voiceApi = voiceApi,
        eglBaseProvider = mockk<EglBaseProvider>(relaxed = true),
        factoryProvider = factoryProvider,
        voiceScope = voiceScope,
    )

    // ========================================================================
    // One PeerConnectionFactory per process
    // ========================================================================

    @Test
    fun `factory created once across two joins`() = runTest {
        // A relaxed mock stands in for the native factory; its peer connections,
        // sources and tracks are relaxed mocks too, so no native WebRTC runs.
        val factory = mockk<PeerConnectionFactory>(relaxed = true)
        var created = 0
        val provider = DefaultPeerConnectionFactoryProvider(create = {
            created++
            factory
        })
        val voiceApi = mockk<VoiceApi>()
        coEvery { voiceApi.getIceServers() } returns IceServerConfig(iceServers = emptyList())
        val manager = newWebRtcManager(voiceApi = voiceApi, factoryProvider = provider)

        repeat(2) {
            // Same steps as VoiceRepository.joinChannel, then the leave/dispose path.
            manager.initialize()
            manager.createSubscriberPeerConnection()
            manager.createPublisherOffer()
            manager.closePeerConnections()
            manager.dispose()
        }

        assertEquals(1, created)
        // Two joins, each with a subscriber and a publisher PC.
        verify(exactly = 4) { factory.createPeerConnection(any<PeerConnection.RTCConfiguration>(), any<PeerConnection.Observer>()) }
        // dispose() releases calls, never the shared factory.
        verify(exactly = 0) { factory.dispose() }
    }

    @Test
    fun `factory provider returns the same instance`() {
        var created = 0
        val provider = DefaultPeerConnectionFactoryProvider(create = {
            created++
            mockk<PeerConnectionFactory>(relaxed = true)
        })

        val first = provider.get()
        val second = provider.get()

        assertSame(first, second)
        assertEquals(1, created)
    }

    // ========================================================================
    // ICE candidate serialization
    // ========================================================================

    @Test
    fun `IceCandidateData serializes to correct JSON format`() {
        val candidate = IceCandidateData(
            candidate = "candidate:1 1 udp 2113937151 192.168.1.1 5000 typ host",
            sdpMLineIndex = 0,
            sdpMid = "audio"
        )

        val json = candidate.toJson()
        val parsed = Json.decodeFromString<IceCandidateData>(json)

        assertEquals(candidate.candidate, parsed.candidate)
        assertEquals(candidate.sdpMLineIndex, parsed.sdpMLineIndex)
        assertEquals(candidate.sdpMid, parsed.sdpMid)
    }

    @Test
    fun `IceCandidateData deserializes from JSON string`() {
        val json = """{"candidate":"candidate:1 1 udp 2113937151 10.0.0.1 5000 typ host","sdpMLineIndex":1,"sdpMid":"video"}"""

        val data = IceCandidateData.fromJson(json)

        assertEquals("candidate:1 1 udp 2113937151 10.0.0.1 5000 typ host", data.candidate)
        assertEquals(1, data.sdpMLineIndex)
        assertEquals("video", data.sdpMid)
    }

    @Test
    fun `IceCandidateData round-trips through JSON`() {
        val original = IceCandidateData(
            candidate = "candidate:842163049 1 udp 1677729535 93.184.216.34 50000 typ srflx raddr 10.0.0.1 rport 5000",
            sdpMLineIndex = 0,
            sdpMid = "0"
        )

        val roundTripped = IceCandidateData.fromJson(original.toJson())

        assertEquals(original, roundTripped)
    }

    @Test
    fun `IceCandidateData handles null sdpMid`() {
        val candidate = IceCandidateData(
            candidate = "candidate:1 1 udp 2113937151 10.0.0.1 5000 typ host",
            sdpMLineIndex = 0,
            sdpMid = null
        )

        val json = candidate.toJson()
        val parsed = IceCandidateData.fromJson(json)

        assertNull(parsed.sdpMid)
        assertEquals(candidate, parsed)
    }

    @Test
    fun `IceCandidateData deserialization ignores unknown fields`() {
        val json = """{"candidate":"candidate:1 1 udp 2113937151 10.0.0.1 5000 typ host","sdpMLineIndex":0,"sdpMid":"audio","extraField":"ignored"}"""

        val data = IceCandidateData.fromJson(json)

        assertEquals("candidate:1 1 udp 2113937151 10.0.0.1 5000 typ host", data.candidate)
        assertEquals(0, data.sdpMLineIndex)
        assertEquals("audio", data.sdpMid)
    }

    @Test
    fun `IceCandidateData handles complex SDP candidate string`() {
        // Real-world ICE candidate with relay
        val sdp = "candidate:3 1 tcp 1518280447 93.184.216.34 9 typ relay raddr 10.0.0.1 rport 0 generation 0 ufrag abc network-id 1"
        val candidate = IceCandidateData(
            candidate = sdp,
            sdpMLineIndex = 2,
            sdpMid = "data"
        )

        val roundTripped = IceCandidateData.fromJson(candidate.toJson())

        assertEquals(sdp, roundTripped.candidate)
        assertEquals(2, roundTripped.sdpMLineIndex)
        assertEquals("data", roundTripped.sdpMid)
    }

    @Test
    fun `IceCandidateData handles empty candidate string`() {
        // End-of-candidates indicator
        val candidate = IceCandidateData(
            candidate = "",
            sdpMLineIndex = 0,
            sdpMid = "0"
        )

        val roundTripped = IceCandidateData.fromJson(candidate.toJson())

        assertEquals("", roundTripped.candidate)
    }

    // ========================================================================
    // IceServer conversion (data mapping)
    // ========================================================================

    @Test
    fun `IceServer model holds URLs and optional credentials`() {
        val server = io.wolftown.kaiku.data.api.IceServer(
            urls = listOf("stun:stun.example.com:3478"),
            username = null,
            credential = null
        )

        assertEquals(1, server.urls.size)
        assertEquals("stun:stun.example.com:3478", server.urls[0])
        assertNull(server.username)
        assertNull(server.credential)
    }

    @Test
    fun `IceServer model holds TURN credentials`() {
        val server = io.wolftown.kaiku.data.api.IceServer(
            urls = listOf("turn:turn.example.com:3478", "turns:turn.example.com:5349"),
            username = "user",
            credential = "pass"
        )

        assertEquals(2, server.urls.size)
        assertEquals("user", server.username)
        assertEquals("pass", server.credential)
    }

    @Test
    fun `IceServerConfig holds list of servers`() {
        val config = io.wolftown.kaiku.data.api.IceServerConfig(
            iceServers = listOf(
                io.wolftown.kaiku.data.api.IceServer(
                    urls = listOf("stun:stun.example.com:3478")
                ),
                io.wolftown.kaiku.data.api.IceServer(
                    urls = listOf("turn:turn.example.com:3478"),
                    username = "user",
                    credential = "pass"
                )
            )
        )

        assertEquals(2, config.iceServers.size)
    }

    @Test
    fun `IceServerConfig serialization round-trip`() {
        val config = io.wolftown.kaiku.data.api.IceServerConfig(
            iceServers = listOf(
                io.wolftown.kaiku.data.api.IceServer(
                    urls = listOf("stun:stun.l.google.com:19302")
                ),
                io.wolftown.kaiku.data.api.IceServer(
                    urls = listOf("turn:turn.example.com:3478"),
                    username = "testuser",
                    credential = "testpass"
                )
            )
        )

        val json = Json.encodeToString(
            io.wolftown.kaiku.data.api.IceServerConfig.serializer(),
            config
        )
        val parsed = Json.decodeFromString(
            io.wolftown.kaiku.data.api.IceServerConfig.serializer(),
            json
        )

        assertEquals(config, parsed)
        assertEquals(2, parsed.iceServers.size)
        assertNull(parsed.iceServers[0].username)
        assertEquals("testuser", parsed.iceServers[1].username)
    }

    // ========================================================================
    // AudioRoute enum
    // ========================================================================

    @Test
    fun `AudioRoute enum contains all expected routes`() {
        val routes = AudioRoute.entries
        assertEquals(4, routes.size)
        assertTrue(routes.contains(AudioRoute.Speaker))
        assertTrue(routes.contains(AudioRoute.Earpiece))
        assertTrue(routes.contains(AudioRoute.Bluetooth))
        assertTrue(routes.contains(AudioRoute.WiredHeadset))
    }

    // ========================================================================
    // ICE candidate buffer cap
    // ========================================================================
    // These tests exercise the pure-Kotlin buffering path in addIceCandidate,
    // which does not touch the native WebRTC runtime when remoteDescriptionSet
    // is false. WebRtcManager is instantiated via newWebRtcManager(), which
    // injects a mocked EglBaseProvider so EglBase is never constructed.

    @Test
    fun `addIceCandidate buffers up to MAX_PENDING_CANDIDATES then drops`() {
        val webRtcManager = newWebRtcManager()
        // subscriberRemoteDescriptionSet defaults to false — buffering branch is taken.

        repeat(101) { i ->
            webRtcManager.addIceCandidate(
                "subscriber",
                """{"candidate":"c$i","sdpMLineIndex":0,"sdpMid":"0"}"""
            )
        }

        assertEquals(100, webRtcManager.subscriberPendingCandidatesSize())
    }

    @Test
    fun `addIceCandidate buffers publisher candidates up to cap then drops`() {
        val webRtcManager = newWebRtcManager()
        // publisherRemoteDescriptionSet defaults to false — buffering branch is taken.

        repeat(101) { i ->
            webRtcManager.addIceCandidate(
                "publisher",
                """{"candidate":"c$i","sdpMLineIndex":0,"sdpMid":"0"}"""
            )
        }

        assertEquals(100, webRtcManager.publisherPendingCandidatesSize())
    }

    @Test
    fun `addIceCandidate routes by pcType to publisher vs subscriber buffer`() {
        val webRtcManager = newWebRtcManager()

        // Add 5 publisher candidates
        repeat(5) { i ->
            webRtcManager.addIceCandidate(
                "publisher",
                """{"candidate":"pub-$i","sdpMLineIndex":0,"sdpMid":"0"}"""
            )
        }
        // Add 3 subscriber candidates
        repeat(3) { i ->
            webRtcManager.addIceCandidate(
                "subscriber",
                """{"candidate":"sub-$i","sdpMLineIndex":0,"sdpMid":"0"}"""
            )
        }

        assertEquals(5, webRtcManager.publisherPendingCandidatesSize())
        assertEquals(3, webRtcManager.subscriberPendingCandidatesSize())
    }

    @Test
    fun `addIceCandidate with unknown pcType is no-op`() {
        val webRtcManager = newWebRtcManager()

        webRtcManager.addIceCandidate(
            "invalid-type",
            """{"candidate":"c0","sdpMLineIndex":0,"sdpMid":"0"}"""
        )

        assertEquals(0, webRtcManager.publisherPendingCandidatesSize())
        assertEquals(0, webRtcManager.subscriberPendingCandidatesSize())
    }

    // ========================================================================
    // publisherLink StateFlow
    // ========================================================================
    // "Connected" is the publisher connection alone, together with the
    // generation of that connection. A test-only `internal` mutator on
    // WebRtcManager sets the publisher's ICE state without a live
    // PeerConnection.

    @Test
    fun `publisherLink starts false before any PC connects`() {
        val webRtcManager = newWebRtcManager()

        assertFalse(webRtcManager.publisherLink.value.connected)
    }

    @OptIn(ExperimentalCoroutinesApi::class)
    @Test
    fun `publisherLink follows the publisher PC only`() = runTest {
        val webRtcManager = newWebRtcManager(voiceScope = backgroundScope)

        webRtcManager.setPublisherIceStateForTest(PeerConnection.IceConnectionState.CONNECTED)
        runCurrent()
        assertTrue(webRtcManager.publisherLink.value.connected)

        webRtcManager.setPublisherIceStateForTest(PeerConnection.IceConnectionState.COMPLETED)
        runCurrent()
        assertTrue(webRtcManager.publisherLink.value.connected)

        webRtcManager.setPublisherIceStateForTest(PeerConnection.IceConnectionState.DISCONNECTED)
        runCurrent()
        assertFalse(webRtcManager.publisherLink.value.connected)
    }

    // ========================================================================
    // Publisher offer / answer flow
    // ========================================================================
    // createPublisherOffer / handlePublisherAnswer require
    // PeerConnectionFactory.createPeerConnection + createAudioSource +
    // createAudioTrack — all native-only. See header comment: these belong in
    // instrumented (androidTest) tests.

    // ========================================================================
    // Connected means the publisher; closed connections stay silent
    // ========================================================================

    @OptIn(ExperimentalCoroutinesApi::class)
    @Test
    fun `publisher connected alone counts as connected (empty room)`() = runTest {
        val webRtcManager = newWebRtcManager(voiceScope = backgroundScope)

        // Nobody else publishes: the server never offers the subscriber PC.
        webRtcManager.setPublisherIceStateForTest(PeerConnection.IceConnectionState.CONNECTED)
        runCurrent()

        assertTrue(webRtcManager.publisherLink.value.connected)
    }

    @OptIn(ExperimentalCoroutinesApi::class)
    @Test
    fun `a late ICE callback from a closed connection is ignored`() = runTest {
        val factory = mockk<PeerConnectionFactory>(relaxed = true)
        val observers = mutableListOf<PeerConnection.Observer>()
        io.mockk.every {
            factory.createPeerConnection(any<PeerConnection.RTCConfiguration>(), capture(observers))
        } answers { mockk(relaxed = true) }
        val voiceApi = mockk<VoiceApi>()
        coEvery { voiceApi.getIceServers() } returns IceServerConfig(iceServers = emptyList())
        val manager = newWebRtcManager(
            voiceScope = backgroundScope,
            voiceApi = voiceApi,
            factoryProvider = DefaultPeerConnectionFactoryProvider(create = { factory }),
        )
        val trouble = mutableListOf<IceTrouble>()
        manager.onIceTrouble = { t, _ -> trouble += t }
        manager.initialize()

        // Attempt 1 (timed out) and its connections are closed; attempt 2 starts.
        manager.createSubscriberPeerConnection()
        manager.createPublisherOffer()
        val (oldSub, oldPub) = observers[0] to observers[1]
        manager.closePeerConnections()
        manager.createSubscriberPeerConnection()
        manager.createPublisherOffer()
        runCurrent()

        oldPub.onIceConnectionChange(PeerConnection.IceConnectionState.CONNECTED)
        oldSub.onIceConnectionChange(PeerConnection.IceConnectionState.CONNECTED)
        oldPub.onIceConnectionChange(PeerConnection.IceConnectionState.FAILED)
        runCurrent()

        assertFalse(manager.publisherLink.value.connected)
        assertEquals(emptyList<IceTrouble>(), trouble)

        // The current publisher still counts.
        observers[3].onIceConnectionChange(PeerConnection.IceConnectionState.CONNECTED)
        runCurrent()
        assertTrue(manager.publisherLink.value.connected)
    }

    @Test
    fun `a connection created after the session ended is closed and not installed`() = runTest {
        val factory = mockk<PeerConnectionFactory>(relaxed = true)
        val created = mutableListOf<PeerConnection>()
        io.mockk.every {
            factory.createPeerConnection(any<PeerConnection.RTCConfiguration>(), any<PeerConnection.Observer>())
        } answers { mockk<PeerConnection>(relaxed = true).also { created += it } }
        val voiceApi = mockk<VoiceApi>()
        coEvery { voiceApi.getIceServers() } returns IceServerConfig(iceServers = emptyList())
        val manager = newWebRtcManager(
            voiceApi = voiceApi,
            factoryProvider = DefaultPeerConnectionFactoryProvider(create = { factory }),
        )
        manager.initialize()

        // The session ends while the native create runs: the guard says no afterwards.
        var live = true
        io.mockk.every {
            factory.createPeerConnection(any<PeerConnection.RTCConfiguration>(), any<PeerConnection.Observer>())
        } answers { live = false; mockk<PeerConnection>(relaxed = true).also { created += it } }

        assertFalse(manager.createSubscriberPeerConnection(guardOf { live }))
        live = true
        assertFalse(manager.createPublisherOffer(guardOf { live }))

        assertEquals(2, created.size)
        created.forEach { pc ->
            verify { pc.close() }
            verify { pc.dispose() }
        }
        // Nothing installed: no mic track, no offer.
        assertNull(manager.localAudioTrack)
        verify(exactly = 0) { created[1].createOffer(any(), any()) }
    }

    private fun guardOf(live: () -> Boolean) = SessionGuard { install ->
        if (live()) {
            install()
            true
        } else {
            false
        }
    }

    /** A manager whose factory hands out relaxed PCs and records their observers. */
    private fun managerWithObservers(
        scope: CoroutineScope,
        observers: MutableList<PeerConnection.Observer>,
    ): WebRtcManager {
        val factory = mockk<PeerConnectionFactory>(relaxed = true)
        io.mockk.every {
            factory.createPeerConnection(any<PeerConnection.RTCConfiguration>(), capture(observers))
        } answers { mockk(relaxed = true) }
        val voiceApi = mockk<VoiceApi>()
        coEvery { voiceApi.getIceServers() } returns IceServerConfig(iceServers = emptyList())
        return newWebRtcManager(
            voiceScope = scope,
            voiceApi = voiceApi,
            factoryProvider = DefaultPeerConnectionFactoryProvider(create = { factory }),
        )
    }

    // ========================================================================
    // Connection generations and the session guard
    // ========================================================================

    @Test
    fun `a callback validated before a retirement cannot publish CONNECTED after it`() = runTest {
        val observers = mutableListOf<PeerConnection.Observer>()
        val manager = managerWithObservers(backgroundScope, observers)
        manager.initialize()
        manager.createSubscriberPeerConnection()
        manager.createPublisherOffer()
        val oldPub = observers[1]

        // The old publisher's callback passes its identity check; right then a
        // timeout clean-up on another thread retires the connections.
        var retiredWhileCallbackRan = false
        val cleanup = Thread { manager.closePeerConnections() }
        manager.afterIdentityCheckForTest = {
            manager.afterIdentityCheckForTest = null
            cleanup.start()
            cleanup.join(300)
            retiredWhileCallbackRan = !cleanup.isAlive
        }
        oldPub.onIceConnectionChange(PeerConnection.IceConnectionState.CONNECTED)
        // The clean-up completes once the callback released the monitor.
        cleanup.join(5_000)

        // The retirement waited for the callback, and the link it left says
        // "not connected" for the new generation: no stale success survives.
        assertFalse(retiredWhileCallbackRan)
        assertEquals(PublisherLink(manager.currentGeneration, false), manager.publisherLink.value)
    }

    @Test
    fun `the publisher link carries the generation of its connection`() = runTest {
        val observers = mutableListOf<PeerConnection.Observer>()
        val manager = managerWithObservers(backgroundScope, observers)
        manager.initialize()
        val gen = manager.closePeerConnections()
        manager.createSubscriberPeerConnection()
        manager.createPublisherOffer()

        observers[1].onIceConnectionChange(PeerConnection.IceConnectionState.CONNECTED)

        assertEquals(PublisherLink(gen, true), manager.publisherLink.value)
    }

    @Test
    fun `the reconnecting indication follows the publisher only`() = runTest {
        val observers = mutableListOf<PeerConnection.Observer>()
        val manager = managerWithObservers(backgroundScope, observers)
        val trouble = mutableListOf<IceTrouble>()
        manager.onIceTrouble = { t, _ -> trouble += t }
        manager.initialize()
        manager.createSubscriberPeerConnection()
        manager.createPublisherOffer()
        val (sub, pub) = observers[0] to observers[1]

        pub.onIceConnectionChange(PeerConnection.IceConnectionState.DISCONNECTED)
        sub.onIceConnectionChange(PeerConnection.IceConnectionState.DISCONNECTED)
        // Only the subscriber comes back: the mic upload is still down.
        sub.onIceConnectionChange(PeerConnection.IceConnectionState.CONNECTED)
        assertEquals(listOf(IceTrouble.Disconnected), trouble)

        pub.onIceConnectionChange(PeerConnection.IceConnectionState.CONNECTED)
        assertEquals(listOf(IceTrouble.Disconnected, IceTrouble.Recovered), trouble)

        // A failed subscriber still asks for a rejoin.
        sub.onIceConnectionChange(PeerConnection.IceConnectionState.FAILED)
        assertEquals(IceTrouble.Failed, trouble.last())
    }

    // ========================================================================
    // Mute and the microphone track
    // ========================================================================

    /** A manager whose factory hands out relaxed PCs and records the microphone tracks it creates. */
    private fun managerWithMicTracks(scope: CoroutineScope, tracks: MutableList<AudioTrack>): WebRtcManager {
        val factory = mockk<PeerConnectionFactory>(relaxed = true)
        io.mockk.every { factory.createAudioTrack(any(), any()) } answers {
            mockk<AudioTrack>(relaxed = true).also { tracks += it }
        }
        val voiceApi = mockk<VoiceApi>()
        coEvery { voiceApi.getIceServers() } returns IceServerConfig(iceServers = emptyList())
        return newWebRtcManager(
            voiceScope = scope,
            voiceApi = voiceApi,
            factoryProvider = DefaultPeerConnectionFactoryProvider(create = { factory }),
        )
    }

    @Test
    fun `mute reaches the microphone track of the current connections only`() = runTest {
        val tracks = mutableListOf<AudioTrack>()
        val manager = managerWithMicTracks(backgroundScope, tracks)
        manager.initialize()
        manager.createSubscriberPeerConnection()
        manager.createPublisherOffer()

        manager.setMuted(true)
        verify(exactly = 1) { tracks[0].setEnabled(false) }

        // A rejoin: the old track is disposed, a new one is made, muted from the start.
        manager.closePeerConnections()
        verify(exactly = 1) { tracks[0].dispose() }
        manager.createSubscriberPeerConnection()
        manager.createPublisherOffer()
        verify(exactly = 1) { tracks[1].setEnabled(false) }

        manager.setMuted(false)
        verify(exactly = 1) { tracks[1].setEnabled(true) }
        // Created enabled, muted once, and never touched after its disposal.
        verify(exactly = 2) { tracks[0].setEnabled(any()) }
    }

    @Test
    fun `mute without connections only remembers the state`() = runTest {
        val tracks = mutableListOf<AudioTrack>()
        val manager = managerWithMicTracks(backgroundScope, tracks)
        manager.initialize()
        manager.createSubscriberPeerConnection()
        manager.createPublisherOffer()
        manager.closePeerConnections()

        manager.setMuted(true)

        assertTrue(manager.isMuted)
        // Only the call at its creation.
        verify(exactly = 1) { tracks[0].setEnabled(any()) }
    }

    @Test
    fun `a mute that hits a disposed track is ignored`() = runTest {
        val tracks = mutableListOf<AudioTrack>()
        val manager = managerWithMicTracks(backgroundScope, tracks)
        manager.initialize()
        manager.createSubscriberPeerConnection()
        manager.createPublisherOffer()
        // What WebRTC does for a track that was disposed underneath the caller.
        io.mockk.every { tracks[0].setEnabled(any()) } throws
            IllegalStateException("MediaStreamTrack has been disposed.")

        manager.setMuted(true)

        assertTrue(manager.isMuted)
    }

    @Test
    fun `closing the connections waits for a mute that is being applied`() = runTest {
        val tracks = mutableListOf<AudioTrack>()
        val manager = managerWithMicTracks(backgroundScope, tracks)
        manager.initialize()
        manager.createSubscriberPeerConnection()
        manager.createPublisherOffer()
        var disposed = false
        io.mockk.every { tracks[0].dispose() } answers { disposed = true }
        // setEnabled is still waiting for WebRTC's thread when a rejoin closes the connections.
        var disposedWhileMuting = false
        val closer = Thread { manager.closePeerConnections() }
        io.mockk.every { tracks[0].setEnabled(false) } answers {
            closer.start()
            closer.join(300)
            disposedWhileMuting = disposed
            true
        }

        manager.setMuted(true)
        closer.join(5_000)

        assertFalse("the track was disposed while setEnabled was using it", disposedWhileMuting)
        assertTrue(disposed)
        assertNull(manager.localAudioTrack)
    }

    @Test
    fun `a new connection is installed inside the session guard, in one step with its check`() = runTest {
        val observers = mutableListOf<PeerConnection.Observer>()
        val manager = managerWithObservers(backgroundScope, observers)
        manager.initialize()
        val installedInsideGuard = mutableListOf<Boolean>()
        val guard = SessionGuard { install ->
            val before = manager.hasSubscriberPcForTest() || manager.hasPublisherPcForTest()
            install()
            installedInsideGuard += !before &&
                (manager.hasSubscriberPcForTest() || manager.hasPublisherPcForTest())
            true
        }

        assertTrue(manager.createSubscriberPeerConnection(guard))
        manager.closePeerConnections()
        assertTrue(manager.createPublisherOffer(guard))

        // Checks without install (before the native create) report false; the
        // install steps each installed their connection inside the guard.
        assertEquals(2, installedInsideGuard.count { it })
    }
}
