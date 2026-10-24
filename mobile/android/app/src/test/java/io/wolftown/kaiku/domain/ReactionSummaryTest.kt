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
}
