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

    @org.junit.Test fun `a pending link is taken once and consumed only if it is still the same`() {
        val a = io.wolftown.kaiku.ui.auth.setup.SetupEntry("https://chat.example.com", "AAAA", io.wolftown.kaiku.ui.auth.setup.SetupEntry.Source.LINK)
        val b = a.copy(tokenB64u = "BBBB")
        PendingSetupLinks.post(a)
        org.junit.Assert.assertEquals(a, PendingSetupLinks.take())
        org.junit.Assert.assertNull(PendingSetupLinks.take())
        PendingSetupLinks.post(a)
        PendingSetupLinks.post(b) // a newer link arrives before the screen consumed the first
        PendingSetupLinks.consume(a)
        org.junit.Assert.assertEquals(b, PendingSetupLinks.pending.value)
        PendingSetupLinks.consume(b)
        org.junit.Assert.assertNull(PendingSetupLinks.pending.value)
        PendingSetupLinks.post(a)
        PendingSetupLinks.clear()
        org.junit.Assert.assertNull(PendingSetupLinks.pending.value)
    }

    @org.junit.Test fun `a scanned sign-in code is handed over once`() {
        val p = io.wolftown.kaiku.domain.ScanPayload.Login("https://chat.example.com", "AAAA")
        io.wolftown.kaiku.navigation.ScanHandoff.putLogin(p)
        org.junit.Assert.assertEquals(p, io.wolftown.kaiku.navigation.ScanHandoff.takeLogin())
        org.junit.Assert.assertNull(io.wolftown.kaiku.navigation.ScanHandoff.takeLogin())
        io.wolftown.kaiku.navigation.ScanHandoff.putLogin(p)
        io.wolftown.kaiku.navigation.ScanHandoff.clear()
        org.junit.Assert.assertNull(io.wolftown.kaiku.navigation.ScanHandoff.takeLogin())
    }
}
