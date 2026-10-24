package io.wolftown.kaiku.domain

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class ServerAddressTest {

    private fun ok(input: String): String = normalizeServerAddress(input).getOrThrow()

    private fun reason(input: String): InvalidServerAddressException.Reason {
        val error = normalizeServerAddress(input).exceptionOrNull()
        assertTrue("expected a rejection for '$input'", error is InvalidServerAddressException)
        return (error as InvalidServerAddressException).reason
    }

    @Test
    fun `bare host gets https`() {
        assertEquals("https://kaiku.pmind.de", ok("kaiku.pmind.de"))
        assertEquals("https://kaiku.pmind.de", ok("  Kaiku.PMind.de  "))
    }

    @Test
    fun `https address is kept and port survives`() {
        assertEquals("https://kaiku.pmind.de", ok("https://kaiku.pmind.de"))
        assertEquals("https://kaiku.example:8443", ok("kaiku.example:8443/"))
    }

    @Test
    fun `http is rejected`() {
        assertEquals(InvalidServerAddressException.Reason.NOT_SECURE, reason("http://kaiku.pmind.de"))
        assertEquals(InvalidServerAddressException.Reason.NOT_SECURE, reason("ftp://kaiku.pmind.de"))
    }

    @Test
    fun `path is stripped`() {
        assertEquals("https://kaiku.pmind.de", ok("https://kaiku.pmind.de/"))
        assertEquals("https://kaiku.pmind.de", ok("kaiku.pmind.de/app/login?x=1#top"))
        assertEquals("https://kaiku.pmind.de", ok("https://kaiku.pmind.de?invite=abc"))
    }

    @Test
    fun `blank is rejected`() {
        assertEquals(InvalidServerAddressException.Reason.BLANK, reason(""))
        assertEquals(InvalidServerAddressException.Reason.BLANK, reason("   "))
    }

    @Test
    fun `malformed addresses are rejected`() {
        assertEquals(InvalidServerAddressException.Reason.MALFORMED, reason("https://"))
        assertEquals(InvalidServerAddressException.Reason.MALFORMED, reason("two words.example"))
        assertEquals(InvalidServerAddressException.Reason.MALFORMED, reason("user@kaiku.pmind.de"))
        assertEquals(InvalidServerAddressException.Reason.MALFORMED, reason("kaiku.pmind.de:abc"))
        assertEquals(InvalidServerAddressException.Reason.MALFORMED, reason("/just/a/path"))
    }
}
