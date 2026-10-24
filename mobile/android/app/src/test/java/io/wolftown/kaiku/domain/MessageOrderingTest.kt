package io.wolftown.kaiku.domain

import io.wolftown.kaiku.domain.model.Message
import io.wolftown.kaiku.domain.model.User
import org.junit.Assert.assertEquals
import org.junit.Test

class MessageOrderingTest {

    private val author = User(id = "user-1", username = "alice", displayName = "Alice")

    private fun msg(id: String, createdAt: String, content: String = id) =
        Message(id = id, channelId = "ch-1", author = author, content = content, createdAt = createdAt)

    @Test
    fun `merge orders newest-first server page ascending`() {
        val serverPage = listOf(
            msg("newer", "2026-03-12T10:01:00Z"),
            msg("older", "2026-03-12T10:00:00.123456Z"),
        )

        val result = MessageOrdering.merge(emptyList(), serverPage)

        assertEquals(listOf("older", "newer"), result.map { it.id })
    }

    @Test
    fun `orders by instant not string`() {
        // As strings ".5Z" > ".123456Z" > ":01Z" is wrong; as instants
        // 00.123456 < 00.5 < 01.
        val incoming = listOf(
            msg("c", "2026-03-12T10:00:01Z"),
            msg("b", "2026-03-12T10:00:00.5Z"),
            msg("a", "2026-03-12T10:00:00.123456Z"),
        )

        val result = MessageOrdering.merge(emptyList(), incoming)

        assertEquals(listOf("a", "b", "c"), result.map { it.id })
    }

    @Test
    fun `merge dedupes by id and incoming wins`() {
        val existing = listOf(
            msg("m1", "2026-03-12T10:00:00Z", content = "old copy"),
            msg("m2", "2026-03-12T10:01:00Z"),
        )
        val incoming = listOf(
            msg("m1", "2026-03-12T10:00:00Z", content = "new copy"),
            msg("m0", "2026-03-12T09:59:00Z"),
        )

        val result = MessageOrdering.merge(existing, incoming)

        assertEquals(listOf("m0", "m1", "m2"), result.map { it.id })
        assertEquals("new copy", result.single { it.id == "m1" }.content)
    }

    @Test
    fun `unparseable createdAt sorts last`() {
        val incoming = listOf(
            msg("broken", "not-a-date"),
            msg("empty", ""),
            msg("ok", "2026-03-12T10:00:00Z"),
        )

        val result = MessageOrdering.merge(emptyList(), incoming)

        // Valid timestamps first; unparseable ones after, tie-broken by id.
        assertEquals(listOf("ok", "broken", "empty"), result.map { it.id })
    }

    @Test
    fun `equal timestamps are ordered by id`() {
        val incoming = listOf(
            msg("b", "2026-03-12T10:00:00Z"),
            msg("a", "2026-03-12T10:00:00Z"),
        )

        assertEquals(listOf("a", "b"), MessageOrdering.merge(emptyList(), incoming).map { it.id })
    }
}
