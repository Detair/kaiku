package io.wolftown.kaiku.domain

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import java.time.Instant
import java.time.ZoneId
import java.util.Locale

class MessageTimeTest {

    private val berlin = ZoneId.of("Europe/Berlin")

    // Locale is explicit so the result does not depend on the JVM default.
    private fun format(iso: String, now: String) =
        formatMessageTime(iso, berlin, Instant.parse(now), Locale.GERMANY)

    @Test
    fun `utc time shown in local zone`() {
        // 18:30 UTC is 19:30 in Berlin in November (UTC+1).
        assertEquals("19:30", format("2026-11-12T18:30:00Z", now = "2026-11-12T20:00:00Z"))
    }

    @Test
    fun `same day is judged in the local zone`() {
        // 23:30 UTC on the 11th is already the 12th in Berlin.
        assertEquals("00:30", format("2026-11-11T23:30:00Z", now = "2026-11-12T10:00:00Z"))
    }

    @Test
    fun `older message shows date`() {
        val text = format("2026-11-10T18:30:00Z", now = "2026-11-12T20:00:00Z")
        assertTrue(text, text.contains("19:30"))
        assertTrue(text, text.startsWith("10.11."))
        assertNotEquals("19:30", text)
    }

    @Test
    fun `blank or invalid timestamp gives empty text`() {
        assertEquals("", format("", now = "2026-11-12T20:00:00Z"))
        assertEquals("", format("not a time", now = "2026-11-12T20:00:00Z"))
    }
}
