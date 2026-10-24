package io.wolftown.kaiku.data.api

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class ServerUrlsTest {

    @Test
    fun `relative path is prefixed with server base`() {
        assertEquals(
            "https://k.example/api/files/a.png",
            ServerUrls.resolve("https://k.example/", "/api/files/a.png")
        )
        assertEquals(
            "https://k.example/api/files/a.png",
            ServerUrls.resolve("https://k.example", "/api/files/a.png")
        )
    }

    @Test
    fun `absolute https url is unchanged`() {
        assertEquals(
            "https://cdn.example/a.png",
            ServerUrls.resolve("https://k.example", "https://cdn.example/a.png")
        )
        assertEquals(
            "http://cdn.example/a.png",
            ServerUrls.resolve("https://k.example", "http://cdn.example/a.png")
        )
        // A missing base does not matter for absolute URLs.
        assertEquals(
            "https://cdn.example/a.png",
            ServerUrls.resolve(null, "https://cdn.example/a.png")
        )
    }

    @Test
    fun `null blank or missing base yields null`() {
        assertNull(ServerUrls.resolve("https://k.example", null))
        assertNull(ServerUrls.resolve("https://k.example", ""))
        assertNull(ServerUrls.resolve("https://k.example", "   "))
        assertNull(ServerUrls.resolve(null, "/api/files/a.png"))
        assertNull(ServerUrls.resolve("  ", "/api/files/a.png"))
    }

    @Test
    fun `other forms yield null`() {
        // Protocol-relative URLs are not resolved.
        assertNull(ServerUrls.resolve("https://k.example", "//other.example/a.png"))
        assertNull(ServerUrls.resolve("https://k.example", "api/files/a.png"))
        assertNull(ServerUrls.resolve("https://k.example", "ftp://k.example/a.png"))
        assertNull(ServerUrls.resolve("https://k.example", "javascript:alert(1)"))
    }
}
