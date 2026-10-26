package io.wolftown.kaiku.push

import io.wolftown.kaiku.R
import io.wolftown.kaiku.navigation.DeepLinks
import org.junit.Assert.assertEquals
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

    @Test
    fun `ringing notification times out a bit after the ring timeout`() {
        val timeout = CallNotificationText.timeoutAfterMs(n(CallKind.INCOMING, "Bob"))
        assertEquals(CallNotificationText.RINGING_TIMEOUT_MS, timeout)
        assertTrue(timeout!! > 45_000L)
    }

    @Test
    fun `missed call notification never times out`() {
        assertNull(CallNotificationText.timeoutAfterMs(n(CallKind.MISSED, "Bob")))
    }
}
