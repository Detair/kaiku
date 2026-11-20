package io.wolftown.kaiku.data.repository

import io.wolftown.kaiku.data.call.CallVoiceEnd
import io.wolftown.kaiku.data.call.VoiceEnd
import io.wolftown.kaiku.data.voice.RejoinVerdict
import io.wolftown.kaiku.data.ws.ClientEvent
import kotlinx.coroutines.CoroutineStart
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.launch
import kotlinx.coroutines.test.TestScope
import kotlinx.coroutines.test.runCurrent
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * The one door into voice: a session is replaced or torn down by one caller
 * at a time, a leave names the session it means, a teardown waits for the
 * session's owner, and what is said about a call's session carries its tag.
 */
@OptIn(ExperimentalCoroutinesApi::class)
class VoiceDoorTest {

    private val dm = "dm-1"

    private fun call(tag: Long) = VoiceRepository.CallSession({ RejoinVerdict.Allowed }, "Anna", tag)

    // -- Sign-out and the owner (the release waits for the owner) -------------------------

    @Test
    fun `a sign-out while the publisher is being set up closes nothing under the owner's hands`() = rigTest { rig ->
        rig.socket.ready()
        // Between "publisher installed" and "microphone track attached".
        rig.at("pub:installed") { rig.signOut() }
        rig.repository.startJoin(rig.channel)
        runCurrent()
        rig.awaitRelease()
        runCurrent()

        assertEquals("a call into a connection that was already closed: ${rig.native}", 0, rig.callsIntoClosed)
        assertTrue(
            "the connections were closed before the owner had finished with them: ${rig.native}",
            rig.native.indexOf("track@0") in 0 until rig.native.indexOf("close@0"),
        )
        rig.assertEndedAt("SIGNOUT", ownLeave = true)
        // What the owner still did after the sign-out (audio focus) was given up after it.
        assertTrue("audio focus kept after the sign-out: ${rig.log}", rig.log.lastIndexOf("audio:abandon") > rig.log.lastIndexOf("audio:focus"))
    }

    @Test
    fun `a join right after a sign-out waits until the old session is cleaned up`() = rigTest { rig ->
        rig.socket.ready()
        rig.at("pub:installed") {
            rig.signOut()
            // The next account is in and joins at once, while the old owner has not finished.
            rig.sessions.signIn("bert")
            rig.repository.startJoin("ch-2")
        }
        rig.repository.startJoin(rig.channel)
        runCurrent()
        rig.awaitRelease()
        runCurrent()

        assertEquals(0, rig.callsIntoClosed)
        val setUps = rig.log.withIndex().filter { it.value == "sub" }.map { it.index }
        assertEquals("both sessions were set up: ${rig.log}", 2, setUps.size)
        assertTrue(
            "the next session was set up before the old one was closed: ${rig.log}",
            rig.log.subList(setUps[0], setUps[1]).contains("close"),
        )
        assertEquals("ch-2", rig.repository.currentChannelId.value)
        assertEquals(listOf("join:ch-1", "leave:ch-1", "join:ch-2"), rig.joinsAndLeaves())
    }

    @Test
    fun `nothing is opened for a session that ended while its join waited`() = rigTest { rig ->
        rig.socket.ready()
        rig.joinAndConnectFirst()
        // A join of the old account is asked for, then the session ends before it got in.
        rig.at("close") { rig.signOut() }
        launch(start = CoroutineStart.UNDISPATCHED) { runCatching { rig.repository.joinChannel("ch-2") } }
        runCurrent()
        rig.awaitRelease()
        runCurrent()

        assertEquals(null, rig.repository.currentChannelId.value)
        assertFalse("a session of the ended account was joined: ${rig.joinsAndLeaves()}", "join:ch-2" in rig.joinsAndLeaves())
    }

    @Test
    fun `no voice session is opened while nobody is signed in or the session is being signed out`() = rigTest { rig ->
        rig.socket.ready()
        // The sign-out has started; the tokens are still stored.
        rig.sessions.generation.close(rig.sessions.sessionId)
        assertTrue(runCatching { rig.repository.joinChannel("ch-2") }.isFailure)
        rig.sessions.signOut()
        assertTrue(runCatching { rig.repository.joinChannel("ch-3") }.isFailure)
        runCurrent()

        assertEquals(null, rig.repository.currentChannelId.value)
        assertTrue("something was set up: ${rig.log}", rig.log.none { it == "sub" || it.startsWith("send:") || it == "service:start" })
    }

    // -- Voice channels and calls through the same door -----------------------------------

