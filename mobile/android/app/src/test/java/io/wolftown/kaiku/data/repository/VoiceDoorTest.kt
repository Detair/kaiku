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

    /** Never two sessions at once: every join after the first follows the leave of the one before. */
    private fun VoiceRig.assertOneSessionAtATime() {
        var joined: String? = null
        for (frame in joinsAndLeaves()) {
            val channel = frame.substringAfter(':')
            if (frame.startsWith("join:")) {
                assertEquals("joined $channel while still in $joined: ${joinsAndLeaves()}", null, joined)
                joined = channel
            } else if (joined == channel) {
                joined = null
            }
        }
        assertEquals("the channel shown is not the one voice is in: ${joinsAndLeaves()}", joined, repository.currentChannelId.value)
        assertEquals(repository.currentChannelId.value, repository.current.value?.channelId)
        assertEquals("a voice_join reached the server while it still held our place: $log", 0, server.refusedJoins)
        // Connections of two sessions never exist together: a close lies between two set-ups.
        val installs = log.withIndex().filter { it.value == "sub" }.map { it.index }
        installs.zipWithNext().forEach { (a, b) ->
            assertTrue("two sessions set up without a teardown in between: $log", log.subList(a, b).contains("close"))
        }
    }

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
}
