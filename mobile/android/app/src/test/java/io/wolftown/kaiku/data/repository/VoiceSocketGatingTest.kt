package io.wolftown.kaiku.data.repository

import io.wolftown.kaiku.data.voice.IceTrouble
import io.wolftown.kaiku.data.voice.VoiceEndNotice
import io.wolftown.kaiku.data.voice.VoiceLinkState
import io.wolftown.kaiku.data.ws.ClientEvent
import io.wolftown.kaiku.data.ws.ServerEvent
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.test.advanceTimeBy
import kotlinx.coroutines.test.runCurrent
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * `voice_join` and the authenticated socket (contract C32 with its
 * amendments): the join goes out once per authenticated socket that needs it,
 * whatever the socket does in between, and a call that is up never gets a
 * second one.
 *
 * The sockets come and go at exact points of the repository's work ([VoiceRig]),
 * including faster than anything can observe them: a socket that is lost and
 * replaced between two looks at the socket state.
 */
@OptIn(ExperimentalCoroutinesApi::class)
class VoiceSocketGatingTest {

    private fun VoiceRig.voiceFrames() = socket.frames.map { it.second::class.simpleName }

    private fun VoiceRig.assertGaveUpWithoutAnyFrame() {
        assertEquals("voice frames: $log", emptyList<String?>(), voiceFrames())
        assertEquals(null, repository.currentChannelId.value)
        assertEquals(VoiceEndNotice.RejoinFailed(channel, wasMuted = false), repository.endNotice.value)
        assertEquals(VoiceLinkState.Stable, repository.linkState.value)
        assertTrue("call service not stopped: $log", "service:stop" in log)
        assertNoCallNotification()
    }

    // -- 1. A socket that is replaced between the state check and the send -----

    @Test
    fun `a socket lost at the join send and replaced before the retry gets the join once`() = rigTest { rig ->
        rig.socket.ready() // socket 1
        // Socket 1 drops between the state check and the send ...
        rig.at("join:check") { rig.socket.drop() }
        // ... and socket 2 is authenticated before the retry path looks at anything.
        rig.at("join:refused") { rig.socket.ready() }

        rig.repository.startJoin(rig.channel)
        runCurrent()

        // Sent once, on socket 2, and the call connects.
        rig.assertCallUp(joins = listOf(2L))
        advanceTimeBy(180_000); runCurrent()
        // No recovery on top: nothing closed, nothing joined again.
        rig.assertCallUp(joins = listOf(2L))
        assertEquals(0, rig.closes())
    }

    @Test
    fun `a socket lost at a rejoin's join send does not use up the attempt`() = rigTest { rig ->
        rig.joinAndConnect() // socket 1
        rig.socket.drop(); runCurrent()
        rig.socket.ready(); runCurrent() // socket 2
        assertEquals(VoiceLinkState.Rejoining(1), rig.repository.linkState.value)
        // The attempt's voice_join meets the same race: socket 2 is lost at the
        // send, socket 3 is there before the attempt looks again.
        rig.at("join:check", nth = 2) { rig.socket.drop() }
        rig.at("join:refused") { rig.socket.ready() }

        advanceTimeBy(1_000); runCurrent()
        assertEquals(listOf(1L), rig.joins())
        // Socket loss: attempt 1 is not used up, and it runs again 1 s after socket 3.
        assertEquals(VoiceLinkState.Rejoining(1), rig.repository.linkState.value)
        advanceTimeBy(999); runCurrent()
        assertEquals(listOf(1L), rig.joins())
        advanceTimeBy(1); runCurrent()

        rig.assertCallUp(joins = listOf(1L, 3L))
        advanceTimeBy(180_000); runCurrent()
        rig.assertCallUp(joins = listOf(1L, 3L))
    }

