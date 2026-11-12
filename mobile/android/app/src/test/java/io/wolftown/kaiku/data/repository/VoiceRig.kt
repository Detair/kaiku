package io.wolftown.kaiku.data.repository

import android.content.Context
import io.mockk.coEvery
import io.mockk.every
import io.mockk.mockk
import io.mockk.mockkObject
import io.mockk.unmockkObject
import io.wolftown.kaiku.data.voice.AudioRouteManager
import io.wolftown.kaiku.data.voice.IceTrouble
import io.wolftown.kaiku.data.voice.MicPermissionChecker
import io.wolftown.kaiku.data.voice.PublisherLink
import io.wolftown.kaiku.data.voice.SessionGuard
import io.wolftown.kaiku.data.voice.VoiceEndNotice
import io.wolftown.kaiku.data.voice.VoiceLinkState
import io.wolftown.kaiku.data.voice.VoiceServiceEvents
import io.wolftown.kaiku.data.voice.WebRtcManager
import io.wolftown.kaiku.data.ws.ClientEvent
import io.wolftown.kaiku.data.ws.KaikuWebSocket
import io.wolftown.kaiku.data.ws.ServerEvent
import io.wolftown.kaiku.service.OngoingCallNotification
import io.wolftown.kaiku.service.VoiceCallService
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.CoroutineStart
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.awaitCancellation
import kotlinx.coroutines.cancel
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.launch
import kotlinx.coroutines.test.StandardTestDispatcher
import kotlinx.coroutines.test.TestScope
import kotlinx.coroutines.test.runCurrent
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue

/** Runs [body] with a fresh [VoiceRig] on the test's virtual time. */
@OptIn(ExperimentalCoroutinesApi::class)
internal fun rigTest(body: suspend TestScope.(VoiceRig) -> Unit) = runTest {
    val rig = VoiceRig(this)
    try {
        body(rig)
    } finally {
        rig.close()
    }
}

/**
 * A [VoiceRepository] on virtual time for interleaving tests, wired to
 * - a controllable socket ([socket]): authenticated sockets come ([FakeSocket.ready])
 *   and go ([FakeSocket.drop]) when the test says so,
 * - a small model of the server behind it ([server]): a full voice channel in
 *   which this user holds at most one place, and
 * - a recording stand-in for the WebRTC layer.
 *
 * Everything that reaches the socket, the WebRTC layer and the call service is
 * appended to [log] in order. [at] runs a block inside a named step, the way
 * another thread would interleave with the repository at exactly that point:
 *
 * - `join:check`: a `voice_join` is about to be sent; the caller has read the
 *   socket state, the socket has not checked it yet.
 * - `join:refused`: the socket refused that `voice_join`; the caller has not
 *   looked at the socket state again yet.
 * - `join:sent`: the `voice_join` went out.
 * - `sub:create`, `pub:create`: a peer connection is being created (the native
 *   step, before it is installed).
 * - `close`: the peer connections are being closed.
 * - `socket:ready`: a socket was just authenticated; nothing has looked at it yet.
 *
 * The `join:` steps run inside the repository's own critical section. A
 * socket event there is exactly the race under test. A session end there
 * stands for one right after that section: `join:refused` and `join:sent`
 * have such an equivalent, `join:check` has one only if the socket is lost in
 * that same step (real threads cannot end the session between the check and
 * the send).
 */
@OptIn(ExperimentalCoroutinesApi::class)
internal class VoiceRig(private val test: TestScope) {

    val channel = "ch-1"
    val log = mutableListOf<String>()

    private val hooks = mutableMapOf<String, MutableList<() -> Unit>>()
    private val lastHooks = mutableMapOf<String, MutableList<() -> Unit>>()
    private val reached = mutableMapOf<String, Int>()

    private val dispatcher = StandardTestDispatcher(test.testScheduler)
    private val voiceScope = CoroutineScope(SupervisorJob() + dispatcher)

    /** For what the user or another thread does next to the repository. */
    private val side = CoroutineScope(SupervisorJob() + dispatcher)

    val events = MutableSharedFlow<ServerEvent>(extraBufferCapacity = 64)
    val socket = FakeSocket()
    val server = FakeServer()
    val service = FakeCallService()

    private val webSocket: KaikuWebSocket = mockk(relaxed = true)
    private val rtc: WebRtcManager = mockk(relaxed = true)
    private val publisherLink = MutableStateFlow(PublisherLink(generation = 0L, connected = false))
    private var generation = 0L
    private var iceTrouble: ((IceTrouble, Long) -> Unit)? = null

    val repository: VoiceRepository

