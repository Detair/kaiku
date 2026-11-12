package io.wolftown.kaiku.data.repository

import io.wolftown.kaiku.data.voice.VoiceEndNotice
import io.wolftown.kaiku.data.voice.VoiceLinkState
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.test.advanceTimeBy
import kotlinx.coroutines.test.runCurrent
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * The call service and its notification over the life of a call
 * ([VoiceRig.FakeCallService] plays Android's part): the service is started
 * once, at the tap, the notification follows the voice link, and when the
 * call ends (give-up, leave, removal) none is posted any more and none is
 * left, also while the call was "reconnecting".
 */
@OptIn(ExperimentalCoroutinesApi::class)
class VoiceCallNotificationTest {

    @Test
    fun `the notification says reconnecting while a join is held and during a rejoin`() = rigTest { rig ->
        rig.repository.startJoin(rig.channel)
        runCurrent() // held: no socket yet, but the service is up
        assertEquals(listOf("foreground", "post:reconnecting"), rig.service.notifications)

        rig.socket.ready(); runCurrent()
        rig.assertCallUp(joins = listOf(1L))
        assertEquals("post:in-call", rig.service.notifications.last())

        rig.socket.drop(); runCurrent()
        assertEquals("post:reconnecting", rig.service.notifications.last())
        rig.socket.ready(); runCurrent()
        advanceTimeBy(1_000); runCurrent()
        rig.assertCallUp(joins = listOf(1L, 2L))

        // One service, one notification, updated in place.
        assertEquals(
            listOf("foreground", "post:reconnecting", "post:in-call", "post:reconnecting", "post:in-call"),
            rig.service.notifications,
        )
    }

    @Test
    fun `a give-up leaves no notification behind`() = rigTest { rig ->
        rig.joinAndConnect()
        rig.socket.drop(); runCurrent()

        advanceTimeBy(60_000); runCurrent()
        rig.assertGaveUp()
        // The link went back to "stable" after the service was told to stop.
        assertEquals(VoiceLinkState.Stable, rig.repository.linkState.value)
        advanceTimeBy(10_000); runCurrent()

        rig.assertNoCallNotification()
        assertEquals(listOf("stop", "cancel", "destroyed"), rig.service.notifications.takeLast(3))
    }

    @Test
    fun `a give-up after three failed attempts leaves no notification behind`() = rigTest { rig ->
        rig.joinAndConnect()
        rig.mediaConnects = false
        rig.socket.drop(); runCurrent()
        rig.socket.ready(); runCurrent()

        // 1 s + 15 s, 3 s + 15 s, 9 s + 15 s.
        advanceTimeBy(58_000); runCurrent()
        // (No look at the full channel here: attempts two and three repeat the
        // voice_join on the socket that already carries the first one.)
        assertEquals(null, rig.repository.currentChannelId.value)
        assertEquals(VoiceEndNotice.RejoinFailed(rig.channel, wasMuted = false), rig.repository.endNotice.value)
        assertEquals(VoiceLinkState.Stable, rig.repository.linkState.value)
        assertTrue("call service not stopped: ${rig.log}", "service:stop" in rig.log)
        advanceTimeBy(10_000); runCurrent()

        rig.assertNoCallNotification()
        assertEquals(listOf("stop", "cancel", "destroyed"), rig.service.notifications.takeLast(3))
    }

    @Test
    fun `leaving while reconnecting leaves no notification behind`() = rigTest { rig ->
        rig.joinAndConnect()
        rig.socket.drop(); runCurrent()
        assertEquals("post:reconnecting", rig.service.notifications.last())

        rig.leave()
        advanceTimeBy(10_000); runCurrent()

        rig.assertEndedAt("LEAVE", ownLeave = true)
        assertEquals(listOf("stop", "cancel", "destroyed"), rig.service.notifications.takeLast(3))
    }

    @Test
    fun `a removal while reconnecting leaves no notification behind`() = rigTest { rig ->
        rig.joinAndConnect()
        rig.mediaConnects = false
        rig.socket.drop(); runCurrent()
        rig.socket.ready(); runCurrent()
        advanceTimeBy(1_000); runCurrent() // a rejoin attempt is running
        assertEquals(VoiceLinkState.Rejoining(1), rig.repository.linkState.value)

        rig.removed()
        advanceTimeBy(10_000); runCurrent()

        rig.assertEndedAt("REMOVED", ownLeave = false)
        assertEquals(listOf("stop", "cancel", "destroyed"), rig.service.notifications.takeLast(3))
    }

    @Test
    fun `a held join that is given up never showed more than its one notification`() = rigTest { rig ->
        rig.repository.startJoin(rig.channel)
        runCurrent()

        advanceTimeBy(60_000); runCurrent()
        advanceTimeBy(10_000); runCurrent()

        rig.assertNoCallNotification()
        assertEquals(
            listOf("foreground", "post:reconnecting", "stop", "cancel", "destroyed"),
            rig.service.notifications,
        )
        assertTrue("service:start" in rig.log && "service:stop" in rig.log)
    }
}
