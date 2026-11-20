package io.wolftown.kaiku.data.call

import io.ktor.http.HttpStatusCode
import io.mockk.coEvery
import io.mockk.coVerify
import io.mockk.verify
import io.mockk.every
import io.mockk.mockk
import io.wolftown.kaiku.data.api.ApiException
import io.wolftown.kaiku.data.api.CallApi
import io.wolftown.kaiku.data.api.CallStateDto
import io.wolftown.kaiku.data.session.FakeSessions
import io.wolftown.kaiku.data.session.SessionGeneration
import io.wolftown.kaiku.data.repository.VoiceRepository
import io.wolftown.kaiku.data.voice.RejoinGate
import io.wolftown.kaiku.data.voice.RejoinVerdict
import io.wolftown.kaiku.data.ws.KaikuWebSocket
import io.wolftown.kaiku.data.ws.ServerEvent
import io.wolftown.kaiku.data.ws.VoiceRemovedReason
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.async
import kotlinx.coroutines.asCoroutineDispatcher
import kotlinx.coroutines.cancel
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

    private fun TestScope.rig() = CallRig(this)

    @Test
    fun `an incoming call rings with the display name and accepting answers then joins voice`() = runTest {
        val r = rig()
        r.ring()
        assertEquals(CallState.Incoming(dm, "c-1", "anna", "Anna", 0L), r.state)
        assertEquals(listOf("ring:c-1:Anna"), r.notifier.log)

        r.accept()
        runCurrent()

        assertEquals(listOf("join"), r.api.requests)
        coVerify(exactly = 1) { r.voice.joinCall(dm, match<VoiceRepository.CallSession> { it.title == "Anna" }) }
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
        assertTrue(r.api.requests.isEmpty())
    }

    @Test
    fun `an incoming ring ends after 100 s as missed once the server says it is gone`() = runTest {
        val r = rig()
        r.ring()
        advanceTimeBy(99_999); runCurrent()
        assertTrue(r.state is CallState.Incoming)
        advanceTimeBy(1); runCurrent()
        assertEquals(CallState.Idle, r.state)
        assertEquals(listOf("ring:c-1:Anna", "quiet:c-1", "missed:c-1"), r.notifier.log)
        // The server was asked, and told nothing.
        assertEquals(listOf("get"), r.api.requests)
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
        r.startCall()
        runCurrent()
        assertEquals(listOf("start"), r.api.requests)
        assertEquals(CallState.Outgoing(dm, "c-1", 0L, voice = true), r.state)
        coVerify(exactly = 1) { r.voice.joinCall(dm, match<VoiceRepository.CallSession> { it.title == "Anna" }) }

        advanceTimeBy(100_000); runCurrent()

        assertEquals(CallState.Ended(dm, CallEndReason.NoAnswer), r.state)
        assertEquals(listOf("start", "leave"), r.api.requests)
        coVerify(exactly = 1) { r.voice.leaveCall(any()) }
        advanceTimeBy(CallMachine.ENDED_SHOWN_MS); runCurrent()
        assertEquals(CallState.Idle, r.state)
    }

    @Test
    fun `the other side answering connects an outgoing call`() = runTest {
        val r = rig()
        r.startCall()
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
        r.startCall()
        runCurrent()
        assertEquals(CallState.Idle, r.state)
        coVerify(exactly = 0) { r.voice.joinCall(any(), any<VoiceRepository.CallSession>()) }
    }

    @Test
    fun `a blocked start says so`() = runTest {
        val r = rig()
        r.api.startError = ApiException(HttpStatusCode.Forbidden, "blocked", code = "BLOCKED")
        r.startCall()
        runCurrent()
        assertEquals(CallState.Ended(dm, CallEndReason.Blocked), r.state)
    }

    @Test
    fun `a resync after reconnecting turns a vanished ring into a missed call`() = runTest {
        val r = rig()
        r.ring()
        r.api.current = null
        r.server(ServerEvent.Ready("me"))
        assertEquals(listOf("get"), r.api.requests)
        assertEquals(CallState.Idle, r.state)
        assertEquals(listOf("ring:c-1:Anna", "stop:c-1", "missed:c-1"), r.notifier.log)
    }

    @Test
    fun `a resync that finds no call ends a connected call silently`() = runTest {
        val r = rig()
        r.ring()
        r.accept()
        runCurrent()
        r.api.current = null
        r.server(ServerEvent.Ready("me"))
        runCurrent()
        assertEquals(CallState.Idle, r.state)
        coVerify(exactly = 1) { r.voice.leaveCall(any()) }
        assertTrue("leave" !in r.api.requests)
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
        assertTrue(r.api.requests.isEmpty())
    }

    @Test
    fun `the rejoin gate allows only while the server has this user in the call`() = runTest {
        val r = rig()
        r.ring()
        r.accept()
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
        r.accept()
        runCurrent()
        r.callEnds.tryEmit(CallVoiceEnd(r.tag, VoiceEnd.Removed(VoiceRemovedReason.SessionReplaced)))
        runCurrent()
        assertEquals(CallState.Ended(dm, CallEndReason.Removed, VoiceRemovedReason.SessionReplaced), r.state)
        // Taking the call on another device must not end it for everyone.
        assertEquals(listOf("join"), r.api.requests)
    }

    @Test
    fun `hang up leaves voice and the call`() = runTest {
        val r = rig()
        r.ring()
        r.accept()
        runCurrent()
        r.hangUp()
        runCurrent()
        assertEquals(CallState.Ended(dm, CallEndReason.Left), r.state)
        assertEquals(listOf("join", "leave"), r.api.requests)
        coVerify(exactly = 1) { r.voice.leaveCall(any()) }
    }

    @Test
    fun `leave does not touch a guild voice channel the user moved to`() = runTest {
        val r = rig()
        r.ring()
        r.accept()
        runCurrent()
        r.voiceChannel.value = "guild-voice"
        r.server(ServerEvent.CallEnded(dm, "c-1", "last_left"))
        runCurrent()
        // The leave names the call's own session; the voice layer leaves nothing else.
        coVerify(exactly = 0) { r.voice.leaveChannel() }
        assertEquals("guild-voice", r.voiceChannel.value)
    }

    @Test
    fun `decline from the notification of a ring this process does not show tells the server`() = runTest {
        val r = rig()
        // The server is asked first: it still rings that very call.
        r.serverRings("c-5")
        r.declineFromNotification("c-5")
        assertEquals(listOf("get", "decline"), r.api.requests)
        assertEquals(listOf("stop:c-5"), r.notifier.log)
    }

    @Test
    fun `decline from an old notification never declines the call the server has now`() = runTest {
        val r = rig()
        // The notification's call is over; the same person calls again and the server rings that.
        r.serverRings("c-6")
        r.declineFromNotification("c-5")
        assertEquals(listOf("get"), r.api.requests)
    }

    @Test
    fun `an action of another account's notification does nothing`() = runTest {
        val r = rig()
        r.serverRings("c-5")
        assertEquals(false, r.declineFromNotification("c-5", account = "other"))
        assertEquals(false, r.ringNotification("c-5", account = "other"))
        assertEquals(false, r.startCall(account = "other"))
        runCurrent()
        assertTrue(r.api.requests.isEmpty())
        assertEquals(CallState.Idle, r.state)
    }

    @Test
    fun `a ring known only from its notification becomes the shown call`() = runTest {
        val r = rig()
        r.serverRings()
        r.ringNotification()
        runCurrent()
        // Known only from its notification: shown, and checked with the server at once.
        assertEquals(listOf("get"), r.api.requests)
        assertEquals(CallState.Incoming(dm, "c-1", "anna", "Anna", 0L), r.state)
        r.decline()
        runCurrent()
        assertEquals(listOf("get", "decline"), r.api.requests)
    }

    @Test
    fun `a ring from a notification that the server no longer has is a missed call, not a ring`() = runTest {
        val r = rig()
        r.api.current = null
        r.ringNotification()
        runCurrent()
        assertEquals(CallState.Idle, r.state)
        assertTrue("missed:c-1" in r.notifier.log)
    }

    @Test
    fun `accept from a notification whose call is over joins nothing`() = runTest {
        val r = rig()
        // Accept straight from the notification: ring and accept arrive together, the check is still out.
        r.api.getGate = kotlinx.coroutines.CompletableDeferred()
        r.serverRings("c-2") // the server rings another call by now
        r.ringNotification("c-1")
        r.accept("c-1")
        r.api.getGate!!.complete(Unit)
        runCurrent()
        assertTrue("join" !in r.api.requests)
        assertEquals(CallState.Ended(dm, CallEndReason.Gone), r.state)
        coVerify(exactly = 0) { r.voice.joinCall(any(), any<VoiceRepository.CallSession>()) }
    }

    @Test
    fun `an answer the server gives for another call is left again and not joined`() = runTest {
        val r = rig()
        r.ring("c-1")
        r.api.joinAnswers = "c-2"
        r.accept()
        runCurrent()
        assertEquals(listOf("join", "leave"), r.api.requests)
        assertEquals(CallState.Ended(dm, CallEndReason.Gone), r.state)
        coVerify(exactly = 0) { r.voice.joinCall(any(), any<VoiceRepository.CallSession>()) }
    }

    @Test
    fun `accept for a ring that is no longer the one shown answers nothing`() = runTest {
        val r = rig()
        r.ring("c-1")
        val asked = r.shown() // the permission dialog is open for this ring
        r.server(ServerEvent.CallEnded(dm, "c-1", "cancelled"))
        r.ring("c-2")
        // The permission is granted now: the accept is for c-1, which is over.
        r.controller.submit(CallInput.Accept(asked))
        runCurrent()
        assertEquals("c-2", (r.state as CallState.Incoming).callId)
        assertTrue("join" !in r.api.requests)
    }

    @Test
    fun `sign-out ends the call and stops every ring at once`() = runTest {
        val r = rig()
        r.ring()
        r.signOut()
        // Synchronous: the call and the rings are gone before the account state is cleared.
        assertTrue("stopAll" in r.notifier.log)
        assertEquals(CallState.Idle, r.state)
        runCurrent()
        assertEquals(CallState.Idle, r.state)
    }

    @Test
    fun `a ring right after the session was restored rings`() = runTest {
        val r = rig()
        r.sessions.session = io.wolftown.kaiku.data.local.AuthSession.LoggedOut
        runCurrent()
        // A screen opened from a notification restores the session and applies its ring at once.
        r.sessions.signIn("me")
        r.serverRings()
        r.ringNotification()
        runCurrent()
        assertTrue(r.state is CallState.Incoming)
    }

    @Test
    fun `another signed-in user ends the call and does not ring for the old account`() = runTest {
        val r = rig()
        r.ring()
        r.signOut()
        r.sessions.signIn("other")
        runCurrent()
        assertEquals(CallState.Idle, r.state)
        // The old account's socket is still up for a moment: its ring is not the new account's.
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
        coEvery { r.voice.joinCall(any(), any<VoiceRepository.CallSession>()) } coAnswers {
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
        r.accept()
        runCurrent()
        when (point) {
            Point.LeavingPreviousChannel -> Unit
            Point.SettingUp -> { leavingPrevious.complete(Unit); runCurrent() }
            Point.Joined -> { leavingPrevious.complete(Unit); settingUp.complete(Unit); runCurrent() }
        }

        r.hangUp()
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
        coEvery { r.voice.joinCall(any(), any<VoiceRepository.CallSession>()) } coAnswers {
            r.voiceChannel.value = firstArg()
            try { settingUp.await() } catch (c: kotlinx.coroutines.CancellationException) {
                r.voiceChannel.value = null
                throw c
            }
        }
        r.ring()
        r.accept()
        runCurrent()
        r.server(ServerEvent.CallEnded(dm, "c-1", "cancelled"))
        runCurrent()
        assertTrue(r.voiceChannel.value != dm)
    }

    @Test
    fun `a late start answer after hang up is left on the server`() = runTest {
        val r = rig()
        val answer = kotlinx.coroutines.CompletableDeferred<Unit>()
        coEvery { r.voice.joinCall(any(), any<VoiceRepository.CallSession>()) } returns Unit
        val api = r.api
        r.startCall()
        r.hangUp()
        runCurrent()
        // FakeApi answers at once; the start is processed after the hang-up.
        assertEquals(listOf("start", "leave"), api.requests)
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
        r.accept()
        runCurrent()
        r.server(ServerEvent.IncomingCall("dm-2", "c-9", "bob", "bob"))
        advanceTimeBy(100_000); runCurrent()
        assertTrue("missed:c-9" in r.notifier.log)
    }

    // -- Order, identity and lanes ---------------------------------------------------

    @Test
    fun `a removal before the voice join was set up ends the call without telling the server`() = runTest {
        val r = rig()
        coEvery { r.voice.joinCall(any(), any<VoiceRepository.CallSession>()) } throws
            io.wolftown.kaiku.data.voice.VoiceRemovedException(VoiceRemovedReason.SessionReplaced)
        r.ring()
        r.accept()
        runCurrent()
        assertEquals(CallState.Ended(dm, CallEndReason.Removed, VoiceRemovedReason.SessionReplaced), r.state)
        assertEquals(listOf("join"), r.api.requests)
    }

    @Test
    fun `a conversation that is gone or left is a definite no call for the resync and the rejoin`() = runTest {
        val r = rig()
        r.ring()
        r.api.getError = ApiException(HttpStatusCode.Forbidden, "not a participant", code = "FORBIDDEN")
        r.server(ServerEvent.Ready("me"))
        runCurrent()
        assertEquals(CallState.Idle, r.state)
        assertTrue("missed:c-1" in r.notifier.log)

        r.ring("c-2")
        r.accept()
        runCurrent()
        r.api.getError = ApiException(HttpStatusCode.NotFound, "gone", code = "NOT_FOUND")
        assertEquals(RejoinVerdict.Refused, r.gate!!.mayRejoin(dm))
    }

    @Test
    fun `the rejoin check is for the state when it asked, not when the answer came`() = runTest {
        val r = rig()
        r.ring()
        r.accept()
        runCurrent()
        val gate = r.gate!!
        r.api.current = CallStateDto(dm, "c-1", "active", participants = listOf("anna", "me"))
        r.api.getGate = kotlinx.coroutines.CompletableDeferred()
        val verdict = backgroundScope.async { gate.mayRejoin(dm) }
        runCurrent()
        // Carl joins while the check is on its way; the older answer must not drop him.
        r.server(ServerEvent.CallParticipantJoined(dm, "c-1", "carl", "carl"))
        r.api.getGate!!.complete(Unit)
        assertEquals(RejoinVerdict.Allowed, verdict.await())
        runCurrent()
        assertEquals(setOf("anna", "carl"), (r.state as CallState.Connected).others)
    }

    @Test
    fun `decline from the notification of the shown ring returns only after the server was told`() = runTest {
        val r = rig()
        r.ring()
        r.api.declineGate = kotlinx.coroutines.CompletableDeferred()
        val done = backgroundScope.async { r.declineFromNotification("c-1") }
        runCurrent()
        assertTrue("returned before the request", !done.isCompleted)
        r.api.declineGate!!.complete(Unit)
        runCurrent()
        assertTrue(done.isCompleted)
        assertEquals(listOf("decline"), r.api.requests)
    }

    @Test
    fun `decline from a notification of another call than the shown one declines nothing`() = runTest {
        val r = rig()
        r.ring("c-2")
        r.declineFromNotification("c-1")
        runCurrent()
        // The shown ring is another call of that conversation: the old one is over, and the
        // server's decline would hit the call that rings now.
        assertEquals("c-2", (r.state as CallState.Incoming).callId)
        assertTrue(r.api.requests.isEmpty())
        assertTrue(r.notifier.log.none { it.startsWith("stop:") })
    }

    @Test
    fun `an ended call's voice leave never ends the next call's voice in the same conversation`() = runTest {
        val r = rig()
        val leaving = kotlinx.coroutines.CompletableDeferred<Unit>()
        coEvery { r.voice.leaveCall(any()) } coAnswers {
            leaving.await() // the first call's leave takes its time
            r.voiceChannel.value = null
        }
        r.ring("c-1")
        r.accept()
        runCurrent()
        r.hangUp()
        runCurrent()
        // A new call of the same conversation is answered at once.
        r.dismissEnded()
        r.ring("c-2")
        r.accept()
        runCurrent()
        leaving.complete(Unit)
        runCurrent()

        assertEquals(dm, r.voiceChannel.value)
        assertTrue(r.state is CallState.Connected)
    }

    @Test
    fun `a ring known from its notification is not posted again`() = runTest {
        val r = rig()
        r.serverRings()
        r.ringNotification()
        runCurrent()
        assertTrue(r.state is CallState.Incoming)
        assertTrue(r.notifier.log.none { it.startsWith("ring:") })
    }

    // -- The signed-in session -------------------------------------------------------

    @Test
    fun `a late start answer of the previous account does nothing once the next account is in`() = runTest {
        val r = rig()
        val first = r.sessions.sessionId
        r.api.startGate = kotlinx.coroutines.CompletableDeferred()
        r.startCall()
        runCurrent()
        // Hung up before the start was answered: its late answer would be left on the server again.
        r.hangUp()
        runCurrent()
        // Sign-out, then another account signs in on this phone.
        r.signOut()
        r.sessions.signIn("bert")
        r.socketOf("bert")
        // The old account's start is answered only now.
        r.api.startGate!!.complete(Unit)
        runCurrent()
        advanceTimeBy(10_000); runCurrent()

        assertEquals("a request of the old account went out after the sign-out", listOf("start"), r.api.requests)
        assertTrue(r.api.sessions.all { it == first })
        assertEquals(CallState.Idle, r.state)
        assertTrue(r.notifier.log.none { it.startsWith("OVER:") })
    }

    @Test
    fun `nothing of a call goes on after the sign-out - no request, no timer, no voice, no post`() = runTest {
        val r = rig()
        r.ring()
        r.server(ServerEvent.IncomingCall("dm-2", "c-7", "bob", "bob")) // a busy ring with its own 100 s
        r.accept()
        runCurrent()
        val posted = r.notifier.log.size
        val sent = r.api.requests.size

        r.signOut()
        r.sessions.signIn("bert")
        r.socketOf("bert")
        // Everything the old call still had coming: its events, its voice's end, its timers.
        r.server(ServerEvent.CallParticipantLeft(r.dm, "c-1", "anna"))
        r.callEnds.tryEmit(CallVoiceEnd(r.tag, VoiceEnd.Lost))
        advanceTimeBy(200_000); runCurrent()

        assertEquals(CallState.Idle, r.state)
        assertEquals("a request after the sign-out: ${r.api.requests}", sent, r.api.requests.size)
        assertEquals(listOf("stopAll"), r.notifier.log.drop(posted))
        assertEquals(0, r.controller.busyLanes())
    }

    /** Coroutines below [this] that are still running. */
    private fun kotlinx.coroutines.Job.running(): Int = children.sumOf { (if (it.isActive) 1 else 0) + it.running() }

    @Test
    fun `the sign-out cancels the call's request that is out, and nothing of its owner runs on`() = runTest {
        val r = rig()
        runCurrent()
        val before = backgroundScope.coroutineContext[kotlinx.coroutines.Job]!!.running()
        r.api.hold = true
        r.startCall()
        runCurrent()
        assertEquals(listOf("start"), r.api.requests)
        assertTrue("the call has work running", backgroundScope.coroutineContext[kotlinx.coroutines.Job]!!.running() > before)

        r.signOut()
        runCurrent()
        // The request's coroutine was cancelled by the sign-out itself, not dropped at its answer.
        assertEquals("the request that was out is not cancelled", listOf("start"), r.api.cancelled)
        assertEquals(
            "coroutines of the ended session's call still run",
            before, backgroundScope.coroutineContext[kotlinx.coroutines.Job]!!.running(),
        )
        // Its ring cap does not fire either.
        val posted = r.notifier.log.toList()
        advanceTimeBy(2 * CallMachine.RING_CAP_MS); runCurrent()
        assertEquals(posted, r.notifier.log)
        assertEquals(listOf("start"), r.api.requests)
    }

    @Test
    fun `a session ending on another thread while the call owner carries out a step stops the rest of it`() {
        // The owner on its own thread; the session-end hook on this one, in the middle of a step.
        val actor = java.util.concurrent.Executors.newSingleThreadExecutor()
        val scope = kotlinx.coroutines.CoroutineScope(kotlinx.coroutines.SupervisorJob() + actor.asCoroutineDispatcher())
        try {
            val sessions = FakeSessions("me")
            val api = FakeCallApi()
            val notifier = FakeCallNotifier()
            val inEffect = java.util.concurrent.CountDownLatch(1)
            val hookDone = java.util.concurrent.CountDownLatch(1)
            val voice: VoiceRepository = mockk(relaxed = true) {
                every { callEnds } returns MutableSharedFlow()
                every { currentChannelId } returns MutableStateFlow(null)
                // The microphone service, the first effect of the step: the session ends right here.
                every { holdForCall(any(), any(), any(), any(), any()) } answers {
                    inEffect.countDown()
                    hookDone.await(5, java.util.concurrent.TimeUnit.SECONDS)
                    true
                }
            }
            val controller = CallController(
                api = api,
                voice = voice,
                webSocket = mockk(relaxed = true) { every { events } returns MutableSharedFlow() },
                sessions = sessions.generation,
                notifier = notifier,
                directory = object : CallDirectory {
                    override fun nameOf(channelId: String, userId: String): String? = null
                    override fun titleOf(channelId: String): String? = null
                },
                clock = { 0L },
                voiceScope = scope,
                wallClock = { WALL },
            )
            controller.submit(CallInput.Start("me", dm))
            assertTrue("the owner never reached the step", inEffect.await(5, java.util.concurrent.TimeUnit.SECONDS))
            // The hook, as the token store runs it: the advance, then the calls' part.
            sessions.generation.advance()
            controller.endForSignOut()
            sessions.session = io.wolftown.kaiku.data.local.AuthSession.LoggedOut
            hookDone.countDown()
            // Let the owner's thread finish whatever it still does.
            kotlinx.coroutines.runBlocking(actor.asCoroutineDispatcher()) {}
            kotlinx.coroutines.runBlocking(actor.asCoroutineDispatcher()) {}

            assertEquals("a request of the ended session went out", emptyList<String>(), api.requests)
            assertEquals(CallState.Idle, controller.state.value)
            assertEquals(listOf("stopAll"), notifier.log)
        } finally {
            scope.cancel()
            actor.shutdownNow()
        }
    }

    @Test
    fun `a session that is being signed out takes no new call`() = runTest {
        val r = rig()
        // The sign-out has started (its teardown runs); the tokens are still stored.
        r.sessions.generation.close(r.sessions.sessionId)
        r.ring()
        assertEquals(false, r.startCall())
        runCurrent()
        assertEquals(CallState.Idle, r.state)
        assertTrue(r.api.requests.isEmpty())
        assertTrue(r.notifier.log.isEmpty())
    }

    @Test
    fun `the call's events are taken only from the socket of the signed-in account`() = runTest {
        val r = rig()
        // The socket on the line is still the previous account's.
        r.socketOf("someone-else")
        r.ring()
        assertEquals(CallState.Idle, r.state)
        assertTrue(r.notifier.log.isEmpty())
        r.socketOf("me")
        r.ring()
        assertTrue(r.state is CallState.Incoming)
    }

    // -- Request lanes ---------------------------------------------------------------

    private fun kotlinx.coroutines.Job.descendants(): Int = children.sumOf { 1 + it.descendants() }

    @Test
    fun `no request lane or worker stays behind once its requests are done`() = runTest {
        val r = rig()
        fun callAndHangUp(channel: String) {
            r.controller.submit(CallInput.Start("me", channel))
            runCurrent()
            r.controller.submit(CallInput.HangUp(CallRef("me", channel, "c-1")))
            runCurrent()
            // Past the ended line and every timer of that call.
            advanceTimeBy(CallMachine.RING_CAP_MS + 10_000); runCurrent()
        }
        val jobs = backgroundScope.coroutineContext[kotlinx.coroutines.Job]!!
        callAndHangUp("dm-0")
        val afterOne = jobs.descendants()
        for (i in 1..5) callAndHangUp("dm-$i")

        assertEquals("a coroutine stayed behind per conversation called", afterOne, jobs.descendants())
        assertEquals(0, r.controller.busyLanes())
    }

    // -- Rings that come by push ------------------------------------------------------

    @Test
    fun `a ring that comes by push while a call is on does not ring and is missed after its 100 s`() = runTest {
        val r = rig()
        r.ring()
        r.accept()
        runCurrent()
        assertTrue(r.state is CallState.Connected)

        r.pushRing("c-7", channel = "dm-2")
        assertTrue("rang over the call that is on: ${r.notifier.log}", r.notifier.log.none { it.startsWith("ring:c-7") })
        assertTrue(r.state is CallState.Connected)
        advanceTimeBy(CallMachine.RING_CAP_MS + CallController.CAP_CHECK_MS); runCurrent()
        assertTrue("missed:c-7" in r.notifier.log)
    }

    @Test
    fun `a ring by push only ends 100 s after its wake was received and is missed once the server says it is gone`() = runTest {
        val r = rig()
        // The wake was received 30 s ago; its work got here only now.
        r.pushRing("c-1", receivedAgoMs = 30_000)
        assertEquals("ring:c-1:Anna", r.notifier.log.single())
        advanceTimeBy(69_000); runCurrent()
        assertTrue(r.state is CallState.Incoming)
        advanceTimeBy(1_000 + CallController.CAP_CHECK_MS); runCurrent()
        assertEquals(CallState.Idle, r.state)
        assertEquals(listOf("ring:c-1:Anna", "quiet:c-1", "missed:c-1"), r.notifier.log)
    }

    @Test
    fun `a ring that comes by push and by socket rings once and ends once`() = runTest {
        val r = rig()
        r.pushRing("c-1")
        r.ring("c-1")
        assertEquals(listOf("ring:c-1:Anna"), r.notifier.log)
        r.pushEnded("c-1", "cancelled")
        r.server(ServerEvent.CallEnded(r.dm, "c-1", "cancelled"))
        assertEquals(listOf("ring:c-1:Anna", "stop:c-1", "missed:c-1"), r.notifier.log)
        assertEquals(CallState.Idle, r.state)
    }

    @Test
    fun `an ended wake for a ring this process never saw is a missed call`() = runTest {
        val r = rig()
        // It never rang here (offline during the ring): as before.
        r.pushEnded("c-4", "no_answer")
        assertEquals(listOf("stop:c-4", "missed:c-4"), r.notifier.log)
        // Declined by everyone else: nothing was missed.
        r.pushEnded("c-5", "all_declined")
        assertEquals(listOf("stop:c-4", "missed:c-4", "stop:c-5"), r.notifier.log)
    }

    @Test
    fun `a late ended wake of a group call whose ring is over here claims no missed call`() = runTest {
        val r = rig()
        // Rung here 120 s ago by a process that is gone. A group call: this user may have
        // declined it on another device, and the wake does not say.
        r.pushEnded("c-4", "no_answer", direct = false, ringReceivedAgoMs = 120_000)
        assertEquals(listOf("stop:c-4"), r.notifier.log)
        // The conversation's shape from the chat list where the wake did not find it: one-to-one.
        r.direct = true
        r.pushEnded("c-6", "cancelled", ringReceivedAgoMs = 120_000)
        assertEquals(listOf("stop:c-4", "stop:c-6", "missed:c-6"), r.notifier.log)
    }

    @Test
    fun `an ended wake of a group call that still rang here when this process went is a missed call as before`() = runTest {
        val r = rig()
        // The ring was received 30 s ago by a process that is gone; its end comes within its 100 s.
        r.pushEnded("c-4", "no_answer", direct = false, ringReceivedAgoMs = 30_000)
        assertEquals(listOf("stop:c-4", "missed:c-4"), r.notifier.log)
        // Received 120 s ago: its ring is over here, the end comes late and cannot tell.
        r.pushEnded("c-5", "no_answer", direct = false, ringReceivedAgoMs = 120_000)
        assertEquals(listOf("stop:c-4", "missed:c-4", "stop:c-5"), r.notifier.log)
    }

    @Test
    fun `a ring by push whose 100 s are over in a process without a restored session claims nothing until its end can tell`() = runTest {
        val r = CallRig(this, restored = false)
        r.pushRing("c-1")
        assertEquals(listOf("ring:c-1:Anna"), r.notifier.log)
        advanceTimeBy(CallMachine.RING_CAP_MS + CallController.CAP_CHECK_MS); runCurrent()
        assertEquals(CallState.Idle, r.state)
        // Nothing could be asked: no request goes out without a restored session.
        assertEquals(emptyList<String>(), r.api.requests)
        assertEquals(listOf("ring:c-1:Anna", "quiet:c-1"), r.notifier.log)
        // A group call's late end cannot tell: nothing.
        r.pushEnded("c-1", "no_answer", direct = false)
        assertEquals(listOf("ring:c-1:Anna", "quiet:c-1", "stop:c-1"), r.notifier.log)
    }

    @Test
    fun `a ring by push whose 100 s are over is missed when its one-to-one end comes`() = runTest {
        val r = CallRig(this, restored = false)
        r.pushRing("c-1")
        advanceTimeBy(CallMachine.RING_CAP_MS + CallController.CAP_CHECK_MS); runCurrent()
        r.pushEnded("c-1", "no_answer", direct = true)
        assertEquals(listOf("ring:c-1:Anna", "quiet:c-1", "missed:c-1"), r.notifier.log)
    }

    @Test
    fun `a push wake of another account than the signed-in one reaches nobody`() = runTest {
        val r = rig()
        assertEquals(false, r.pushRing("c-1", account = "someone-else"))
        assertEquals(CallState.Idle, r.state)
        assertTrue(r.notifier.log.isEmpty())
    }

    @Test
    fun `a ring from a notification counts its 100 s from when the ring was received`() = runTest {
        val r = rig()
        r.serverRings()
        r.ringNotification(receivedAgoMs = 90_000)
        runCurrent()
        assertTrue(r.state is CallState.Incoming)
        advanceTimeBy(10_000 + CallController.CAP_CHECK_MS); runCurrent()
        assertEquals(CallState.Idle, r.state)
        assertTrue("missed:c-1" in r.notifier.log)
    }

    // -- A missed call is only claimed on knowledge -------------------------------------

    @Test
    fun `a ring by push that this user answered on another device is no missed call when its 100 s are over`() = runTest {
        val r = rig()
        r.pushRing("c-1")
        // Answered on the desktop; the phone has no socket and hears nothing of it.
        r.api.current = CallStateDto(r.dm, "c-1", "active", startedBy = "anna", participants = listOf("anna", "me"))
        advanceTimeBy(CallMachine.RING_CAP_MS + CallController.CAP_CHECK_MS); runCurrent()
        assertEquals(CallState.Idle, r.state)
        assertTrue("a call that was answered shows as missed: ${r.notifier.log}", r.notifier.log.none { it.startsWith("missed") })
    }

    @Test
    fun `a ring by push that this user declined on another device is no missed call when its 100 s are over`() = runTest {
        val r = rig()
        r.pushRing("c-1")
        // A group call, declined on the desktop; the others let it ring.
        r.api.current = CallStateDto(r.dm, "c-1", "ringing", startedBy = "anna", declinedBy = listOf("me"))
        advanceTimeBy(CallMachine.RING_CAP_MS + CallController.CAP_CHECK_MS); runCurrent()
        assertEquals(CallState.Idle, r.state)
        assertTrue("a call that was declined shows as missed: ${r.notifier.log}", r.notifier.log.none { it.startsWith("missed") })
    }

    @Test
    fun `a ring whose 100 s are over while the server cannot be asked is not called missed`() = runTest {
        val r = rig()
        r.pushRing("c-1")
        r.api.getError = java.io.IOException("offline")
        advanceTimeBy(CallMachine.RING_CAP_MS + CallController.CAP_CHECK_MS); runCurrent()
        assertEquals(CallState.Idle, r.state)
        assertTrue("claimed without knowing: ${r.notifier.log}", r.notifier.log.none { it.startsWith("missed") })
    }

    // -- The ring notification going away ----------------------------------------------

    @Test
    fun `the shown ring's notification timing out ends the ring, missed once the server says it is gone`() = runTest {
        val r = rig()
        r.ring()
        r.ringExpired("c-1")
        runCurrent()
        assertEquals(CallState.Idle, r.state)
        assertEquals(listOf("ring:c-1:Anna", "quiet:c-1", "missed:c-1"), r.notifier.log)
    }

    @Test
    fun `a timed-out notification of a ring this process does not show is left to what is known about that call`() = runTest {
        val r = rig()
        r.ringExpired("c-3")
        runCurrent()
        assertEquals(listOf("missed-if-ringing:c-3"), r.notifier.log)
        // A notification of an older call while a newer one rings there changes nothing.
        r.ring("c-4")
        r.ringExpired("c-2")
        runCurrent()
        assertEquals("c-4", (r.state as CallState.Incoming).callId)
        assertEquals(listOf("missed-if-ringing:c-3", "ring:c-4:Anna"), r.notifier.log)
    }

    // -- The microphone service starts at the tap --------------------------------------

    @Test
    fun `accepting starts the microphone service before the answer goes to the server`() = runTest {
        val r = rig()
        r.ring()
        r.accept()
        runCurrent()
        assertEquals(listOf("hold", "join"), r.api.calls)
    }

    @Test
    fun `calling starts the microphone service before the call is started on the server`() = runTest {
        val r = rig()
        r.startCall()
        runCurrent()
        assertEquals(listOf("hold", "start"), r.api.calls)
    }

    @Test
    fun `a call that ends before its voice session opened gives the microphone service up`() = runTest {
        val r = rig()
        r.api.startError = ApiException(HttpStatusCode.Forbidden, "blocked", code = CallApi.BLOCKED)
        r.startCall()
        runCurrent()
        assertEquals(CallState.Ended(r.dm, CallEndReason.Blocked), r.state)
        // The leave of the call's own voice tag: the voice layer stops the service nobody took over.
        coVerify(exactly = 1) { r.voice.leaveCall(any()) }
        coVerify(exactly = 0) { r.voice.joinCall(any(), any<VoiceRepository.CallSession>()) }
    }

    @Test
    fun `a microphone service Android refuses ends the answer as failed and answers nothing on the server`() = runTest {
        val r = rig()
        r.holdGranted = false
        r.ring()
        r.accept()
        runCurrent()
        assertEquals(CallState.Ended(r.dm, CallEndReason.Failed), r.state)
        // Neither answered nor left nor declined: the call goes on ringing on the user's other devices.
        assertEquals(emptyList<String>(), r.api.requests)
        coVerify(exactly = 0) { r.voice.joinCall(any(), any<VoiceRepository.CallSession>()) }
        // It does not ring again here, and its end is no missed call (the user saw it and tried).
        r.ring()
        r.server(ServerEvent.CallEnded(r.dm, "c-1", "no_answer", null))
        assertTrue(r.notifier.log.none { it.startsWith("missed") })
        assertEquals(1, r.notifier.log.count { it.startsWith("ring:") })
    }

    @Test
    fun `a microphone service Android refuses starts no call on the server`() = runTest {
        val r = rig()
        r.holdGranted = false
        r.startCall()
        runCurrent()
        assertEquals(CallState.Ended(r.dm, CallEndReason.Failed), r.state)
        assertEquals(emptyList<String>(), r.api.requests)
        // Nothing is on its way that could still ring the other side.
        advanceTimeBy(CallMachine.RING_CAP_MS + 1_000)
        runCurrent()
        assertEquals(emptyList<String>(), r.api.requests)
        coVerify(exactly = 0) { r.voice.joinCall(any(), any<VoiceRepository.CallSession>()) }
    }

    @Test
    fun `the microphone service is asked for the signed-in session the call belongs to`() = runTest {
        val r = rig()
        val session = r.sessions.generation.enter()!!
        r.startCall()
        runCurrent()
        assertEquals(listOf(session), r.holdTickets)
    }

    @Test
    fun `mute and disconnect on the service's notification before the call's session is open act on the call`() = runTest {
        val r = rig()
        r.voiceChannel.value = "guild-voice" // still in a voice channel while the call starts
        r.api.hold = true // the start is still on its way to the server
        r.startCall()
        runCurrent()

        r.held!!.onMuteToggle()
        runCurrent()
        assertEquals(true, (r.state as CallState.Outgoing).muted)
        verify(exactly = 0) { r.voice.toggleMute() }

        r.held!!.onDisconnect()
        runCurrent()
        assertEquals(CallState.Ended(r.dm, CallEndReason.Left), r.state)
        coVerify(exactly = 0) { r.voice.leaveChannel() }
        // The start's answer comes: that call is left, once.
        r.api.releaseAll()
        runCurrent()
        assertEquals(listOf("start", "leave"), r.api.requests)
    }

    @Test
    fun `disconnect on the service's notification while an answer is on its way leaves the call once`() = runTest {
        val r = rig()
        r.api.hold = true
        r.ring()
        r.accept()
        runCurrent()
        r.held!!.onDisconnect()
        runCurrent()
        assertEquals(CallState.Ended(r.dm, CallEndReason.Left), r.state)
        r.api.releaseAll()
        runCurrent()
        assertEquals(listOf("join", "leave"), r.api.requests)
        coVerify(exactly = 0) { r.voice.joinCall(any(), any<VoiceRepository.CallSession>()) }
    }

    // -- The call's mute ------------------------------------------------------------------

    @Test
    fun `mute on the calling screen never touches the voice channel the user is still in`() = runTest {
        val r = rig()
        r.voiceChannel.value = "guild-voice" // still in a voice channel while the call starts
        r.api.startGate = kotlinx.coroutines.CompletableDeferred()
        r.startCall()
        runCurrent()
        r.setMuted(true)
        runCurrent()

        // Not the microphone that is on now (the channel's): only this call's session is named.
        verify(exactly = 0) { r.voice.toggleMute() }
        verify(exactly = 1) { r.voice.setCallMuted(any(), true) }
        assertEquals(true, (r.state as CallState.Outgoing).muted)

        // The call's session opens muted: the join does not reset the user's choice.
        r.api.startGate!!.complete(Unit)
        runCurrent()
        assertEquals(true, r.openedMuted)
    }

    @Test
    fun `a mute tapped right before the call's session opens is what it opens with`() = runTest {
        val r = rig()
        r.ring()
        r.accept()
        r.setMuted(true)
        runCurrent()
        assertEquals(true, r.openedMuted)
        assertEquals(true, (r.state as CallState.Connected).muted)
    }

    @Test
    fun `mute tapped on the call's notification goes through the call`() = runTest {
        val r = rig()
        r.ring()
        r.accept()
        runCurrent()
        // The voice layer hands a Mute on its own controls to the call.
        r.session!!.onMuteToggle!!.invoke()
        runCurrent()
        assertEquals(true, (r.state as CallState.Connected).muted)
        verify(exactly = 1) { r.voice.setCallMuted(r.tag, true) }
    }

    @Test
    fun `a microphone service that reports it could not start ends the answer as failed`() = runTest {
        val r = rig()
        r.api.hold = true // the answer is still on its way to the server
        r.ring()
        r.accept()
        runCurrent()
        // The service was started at the tap; now it reports that it could not go to the foreground.
        r.serviceFailed!!.invoke()
        runCurrent()
        assertEquals(CallState.Ended(r.dm, CallEndReason.Failed), r.state)
        r.api.releaseAll()
        runCurrent()
        assertEquals(listOf("join", "leave"), r.api.requests)
        coVerify(exactly = 0) { r.voice.joinCall(any(), any<VoiceRepository.CallSession>()) }
    }
}