    @Test
    fun `a voice channel join arriving while a call's session opens never leaves two sessions`() = rigTest { rig ->
        rig.socket.ready()
        // The call's join is opening its session (the mute state is being applied) ...
        rig.at("mute") {
            // ... when the user taps a voice channel, as another thread would at this very point.
            launch(start = CoroutineStart.UNDISPATCHED) { runCatching { rig.repository.joinChannel("guild-1") } }
        }
        launch(start = CoroutineStart.UNDISPATCHED) { runCatching { rig.repository.joinCall(dm, call(1)) } }
        runCurrent()

        rig.assertOneSessionAtATime()
        // The later request wins: voice is in the channel the user tapped last.
        assertEquals("guild-1", rig.repository.currentChannelId.value)
    }

    @Test
    fun `a call's join arriving while a voice channel join opens never leaves two sessions`() = rigTest { rig ->
        rig.socket.ready()
        rig.at("mute") {
            launch(start = CoroutineStart.UNDISPATCHED) { runCatching { rig.repository.joinCall(dm, call(1)) } }
        }
        rig.repository.startJoin("guild-1")
        runCurrent()

        rig.assertOneSessionAtATime()
        assertEquals(dm, rig.repository.currentChannelId.value)
    }

    @Test
    fun `a join that replaces a session closes it completely before it sets up its own`() = rigTest { rig ->
        rig.socket.ready()
        rig.joinAndConnectFirst()
        launch(start = CoroutineStart.UNDISPATCHED) { runCatching { rig.repository.joinCall(dm, call(1)) } }
        runCurrent()

        rig.assertOneSessionAtATime()
        assertEquals(listOf("join:ch-1", "leave:ch-1", "join:$dm"), rig.joinsAndLeaves())
    }

    // -- A leave names its session ---------------------------------------------------------

    @Test
    fun `the leave of a call that is over does not touch the voice channel joined since`() = rigTest { rig ->
        rig.socket.ready()
        launch(start = CoroutineStart.UNDISPATCHED) { rig.repository.joinCall(dm, call(1)) }
        runCurrent()
        // The user moves to a voice channel; the call's leave comes after that.
        rig.repository.startJoin("guild-1")
        runCurrent()
        rig.repository.leaveCall(1)
        runCurrent()

        assertEquals("guild-1", rig.repository.currentChannelId.value)
        assertEquals(listOf("join:$dm", "leave:$dm", "join:guild-1"), rig.joinsAndLeaves())
    }

    @Test
    fun `the leave of an earlier call does not end the next call in the same conversation`() = rigTest { rig ->
        rig.socket.ready()
        launch(start = CoroutineStart.UNDISPATCHED) { rig.repository.joinCall(dm, call(1)) }
        runCurrent()
        launch(start = CoroutineStart.UNDISPATCHED) { rig.repository.joinCall(dm, call(2)) }
        runCurrent()
        // The first call's leave arrives late.
        rig.repository.leaveCall(1)
        runCurrent()

        assertEquals(dm, rig.repository.currentChannelId.value)
        assertEquals(2L, rig.repository.current.value?.callTag)
        assertEquals(listOf("join:$dm", "leave:$dm", "join:$dm"), rig.joinsAndLeaves())
    }

    @Test
    fun `a leave means the session that was current when it was asked for`() = rigTest { rig ->
        rig.socket.ready()
        rig.joinAndConnectFirst()
        // While the leave closes ch-1, a call's session is asked for.
        rig.at("close") {
            launch(start = CoroutineStart.UNDISPATCHED) { runCatching { rig.repository.joinCall(dm, call(1)) } }
        }
        rig.repository.leaveChannel()
        runCurrent()

        // The leave was for ch-1 only: the call's session is up.
        assertEquals(dm, rig.repository.currentChannelId.value)
        rig.assertOneSessionAtATime()
    }

    // -- What is said about a call's session carries its tag -------------------------------

    private fun TestScope.collectEnds(rig: VoiceRig): List<CallVoiceEnd> {
        val ends = mutableListOf<CallVoiceEnd>()
        backgroundScope.launch(start = CoroutineStart.UNDISPATCHED) { rig.repository.callEnds.collect { ends += it } }
        return ends
    }

    @Test
    fun `the end of a call's voice names that call, also when the next call is in the same conversation`() = rigTest { rig ->
        val ends = collectEnds(rig)
        rig.socket.ready()
        launch(start = CoroutineStart.UNDISPATCHED) { rig.repository.joinCall(dm, call(1)) }
        runCurrent()
        rig.repository.leaveCall(1)
        launch(start = CoroutineStart.UNDISPATCHED) { rig.repository.joinCall(dm, call(2)) }
        runCurrent()

        assertEquals(listOf(CallVoiceEnd(1, VoiceEnd.Left)), ends)
        assertEquals(2L, rig.repository.current.value?.callTag)
    }

