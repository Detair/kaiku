package io.wolftown.kaiku.navigation

import io.wolftown.kaiku.ui.auth.setup.SetupEntry
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class SetupLinksTest {
    private val token = "AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8"

    @Test fun `setup link parses kaiku and https forms`() {
        assertEquals(
            SetupEntry("https://chat.example.com", token, SetupEntry.Source.LINK),
            SetupLinks.parse("kaiku://setup/v1?o=https%3A%2F%2Fchat.example.com&t=$token"),
        )
        assertEquals(
            SetupEntry("https://chat.example.com", token, SetupEntry.Source.LINK),
            SetupLinks.parse("https://chat.example.com/setup#v=1&t=$token"),
        )
        assertNull(SetupLinks.parse("kaiku://login/v1?o=https%3A%2F%2Fchat.example.com&t=$token"))
        assertNull(SetupLinks.parse("kaiku://auth/callback?code=x"))
        assertNull(SetupLinks.parse("https://chat.example.com/setup#v=2&t=$token"))
    }

    @Test fun `pending links are taken once`() {
        val e = SetupEntry("https://chat.example.com", token, SetupEntry.Source.LINK)
        PendingSetupLinks.post(e)
        assertEquals(e, PendingSetupLinks.pending.value)
        assertEquals(e, PendingSetupLinks.take())
        assertNull(PendingSetupLinks.take())
    }

    @Test fun `entry never prints its token`() {
        val e = SetupEntry("https://chat.example.com", token, SetupEntry.Source.TICKET, ticket = "ticket-secret")
        assertEquals(false, e.toString().contains(token) || e.toString().contains("ticket-secret"))
    }
}
