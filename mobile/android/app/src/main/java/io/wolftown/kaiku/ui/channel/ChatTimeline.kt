package io.wolftown.kaiku.ui.channel

import io.wolftown.kaiku.domain.model.Message
import java.time.LocalDate
import java.time.OffsetDateTime
import java.time.ZoneId

/** One row of the chat list: a day separator or a message (AND-B-21). */
sealed interface TimelineRow {
    val key: String

    data class Day(val date: LocalDate) : TimelineRow {
        override val key: String get() = "day-$date"
    }

    data class Item(val message: Message) : TimelineRow {
        override val key: String get() = message.id
    }
}

/** How a day separator is labelled. */
sealed interface DayLabel {
    data object Today : DayLabel
    data object Yesterday : DayLabel
    data class Date(val date: LocalDate) : DayLabel
}

fun dayLabel(date: LocalDate, today: LocalDate): DayLabel = when (date) {
    today -> DayLabel.Today
    today.minusDays(1) -> DayLabel.Yesterday
    else -> DayLabel.Date(date)
}

/** The day [iso] falls on in [zone], or null when it cannot be read. */
fun messageDay(iso: String, zone: ZoneId): LocalDate? = try {
    if (iso.isBlank()) null else OffsetDateTime.parse(iso).atZoneSameInstant(zone).toLocalDate()
} catch (_: java.time.DateTimeException) {
    null
}

/**
 * Messages (oldest first) with a separator before the first message of each
 * day in [zone]. A message without a readable time joins the day before it.
 */
fun buildTimeline(messages: List<Message>, zone: ZoneId): List<TimelineRow> {
    val rows = ArrayList<TimelineRow>(messages.size + 4)
    var currentDay: LocalDate? = null
    for (message in messages) {
        val day = messageDay(message.createdAt, zone)
        if (day != null && day != currentDay) {
            rows += TimelineRow.Day(day)
            currentDay = day
        }
        rows += TimelineRow.Item(message)
    }
    return rows
}

/**
 * Messages from others that arrived after [previousLastId] (for the TalkBack
 * announcement and the "new messages" count). Empty when [previousLastId] is
 * unknown or gone (first load, reload): nothing is announced then.
 */
fun newIncoming(previousLastId: String?, messages: List<Message>, currentUserId: String?): List<Message> {
    if (previousLastId == null) return emptyList()
    val at = messages.indexOfLast { it.id == previousLastId }
    if (at < 0) return emptyList()
    return messages.subList(at + 1, messages.size).filter { it.author.id != currentUserId }
}