    @Test
    fun `a call's session replaced by a voice channel reports its end with its tag`() = rigTest { rig ->
        val ends = collectEnds(rig)
        rig.socket.ready()
        launch(start = CoroutineStart.UNDISPATCHED) { rig.repository.joinCall(dm, call(7)) }
        runCurrent()
        rig.repository.startJoin("guild-1")
        runCurrent()

        assertEquals(listOf(CallVoiceEnd(7, VoiceEnd.Left)), ends)
        assertEquals(null, rig.repository.current.value?.callTag)
    }

    // -- The microphone service of a call starts when it is answered ------------------------

    private fun VoiceRig.serviceLog() = log.filter { it.startsWith("service:") }

    @Test
    fun `a call's microphone service starts at the tap and its session takes it over`() = rigTest { rig ->
        rig.socket.ready()
        assertTrue(rig.repository.holdForCall(1, dm, "Anna"))
        // Up before anything of the session exists.
        assertEquals(listOf("service:start"), rig.serviceLog())
        assertTrue(rig.log.none { it == "sub" })

        launch(start = CoroutineStart.UNDISPATCHED) { rig.repository.joinCall(dm, call(1)) }
        runCurrent()
        assertEquals("started again for its session", listOf("service:start"), rig.serviceLog())
        assertEquals(dm, rig.repository.currentChannelId.value)

        rig.repository.leaveCall(1)
        runCurrent()
        assertEquals(listOf("service:start", "service:stop"), rig.serviceLog())
    }

    @Test
    fun `a microphone service no session took over is stopped when its call ends`() = rigTest { rig ->
        assertTrue(rig.repository.holdForCall(1, dm, "Anna"))
        // The call failed before its voice session was asked for.
        rig.repository.leaveCall(1)
        runCurrent()
        assertEquals(listOf("service:start", "service:stop"), rig.serviceLog())
        rig.assertNoCallNotification()
    }

    @Test
    fun `answering a call in a voice channel keeps the microphone service up through the switch`() = rigTest { rig ->
        rig.socket.ready()
        rig.joinAndConnectFirst()
        assertTrue(rig.repository.holdForCall(1, dm, "Anna"))
        launch(start = CoroutineStart.UNDISPATCHED) { rig.repository.joinCall(dm, call(1)) }
        runCurrent()

        assertEquals(dm, rig.repository.currentChannelId.value)
        assertTrue("the service was stopped between the two sessions: ${rig.log}", rig.serviceLog().none { it == "service:stop" })
        rig.assertOneSessionAtATime()
    }

    @Test
    fun `a call that fails while a voice channel is still on leaves the microphone service to the channel`() = rigTest { rig ->
        rig.socket.ready()
        rig.joinAndConnectFirst()
        assertTrue(rig.repository.holdForCall(1, dm, "Anna"))
        rig.repository.leaveCall(1)
        runCurrent()

        assertEquals(rig.channel, rig.repository.currentChannelId.value)
        assertTrue(rig.serviceLog().none { it == "service:stop" })
        assertTrue(rig.service.notificationShown)
    }

    @Test
    fun `a sign-out while a call is being answered stops its microphone service`() = rigTest { rig ->
        assertTrue(rig.repository.holdForCall(1, dm, "Anna"))
        rig.signOut()
        // Stopped on the release thread.
        val deadline = System.currentTimeMillis() + 5_000
        while ("service:stop" !in rig.log && System.currentTimeMillis() < deadline) Thread.sleep(5)
        assertEquals(listOf("service:start", "service:stop"), rig.serviceLog())
    }

    @Test
    fun `a held microphone service that reports it could not start tells the call, once`() = rigTest { rig ->
        var failed = 0
        assertTrue(rig.repository.holdForCall(1, dm, "Anna") { failed++ })
        // The service could not go to the foreground and stopped itself.
        rig.serviceEvents.emit(io.wolftown.kaiku.data.voice.VoiceServiceEvent.StartFailed)
        runCurrent()
        rig.serviceEvents.emit(io.wolftown.kaiku.data.voice.VoiceServiceEvent.StartFailed)
        runCurrent()
        assertEquals(1, failed)
        // Nothing is held any more: the call's leave has nothing to stop.
        rig.repository.leaveCall(1)
        runCurrent()
        assertEquals(listOf("service:start"), rig.serviceLog())
    }

