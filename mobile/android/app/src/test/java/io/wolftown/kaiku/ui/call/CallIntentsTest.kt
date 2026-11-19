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
}
