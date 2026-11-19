package io.wolftown.kaiku.data.call

import io.ktor.http.HttpStatusCode
import io.mockk.coEvery
import io.mockk.coVerify
import io.mockk.every
import io.mockk.mockk
import io.wolftown.kaiku.data.api.ApiException
import io.wolftown.kaiku.data.api.CallApi
import io.wolftown.kaiku.data.api.CallStateDto
import io.wolftown.kaiku.data.local.AuthState
import io.wolftown.kaiku.data.repository.VoiceRepository
import io.wolftown.kaiku.data.voice.RejoinGate
import io.wolftown.kaiku.data.voice.RejoinVerdict
import io.wolftown.kaiku.data.ws.KaikuWebSocket
import io.wolftown.kaiku.data.ws.ServerEvent
import io.wolftown.kaiku.data.ws.VoiceRemovedReason
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.test.TestScope
import kotlinx.coroutines.test.advanceTimeBy
import kotlinx.coroutines.test.currentTime
import kotlinx.coroutines.test.runCurrent
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * The call owner on virtual time: socket events in, REST and voice out, the
 * 100 s ring cap, the call resync, the rejoin gate and the signed-in user.
 */
@OptIn(ExperimentalCoroutinesApi::class)
class CallControllerTest {

    private val dm = "dm-1"

    private class FakeApi : CallApi {
        val calls = mutableListOf<String>()
        var current: CallStateDto? = null
        var startError: Exception? = null
        var getError: Exception? = null

        override suspend fun get(channelId: String): CallStateDto? {
            calls += "get"
            getError?.let { throw it }
            return current
        }

        override suspend fun start(channelId: String): CallStateDto {
            calls += "start"
            startError?.let { throw it }
            return CallStateDto(channelId, "c-1", "ringing", startedBy = "me")
        }

        override suspend fun join(channelId: String): CallStateDto {
            calls += "join"
            return CallStateDto(channelId, "c-1", "active", participants = listOf("anna", "me"))
        }

        override suspend fun decline(channelId: String): CallStateDto {
            calls += "decline"
            return CallStateDto(channelId, "c-1", "ended")
        }

        override suspend fun leave(channelId: String): CallStateDto {
            calls += "leave"
            return CallStateDto(channelId, "c-1", "ended")
        }
    }

    private class FakeNotifier : CallNotifier {
        val log = mutableListOf<String>()
        override fun ring(channelId: String, callId: String, callerId: String, callerName: String?) { log += "ring:$callId:$callerName" }
        override fun stopRing(channelId: String, callId: String) { log += "stop:$callId" }
        override fun missed(channelId: String, callId: String, callerName: String?) { log += "missed:$callId" }
        override fun stopAll() { log += "stopAll" }
    }

    private inner class Rig(val test: TestScope) {
        val api = FakeApi()
        val notifier = FakeNotifier()
        val events = MutableSharedFlow<ServerEvent>(extraBufferCapacity = 16)
        val callEnds = MutableSharedFlow<CallVoiceEnd>(extraBufferCapacity = 8)
        val voiceChannel = MutableStateFlow<String?>(null)
        var gate: RejoinGate? = null

        /** Clock jump on top of virtual time (device sleep). */
        var slept = 0L

        val voice: VoiceRepository = mockk(relaxed = true) {
            every { callEnds } returns this@Rig.callEnds
            every { currentChannelId } returns voiceChannel
            coEvery { joinCall(any(), any(), any()) } coAnswers {
                gate = thirdArg()
                voiceChannel.value = firstArg()
            }
            coEvery { leaveChannel() } coAnswers { voiceChannel.value = null }
        }
        val socket: KaikuWebSocket = mockk(relaxed = true) { every { events } returns this@Rig.events }
        val auth = AuthState(test.backgroundScope).also { it.setLoggedIn("me") }

        val controller = CallController(
            api = api,
            voice = voice,
            webSocket = socket,
            authState = auth,
            notifier = notifier,
            directory = object : CallDirectory {
                override fun nameOf(channelId: String, userId: String) = if (userId == "anna") "Anna" else null
                override fun titleOf(channelId: String) = "Anna"
            },
            clock = { test.currentTime + slept },
            voiceScope = test.backgroundScope,
        )

        init {
            test.runCurrent()
            controller.start()
            test.runCurrent()
        }

        val state get() = controller.state.value

        fun server(event: ServerEvent) {
            events.tryEmit(event)
            test.runCurrent()
        }

        fun ring(callId: String = "c-1") = server(ServerEvent.IncomingCall(dm, callId, "anna", "anna"))
    }

    private fun TestScope.rig() = Rig(this)

