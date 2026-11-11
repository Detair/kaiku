package io.wolftown.kaiku.ui.channel

import io.wolftown.kaiku.domain.model.Message
import io.wolftown.kaiku.domain.model.User
import org.junit.Assert.assertEquals
import org.junit.Test

/** AND-B-11 / AND-B-5 / AND-B-22: the actions TalkBack and the menu offer, and the reply quote. */
class MessageActionsTest {

    @Test
    fun `own message offers react copy edit delete`() {
        assertEquals(
            listOf(MessageAction.REACT, MessageAction.COPY, MessageAction.EDIT, MessageAction.DELETE),
            messageActions(isOwnMessage = true, hasText = true),
        )
    }

    @Test
    fun `other people's message offers react and copy`() {
        assertEquals(listOf(MessageAction.REACT, MessageAction.COPY), messageActions(false, true))
    }

    @Test
    fun `photo without text has nothing to copy or edit`() {
        assertEquals(listOf(MessageAction.REACT, MessageAction.DELETE), messageActions(true, false))
    }

    @Test
    fun `reply quote names the author and shortens the text`() {
        val long = "word ".repeat(40)
        val original = Message(
            id = "m", channelId = "c", author = User("u", "anna", "Anna"),
            content = "**Hi**\n$long",
        )
        val quote = replyQuote(original)
        assertEquals("Anna", quote.author)
        assertEquals(80, quote.snippet.length)
        assertEquals("Hi word", quote.snippet.take(7))
    }

    @Test
    fun `reply quote of an encrypted message has no text`() {
        val original = Message(id = "m", channelId = "c", author = User("u", "a", "Anna"), content = "xyz", encrypted = true)
        assertEquals("", replyQuote(original).snippet)
    }

    @Test
    fun `copy takes the text without markdown markers`() {
        assertEquals("bold and code", plainMessageText("**bold** and `code`"))
    }
}
