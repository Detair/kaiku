package io.wolftown.kaiku.domain

import io.wolftown.kaiku.domain.model.DmConversation
import io.wolftown.kaiku.domain.model.LastMessagePreview
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class ChatsSummaryTest {
    @Test
    fun `no chats`() {
        assertEquals(ChatsSummary(0, null), chatsSummary(emptyList()))
    }

    @Test
    fun `sums unread over all conversations, never negative`() {
        val s = chatsSummary(
            listOf(
                DmConversation(id = "new", unreadCount = -1),
                DmConversation(id = "a", unreadCount = 3, lastMessage = LastMessagePreview("1", "x")),
                DmConversation(id = "b", unreadCount = 1, lastMessage = LastMessagePreview("2", "y")),
            )
        )
        assertEquals(4, s.unread)
        // The preview is the conversation the chat list shows first.
        assertEquals("new", s.newest?.id)
    }

    @Test
    fun `the row previews the conversation with the latest activity, like the chat list`() {
        // Anna's message is unread; then the user wrote to Bert: the list shows Bert first.
        val s = chatsSummary(
            listOf(
                DmConversation(id = "bert", unreadCount = 0, lastMessage = LastMessagePreview("2", "hi")),
                DmConversation(id = "anna", unreadCount = 1, lastMessage = LastMessagePreview("1", "ok")),
            )
        )
        // The number still counts Anna's unread message.
        assertEquals(1, s.unread)
        assertEquals("bert", s.newest?.id)
    }

    @Test
    fun `without unread messages the newest conversation is shown even without messages yet`() {
        val s = chatsSummary(
            listOf(
                DmConversation(id = "new"),
                DmConversation(id = "a", lastMessage = LastMessagePreview("1", "x")),
            )
        )
        assertEquals("new", s.newest?.id)
        assertEquals("new", chatsSummary(listOf(DmConversation(id = "new"))).newest?.id)
    }
}
