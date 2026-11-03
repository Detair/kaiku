package io.wolftown.kaiku.domain

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class KaikuPayloadsTest {

    private val token = "oKGio6SlpqeoqaqrrK2ur7CxsrO0tba3uLm6u7y9vr8"

    @Test fun `login payload parses origin and token`() {
        assertEquals(
            ScanPayload.Login("https://chat.example.com", token),
            KaikuPayloads.parse("kaiku://login/v1?o=https%3A%2F%2Fchat.example.com&t=$token"),
        )
    }

    @Test fun `setup url parses the fragment token`() {
        assertEquals(
            ScanPayload.Setup("https://chat.example.com", token),
            KaikuPayloads.parse("https://chat.example.com/setup#v=1&t=$token"),
        )
        assertEquals(
            ScanPayload.Setup("https://chat.example.com", token),
            KaikuPayloads.parse("kaiku://setup/v1?o=https%3A%2F%2Fchat.example.com&t=$token"),
        )
        assertEquals(
            ScanPayload.Setup("http://localhost:8080", token),
            KaikuPayloads.parse("http://localhost:8080/setup#v=1&t=$token"),
        )
        // Plain http is development only.
        assertEquals(ScanPayload.NotKaiku, KaikuPayloads.parse("http://chat.example.com/setup#v=1&t=$token"))
    }

    @Test fun `wrong token length is not kaiku`() {
        assertEquals(ScanPayload.NotKaiku, KaikuPayloads.parse("kaiku://login/v1?o=https%3A%2F%2Fchat.example.com&t=${token}A"))
        assertEquals(ScanPayload.NotKaiku, KaikuPayloads.parse("https://chat.example.com/setup#v=1&t=${token.dropLast(1)}"))
        assertEquals(ScanPayload.NotKaiku, KaikuPayloads.parse("kaiku://login/v1?o=https%3A%2F%2Fchat.example.com&t=${token.dropLast(1)}+"))
        assertEquals(ScanPayload.NotKaiku, KaikuPayloads.parse("https://example.com/some/page"))
        assertEquals(ScanPayload.NotKaiku, KaikuPayloads.parse("WIFI:S:home;T:WPA;P:secret;;"))
    }

    @Test fun `v2 needs update`() {
        assertEquals(ScanPayload.NeedsUpdate, KaikuPayloads.parse("kaiku://login/v2?o=https%3A%2F%2Fchat.example.com&t=$token"))
        assertEquals(ScanPayload.NeedsUpdate, KaikuPayloads.parse("https://chat.example.com/setup#v=2&t=$token"))
        assertEquals(ScanPayload.NeedsUpdate, KaikuPayloads.parse("kaiku://setup/v7?o=x&t=y"))
    }

    @Test fun `old qr login code asks to update the pc`() {
        assertEquals(
            ScanPayload.OldKaikuCode,
            KaikuPayloads.parse("kaiku://qr/login?server=https%3A%2F%2Fchat.example.com&token=abc"),
        )
    }

    @Test fun `default port and case are canonicalised`() {
        assertEquals("https://chat.example.com", KaikuPayloads.canonicalOrigin("HTTPS://Chat.Example.COM:443/some/path?q=1"))
        assertEquals("https://chat.example.com:8443", KaikuPayloads.canonicalOrigin("https://chat.example.com:8443/"))
        assertEquals("http://localhost", KaikuPayloads.canonicalOrigin("http://localhost:80"))
        assertNull(KaikuPayloads.canonicalOrigin("ftp://chat.example.com"))
        assertNull(KaikuPayloads.canonicalOrigin("not a url"))
        assertEquals(
            ScanPayload.Login("https://chat.example.com", token),
            KaikuPayloads.parse("kaiku://login/v1?o=https%3A%2F%2FCHAT.example.com%3A443&t=$token"),
        )
    }

    @Test fun `pin keeps the own origin and refuses another`() {
        val login = ScanPayload.Login("https://chat.example.com", token)
        assertEquals(login, KaikuPayloads.pin(login, "https://chat.example.com"))
        assertEquals(login, KaikuPayloads.pin(login, null))
        assertEquals(ScanPayload.OtherServer("https://evil.example.com"), KaikuPayloads.pin(ScanPayload.Login("https://evil.example.com", token), "https://chat.example.com"))
        assertEquals(ScanPayload.OtherServer("https://chat.example.com:8443"), KaikuPayloads.pin(ScanPayload.Setup("https://chat.example.com:8443", token), "https://chat.example.com"))
    }
}
