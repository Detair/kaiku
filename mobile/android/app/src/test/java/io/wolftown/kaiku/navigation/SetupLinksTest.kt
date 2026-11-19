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

    @Test fun `both link forms give the same setup and are checked the same way`() {
        // C20: the verified https App Link and the kaiku:// fallback reach the same SetupEntry, so the
        // setup flow applies the same origin pin and the same "Set up on {server}?" question to both.
        val cases = listOf(
            "https://chat.example.com" to "https://chat.example.com",
            "https://Chat.Example.COM:443" to "https://chat.example.com",
            "https://chat.example.com:8443" to "https://chat.example.com:8443",
        )
        for ((given, canonical) in cases) {
            val enc = java.net.URLEncoder.encode(given, "UTF-8")
            val expected = SetupEntry(canonical, token, SetupEntry.Source.LINK)
            assertEquals(given, expected, SetupLinks.parse("kaiku://setup/v1?o=$enc&t=$token"))
            assertEquals(given, expected, SetupLinks.parse("$given/setup#v=1&t=$token"))
        }
        // A user part in the origin is refused in both forms.
        assertNull(SetupLinks.parse("kaiku://setup/v1?o=https%3A%2F%2Fme%40chat.example.com&t=$token"))
        assertNull(SetupLinks.parse("https://me@chat.example.com/setup#v=1&t=$token"))
        // Plain http only for local test servers, in both forms.
        assertNull(SetupLinks.parse("kaiku://setup/v1?o=http%3A%2F%2Fchat.example.com&t=$token"))
        assertNull(SetupLinks.parse("http://chat.example.com/setup#v=1&t=$token"))
        // Only the /setup path is a setup link (the App Link filter's path).
        assertNull(SetupLinks.parse("https://chat.example.com/setup/x#v=1&t=$token"))
        assertNull(SetupLinks.parse("https://chat.example.com/auth/callback#v=1&t=$token"))
    }
}
