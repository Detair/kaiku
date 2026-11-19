package io.wolftown.kaiku.data.auth

import android.app.Activity
import io.mockk.coEvery
import io.mockk.coVerify
import io.mockk.mockk
import io.mockk.slot
import kotlinx.coroutines.test.runTest
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/** A passkey request is handed to Credential Manager only for the configured server's exact host. */
class RpIdBindingTest {

    /**
     * The whole rule as one table: server URL, RP ID from the request, allowed. The RP ID must
     * equal the server's host, already in canonical form (lower-case ASCII, punycode); nothing is
     * converted and no parent domain counts.
     */
    private val table: List<Triple<String?, String?, Boolean>> = listOf(
        Triple("https://kaiku.example", "kaiku.example", true),
        Triple("https://chat.kaiku.example", "chat.kaiku.example", true),
        // Parent domains, including public suffixes, never count.
        Triple("https://chat.kaiku.example", "kaiku.example", false),
        Triple("https://a.co.uk", "co.uk", false),
        Triple("https://x.github.io", "github.io", false),
        Triple("https://mallory.duckdns.org", "duckdns.org", false),
        // Children, siblings, look-alikes.
        Triple("https://kaiku.example", "chat.kaiku.example", false),
        Triple("https://kaiku.example", "evil-kaiku.example", false),
        Triple("https://kaiku.example", "evilkaiku.example", false),
        Triple("https://kaiku.example", "other.example", false),
        // Not canonical: refused, never converted (what is checked is what is forwarded).
        Triple("https://kaiku.example", "KAIKU.example", false),
        Triple("https://kaiku.example", "Kaiku.Example", false),
        Triple("https://strasse.example", "straße.example", false),
        Triple("https://xn--strae-oqa.example", "straße.example", false),
        Triple("https://xn--bcher-kva.example", "bücher.example", false),
        Triple("https://kaiku.example", "ｋａｉｋｕ.example", false), // full-width letters
        Triple("https://kaiku.example", "kaiku。example", false), // ideographic full stop
        Triple("https://kaiku.example", "kai​ku.example", false), // zero-width space
        Triple("https://kaiku.example", "kai‍ku.example", false), // zero-width joiner
        Triple("https://kaiku.example", "kai­ku.example", false), // soft hyphen
        Triple("https://kaiku.example", "﻿kaiku.example", false),
        Triple("https://kaiku.example", "kaiku.example.", false), // trailing dot
        Triple("https://kaiku.example", ".kaiku.example", false),
        Triple("https://kaiku.example", "kaiku..example", false),
        Triple("https://kaiku.example", " kaiku.example", false),
        Triple("https://kaiku.example", "kaiku.example ", false),
        Triple("https://kaiku.example", "kaiku.example\n", false),
        Triple("https://kaiku.example:8443", "kaiku.example:8443", false), // port in the RP ID
        Triple("https://kaiku.example", "kaiku.example/x", false),
        Triple("https://kaiku.example", "user@kaiku.example", false),
        Triple("https://kaiku.example", "https://kaiku.example", false),
        Triple("https://kaiku.example", "*.kaiku.example", false),
        Triple("https://kaiku.example", "", false),
        Triple("https://kaiku.example", null, false),
        // Internationalised hosts: the server sends punycode, and so must the request.
        Triple("https://xn--bcher-kva.example", "xn--bcher-kva.example", true),
        // The server URL: its port and path are ignored; user info, cleartext, junk refuse.
        Triple("https://kaiku.example:8443", "kaiku.example", true),
        Triple("https://kaiku.example:8443/some/path?q#f", "kaiku.example", true),
        Triple("https://KAIKU.Example", "kaiku.example", true),
        Triple("https://kaiku.example.", "kaiku.example", true),
        Triple("https://user@kaiku.example", "kaiku.example", false),
        Triple("https://user:pw@kaiku.example", "kaiku.example", false),
        Triple("http://kaiku.example", "kaiku.example", false), // the app never talks cleartext
        Triple("ftp://kaiku.example", "kaiku.example", false),
        Triple("//kaiku.example", "kaiku.example", false),
        Triple("not a url", "kaiku.example", false),
        Triple("", "kaiku.example", false),
        Triple(null, "kaiku.example", false),
        // IP addresses: exactly equal only.
        Triple("https://192.168.1.5:8443", "192.168.1.5", true),
        Triple("https://192.168.1.5", "168.1.5", false),
        Triple("https://192.168.1.5", "192.168.1.6", false),
        Triple("https://[::1]:8443", "::1", true),
        Triple("https://[::1]:8443", "[::1]", false),
        Triple("https://[::1]:8443", "::2", false),
        Triple("https://kaiku.example", "127.0.0.1", false),
        // Single-label test hosts: exactly equal only.
        Triple("https://localhost:8443", "localhost", true),
        Triple("https://kaiku.example", "example", false),
    )

    @Test fun `the rp id must equal the configured server's host`() {
        val wrong = table.filter { (server, rp, expected) -> RpIdBinding.allowed(rp, server) != expected }
        assertTrue("decided wrongly: $wrong", wrong.isEmpty())
    }

    private val activity: Activity = mockk(relaxed = true)
    private val inner: PasskeyCredentials = mockk(relaxed = true)

    private fun bound(server: String?) = ServerBoundPasskeys(inner) { server }

