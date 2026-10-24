package io.wolftown.kaiku.domain

import io.wolftown.kaiku.domain.model.Message
import java.time.Instant
import java.time.format.DateTimeParseException

/**
 * Single source of truth for the order of messages in a channel list.
 *
 * The server returns history pages newest first, while WebSocket events and
 * sent messages arrive one at a time. Every path that changes a channel's
 * message list goes through [merge] so the list is always oldest-to-newest
 * and free of duplicates.
 */
object MessageOrdering {

    /**
     * Returns the union of [existing] and [incoming] by message id (the
     * incoming copy wins), sorted ascending by `createdAt` as an [Instant]
     * and then by id. Messages whose `createdAt` cannot be parsed sort last.
     */
    fun merge(existing: List<Message>, incoming: List<Message>): List<Message> {
        val byId = LinkedHashMap<String, Message>(existing.size + incoming.size)
        for (message in existing) byId[message.id] = message
        for (message in incoming) byId[message.id] = message
        return byId.values
            .map { it to parseInstant(it.createdAt) }
            .sortedWith(ORDER)
            .map { it.first }
    }

    private val ORDER: Comparator<Pair<Message, Instant?>> =
        compareBy<Pair<Message, Instant?>, Instant?>(nullsLast<Instant>()) { it.second }
            .thenBy { it.first.id }

    private fun parseInstant(value: String): Instant? =
        try {
            Instant.parse(value)
        } catch (e: DateTimeParseException) {
            null
        }
}
