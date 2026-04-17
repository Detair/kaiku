package io.wolftown.kaiku.data.voice

import android.content.Context
import io.mockk.mockk
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
import org.junit.Assert.assertTrue
import org.junit.Test
import org.webrtc.PeerConnection

/**
 * Unit tests for the WebRTC voice layer.
 *
 * The `org.webrtc.*` classes (PeerConnection, AudioTrack, etc.) require the
 * Android runtime and cannot be instantiated in JVM unit tests. Therefore,
 * this test class focuses on:
 *
 * - ICE candidate serialization / deserialization ([IceCandidateData])
 * - Mute state logic (pure boolean tracking)
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
     * The provider's `create()` is never invoked unless the test reads `.eglBase`.
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
    ): WebRtcManager = WebRtcManager(
        context = mockk<Context>(relaxed = true),
        voiceApi = mockk<VoiceApi>(relaxed = true),
        eglBaseProvider = mockk<EglBaseProvider>(relaxed = true),
        voiceScope = voiceScope,
    )

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
    // Mute state logic
    // ========================================================================
    // WebRtcManager requires android.content.Context (Hilt @ApplicationContext)
    // and cannot be instantiated in pure JVM unit tests. The isMuted field and
    // setMuted() method need Robolectric or instrumented tests.
    //
    // TODO: isMuted / setMuted tests require Robolectric (PeerConnection dependency)

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
    // voiceIceConnected combined StateFlow
    // ========================================================================
    // The combined flow is backed by an Eagerly-started stateIn scope that
    // observes both publisherIceState and subscriberIceState. Test-only
    // `internal` mutators on WebRtcManager feed the underlying StateFlows
    // without requiring a live PeerConnection.

    @Test
    fun `voiceIceConnected starts false before any PC connects`() {
        val webRtcManager = newWebRtcManager()

        assertFalse(webRtcManager.voiceIceConnected.value)
    }

    @OptIn(ExperimentalCoroutinesApi::class)
    @Test
    fun `voiceIceConnected emits true only when both PCs reach Connected`() = runTest {
        val webRtcManager = newWebRtcManager(voiceScope = backgroundScope)

        // Only publisher CONNECTED — combined is still false.
        webRtcManager.setPublisherIceStateForTest(
            PeerConnection.IceConnectionState.CONNECTED
        )
        runCurrent()
        assertFalse(webRtcManager.voiceIceConnected.value)

        // Both CONNECTED — combined becomes true.
        webRtcManager.setSubscriberIceStateForTest(
            PeerConnection.IceConnectionState.CONNECTED
        )
        runCurrent()
        assertTrue(webRtcManager.voiceIceConnected.value)

        // Subscriber drops to DISCONNECTED — combined returns to false.
        webRtcManager.setSubscriberIceStateForTest(
            PeerConnection.IceConnectionState.DISCONNECTED
        )
        runCurrent()
        assertFalse(webRtcManager.voiceIceConnected.value)
    }

    // ========================================================================
    // Publisher offer / answer flow
    // ========================================================================
    // createPublisherOffer / handlePublisherAnswer require
    // PeerConnectionFactory.createPeerConnection + createAudioSource +
    // createAudioTrack — all native-only. See header comment: these belong in
    // instrumented (androidTest) tests.
}