    @Test
    fun `an incoming call rings with the display name and accepting answers then joins voice`() = runTest {
        val r = rig()
        r.ring()
        assertEquals(CallState.Incoming(dm, "c-1", "anna", "Anna", 0L), r.state)
        assertEquals(listOf("ring:c-1:Anna"), r.notifier.log)

        r.controller.accept(dm)
        runCurrent()

        assertEquals(listOf("join"), r.api.calls)
        coVerify(exactly = 1) { r.voice.joinCall(dm, "Anna", any()) }
        assertEquals(CallState.Connected(dm, "c-1", 0L, setOf("anna")), r.state)
        assertEquals(listOf("ring:c-1:Anna", "stop:c-1"), r.notifier.log)
    }

    @Test
    fun `answering on another device stops ringing here`() = runTest {
        val r = rig()
        r.ring()
        r.server(ServerEvent.CallParticipantJoined(dm, "c-1", "me", "me"))
        assertEquals(CallState.Idle, r.state)
        assertEquals(listOf("ring:c-1:Anna", "stop:c-1"), r.notifier.log)
        assertTrue(r.api.calls.isEmpty())
    }

    @Test
    fun `an incoming ring ends after 100 s as missed`() = runTest {
        val r = rig()
        r.ring()
        advanceTimeBy(99_999); runCurrent()
        assertTrue(r.state is CallState.Incoming)
        advanceTimeBy(1); runCurrent()
        assertEquals(CallState.Idle, r.state)
        assertEquals(listOf("ring:c-1:Anna", "stop:c-1", "missed:c-1"), r.notifier.log)
        assertTrue(r.api.calls.isEmpty())
    }

    @Test
    fun `the ring cap counts time the device slept`() = runTest {
        val r = rig()
        r.ring()
        advanceTimeBy(1_000); runCurrent()
        r.slept = 120_000 // the phone slept two minutes; no delay ran meanwhile
        advanceTimeBy(5_000); runCurrent()
        assertEquals(CallState.Idle, r.state)
        assertTrue("missed:c-1" in r.notifier.log)
    }

    @Test
    fun `an outgoing call starts, joins voice, connects and is capped at 100 s if nobody answers`() = runTest {
        val r = rig()
        r.controller.startCall(dm)
        runCurrent()
        assertEquals(listOf("start"), r.api.calls)
        assertEquals(CallState.Outgoing(dm, "c-1", 0L), r.state)
        coVerify(exactly = 1) { r.voice.joinCall(dm, "Anna", any()) }

        advanceTimeBy(100_000); runCurrent()

        assertEquals(CallState.Ended(dm, CallEndReason.NoAnswer), r.state)
        assertEquals(listOf("start", "leave"), r.api.calls)
        coVerify(exactly = 1) { r.voice.leaveChannel() }
        advanceTimeBy(CallMachine.ENDED_SHOWN_MS); runCurrent()
        assertEquals(CallState.Idle, r.state)
    }

    @Test
    fun `the other side answering connects an outgoing call`() = runTest {
        val r = rig()
        r.controller.startCall(dm)
        runCurrent()
        advanceTimeBy(4_000)
        r.server(ServerEvent.CallParticipantJoined(dm, "c-1", "anna", "anna"))
        assertEquals(CallState.Connected(dm, "c-1", 4_000L, setOf("anna")), r.state)
        // Connected: the ring cap no longer applies.
        advanceTimeBy(200_000); runCurrent()
        assertTrue(r.state is CallState.Connected)
    }

    @Test
    fun `glare goes back to idle`() = runTest {
        val r = rig()
        r.api.startError = ApiException(HttpStatusCode.Conflict, "exists", code = CallApi.CALL_ALREADY_EXISTS)
        r.controller.startCall(dm)
        runCurrent()
        assertEquals(CallState.Idle, r.state)
        coVerify(exactly = 0) { r.voice.joinCall(any(), any(), any()) }
    }

    @Test
    fun `a blocked start says so`() = runTest {
        val r = rig()
        r.api.startError = ApiException(HttpStatusCode.Forbidden, "blocked", code = "BLOCKED")
        r.controller.startCall(dm)
        runCurrent()
        assertEquals(CallState.Ended(dm, CallEndReason.Blocked), r.state)
    }

    @Test
    fun `a resync after reconnecting turns a vanished ring into a missed call`() = runTest {
        val r = rig()
        r.ring()
        r.api.current = null
        r.server(ServerEvent.Ready("me"))
        assertEquals(listOf("get"), r.api.calls)
        assertEquals(CallState.Idle, r.state)
        assertEquals(listOf("ring:c-1:Anna", "stop:c-1", "missed:c-1"), r.notifier.log)
    }

