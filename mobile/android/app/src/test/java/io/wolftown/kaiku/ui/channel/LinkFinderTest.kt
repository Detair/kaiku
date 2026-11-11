package io.wolftown.kaiku.ui.channel

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class LinkFinderTest {

    private fun urls(text: String) = LinkFinder.find(text).map { it.url }

    @Test
    fun `finds https and www links`() {
        assertEquals(
            listOf("https://photos.example.org/album/1", "https://www.bahn.de"),
            urls("Album: https://photos.example.org/album/1 and www.bahn.de"),
        )
    }

    @Test
    fun `sentence punctuation is not part of the link`() {
        assertEquals(listOf("https://example.org/a"), urls("See https://example.org/a."))
        assertEquals(listOf("https://example.org/a"), urls("(see https://example.org/a)"))
        assertEquals(listOf("https://en.wikipedia.org/wiki/Foo_(bar)"), urls("https://en.wikipedia.org/wiki/Foo_(bar)!"))
    }

    @Test
    fun `range covers exactly the link text`() {
        val text = "go to https://example.org, now"
        val link = LinkFinder.find(text).single()
        assertEquals("https://example.org", text.substring(link.start, link.end))
    }

    @Test
    fun `other schemes are never links`() {
        assertTrue(urls("javascript:alert(1) intent://x file:///etc/passwd").isEmpty())
        assertTrue(urls("ftp://example.org").isEmpty())
    }

    @Test
    fun `bare scheme or host without dot is not a link`() {
        assertTrue(urls("https:// and www. and http://localhost").isEmpty())
    }
}
