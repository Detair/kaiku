package io.wolftown.kaiku.push

import io.wolftown.kaiku.R
import io.wolftown.kaiku.domain.model.Attachment
import io.wolftown.kaiku.domain.model.Message
import io.wolftown.kaiku.domain.model.User
import org.junit.Assert.assertEquals
import org.junit.Test

class NotificationTextTest {

    private fun message(content: String, encrypted: Boolean = false, attachments: List<Attachment> = emptyList()) =
        Message(
            id = "m1",
            channelId = "c1",
            author = User(id = "u1", username = "alice", displayName = "Alice"),
            content = content,
            encrypted = encrypted,
            attachments = attachments,
        )

    private fun attachment(mime: String) =
        Attachment(id = "a1", filename = "f", mimeType = mime, size = 1, url = "/x")

    @Test
    fun `encrypted message uses placeholder`() {
        assertEquals(LineText.Res(R.string.notif_encrypted_message), NotificationText.forMessage(message("ciphertext", encrypted = true)))
    }

    @Test
    fun `photo only message says photo`() {
        assertEquals(LineText.Res(R.string.notif_sent_photo), NotificationText.forMessage(message(" ", attachments = listOf(attachment("image/png")))))
    }

    @Test
    fun `file only message says file`() {
        assertEquals(LineText.Res(R.string.notif_sent_file), NotificationText.forMessage(message("", attachments = listOf(attachment("application/pdf")))))
    }

    @Test
    fun `long text is truncated to 300 chars`() {
        val text = NotificationText.forMessage(message("x".repeat(500)))
        assertEquals(LineText.Raw("x".repeat(300)), text)
    }

    @Test
    fun `text with attachment shows the text`() {
        assertEquals(LineText.Raw("look"), NotificationText.forMessage(message("look", attachments = listOf(attachment("image/png")))))
    }
}