    @Test
    fun `a socket replaced while a rejoin sets up its connections gets exactly one join`() = rigTest { rig ->
        rig.joinAndConnect() // socket 1
        rig.socket.drop(); runCurrent()
        rig.socket.ready(); runCurrent() // socket 2
        // The attempt for socket 2 is creating its subscriber when socket 2 is
        // replaced by socket 3; the attempt sends its voice_join without
        // having looked at the socket again.
        rig.at("sub:create", nth = 2) { rig.socket.drop(); rig.socket.ready() }

        advanceTimeBy(1_000); runCurrent()
        advanceTimeBy(1_000); runCurrent()

        // Whatever went out is on socket 3 and known to be there: the call is
        // up, and nothing rejoins a working session on that socket again.
        rig.assertCallUp(joins = listOf(1L, 3L))
        advanceTimeBy(180_000); runCurrent()
        rig.assertCallUp(joins = listOf(1L, 3L))
    }

    // -- 2. Rapid flapping while the first join is held ------------------------

    @Test
    fun `a held join outlives five sockets that die at the send and goes out once on the sixth`() = rigTest { rig ->
        rig.repository.startJoin(rig.channel)
        runCurrent() // held: there is no socket
        for (n in 1..5) rig.at("join:check", nth = n) { rig.socket.drop() }

        // Five sockets within 2 s, each lost at the very moment the join would go out.
        repeat(5) {
            rig.socket.ready(); runCurrent()
            advanceTimeBy(400); runCurrent()
        }
        assertEquals(emptyList<String?>(), rig.voiceFrames())
        assertEquals(rig.channel, rig.repository.currentChannelId.value)

        rig.socket.ready(); runCurrent() // socket 6 stays
        rig.assertCallUp(joins = listOf(6L))
        advanceTimeBy(180_000); runCurrent()
        rig.assertCallUp(joins = listOf(6L))
        assertEquals(0, rig.closes())
    }

    @Test
    fun `five flaps between two looks at the socket leave one join on the socket that stays`() = rigTest { rig ->
        rig.socket.ready() // socket 1
        rig.at("join:check") { rig.socket.drop() }
        // Sockets 2 to 5 come and go and socket 6 stays, all before the retry looks.
        rig.at("join:refused") {
            repeat(4) { rig.socket.ready(); rig.socket.drop() }
            rig.socket.ready()
        }

        rig.repository.startJoin(rig.channel)
        runCurrent()

        rig.assertCallUp(joins = listOf(6L))
        advanceTimeBy(180_000); runCurrent()
        rig.assertCallUp(joins = listOf(6L))
        assertEquals(0, rig.closes())
    }

    @Test
    fun `flaps that end without a socket give up a held join at the 60 s cap`() = rigTest { rig ->
        rig.socket.ready() // socket 1
        rig.at("join:check") { rig.socket.drop() }
        // Sockets 2 to 5 come and go before the retry looks; none stays.
        rig.at("join:refused") { repeat(4) { rig.socket.ready(); rig.socket.drop() } }

        rig.repository.startJoin(rig.channel)
        runCurrent()
        advanceTimeBy(59_999); runCurrent()
        assertEquals(rig.channel, rig.repository.currentChannelId.value)
        assertEquals(VoiceLinkState.Interrupted, rig.repository.linkState.value)
        assertEquals(emptyList<String?>(), rig.voiceFrames())

        advanceTimeBy(1); runCurrent()
        rig.assertGaveUpWithoutAnyFrame()
    }

    // -- 2. Rapid flapping while a rejoin attempt is in flight -----------------