    @Test fun `a request for another server never reaches Credential Manager`() = runTest {
        val p = bound("https://kaiku.example")
        assertEquals(PasskeyOutcome.WrongServer, p.get(activity, """{"challenge":"AQ","rpId":"evil.example"}"""))
        assertEquals(PasskeyOutcome.WrongServer, p.create(activity, """{"challenge":"AQ","rp":{"id":"evil.example"}}"""))
        assertEquals(PasskeyOutcome.WrongServer, p.get(activity, """{"challenge":"AQ"}"""))
        assertEquals(PasskeyOutcome.WrongServer, p.get(activity, "not json"))
        assertEquals(PasskeyOutcome.WrongServer, p.get(activity, "[]"))
        coVerify(exactly = 0) { inner.get(any(), any()) }
        coVerify(exactly = 0) { inner.create(any(), any()) }
    }

    @Test fun `sign-in reads only a string rpId, never rp id`() = runTest {
        val p = bound("https://kaiku.example")
        for (rpId in listOf("""["kaiku.example"]""", "1", "true", "null", """{"id":"kaiku.example"}""")) {
            val json = """{"challenge":"AQ","rpId":$rpId,"rp":{"id":"kaiku.example"}}"""
            assertEquals(json, PasskeyOutcome.WrongServer, p.get(activity, json))
        }
        assertEquals(PasskeyOutcome.WrongServer, p.get(activity, """{"challenge":"AQ","rp":{"id":"kaiku.example"}}"""))
        coVerify(exactly = 0) { inner.get(any(), any()) }
    }

    @Test fun `creation reads only a string rp id, never rpId`() = runTest {
        val p = bound("https://kaiku.example")
        assertEquals(PasskeyOutcome.WrongServer, p.create(activity, """{"challenge":"AQ","rpId":"kaiku.example"}"""))
        assertEquals(PasskeyOutcome.WrongServer, p.create(activity, """{"challenge":"AQ","rpId":"kaiku.example","rp":{"id":["kaiku.example"]}}"""))
        assertEquals(PasskeyOutcome.WrongServer, p.create(activity, """{"challenge":"AQ","rp":{"id":null}}"""))
        assertEquals(PasskeyOutcome.WrongServer, p.create(activity, """{"challenge":"AQ","rp":"kaiku.example"}"""))
        coVerify(exactly = 0) { inner.create(any(), any()) }
    }

    @Test fun `a request for the configured server is passed on`() = runTest {
        coEvery { inner.get(any(), any()) } returns PasskeyOutcome.Success("{}")
        coEvery { inner.create(any(), any()) } returns PasskeyOutcome.Success("{}")
        val p = bound("https://kaiku.example:8443")
        assertEquals(PasskeyOutcome.Success("{}"), p.get(activity, """{"challenge":"AQ","rpId":"kaiku.example"}"""))
        assertEquals(PasskeyOutcome.Success("{}"), p.create(activity, """{"challenge":"AQ","rp":{"id":"kaiku.example","name":"K"}}"""))
    }

    @Test fun `what was checked is what is forwarded, also with a repeated key`() = runTest {
        val sent = slot<String>()
        coEvery { inner.get(any(), capture(sent)) } returns PasskeyOutcome.Success("{}")
        val p = bound("https://kaiku.example")
        // Whichever copy a reader would take, Credential Manager gets only the one that was checked.
        p.get(activity, """{"challenge":"AQ","rpId":"evil.example","rpId":"kaiku.example","timeout":300000}""")
        assertFalse(sent.captured, sent.captured.contains("evil.example"))
        val obj = Json.parseToJsonElement(sent.captured).jsonObject
        assertEquals("kaiku.example", obj["rpId"]!!.jsonPrimitive.content)
        assertEquals("300000", obj["timeout"]!!.jsonPrimitive.content)

        val created = slot<String>()
        coEvery { inner.create(any(), capture(created)) } returns PasskeyOutcome.Success("{}")
        p.create(activity, """{"challenge":"AQ","rp":{"id":"evil.example","id":"kaiku.example"}}""")
        assertFalse(created.captured, created.captured.contains("evil.example"))
    }

    @Test fun `without a configured server nothing is asked`() = runTest {
        assertEquals(PasskeyOutcome.WrongServer, bound(null).get(activity, """{"rpId":"kaiku.example"}"""))
        coVerify(exactly = 0) { inner.get(any(), any()) }
    }

    @Test fun `a removal signal is passed on only for the configured server`() = runTest {
        val p = bound("https://kaiku.example")
        p.signalRemoved("evil.example", null, "aWQ", null)
        p.signalRemoved("KAIKU.example", null, "aWQ", null)
        p.signalRemoved("kaiku.example.", null, "aWQ", null)
        coVerify(exactly = 0) { inner.signalRemoved(any(), any(), any(), any()) }
        p.signalRemoved("kaiku.example", null, "aWQ", null)
        coVerify(exactly = 1) { inner.signalRemoved("kaiku.example", null, "aWQ", null) }
    }

    @Test fun `the server is read at the moment of the call`() = runTest {
        var server: String? = "https://kaiku.example"
        val p = ServerBoundPasskeys(inner) { server }
        server = "https://other.example"
        assertEquals(PasskeyOutcome.WrongServer, p.get(activity, """{"rpId":"kaiku.example"}"""))
    }

    @Test fun `only the checked wrapper can reach Credential Manager`() {
        // The app binds the wrapper, and the unwrapped implementation cannot be injected anywhere.
        val binding = io.wolftown.kaiku.di.PasskeyModule::class.java.declaredMethods
            .single { it.returnType == PasskeyCredentials::class.java }
        assertEquals(ServerBoundPasskeys::class.java, binding.parameterTypes.single())
        val injectable = CredentialManagerPasskeys::class.java.declaredConstructors
            .any { c -> c.isAnnotationPresent(javax.inject.Inject::class.java) }
        assertFalse("CredentialManagerPasskeys must not be injectable", injectable)
    }
}
