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

/**
 * Keeps the end of the chat in view while the user is there. Whether the
 * user is "at the end" changes only while the list scrolls; when the content
 * below grows or the view gets smaller without a scroll (a message turns into
 * "not sent" with Retry and Delete, the keyboard or a snackbar opens),
 * [shouldReturnToEnd] asks the screen to scroll back to the end.
 */
internal class EndFollower {
    private var following = true

    /** Every change of the list's scroll state. */
    fun onScrollState(scrolling: Boolean, canScrollForward: Boolean) {
        if (scrolling) following = !canScrollForward
    }

    /** The screen moved the list away from the end itself (for example to keep older messages in place). */
    fun leftEnd() {
        following = false
    }

    fun shouldReturnToEnd(scrolling: Boolean, canScrollForward: Boolean): Boolean =
        following && !scrolling && canScrollForward
}
