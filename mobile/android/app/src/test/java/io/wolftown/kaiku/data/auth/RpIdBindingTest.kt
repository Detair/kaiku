package io.wolftown.kaiku.data.auth

import android.app.Activity
import io.mockk.coEvery
import io.mockk.coVerify
import io.mockk.mockk
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/** A passkey request is handed to Credential Manager only for the configured server. */
class RpIdBindingTest {

    private fun ok(server: String?, rpId: String?) = assertTrue("$rpId for $server", RpIdBinding.allowed(rpId, server))
    private fun no(server: String?, rpId: String?) = assertFalse("$rpId for $server", RpIdBinding.allowed(rpId, server))

    @Test fun `the exact host is allowed`() {
        ok("https://kaiku.example", "kaiku.example")
        ok("https://chat.kaiku.example", "chat.kaiku.example")
    }

    @Test fun `a registrable parent domain is allowed`() {
        ok("https://chat.kaiku.example", "kaiku.example")
        ok("https://a.b.kaiku.example", "kaiku.example")
        ok("https://a.b.kaiku.example", "b.kaiku.example")
    }

    @Test fun `a child, a sibling or an unrelated host is refused`() {
        no("https://kaiku.example", "chat.kaiku.example")
        no("https://chat.kaiku.example", "other.kaiku.example")
        no("https://kaiku.example", "other.example")
    }

    @Test fun `a suffix without a label boundary is refused`() {
        no("https://kaiku.example", "evilkaiku.example")
        no("https://evilkaiku.example", "kaiku.example")
        no("https://chat.evilkaiku.example", "kaiku.example")
    }

    @Test fun `a single label is never a parent`() {
        no("https://chat.kaiku.example", "example")
        no("https://kaiku.example", "example")
        no("https://kaiku.example", "")
    }

    @Test fun `case does not matter`() {
        ok("https://KAIKU.Example", "kaiku.EXAMPLE")
        ok("https://Chat.Kaiku.Example", "KAIKU.example")
    }

    @Test fun `the host's trailing dot is dropped, an rpId with one is refused`() {
        ok("https://kaiku.example.", "kaiku.example")
        no("https://kaiku.example", "kaiku.example.")
        no("https://chat.kaiku.example", "kaiku.example.")
    }

    @Test fun `internationalised names compare in punycode`() {
        ok("https://xn--bcher-kva.example", "bücher.example")
        ok("https://xn--bcher-kva.example", "xn--bcher-kva.example")
        ok("https://chat.xn--bcher-kva.example", "bücher.example")
        no("https://xn--bcher-kva.example", "bucher.example")
    }

    @Test fun `the port of the server URL is ignored`() {
        ok("https://kaiku.example:8443", "kaiku.example")
        ok("https://chat.kaiku.example:8443/some/path", "kaiku.example")
        no("https://kaiku.example:8443", "kaiku.example:8443")
    }

    @Test fun `IP literals match only exactly`() {
        ok("http://192.168.1.5:8080", "192.168.1.5")
        no("http://192.168.1.5", "168.1.5")
        no("http://192.168.1.5", "1.5")
        no("http://192.168.1.5", "192.168.1.6")
        ok("http://[::1]:8080", "::1")
        ok("http://[::1]:8080", "[::1]")
        no("http://[::1]:8080", "::2")
        no("https://kaiku.example", "127.0.0.1")
    }

    @Test fun `single-label hosts match only exactly`() {
        ok("http://localhost:8080", "localhost")
        ok("http://kaiku:8080", "kaiku")
        no("http://kaiku:8080", "localhost")
    }

    @Test fun `a missing rpId or server refuses`() {
        no("https://kaiku.example", null)
        no(null, "kaiku.example")
        no("", "kaiku.example")
        no("not a url", "kaiku.example")
    }

    @Test fun `malformed rpIds are refused`() {
        no("https://kaiku.example", ".kaiku.example")
        no("https://chat.kaiku.example", ".kaiku.example")
        no("https://a.kaiku.example", "a..example")
        no("https://kaiku.example", "kaiku.example/x")
        no("https://kaiku.example", "user@kaiku.example")
        no("https://kaiku.example", "https://kaiku.example")
        no("https://kaiku.example", " kaiku.example")
        no("https://chat.kaiku.example", "*.kaiku.example")
    }

    @Test fun `the request's rpId is read from get and create options`() {
        assertEquals("kaiku.example", RpIdBinding.rpIdOf("""{"challenge":"AQ","rpId":"kaiku.example"}"""))
        assertEquals("kaiku.example", RpIdBinding.rpIdOf("""{"challenge":"AQ","rp":{"id":"kaiku.example","name":"K"}}"""))
        assertEquals(null, RpIdBinding.rpIdOf("""{"challenge":"AQ"}"""))
        assertEquals(null, RpIdBinding.rpIdOf("""{"rp":{"name":"K"}}"""))
        assertEquals(null, RpIdBinding.rpIdOf("""{"rpId":{"x":1}}"""))
        assertEquals(null, RpIdBinding.rpIdOf("not json"))
        assertEquals(null, RpIdBinding.rpIdOf("[]"))
    }

    private val activity: Activity = mockk(relaxed = true)
    private val inner: PasskeyCredentials = mockk(relaxed = true)

    private fun bound(server: String?) = ServerBoundPasskeys(inner) { server }

    @Test fun `a request for another server never reaches Credential Manager`() = runTest {
        val p = bound("https://kaiku.example")
        assertEquals(PasskeyOutcome.WrongServer, p.get(activity, """{"challenge":"AQ","rpId":"evil.example"}"""))
        assertEquals(PasskeyOutcome.WrongServer, p.create(activity, """{"challenge":"AQ","rp":{"id":"evil.example"}}"""))
        assertEquals(PasskeyOutcome.WrongServer, p.get(activity, """{"challenge":"AQ"}"""))
        assertEquals(PasskeyOutcome.WrongServer, p.create(activity, """{"challenge":"AQ","rpId":"kaiku.example"}"""))
        coVerify(exactly = 0) { inner.get(any(), any()) }
        coVerify(exactly = 0) { inner.create(any(), any()) }
    }

    @Test fun `without a configured server nothing is asked`() = runTest {
        assertEquals(PasskeyOutcome.WrongServer, bound(null).get(activity, """{"rpId":"kaiku.example"}"""))
        coVerify(exactly = 0) { inner.get(any(), any()) }
    }

    @Test fun `a request for the configured server is passed on unchanged`() = runTest {
        val json = """{"challenge":"AQ","rpId":"kaiku.example"}"""
        val createJson = """{"challenge":"AQ","rp":{"id":"kaiku.example"}}"""
        coEvery { inner.get(activity, json) } returns PasskeyOutcome.Success("{}")
        coEvery { inner.create(activity, createJson) } returns PasskeyOutcome.Success("{}")
        val p = bound("https://chat.kaiku.example")
        assertEquals(PasskeyOutcome.Success("{}"), p.get(activity, json))
        assertEquals(PasskeyOutcome.Success("{}"), p.create(activity, createJson))
    }

    @Test fun `a removal signal for another server is dropped`() = runTest {
        val p = bound("https://kaiku.example")
        p.signalRemoved("evil.example", null, "aWQ", null)
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
}