    init {
        mockkObject(VoiceCallService.Companion)
        every { VoiceCallService.start(any(), any(), any()) } answers {
            log += "service:start"
            service.start()
        }
        every { VoiceCallService.stop(any()) } answers {
            log += "service:stop"
            service.stop()
        }

        stubSocket()
        stubWebRtc()

        val audioRouteManager: AudioRouteManager = mockk(relaxed = true)
        every { audioRouteManager.requestAudioFocus() } answers { log += "audio:focus" }
        every { audioRouteManager.abandonAudioFocus() } answers { log += "audio:abandon" }

        val voiceServiceEvents: VoiceServiceEvents = mockk(relaxed = true)
        every { voiceServiceEvents.events } returns MutableSharedFlow()
        val micPermissionChecker: MicPermissionChecker = mockk()
        every { micPermissionChecker.isGranted() } returns true

        repository = VoiceRepository(
            webRtcManager = rtc,
            webSocket = webSocket,
            audioRouteManager = audioRouteManager,
            voiceServiceEvents = voiceServiceEvents,
            micPermissionChecker = micPermissionChecker,
            context = mockk<Context>(relaxed = true),
            voiceScope = voiceScope,
        )
    }

    fun close() {
        side.cancel()
        voiceScope.cancel()
        unmockkObject(VoiceCallService.Companion)
    }

    // -- Interleaving ---------------------------------------------------------

    /** Runs [block] inside step [step] when it is reached for the [nth] time. */
    fun at(step: String, nth: Int = 1, block: () -> Unit) {
        hooks.getOrPut("$step#$nth") { mutableListOf() } += block
    }

    /** Like [at], but after every [at] block of that step, whenever those were registered. */
    fun atLast(step: String, nth: Int = 1, block: () -> Unit) {
        lastHooks.getOrPut("$step#$nth") { mutableListOf() } += block
    }

    /** Runs [block] at virtual time [millis] after now. */
    fun atTime(millis: Long, block: () -> Unit) {
        side.launch {
            delay(millis)
            block()
        }
    }

    private fun step(name: String) {
        val n = (reached[name] ?: 0) + 1
        reached[name] = n
        hooks.remove("$name#$n")?.forEach { it() }
        lastHooks.remove("$name#$n")?.forEach { it() }
    }

    // -- The socket -----------------------------------------------------------

    inner class FakeSocket {
        private var sockets = 0L

        /** The id of the authenticated socket, or null. Ids never repeat. */
        val authenticated = MutableStateFlow<Long?>(null)

        /** Every frame that left, with the id of the socket that carried it. */
        val frames = mutableListOf<Pair<Long, ClientEvent>>()

        /** A socket is open and has sent Authenticate; its Ready is still out: no authenticated socket. */
        fun connecting() {
            drop()
        }

        /** Ready arrived: a new authenticated socket. Returns its id. */
        fun ready(): Long {
            val id = ++sockets
            authenticated.value = id
            log += "READY@$id"
            events.tryEmit(ServerEvent.Ready(userId = "me"))
            step("socket:ready")
            return id
        }

        /** The socket closed; the server drops its voice peer at once. */
        fun drop() {
            val id = authenticated.value ?: return
            authenticated.value = null
            server.socketClosed(id)
            log += "DROP@$id"
        }

        /** Like [KaikuWebSocket.sendOn]: the frame goes out on socket [id], or not at all. */
        fun sendOn(id: Long, event: ClientEvent): Boolean {
            val join = event is ClientEvent.VoiceJoin
            if (join) step("join:check")
            if (authenticated.value != id) {
                if (join) step("join:refused")
                return false
            }
            frames += id to event
            log += "send:${event::class.simpleName}@$id"
            server.receive(id, event)
            if (join) step("join:sent")
            return true
        }
    }

    private fun stubSocket() {
        every { webSocket.events } returns events
        every { webSocket.authenticatedSocket } returns socket.authenticated
        every { webSocket.sendOn(any(), any()) } answers { socket.sendOn(firstArg(), secondArg()) }
        // Voice frames must name their socket; one that does not shows up in the log.
        every { webSocket.send(any()) } answers {
            log += "send:${firstArg<ClientEvent>()::class.simpleName}@any"
            true
        }
    }

    // -- The server -----------------------------------------------------------