    @Test
    fun `once a call's session has taken the service over, the session handles what the service reports`() = rigTest { rig ->
        rig.socket.ready()
        var failed = 0
        assertTrue(rig.repository.holdForCall(1, dm, "Anna") { failed++ })
        launch(start = CoroutineStart.UNDISPATCHED) { rig.repository.joinCall(dm, call(1)) }
        runCurrent()
        rig.serviceEvents.emit(io.wolftown.kaiku.data.voice.VoiceServiceEvent.StartFailed)
        runCurrent()
        assertEquals("the hold is over: the session ends itself", 0, failed)
        assertEquals(null, rig.repository.currentChannelId.value)
    }

    // -- A call's mute belongs to the call ----------------------------------------------------

    @Test
    fun `muting a call that has no session yet leaves the voice channel's microphone alone`() = rigTest { rig ->
        rig.socket.ready()
        rig.joinAndConnectFirst()
        rig.repository.setCallMuted(1, true)
        assertEquals(false, rig.repository.isMuted.value)
        assertTrue(rig.socket.frames.none { it.second is ClientEvent.VoiceMute })
    }

    @Test
    fun `a call's session opens with the call's mute and tells the server`() = rigTest { rig ->
        rig.socket.ready()
        var muted = true
        launch(start = CoroutineStart.UNDISPATCHED) {
            rig.repository.joinCall(dm, VoiceRepository.CallSession({ RejoinVerdict.Allowed }, "Anna", 1, muted = { muted }))
        }
        runCurrent()
        assertEquals(true, rig.repository.isMuted.value)
        // The mute follows the voice_join on the same socket.
        val frames = rig.socket.frames.map { it.second }
        assertTrue(frames.indexOfFirst { it is ClientEvent.VoiceMute } > frames.indexOfFirst { it is ClientEvent.VoiceJoin })

        muted = false
        rig.repository.setCallMuted(1, false)
        assertEquals(false, rig.repository.isMuted.value)
        assertTrue(rig.socket.frames.last().second is ClientEvent.VoiceUnmute)
        // Another call's mute does not reach this session.
        rig.repository.setCallMuted(2, true)
        assertEquals(false, rig.repository.isMuted.value)
    }

    @Test
    fun `mute on a call's notification asks the call instead of switching the microphone`() = rigTest { rig ->
        rig.socket.ready()
        var asked = 0
        launch(start = CoroutineStart.UNDISPATCHED) {
            rig.repository.joinCall(dm, VoiceRepository.CallSession({ RejoinVerdict.Allowed }, "Anna", 1, onMuteToggle = { asked++ }))
        }
        runCurrent()
        rig.repository.toggleMute()
        assertEquals(1, asked)
        assertEquals("the call decides, not the control", false, rig.repository.isMuted.value)
    }

    // -- Any two requests at any point: one session at a time ----------------------------------

    @Test
    fun `whatever second request arrives at whatever point of a join, there is one voice session at a time`() = kotlinx.coroutines.test.runTest {
        val points = listOf("mute", "sub:create", "join:check", "join:sent", "pub:create", "pub:installed")
        val firsts = listOf<Pair<String, suspend (VoiceRig) -> Unit>>(
            "call" to { it.repository.joinCall(dm, call(1)) },
            "channel" to { it.repository.joinChannel("guild-1") },
        )
        val seconds = listOf<Pair<String, suspend (VoiceRig) -> Unit>>(
            "join channel" to { it.repository.joinChannel("guild-2") },
            "join call" to { it.repository.joinCall("dm-2", call(9)) },
            "leave" to { it.repository.leaveChannel() },
            "leave call" to { it.repository.leaveCall(1) },
        )
        var runs = 0
        for ((firstName, first) in firsts) for (point in points) for ((secondName, second) in seconds) {
            val rig = VoiceRig(this)
            try {
                rig.socket.ready()
                // The second request comes in the middle of the first, as another thread would.
                rig.at(point) { launch(start = CoroutineStart.UNDISPATCHED) { runCatching { second(rig) } } }
                launch(start = CoroutineStart.UNDISPATCHED) { runCatching { first(rig) } }
                runCurrent()
                try {
                    rig.assertOneSessionAtATime()
                } catch (e: AssertionError) {
                    throw AssertionError("$firstName, then $secondName at $point: ${e.message}", e)
                }
                // And it can all be left again: nothing stays behind.
                rig.repository.leaveChannel()
                runCurrent()
                assertEquals("$firstName, then $secondName at $point: ${rig.log}", null, rig.repository.currentChannelId.value)
                assertEquals("$firstName, then $secondName at $point: ${rig.log}", null, rig.server.peer)
                rig.assertNoCallNotification()
                runs++
            } finally {
                rig.close()
            }
        }
        assertEquals(48, runs)
    }
}
