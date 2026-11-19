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
    fun `sums unread, never negative, and skips conversations without messages for the newest`() {
        val s = chatsSummary(
            listOf(
                DmConversation(id = "new", unreadCount = -1),
                DmConversation(id = "a", unreadCount = 3, lastMessage = LastMessagePreview("1", "x")),
                DmConversation(id = "b", unreadCount = 1, lastMessage = LastMessagePreview("2", "y")),
            )
        )
        assertEquals(4, s.unread)
        assertEquals("a", s.newest?.id)
        assertNull(chatsSummary(listOf(DmConversation(id = "new"))).newest)
    }

    @Test
    fun `with unread messages the row shows the newest conversation that has them`() {
        // Anna's message is unread; then the user wrote to Bert (newest, read).
        val s = chatsSummary(
            listOf(
                DmConversation(id = "bert", unreadCount = 0, lastMessage = LastMessagePreview("2", "hi")),
                DmConversation(id = "anna", unreadCount = 1, lastMessage = LastMessagePreview("1", "ok")),
            )
        )
        assertEquals(1, s.unread)
        assertEquals("anna", s.newest?.id)
    }
}
