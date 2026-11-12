package io.wolftown.kaiku.service

import io.wolftown.kaiku.data.voice.VoiceLinkState
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.test.TestScope
import kotlinx.coroutines.test.runCurrent
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * The ongoing-call notification: it follows the voice link while the call is
 * on, and none is left behind when the call is over, in whatever order the
 * end of the call, Android's stop and the service's destruction arrive.
 */
@OptIn(ExperimentalCoroutinesApi::class)
class OngoingCallNotificationTest {

    /** Stands in for Android's notification manager: what it was asked to do, and whether a notification is up. */
    private class Notifications {
        val calls = mutableListOf<String>()
        var shown = false

        fun post(reconnecting: Boolean) {
            calls += if (reconnecting) "post:reconnecting" else "post:in-call"
            shown = true
        }

        fun cancel() {
            calls += "cancel"
            shown = false
        }

        /** The service went to the foreground with its notification. */
        fun foreground() {
            calls += "foreground"
            shown = true
        }

        /** The stop was requested: Android takes the foreground notification away at once. */
        fun stopRequested() {
            calls += "stop"
            shown = false
        }
    }

    private class Harness(scope: TestScope) {
        val notifications = Notifications()
        var callIsOn = true
        val link = MutableStateFlow<VoiceLinkState>(VoiceLinkState.Stable)
        val notification = OngoingCallNotification(
            scope = scope.backgroundScope,
            callIsOn = { callIsOn },
            linkState = link,
            post = notifications::post,
            cancel = notifications::cancel,
        )

        /** The service starts as Android does it: foreground first, then the updates. */
        fun start() {
            notifications.foreground()
            notification.follow()
        }
    }

    @Test
    fun `the notification follows the link while the call is on`() = runTest {
        val h = Harness(this)
        h.start()
        runCurrent()

        h.link.value = VoiceLinkState.Interrupted
        runCurrent()
        // Still reconnecting: nothing new to say.
        h.link.value = VoiceLinkState.Rejoining(1)
        runCurrent()
        h.link.value = VoiceLinkState.Rejoining(2)
        runCurrent()
        h.link.value = VoiceLinkState.Stable
        runCurrent()

        assertEquals(
            listOf("foreground", "post:in-call", "post:reconnecting", "post:in-call"),
            h.notifications.calls,
        )
        assertTrue(h.notifications.shown)
    }

    @Test
    fun `a service started while the join is still held says reconnecting from the start`() = runTest {
        val h = Harness(this)
        h.link.value = VoiceLinkState.Interrupted
        h.start()
        runCurrent()

        assertEquals(listOf("foreground", "post:reconnecting"), h.notifications.calls)
    }

    @Test
    fun `following twice does not post twice`() = runTest {
        val h = Harness(this)
        h.start()
        h.notification.follow()
        runCurrent()

        assertEquals(listOf("foreground", "post:in-call"), h.notifications.calls)
    }

    @Test
    fun `nothing is posted once the call is over, whatever the link does`() = runTest {
        val h = Harness(this)
        h.link.value = VoiceLinkState.Rejoining(3)
        h.start()
        runCurrent()

        // A give-up: the call is cleared, the service is told to stop, and
        // only then does the link go back to stable.
        h.callIsOn = false
        h.notifications.stopRequested()
        h.link.value = VoiceLinkState.Stable
        runCurrent()

        assertEquals(listOf("foreground", "post:reconnecting", "stop"), h.notifications.calls)
        assertFalse(h.notifications.shown)
    }

    @Test
    fun `an update that was on its way when the stop came is removed with the service`() = runTest {
        val h = Harness(this)
        h.link.value = VoiceLinkState.Rejoining(1)
        h.start()
        runCurrent()

        // The link changes while the call still counts as on; by the time the
        // update is posted, Android has already taken the notification away.
        h.notifications.stopRequested()
        h.link.value = VoiceLinkState.Stable
        runCurrent()
        assertTrue(h.notifications.shown)

        h.notification.destroy()

        assertEquals("cancel", h.notifications.calls.last())
        assertFalse(h.notifications.shown)
    }

    @Test
    fun `nothing is posted after the service was destroyed`() = runTest {
        val h = Harness(this)
        h.start()
        runCurrent()

        h.notifications.stopRequested()
        h.notification.destroy()
        h.link.value = VoiceLinkState.Interrupted
        runCurrent()
        h.link.value = VoiceLinkState.Stable
        runCurrent()

        assertEquals(listOf("foreground", "post:in-call", "stop", "cancel"), h.notifications.calls)
        assertFalse(h.notifications.shown)
    }
}
