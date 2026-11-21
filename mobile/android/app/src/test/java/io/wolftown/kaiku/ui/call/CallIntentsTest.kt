package io.wolftown.kaiku.ui.call

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class CallIntentsTest {

    @Test
    fun `a full ring is read back with its action`() {
        val extras = mapOf(
            CallIntents.EXTRA_USER_ID to "me",
            CallIntents.EXTRA_CHANNEL_ID to "dm-1",
            CallIntents.EXTRA_CALL_ID to "c-1",
            CallIntents.EXTRA_CALLER_ID to "anna",
            CallIntents.EXTRA_CALLER_NAME to "Anna",
            CallIntents.EXTRA_ACTION to CallIntents.ACTION_ACCEPT,
        )
        val parsed = CallIntents.parse(extras::get)
        assertEquals(CallIntents.Ring("me", "dm-1", "c-1", "anna", "Anna"), parsed.ring)
        assertEquals(CallIntents.ACTION_ACCEPT, parsed.action)
    }

    @Test
    fun `an incomplete ring is no ring and the default action is show`() {
        val parsed = CallIntents.parse(mapOf(CallIntents.EXTRA_CHANNEL_ID to "dm-1")::get)
        assertNull(parsed.ring)
        assertEquals(CallIntents.ACTION_SHOW, parsed.action)
    }

    // -- What a notification's broadcast means ------------------------------------------

    private val ring = CallIntents.Ring("me", "dm-1", "c-1", "anna", "Anna", receivedAtMs = 1_000_000L)

    @Test
    fun `the ring carries the time it was received and names its call`() {
        val extras = mapOf(
            CallIntents.EXTRA_USER_ID to "me",
            CallIntents.EXTRA_CHANNEL_ID to "dm-1",
            CallIntents.EXTRA_CALL_ID to "c-1",
            CallIntents.EXTRA_CALLER_ID to "anna",
            CallIntents.EXTRA_RECEIVED_AT to "1000000",
        )
        val parsed = CallIntents.parse(extras::get).ring!!
        assertEquals(1_000_000L, parsed.receivedAtMs)
        assertEquals(io.wolftown.kaiku.data.call.CallRef("me", "dm-1", "c-1"), parsed.ref())
    }

    @Test
    fun `decline declines exactly the notification's call for its account`() {
        assertEquals(
            io.wolftown.kaiku.data.call.CallInput.Decline(io.wolftown.kaiku.data.call.CallRef("me", "dm-1", "c-1")),
            CallIntents.inputOf(CallIntents.ACTION_DECLINE, ring, nowMs = 1_005_000L),
        )
    }

    @Test
    fun `a ring notification that goes away after its 100 s is the ring's end`() {
        assertEquals(
            io.wolftown.kaiku.data.call.CallInput.RingExpired(io.wolftown.kaiku.data.call.CallRef("me", "dm-1", "c-1"), "Anna"),
            CallIntents.inputOf(CallIntents.ACTION_RING_GONE, ring, nowMs = 1_100_000L),
        )
    }

    @Test
    fun `a ring notification swiped away early changes nothing`() {
        assertNull(CallIntents.inputOf(CallIntents.ACTION_RING_GONE, ring, nowMs = 1_020_000L))
    }

    @Test
    fun `an unknown broadcast is nothing`() {
        assertNull(CallIntents.inputOf("something.else", ring, nowMs = 1_100_000L))
    }

    @Test
    fun `the intents of two calls' rings in one conversation never share a request code`() {
        val first = CallIntents.ringRequestBase("dm-1", "c-1")
        val second = CallIntents.ringRequestBase("dm-1", "c-2")
        org.junit.Assert.assertTrue((first..first + 3).intersect(second..second + 3).isEmpty())
        org.junit.Assert.assertEquals(first, CallIntents.ringRequestBase("dm-1", "c-1"))
    }
}
