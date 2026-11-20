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
        // The verified https App Link and the kaiku:// fallback reach the same SetupEntry, so the
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

    /** While the app cannot start, the setup link that opened it is kept for the setup screen; nothing else is. */
    @org.junit.Test fun `a setup link that opened the app is kept for later, any other link is not`() {
        PendingSetupLinks.clear()
        org.junit.Assert.assertFalse(PendingSetupLinks.keep(null))
        org.junit.Assert.assertFalse(PendingSetupLinks.keep("kaiku://auth/callback?code=x"))
        org.junit.Assert.assertFalse(PendingSetupLinks.keep("kaiku://login/v1?o=https%3A%2F%2Fchat.example.com&t=$token"))
        org.junit.Assert.assertFalse(PendingSetupLinks.keep("https://chat.example.com/channels/1"))
        assertNull(PendingSetupLinks.pending.value)
        org.junit.Assert.assertTrue(PendingSetupLinks.keep("https://chat.example.com/setup#v=1&t=$token"))
        // Kept, not yet posted: it goes to the setup owner once the app has started.
        assertNull(PendingSetupLinks.pending.value)
        val offered = mutableListOf<SetupEntry>()
        PendingSetupLinks.releaseKept { e, _ -> offered += e; true }
        assertEquals(listOf(SetupEntry("https://chat.example.com", token, SetupEntry.Source.LINK)), offered)
        assertEquals(SetupEntry("https://chat.example.com", token, SetupEntry.Source.LINK), PendingSetupLinks.take())
        assertNull(PendingSetupLinks.take())
        // Released once only.
        PendingSetupLinks.releaseKept { e, _ -> offered += e; true }
        assertEquals(1, offered.size)
    }

    /** A link the setup owner does not use is not posted for the setup screen. */
    @org.junit.Test fun `an opened link is posted only when the setup owner takes it`() {
        PendingSetupLinks.clear()
        val e = SetupEntry("https://chat.example.com", token, SetupEntry.Source.LINK)
        PendingSetupLinks.deliver(e) { _, _ -> false }
        assertNull(PendingSetupLinks.pending.value)
        PendingSetupLinks.deliver(e) { _, _ -> true }
        assertEquals(e, PendingSetupLinks.take())
    }

    /**
     * Every setup link the activity receives goes through the setup owner: the one opened now and
     * the one kept while the app could not start (released once the app starts).
     */
    @org.junit.Test fun `the activity hands every setup link to the setup owner`() {
        val root = generateSequence(java.io.File("").absoluteFile) { it.parentFile }
            .map { java.io.File(it, "src/main/java/io/wolftown/kaiku") }
            .first { it.isDirectory }
        val activity = java.io.File(root, "ui/MainActivity.kt").readText()
        val started = activity.substringAfter("authState.initialize(tokenStorage)").substringBefore("setContent {")
        org.junit.Assert.assertTrue(started.contains("PendingSetupLinks.releaseKept(setupOwner::offer)"))
        org.junit.Assert.assertTrue(activity.contains("PendingSetupLinks.deliver(it, setupOwner::offer)"))
        org.junit.Assert.assertFalse(activity.contains("PendingSetupLinks.post("))
    }

    /** The screen shown while the key record cannot be read keeps the launch link before it returns. */
    @org.junit.Test fun `the unreadable screen keeps the link the app was opened with`() {
        val root = generateSequence(java.io.File("").absoluteFile) { it.parentFile }
            .map { java.io.File(it, "src/main/java/io/wolftown/kaiku") }
            .first { it.isDirectory }
        val activity = java.io.File(root, "ui/MainActivity.kt").readText()
        val gate = activity.substringAfter("if (keyStorage.unreadable()) {").substringBefore("return")
        org.junit.Assert.assertTrue(gate.contains("savedInstanceState == null && PendingSetupLinks.keep(intent?.data?.toString())"))
        org.junit.Assert.assertTrue(gate.contains("KeyStorageUnreadable("))
    }

    /** A link received while the unreadable screen is shown is only kept, and judged after the store was read again. */
    @org.junit.Test fun `a link received on the unreadable screen is kept for after the reload`() {
        val root = generateSequence(java.io.File("").absoluteFile) { it.parentFile }
            .map { java.io.File(it, "src/main/java/io/wolftown/kaiku") }
            .first { it.isDirectory }
        val activity = java.io.File(root, "ui/MainActivity.kt").readText()
        val newIntent = activity.substringAfter("override fun onNewIntent(intent: Intent) {").substringBefore("\n    }")
        val gate = newIntent.substringAfter("if (keyStorage.unreadable()) {").substringBefore("}")
        org.junit.Assert.assertTrue(gate.contains("PendingSetupLinks.keep(intent.data?.toString())"))
        org.junit.Assert.assertTrue(gate.contains("return"))
        org.junit.Assert.assertTrue(newIntent.indexOf("keyStorage.unreadable()") < newIntent.indexOf("handleIntent(intent)"))
    }
}