    /**
     * A full voice channel. This user's `voice_join` is accepted only while
     * the server holds no voice peer of theirs; any further `voice_join` is
     * refused ("channel full"), as the real capacity check does before it
     * would replace a stale peer. A closing socket takes its peer with it.
     */
    inner class FakeServer {
        /** The socket this user's voice peer lives on, or null. */
        var peer: Long? = null
            private set

        /** `voice_join` frames that hit a channel in which we already held our place. */
        var refusedJoins = 0
            private set

        fun receive(socket: Long, event: ClientEvent) {
            when (event) {
                is ClientEvent.VoiceJoin -> if (peer == null) {
                    peer = socket
                } else {
                    refusedJoins++
                    events.tryEmit(ServerEvent.VoiceError(code = "VOICE_ERROR", message = "Channel is full"))
                }
                is ClientEvent.VoiceLeave -> if (peer == socket) peer = null
                else -> {}
            }
        }

        fun socketClosed(socket: Long) {
            if (peer == socket) peer = null
        }

        /** The server removes the peer itself (kick). */
        fun removePeer() {
            peer = null
        }
    }

    // -- The call service -----------------------------------------------------

    /**
     * The call service as far as its notification goes, with Android's part
     * in it: `start` puts the service in the foreground with its notification
     * and the service keeps it in step with the voice link
     * ([OngoingCallNotification], the production code). `stop` makes Android
     * take the foreground notification away at once, while the service's
     * `onDestroy` reaches the main thread a moment later (here: 1 ms).
     */
    inner class FakeCallService {
        /** Whether a call notification is up. */
        var notificationShown = false
            private set

        /** What happened to the notification, in order: foreground, post:…, stop, cancel, destroyed. */
        val notifications = mutableListOf<String>()

        private var running: OngoingCallNotification? = null

        fun start() {
            if (running != null) return
            notifications += "foreground"
            notificationShown = true
            running = OngoingCallNotification(
                scope = side,
                callIsOn = { repository.currentChannelId.value != null },
                linkState = repository.linkState,
                post = { reconnecting ->
                    notifications += if (reconnecting) "post:reconnecting" else "post:in-call"
                    notificationShown = true
                },
                cancel = {
                    notifications += "cancel"
                    notificationShown = false
                },
            ).also { it.follow() }
        }

        fun stop() {
            val stopped = running ?: return
            running = null
            notifications += "stop"
            notificationShown = false
            side.launch {
                delay(1)
                stopped.destroy()
                notifications += "destroyed"
            }
        }
    }

    /** No call notification is up, and none was posted once the service had been told to stop. */
    fun assertNoCallNotification() {
        assertFalse("a call notification stayed behind: ${service.notifications}", service.notificationShown)
        val afterStop = service.notifications.drop(service.notifications.lastIndexOf("stop") + 1)
        assertFalse(
            "a notification was posted after the service was told to stop: ${service.notifications}",
            afterStop.any { it.startsWith("post:") },
        )
    }

    // -- The WebRTC layer -----------------------------------------------------

    private fun stubWebRtc() {
        every { rtc.onIceTrouble = any() } answers { iceTrouble = firstArg() }
        every { rtc.publisherLink } returns publisherLink
        every { rtc.currentGeneration } answers { generation }
        every { rtc.closePeerConnections() } answers {
            step("close")
            log += "close"
            generation++
            publisherLink.value = PublisherLink(generation, false)
            generation
        }
        // Like WebRtcManager: the native create runs (anything may happen
        // meanwhile), then the guard decides whether the connection is
        // installed or closed again.
        coEvery { rtc.createSubscriberPeerConnection(any()) } coAnswers {
            step("sub:create")
            val installed = firstArg<SessionGuard>().runIfCurrent {}
            log += if (installed) "sub" else "sub-discarded"
            installed
        }
        coEvery { rtc.createPublisherOffer(any()) } coAnswers {
            step("pub:create")
            val installed = firstArg<SessionGuard>().runIfCurrent {}
            log += if (installed) "pub" else "pub-discarded"
            // Media connects when the server holds our peer on the socket that is up.
            if (installed && mediaConnects) connectPublisher()
            installed
        }
    }

    /** The set-up of a join (creating the subscriber connection) never finishes; it can still be cancelled. */
    fun hangSubscriberSetup() {
        coEvery { rtc.createSubscriberPeerConnection(any()) } coAnswers { awaitCancellation() }
    }

    /**
     * Whether a new publisher connects by itself as soon as it can. Off: it
     * stays unconnected until the test calls [connectPublisher].
     */
    var mediaConnects = true

    /** The current publisher connects, if the server holds our peer on the socket that is up. */
    fun connectPublisher() {
        val peer = server.peer
        if (peer != null && peer == socket.authenticated.value) {
            publisherLink.value = PublisherLink(generation, true)
        }
    }

    /** The current publisher connection reports ICE trouble. */
    fun iceTrouble(trouble: IceTrouble) {
        iceTrouble!!.invoke(trouble, generation)
    }

    // -- What the user and the server do --------------------------------------

    /** Joins on a first authenticated socket and waits until the call is up. */
    fun joinAndConnect() {
        socket.ready()
        repository.startJoin(channel)
        test.runCurrent()
        assertCallUp(joins = listOf(1L))
    }