    @Test
    fun `a resync that finds no call ends a connected call silently`() = runTest {
        val r = rig()
        r.ring()
        r.controller.accept(dm)
        runCurrent()
        r.api.current = null
        r.server(ServerEvent.Ready("me"))
        runCurrent()
        assertEquals(CallState.Idle, r.state)
        coVerify(exactly = 1) { r.voice.leaveChannel() }
        assertTrue("leave" !in r.api.calls)
    }

    @Test
    fun `a failed resync changes nothing`() = runTest {
        val r = rig()
        r.ring()
        r.api.getError = IllegalStateException("offline")
        r.server(ServerEvent.Ready("me"))
        assertTrue(r.state is CallState.Incoming)
    }

    @Test
    fun `no resync without a call`() = runTest {
        val r = rig()
        r.server(ServerEvent.Ready("me"))
        assertTrue(r.api.calls.isEmpty())
    }

    @Test
    fun `the rejoin gate allows only while the server has this user in the call`() = runTest {
        val r = rig()
        r.ring()
        r.controller.accept(dm)
        runCurrent()
        val gate = r.gate!!

        r.api.current = CallStateDto(dm, "c-1", "active", participants = listOf("anna", "me"))
        assertEquals(RejoinVerdict.Allowed, gate.mayRejoin(dm))
        r.api.current = CallStateDto(dm, "c-1", "active", participants = listOf("anna"))
        assertEquals(RejoinVerdict.Refused, gate.mayRejoin(dm))
        r.api.current = null
        assertEquals(RejoinVerdict.Refused, gate.mayRejoin(dm))
        r.api.getError = IllegalStateException("offline")
        assertEquals(RejoinVerdict.Unknown, gate.mayRejoin(dm))
    }

    @Test
    fun `voice_removed during a call ends it with the reason and does not leave it on the server`() = runTest {
        val r = rig()
        r.ring()
        r.controller.accept(dm)
        runCurrent()
        r.callEnds.tryEmit(CallVoiceEnd(dm, VoiceEnd.Removed(VoiceRemovedReason.SessionReplaced)))
        runCurrent()
        assertEquals(CallState.Ended(dm, CallEndReason.Removed, VoiceRemovedReason.SessionReplaced), r.state)
        // Taking the call on another device must not end it for everyone.
        assertEquals(listOf("join"), r.api.calls)
    }

    @Test
    fun `hang up leaves voice and the call`() = runTest {
        val r = rig()
        r.ring()
        r.controller.accept(dm)
        runCurrent()
        r.controller.hangUp()
        runCurrent()
        assertEquals(CallState.Ended(dm, CallEndReason.Left), r.state)
        assertEquals(listOf("join", "leave"), r.api.calls)
        coVerify(exactly = 1) { r.voice.leaveChannel() }
    }

    @Test
    fun `leave does not touch a guild voice channel the user moved to`() = runTest {
        val r = rig()
        r.ring()
        r.controller.accept(dm)
        runCurrent()
        r.voiceChannel.value = "guild-voice"
        r.server(ServerEvent.CallEnded(dm, "c-1", "last_left"))
        runCurrent()
        coVerify(exactly = 0) { r.voice.leaveChannel() }
    }

    @Test
    fun `decline from the notification of a ring this process does not show tells the server`() = runTest {
        val r = rig()
        r.controller.declineFromNotification(dm, "c-5")
        assertEquals(listOf("decline"), r.api.calls)
        assertEquals(listOf("stop:c-5"), r.notifier.log)
    }

    @Test
    fun `a ring known only from its notification becomes the shown call`() = runTest {
        val r = rig()
        r.controller.onRingNotification(dm, "c-1", "anna", "Anna")
        runCurrent()
        assertEquals(CallState.Incoming(dm, "c-1", "anna", "Anna", 0L), r.state)
        r.controller.decline(dm)
        runCurrent()
        assertEquals(listOf("decline"), r.api.calls)
    }

    @Test
    fun `sign-out ends the call and stops every ring at once`() = runTest {
        val r = rig()
        r.ring()
        r.controller.endForSignOut()
        // Synchronous part: the rings are gone before the account state is cleared.
        assertTrue("stopAll" in r.notifier.log)
        runCurrent()
        assertEquals(CallState.Idle, r.state)
    }

    @Test
    fun `a ring right after the session was restored rings`() = runTest {
        val r = rig()
        r.auth.setLoggedOut()
        runCurrent()
        // A screen opened from a notification restores the session and applies its ring at once.
        r.auth.setLoggedIn("me")
        r.controller.onRingNotification(dm, "c-1", "anna", "Anna")
        runCurrent()
        assertTrue(r.state is CallState.Incoming)
    }