    @Test
    fun `flapping during a rejoin joins once on the socket that stays long enough`() = rigTest { rig ->
        rig.joinAndConnect() // socket 1
        rig.socket.drop(); runCurrent()
        rig.socket.ready(); runCurrent() // socket 2
        // The attempt's voice_join goes out on socket 2, then the socket is lost:
        // the attempt is in flight (joined, publisher not connected).
        rig.at("join:sent", nth = 2) { rig.socket.drop() }
        advanceTimeBy(1_000); runCurrent()
        assertEquals(listOf(1L, 2L), rig.joins())

        // Sockets 3 to 6: each is gone again before the 1 s the rejoin waits.
        repeat(4) {
            rig.socket.ready(); runCurrent()
            advanceTimeBy(300); runCurrent()
            rig.socket.drop(); runCurrent()
            advanceTimeBy(100); runCurrent()
        }
        assertEquals(listOf(1L, 2L), rig.joins())
        // None of it used up an attempt.
        assertEquals(VoiceLinkState.Rejoining(1), rig.repository.linkState.value)

        rig.socket.ready(); runCurrent() // socket 7 stays
        advanceTimeBy(999); runCurrent()
        assertEquals(listOf(1L, 2L), rig.joins())
        advanceTimeBy(1); runCurrent()

        rig.assertCallUp(joins = listOf(1L, 2L, 7L))
        advanceTimeBy(180_000); runCurrent()
        rig.assertCallUp(joins = listOf(1L, 2L, 7L))
    }

    @Test
    fun `five flaps nobody could observe during a rejoin still cancel it without using it up`() = rigTest { rig ->
        rig.joinAndConnect() // socket 1
        rig.socket.drop(); runCurrent()
        rig.socket.ready(); runCurrent() // socket 2
        // Right after the attempt's voice_join left on socket 2: five flaps in
        // one go; socket 7 is up when anything looks at the socket again.
        rig.at("join:sent", nth = 2) { repeat(5) { rig.socket.drop(); rig.socket.ready() } }

        advanceTimeBy(1_000); runCurrent()
        assertEquals(listOf(1L, 2L), rig.joins())
        // Socket 2 is gone: its attempt is cancelled and not used up ...
        assertEquals(VoiceLinkState.Rejoining(1), rig.repository.linkState.value)
        advanceTimeBy(999); runCurrent()
        assertEquals(listOf(1L, 2L), rig.joins())
        // ... and runs again 1 s later, on socket 7.
        advanceTimeBy(1); runCurrent()

        rig.assertCallUp(joins = listOf(1L, 2L, 7L))
        advanceTimeBy(180_000); runCurrent()
        rig.assertCallUp(joins = listOf(1L, 2L, 7L))
    }

    // -- 3. The 60 s without a socket that carries the join ---------------------
    //
    // The 60 s run from the loss of the socket that carried the session's
    // voice_join (for a first join: from its start) and end only when a
    // voice_join went out on an authenticated socket. A socket that merely
    // authenticates changes nothing.

    @Test
    fun `sockets that never carry the join end the call 60 s after the join's socket was lost`() = rigTest { rig ->
        rig.joinAndConnect() // socket 1
        rig.socket.drop(); runCurrent() // the join's socket is lost: the 60 s start
        // A new socket every 1.5 s, each up for half a second: never for the
        // 1 s the rejoin waits, so none of them ever carries a voice_join.
        repeat(39) {
            advanceTimeBy(1_000); runCurrent()
            rig.socket.ready(); runCurrent()
            advanceTimeBy(500); runCurrent()
            rig.socket.drop(); runCurrent()
        }
        advanceTimeBy(1_499); runCurrent() // 59 999 ms after the loss
        assertEquals(rig.channel, rig.repository.currentChannelId.value)
        assertEquals(VoiceLinkState.Rejoining(1), rig.repository.linkState.value)
        assertTrue("call service stopped too early: ${rig.log}", "service:stop" !in rig.log)

        advanceTimeBy(1); runCurrent()

        // Over: connections closed (microphone released), service stopped, Rejoin offered.
        rig.assertGaveUp()
        // Nothing was sent after the first join: no voice_join on a later
        // socket, and no voice_leave for a peer that went with socket 1.
        assertEquals(listOf<String?>("VoiceJoin"), rig.voiceFrames())
        rig.socket.ready()
        advanceTimeBy(180_000); runCurrent()
        assertEquals(listOf<String?>("VoiceJoin"), rig.voiceFrames())
    }

