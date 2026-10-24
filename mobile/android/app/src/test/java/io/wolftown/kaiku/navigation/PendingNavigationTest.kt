package io.wolftown.kaiku.navigation

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
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

class PendingNavigationRulesTest {
    private val auth = setOf("login", "server_url")

    private fun consume(
        loggedIn: Boolean = true, tag: String? = null, user: String? = "u1", route: String? = "home",
    ) = PendingNavigationRules.shouldConsume(loggedIn, tag, user, route, auth)

    @Test fun `logged out does not consume`() = assertFalse(consume(loggedIn = false, user = null))
    @Test fun `auth route does not consume`() = assertFalse(consume(route = "login"))
    @Test fun `null route does not consume`() = assertFalse(consume(route = null))
    @Test fun `foreign user tag does not consume and is dropped`() {
        assertFalse(consume(tag = "u2"))
        assertTrue(PendingNavigationRules.shouldDrop(true, "u2", "u1"))
    }
    @Test fun `matching tag consumes`() = assertTrue(consume(tag = "u1"))
    @Test fun `logged out tag consumes after login`() {
        assertTrue(consume(tag = null))
        assertFalse(PendingNavigationRules.shouldDrop(true, null, "u1"))
    }

    @Test fun `post records user tag and clear resets it`() {
        val p = PendingNavigation()
        p.post("c", "u1")
        assertEquals("u1", p.userTag)
        p.clear()
        assertNull(p.userTag)
    }
}