    @Test
    fun `another signed-in user ends the call and does not ring for the old account`() = runTest {
        val r = rig()
        r.ring()
        r.auth.setLoggedIn("other")
        runCurrent()
        assertEquals(CallState.Idle, r.state)
        r.auth.setLoggedOut()
        runCurrent()
        r.ring("c-2")
        assertEquals(CallState.Idle, r.state)
        assertTrue("ring:c-2:Anna" !in r.notifier.log)
    }

    // -- The call owns its voice join (hang-up at every point of it) --------------------

    private enum class Point { LeavingPreviousChannel, SettingUp, Joined }

    private fun TestScope.hangUpDuringJoin(point: Point) {
        val r = rig()
        val leavingPrevious = kotlinx.coroutines.CompletableDeferred<Unit>()
        val settingUp = kotlinx.coroutines.CompletableDeferred<Unit>()
        r.voiceChannel.value = "guild-voice" // the user is in a guild voice channel
        coEvery { r.voice.joinCall(any(), any(), any()) } coAnswers {
            val ch = firstArg<String>()
            try {
                leavingPrevious.await() // like VoiceRepository leaving the guild channel first
                r.voiceChannel.value = ch // the call's session is open, the microphone captured
                settingUp.await()
            } catch (c: kotlinx.coroutines.CancellationException) {
                // Like VoiceRepository: a cancelled join ends its session.
                if (r.voiceChannel.value == ch) r.voiceChannel.value = null
                throw c
            }
        }
        r.ring()
        r.controller.accept(dm)
        runCurrent()
        when (point) {
            Point.LeavingPreviousChannel -> Unit
            Point.SettingUp -> { leavingPrevious.complete(Unit); runCurrent() }
            Point.Joined -> { leavingPrevious.complete(Unit); settingUp.complete(Unit); runCurrent() }
        }

        r.controller.hangUp()
        runCurrent()
        // Whatever the join would have done next.
        leavingPrevious.complete(Unit)
        settingUp.complete(Unit)
        runCurrent()

        assertEquals(CallState.Ended(dm, CallEndReason.Left), r.state)
        assertTrue("voice still in the call's room at $point", r.voiceChannel.value != dm)
    }

    @Test
    fun `hang up while the join is still leaving the previous voice channel`() = runTest {
        hangUpDuringJoin(Point.LeavingPreviousChannel)
    }

    @Test
    fun `hang up while the join is setting up`() = runTest {
        hangUpDuringJoin(Point.SettingUp)
    }

    @Test
    fun `hang up after the join was set up`() = runTest {
        hangUpDuringJoin(Point.Joined)
    }

    @Test
    fun `the caller ending the call while the join runs also ends the join`() = runTest {
        val r = rig()
        val settingUp = kotlinx.coroutines.CompletableDeferred<Unit>()
        coEvery { r.voice.joinCall(any(), any(), any()) } coAnswers {
            r.voiceChannel.value = firstArg()
            try { settingUp.await() } catch (c: kotlinx.coroutines.CancellationException) {
                r.voiceChannel.value = null
                throw c
            }
        }
        r.ring()
        r.controller.accept(dm)
        runCurrent()
        r.server(ServerEvent.CallEnded(dm, "c-1", "cancelled"))
        runCurrent()
        assertTrue(r.voiceChannel.value != dm)
    }

    @Test
    fun `a late start answer after hang up is left on the server`() = runTest {
        val r = rig()
        val answer = kotlinx.coroutines.CompletableDeferred<Unit>()
        coEvery { r.voice.joinCall(any(), any(), any()) } returns Unit
        val api = r.api
        r.controller.startCall(dm)
        r.controller.hangUp()
        runCurrent()
        // FakeApi answers at once; the start is processed after the hang-up.
        assertEquals(listOf("start", "leave"), api.calls)
        answer.complete(Unit)
    }

    @Test
    fun `a nil call id from an older server is no call id`() = runTest {
        val r = rig()
        r.server(ServerEvent.IncomingCall(dm, "00000000-0000-0000-0000-000000000000", "anna", "anna"))
        assertEquals("", (r.state as CallState.Incoming).callId)
        r.server(ServerEvent.CallParticipantJoined(dm, "00000000-0000-0000-0000-000000000000", "me", "me"))
        assertEquals(CallState.Idle, r.state)
    }

    @Test
    fun `a busy ring is missed after 100 s even without the server's end`() = runTest {
        val r = rig()
        r.ring()
        r.controller.accept(dm)
        runCurrent()
        r.server(ServerEvent.IncomingCall("dm-2", "c-9", "bob", "bob"))
        advanceTimeBy(100_000); runCurrent()
        assertTrue("missed:c-9" in r.notifier.log)
    }
}
