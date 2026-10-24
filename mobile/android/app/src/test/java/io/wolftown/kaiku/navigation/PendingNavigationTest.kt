package io.wolftown.kaiku.navigation

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class PendingNavigationTest {
    @Test fun `pending navigation is consumed once`() {
        val pending = PendingNavigation()
        pending.post("c1")
        assertEquals("c1", pending.channelId.value)
        assertEquals("c1", pending.consume())
        assertNull(pending.consume())
        assertNull(pending.channelId.value)
    }

    @Test fun `pending navigation cleared on logout`() {
        val pending = PendingNavigation()
        pending.post("c1")
        pending.clear()
        assertNull(pending.channelId.value)
        assertNull(pending.consume())
    }
}
