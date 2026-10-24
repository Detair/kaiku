package io.wolftown.kaiku.domain

import io.wolftown.kaiku.domain.model.Message
import io.wolftown.kaiku.domain.model.User
import org.junit.Assert.assertEquals
import org.junit.Test

class MessageDisplayTest {

    private val author = User(id = "u-1", username = "alice", displayName = "Alice")

    private fun message(content: String, encrypted: Boolean) = Message(
        id = "m-1",
        channelId = "ch-1",
        author = author,
        content = content,
        encrypted = encrypted,
    )

    @Test
    fun `encrypted message renders placeholder`() {
        assertEquals(
            MessageBody.EncryptedPlaceholder,
            messageDisplayContent(message(content = "eyJjaXBoZXJ0ZXh0Ijo...", encrypted = true))
        )
    }

    @Test
    fun `plain message renders text`() {
        assertEquals(
            MessageBody.Text("**hello**"),
            messageDisplayContent(message(content = "**hello**", encrypted = false))
        )
    }
}