    @Test
    fun `a join carried at 59 s ends the 60 s and a socket that only authenticates at 59 s 500 does not`() = rigTest { rig ->
        rig.joinAndConnect() // socket 1
        rig.mediaConnects = false
        rig.socket.drop(); runCurrent() // lost: the 60 s start
        advanceTimeBy(58_000); runCurrent()
        rig.socket.ready(); runCurrent() // socket 2 after 58 s
        advanceTimeBy(1_000); runCurrent() // its voice_join goes out after 59 s
        assertEquals(listOf(1L, 2L), rig.joins())

        // Past the 60 s: the attempt goes on; its own 15 s decide now.
        advanceTimeBy(5_000); runCurrent()
        assertEquals(rig.channel, rig.repository.currentChannelId.value)
        assertEquals(VoiceLinkState.Rejoining(1), rig.repository.linkState.value)
        rig.connectPublisher(); runCurrent()
        rig.assertCallUp(joins = listOf(1L, 2L))

        // The same loss again. This time the next socket is authenticated after
        // 59.5 s; its voice_join would be due 1 s later, after the 60 s.
        rig.socket.drop(); runCurrent()
        advanceTimeBy(59_500); runCurrent()
        rig.socket.ready(); runCurrent() // socket 3
        advanceTimeBy(499); runCurrent()
        assertEquals(rig.channel, rig.repository.currentChannelId.value)

        advanceTimeBy(1); runCurrent()

        rig.assertGaveUp()
        assertEquals(listOf(1L, 2L), rig.joins())
        advanceTimeBy(180_000); runCurrent()
        assertEquals(listOf(1L, 2L), rig.joins())
    }

    @Test
    fun `a carried join whose socket is lost again starts new 60 s from that loss`() = rigTest { rig ->
        rig.joinAndConnect() // socket 1
        rig.mediaConnects = false
        rig.socket.drop(); runCurrent() // first loss, at 0 s
        advanceTimeBy(9_000); runCurrent()
        rig.socket.ready(); runCurrent() // socket 2
        advanceTimeBy(1_000); runCurrent() // its voice_join goes out at 10 s: the first 60 s are over
        assertEquals(listOf(1L, 2L), rig.joins())
        advanceTimeBy(10_000); runCurrent()
        rig.socket.drop(); runCurrent() // the carrying socket is lost at 20 s: new 60 s, until 80 s

        // From here on only sockets that never carry a join.
        repeat(39) {
            advanceTimeBy(1_000); runCurrent()
            rig.socket.ready(); runCurrent()
            advanceTimeBy(500); runCurrent()
            rig.socket.drop(); runCurrent()
        }
        advanceTimeBy(1_499); runCurrent() // 79 999 ms: well past 60 s after the first loss
        assertEquals(rig.channel, rig.repository.currentChannelId.value)
        assertEquals(VoiceLinkState.Rejoining(1), rig.repository.linkState.value)

        advanceTimeBy(1); runCurrent() // 80 s

        rig.assertGaveUp()
        assertEquals(listOf(1L, 2L), rig.joins())
    }

    @Test
    fun `a held first join that no socket ever carries gives up 60 s after it started`() = rigTest { rig ->
        rig.repository.startJoin(rig.channel)
        runCurrent() // held: there is no socket
        // A new socket every 1.5 s, each lost at the very moment the join would go out.
        for (n in 1..39) rig.at("join:check", nth = n) { rig.socket.drop() }
        repeat(39) {
            advanceTimeBy(1_500); runCurrent()
            rig.socket.ready(); runCurrent()
        }
        advanceTimeBy(1_499); runCurrent() // 59 999 ms after the start
        assertEquals(rig.channel, rig.repository.currentChannelId.value)
        assertEquals(emptyList<String?>(), rig.voiceFrames())

        advanceTimeBy(1); runCurrent()

        rig.assertGaveUpWithoutAnyFrame()
    }

