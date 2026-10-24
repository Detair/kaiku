package io.wolftown.kaiku.domain

import io.wolftown.kaiku.domain.model.Message
import io.wolftown.kaiku.domain.model.Reaction
import io.wolftown.kaiku.domain.model.User
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class ReactionSummaryTest {

    private val author = User(id = "u1", username = "a", displayName = "A")

    private fun message(reactions: List<Reaction>?) = Message(
        id = "m1", channelId = "c1", author = author, content = "hi", reactions = reactions
    )

    @Test
    fun `reaction summaries count and mark mine`() {
        val msg = message(
            listOf(
                Reaction("👍", count = 2, me = false, users = listOf("u1", "u2")),
                Reaction("🎉", count = 1, me = false, users = listOf("u3")),
                Reaction("🔥", count = 3, me = true, users = emptyList()),
            )
        )

        assertEquals(
            listOf(
                ReactionSummary("👍", 2, mine = true),
                ReactionSummary("🎉", 1, mine = false),
                ReactionSummary("🔥", 3, mine = true),
            ),
            reactionSummaries(msg, currentUserId = "u1")
        )
    }

    @Test
    fun `no reactions gives empty list`() {
        assertTrue(reactionSummaries(message(null), "u1").isEmpty())
    }

    @Test
    fun `add is idempotent and remove drops empty reactions`() {
        val added = null.withReactionAdded("👍", "u2", isMe = false)
            .withReactionAdded("👍", "u2", isMe = false)
        assertEquals(listOf(Reaction("👍", 1, false, listOf("u2"))), added)

        val removed = added.withReactionRemoved("👍", "u2", isMe = false)
        assertTrue(removed.isEmpty())
    }

    @Test
    fun `own reaction sets me and raises count`() {
        val added = listOf(Reaction("👍", 1, false, listOf("u2")))
            .withReactionAdded("👍", "u1", isMe = true)

        assertEquals(Reaction("👍", 2, true, listOf("u2", "u1")), added.single())
        assertEquals(true, reactionSummaries(message(added), null).single().mine)
    }

    @Test
    fun `removing own reaction clears me and lowers count`() {
        val removed = listOf(Reaction("👍", 2, true, listOf("u2", "u1")))
            .withReactionRemoved("👍", "u1", isMe = true)

        assertEquals(Reaction("👍", 1, false, listOf("u2")), removed.single())
    }

    @Test
    fun `removing an unknown reaction changes nothing`() {
        val list = listOf(Reaction("👍", 1, false, listOf("u2")))

        assertEquals(list, list.withReactionRemoved("👍", "u3", isMe = false))
        assertEquals(list, list.withReactionRemoved("🎉", "u2", isMe = false))
    }
}
