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

        fun post(status: Int?) {
            calls += io.wolftown.kaiku.data.repository.postLabel(status)
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
        val callOn = MutableStateFlow(true)
        var callIsOn: Boolean
            get() = callOn.value
            set(value) { callOn.value = value }
        val link = MutableStateFlow<VoiceLinkState>(VoiceLinkState.Stable)

        /** The call's voice is up (its first join done); before that, a stable link means nothing. */
        val callUp = MutableStateFlow(true)
        val gate = CallNotificationGate()

        /** The order `VoiceCallService.start`/`stop` use, the production object. */
        val control = ServiceStartStop(gate)
        val notification = OngoingCallNotification(
            scope = scope.backgroundScope,
            callIsOn = { callIsOn },
            callOn = callOn,
            callUp = callUp,
            gate = gate,
            linkState = link,
            post = notifications::post,
            cancel = notifications::cancel,
        )

        /** The service starts as Android does it: foreground first, then the updates. */
        fun start() {
            control.start { generation ->
                notifications.foreground()
                notification.follow(generation)
            }
        }

        /** `VoiceCallService.stop`: the gate closes, then Android takes the notification away. */
        fun stop() {
            control.stop { notifications.stopRequested() }
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
        h.notification.follow(1L) // the same start again
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
        h.stop()
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
        h.stop()
        h.link.value = VoiceLinkState.Stable
        runCurrent()
        // Nothing is posted once the stop was requested.
        assertEquals(listOf("foreground", "post:reconnecting", "stop"), h.notifications.calls)
        assertFalse(h.notifications.shown)

        h.notification.destroy()

        assertEquals("cancel", h.notifications.calls.last())
        assertFalse(h.notifications.shown)
    }

    @Test
    fun `a new call after a stop opens the gate again and is posted`() = runTest {
        val h = Harness(this)
        h.start()
        runCurrent()
        h.stop()
        h.notification.destroy()

        // The next call starts the service again (a new instance in production).
        val next = OngoingCallNotification(
            scope = backgroundScope,
            callIsOn = { true },
            gate = h.gate,
            linkState = h.link,
            post = h.notifications::post,
            cancel = h.notifications::cancel,
        )
        h.control.start { next.follow(it) }
        runCurrent()
        h.link.value = VoiceLinkState.Interrupted
        runCurrent()

        assertEquals(listOf("post:in-call", "post:reconnecting"), h.notifications.calls.takeLast(2))
    }

    @Test
    fun `the stop closes the gate before the service is told to stop, the start opens it first`() {
        val gate = CallNotificationGate()
        val control = ServiceStartStop(gate)
        var openAtStart = false
        var started = 0L
        control.start { generation -> started = generation; gate.ifOpen(generation) { openAtStart = true } }
        var postedAtStop = false
        control.stop { gate.ifOpen(started) { postedAtStop = true } }
        assertTrue(openAtStart)
        assertFalse(postedAtStop)
    }

    @Test
    fun `nothing is posted after the service was destroyed`() = runTest {
        val h = Harness(this)
        h.start()
        runCurrent()

        h.stop()
        h.notification.destroy()
        h.link.value = VoiceLinkState.Interrupted
        runCurrent()
        h.link.value = VoiceLinkState.Stable
        runCurrent()

        assertEquals(listOf("foreground", "post:in-call", "stop", "cancel"), h.notifications.calls)
        assertFalse(h.notifications.shown)
    }

    @Test
    fun `an update of a stopped call never posts after the next call started`() = runTest {
        val h = Harness(this)
        h.start() // call A
        runCurrent()
        h.stop()
        // Call B starts before A's service instance is destroyed.
        val b = OngoingCallNotification(
            scope = backgroundScope,
            callIsOn = { true },
            gate = h.gate,
            linkState = MutableStateFlow<VoiceLinkState>(VoiceLinkState.Stable),
            post = { },
            cancel = { },
        )
        h.control.start { b.follow(it) }
        runCurrent()
        val before = h.notifications.calls.toList()

        // An update queued for A runs now: it must not post A's notification.
        h.link.value = VoiceLinkState.Interrupted
        runCurrent()

        assertEquals(before, h.notifications.calls)
    }

    @Test
    fun `an action of the ongoing notification applies only to the start it was shown for`() {
        val gate = CallNotificationGate()
        val first = gate.open()
        org.junit.Assert.assertTrue(gate.actionApplies(first))
        // The next call started the service again: the earlier notification's buttons do nothing.
        val second = gate.open()
        org.junit.Assert.assertFalse(gate.actionApplies(first))
        org.junit.Assert.assertTrue(gate.actionApplies(second))
        // Stopped: nothing applies.
        gate.retire()
        org.junit.Assert.assertFalse(gate.actionApplies(second))
    }

    @Test
    fun `a call that comes on after the link was already up shows as in the call`() = runTest {
        val h = Harness(this)
        // The service is held for a call: "Connecting…" from its start, the call not on yet.
        h.callIsOn = false
        h.start()
        runCurrent()
        // The whole connecting phase passed without the notification seeing it: the link is
        // up already when the call comes on.
        h.callIsOn = true
        runCurrent()
        assertEquals("post:in-call", h.notifications.calls.last())
    }

    @Test
    fun `a call that is on before its voice set out shows no "in call" before "Connecting…"`() = runTest {
        val h = Harness(this)
        h.callIsOn = false
        h.callUp.value = false
        h.start()
        runCurrent()
        // The call's session exists, its signalling has not started: the link still reads stable.
        h.callIsOn = true
        runCurrent()
        h.link.value = VoiceLinkState.Connecting
        runCurrent()
        h.callUp.value = true
        h.link.value = VoiceLinkState.Stable
        runCurrent()
        assertEquals(listOf("foreground", "post:connecting", "post:in-call"), h.notifications.calls)
    }
}
