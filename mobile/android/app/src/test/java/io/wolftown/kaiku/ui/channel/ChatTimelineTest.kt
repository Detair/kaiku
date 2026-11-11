package io.wolftown.kaiku.ui.channel

import io.wolftown.kaiku.domain.model.Message
import io.wolftown.kaiku.domain.model.User
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import java.time.LocalDate
import java.time.ZoneId

class ChatTimelineTest {

    private val berlin = ZoneId.of("Europe/Berlin")
    private val anna = User(id = "anna", username = "anna", displayName = "Anna")
    private val me = User(id = "me", username = "me", displayName = "Me")

    private fun msg(id: String, at: String, author: User = anna) =
        Message(id = id, channelId = "c", author = author, content = id, createdAt = at)

    @Test
    fun `a separator starts every local day`() {
        val rows = buildTimeline(
            listOf(
                msg("a", "2026-11-10T21:00:00Z"), // 22:00 in Berlin, 10 Nov
                msg("b", "2026-11-10T23:30:00Z"), // 00:30 in Berlin, 11 Nov
                msg("c", "2026-11-11T08:00:00Z"),
            ),
            berlin,
        )
        assertEquals(
            listOf("day-2026-11-10", "a", "day-2026-11-11", "b", "c"),
            rows.map { it.key },
        )
    }

    @Test
    fun `unreadable time joins the previous day`() {
        val rows = buildTimeline(listOf(msg("a", "2026-11-10T10:00:00Z"), msg("b", "")), berlin)
        assertEquals(listOf("day-2026-11-10", "a", "b"), rows.map { it.key })
    }

    @Test
    fun `labels today and yesterday`() {
        val today = LocalDate.of(2026, 11, 12)
        assertEquals(DayLabel.Today, dayLabel(today, today))
        assertEquals(DayLabel.Yesterday, dayLabel(today.minusDays(1), today))
        assertEquals(DayLabel.Date(today.minusDays(2)), dayLabel(today.minusDays(2), today))
    }

    @Test
    fun `new incoming lists only messages of others after the last seen one`() {
        val list = listOf(msg("a", ""), msg("b", "", me), msg("c", ""), msg("d", ""))
        assertEquals(listOf("c", "d"), newIncoming("a", list, "me").map { it.id })
        assertTrue(newIncoming(null, list, "me").isEmpty())
        assertTrue(newIncoming("gone", list, "me").isEmpty())
    }
}