    /** The user leaves, as a second thread would at this very point. */
    fun leave() {
        log += "LEAVE"
        side.launch(start = CoroutineStart.UNDISPATCHED) { repository.leaveChannel() }
    }

    /** The server removes the user and says so, on the event thread, at this very point. */
    fun removed() {
        log += "REMOVED"
        server.removePeer()
        repository.handleServerEventNow(channel, ServerEvent.VoiceRemoved(channelId = channel, reason = "kicked"))
    }

    // -- What happened --------------------------------------------------------

    /** The sockets that carried a `voice_join`, in order. */
    fun joins(): List<Long> = socket.frames.filter { it.second is ClientEvent.VoiceJoin }.map { it.first }

    fun closes(): Int = log.count { it == "close" }

    /**
     * The call was given up: it is over locally with the Rejoin action, the
     * connections are closed (which ends the microphone capture), audio focus
     * is released and the call service (the microphone foreground service)
     * is stopped.
     */
    fun assertGaveUp(wasMuted: Boolean = false) {
        assertEquals(null, repository.currentChannelId.value)
        assertEquals(VoiceEndNotice.RejoinFailed(channel, wasMuted), repository.endNotice.value)
        assertEquals(VoiceLinkState.Stable, repository.linkState.value)
        assertFalse(repository.isConnected.value)
        val lastInstall = maxOf(log.lastIndexOf("sub"), log.lastIndexOf("pub"))
        assertTrue("connections not closed: $log", log.lastIndexOf("close") > lastInstall)
        assertTrue("audio focus not released: $log", "audio:abandon" in log)
        assertTrue("call service not stopped: $log", "service:stop" in log)
        assertNoCallNotification()
        assertEquals("a voice_join hit a channel we already held our place in: $log", 0, server.refusedJoins)
        assertFalse("a voice frame went out without naming its socket: $log", log.any { it.endsWith("@any") })
    }

    /** The call is up on exactly these joins and nothing went wrong on the way. */
    fun assertCallUp(joins: List<Long>) {
        assertEquals("voice_join frames by socket: $log", joins, joins())
        assertEquals("a voice_join hit a channel we already held our place in: $log", 0, server.refusedJoins)
        assertEquals("the server holds our peer on the last join's socket: $log", joins.last(), server.peer)
        assertEquals(channel, repository.currentChannelId.value)
        assertTrue("not connected: $log", repository.isConnected.value)
        assertEquals(VoiceLinkState.Stable, repository.linkState.value)
        assertEquals(null, repository.endNotice.value)
        assertEquals(null, repository.error.value)
        assertFalse("a voice frame went out without naming its socket: $log", log.any { it.endsWith("@any") })
        // The service was started once and never stopped; its notification is up.
        assertEquals("call service starts and stops: $log", listOf("service:start"), log.filter { it.startsWith("service:") })
        assertTrue("no call notification: ${service.notifications}", service.notificationShown)
    }

    /** The session ended at [marker]: nothing was sent or created after it, and everything is closed. */
    fun assertEndedAt(marker: String, ownLeave: Boolean) {
        val at = log.indexOf(marker)
        assertTrue("marker $marker missing: $log", at >= 0)
        val after = log.drop(at + 1)
        val sent = after.filter { it.startsWith("send:") }
        if (ownLeave) {
            // At most the voice_leave of this very leave.
            assertTrue("frames after $marker: $log", sent.all { it.startsWith("send:VoiceLeave@") } && sent.size <= 1)
        } else {
            assertEquals("frames after $marker: $log", emptyList<String>(), sent)
        }
        assertFalse("subscriber connection installed after $marker: $log", "sub" in after)
        assertFalse("publisher connection installed after $marker: $log", "pub" in after)
        val lastInstall = maxOf(log.lastIndexOf("sub"), log.lastIndexOf("pub"))
        if (lastInstall >= 0) {
            assertTrue("a connection was never closed: $log", log.lastIndexOf("close") > lastInstall)
        }
        assertTrue("call service not stopped after $marker: $log", "service:stop" in after)
        assertTrue("audio focus not released after $marker: $log", "audio:abandon" in after)
        assertEquals("the server still holds our peer: $log", null, server.peer)
        assertEquals("a voice_join hit a channel we already held our place in: $log", 0, server.refusedJoins)
        assertEquals(null, repository.currentChannelId.value)
        assertFalse(repository.isConnected.value)
        assertEquals(VoiceLinkState.Stable, repository.linkState.value)
        assertFalse("a voice frame went out without naming its socket: $log", log.any { it.endsWith("@any") })
        assertNoCallNotification()
    }
}
