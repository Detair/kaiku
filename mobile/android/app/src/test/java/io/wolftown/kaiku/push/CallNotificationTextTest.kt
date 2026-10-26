package io.wolftown.kaiku.push

import io.wolftown.kaiku.R
import io.wolftown.kaiku.navigation.DeepLinks
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class CallNotificationTextTest {

    private val ch = "0f8fad5b-d9cb-469f-a165-70867728950e"
    private val callId = "6f1c2a3b-4d5e-4f60-8a7b-9c0d1e2f3a4b"

    private fun n(kind: CallKind, name: String?) = CallNotification("me", ch, callId, kind, name)

    @Test
    fun `incoming call names the caller`() {
        assertEquals(
            CallNotificationText.Text(R.string.notifications_call_incoming, "Bob"),
            CallNotificationText.of(n(CallKind.INCOMING, "Bob")),
        )
    }

    @Test
    fun `incoming call without a name is generic`() {
        assertEquals(
            CallNotificationText.Text(R.string.notifications_call_incoming_unknown, null),
            CallNotificationText.of(n(CallKind.INCOMING, null)),
        )
    }

    @Test
    fun `missed call names the caller`() {
        assertEquals(
            CallNotificationText.Text(R.string.notifications_call_missed, "Bob"),
            CallNotificationText.of(n(CallKind.MISSED, "Bob")),
        )
    }

    @Test
    fun `missed call without a name is generic`() {
        assertEquals(
            CallNotificationText.Text(R.string.notifications_call_missed_unknown, null),
            CallNotificationText.of(n(CallKind.MISSED, null)),
        )
    }

    @Test
    fun `blank name counts as unknown`() {
        assertEquals(
            CallNotificationText.Text(R.string.notifications_call_missed_unknown, null),
            CallNotificationText.of(n(CallKind.MISSED, "  ")),
        )
    }

    @Test
    fun `tap opens the DM channel tagged with the user`() {
        val target = NotificationTaps.call(n(CallKind.MISSED, "Bob"))

        assertEquals(DeepLinks.channelUri(ch), target.uri)
        assertEquals(ch, DeepLinks.parseChannelId(target.uri))
        assertEquals("me", target.userId)
    }

    @Test
    fun `call tap does not share the message tap request code`() {
        val call = NotificationTaps.call(n(CallKind.INCOMING, null))
        val message = NotificationTaps.message(ch, "me")

        assertEquals(call.uri, message.uri)
        assertNotEquals(message.requestCode, call.requestCode)
    }

    private val callB = "1b2c3d4e-5f60-4a7b-8c9d-0e1f2a3b4c5d"

    @Test
    fun `new ringing call alerts and times out a bit after the ring timeout`() {
        val plan = CallNotificationText.plan(n(CallKind.INCOMING, "Bob"), previous = null)
        assertEquals(CallPostPlan(cancelFirst = true, onlyAlertOnce = false, silent = false,
            timeoutMs = CallNotificationText.RINGING_TIMEOUT_MS), plan)
        assertTrue(CallNotificationText.RINGING_TIMEOUT_MS > 45_000L)
    }

    @Test
    fun `ringing replacing a missed call alerts again`() {
        val plan = CallNotificationText.plan(
            n(CallKind.INCOMING, "Bob").copy(callId = callB),
            previous = PostedCall("me", callId, CallKind.MISSED),
        )
        assertTrue(plan.cancelFirst)
        assertFalse(plan.onlyAlertOnce)
    }

    @Test
    fun `repeat of the same ringing call updates quietly`() {
        val plan = CallNotificationText.plan(
            n(CallKind.INCOMING, "Bob"),
            previous = PostedCall("me", callId, CallKind.INCOMING),
        )
        assertFalse(plan.cancelFirst)
        assertTrue(plan.onlyAlertOnce)
    }

    @Test
    fun `missed call replaces the ringing one fresh, silently and without timeout`() {
        val plan = CallNotificationText.plan(
            n(CallKind.MISSED, "Bob"),
            previous = PostedCall("me", callId, CallKind.INCOMING),
        )
        assertEquals(CallPostPlan(cancelFirst = true, onlyAlertOnce = true, silent = true, timeoutMs = null), plan)
        assertNull(CallNotificationText.plan(n(CallKind.MISSED, null), previous = null).timeoutMs)
    }

    @Test
    fun `ringing timeout counts from when the ringing was received`() {
        val now = 1_000_000L
        val plan = CallNotificationText.plan(
            n(CallKind.INCOMING, "Bob").copy(ringingSinceMs = now - 20_000L),
            previous = null,
            nowMs = now,
        )
        assertEquals(CallNotificationText.RINGING_TIMEOUT_MS - 20_000L, plan.timeoutMs)
    }

    @Test
    fun `ringing timeout never drops below the minimum`() {
        val now = 1_000_000L
        val plan = CallNotificationText.plan(
            n(CallKind.INCOMING, "Bob").copy(ringingSinceMs = now - 58_000L),
            previous = null,
            nowMs = now,
        )
        assertEquals(CallNotificationText.MIN_RINGING_TIMEOUT_MS, plan.timeoutMs)
    }
}