    @Test
    fun `a first join still setting up gives up after 60 s whatever sockets come and go`() = rigTest { rig ->
        // The set-up before the join does not finish (fetching the ICE servers hangs).
        rig.hangSubscriberSetup()
        rig.repository.startJoin(rig.channel)
        runCurrent()
        repeat(39) {
            advanceTimeBy(1_000); runCurrent()
            rig.socket.ready(); runCurrent()
            advanceTimeBy(500); runCurrent()
            rig.socket.drop(); runCurrent()
        }
        advanceTimeBy(1_499); runCurrent()
        assertEquals(rig.channel, rig.repository.currentChannelId.value)

        advanceTimeBy(1); runCurrent()

        rig.assertGaveUpWithoutAnyFrame()
    }


    @Test
    fun `a join held for 60 s without a socket gives up without any voice frame`() = rigTest { rig ->
        rig.socket.connecting()
        rig.repository.startJoin(rig.channel)
        runCurrent()

        advanceTimeBy(59_999); runCurrent()
        assertEquals(rig.channel, rig.repository.currentChannelId.value)
        assertEquals(VoiceLinkState.Interrupted, rig.repository.linkState.value)
        advanceTimeBy(1); runCurrent()
        rig.assertGaveUpWithoutAnyFrame()

        // A socket that comes later pulls nobody back in.
        rig.socket.ready()
        advanceTimeBy(180_000); runCurrent()
        rig.assertGaveUpWithoutAnyFrame()
    }

    @Test
    fun `a join held for 59 s goes out on the first authenticated socket and nothing gives up`() = rigTest { rig ->
        rig.socket.connecting()
        rig.repository.startJoin(rig.channel)
        runCurrent()
        advanceTimeBy(59_000); runCurrent()
        assertEquals(emptyList<String?>(), rig.voiceFrames())

        rig.socket.ready(); runCurrent()

        rig.assertCallUp(joins = listOf(1L))
        advanceTimeBy(180_000); runCurrent()
        rig.assertCallUp(joins = listOf(1L))
        assertEquals(0, rig.closes())
    }

    @Test
    fun `the 60 s cap also ends a join that is still setting up`() = rigTest { rig ->
        rig.socket.connecting()
        // The set-up before the join does not finish (fetching the ICE servers hangs).
        rig.hangSubscriberSetup()
        rig.repository.startJoin(rig.channel)
        runCurrent()

        advanceTimeBy(59_999); runCurrent()
        assertEquals(rig.channel, rig.repository.currentChannelId.value)
        advanceTimeBy(1); runCurrent()
        rig.assertGaveUpWithoutAnyFrame()
    }

    // -- 4. A full channel: a call that is up never gets a second voice_join ----

    @Test
    fun `a call that is up never gets a second voice_join`() = rigTest { rig ->
        rig.joinAndConnect() // socket 1

        // None of this is a socket loss: the Ready of the socket in use
        // (delivered late, or twice), ICE trouble that recovers, mute toggles.
        repeat(3) {
            rig.events.tryEmit(ServerEvent.Ready(userId = "me")); runCurrent()
            advanceTimeBy(20_000); runCurrent()
        }
        rig.iceTrouble(IceTrouble.Disconnected); runCurrent()
        advanceTimeBy(20_000); runCurrent()
        rig.iceTrouble(IceTrouble.Recovered); runCurrent()
        rig.repository.toggleMute()
        rig.repository.toggleMute()
        advanceTimeBy(600_000); runCurrent()

        // The full channel would have refused any second voice_join.
        rig.assertCallUp(joins = listOf(1L))
        assertEquals(0, rig.closes())
        assertEquals(
            listOf<String?>("VoiceJoin", "VoiceMute", "VoiceUnmute"),
            rig.voiceFrames(),
        )
        assertTrue(rig.socket.frames.all { it.first == 1L && it.second !is ClientEvent.VoiceLeave })
    }
}
