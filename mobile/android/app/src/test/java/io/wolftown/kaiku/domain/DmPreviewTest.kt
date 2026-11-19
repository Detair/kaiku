package io.wolftown.kaiku.domain

import io.wolftown.kaiku.domain.model.Attachment
import io.wolftown.kaiku.domain.model.DmConversation
import io.wolftown.kaiku.domain.model.DmParticipant
import io.wolftown.kaiku.domain.model.LastMessagePreview
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test
import java.time.Instant
import java.time.ZoneId
import java.util.Locale

class DmPreviewTest {
    private fun p(id: String, display: String) = DmParticipant(userId = id, username = id, displayName = display)
    private fun att(mime: String) = Attachment("a", "f", mime, 1, "/u")
    private val oneToOne = DmConversation(id = "d", participants = listOf(p("me", "Fabian"), p("anna", "Anna")))
    private val group = DmConversation(id = "g", participants = listOf(p("me", "Fabian"), p("anna", "Anna"), p("bert", "Bert")))

    @Test
    fun `own messages read You, groups name the display name, one-to-one names nobody`() {
        val own = LastMessagePreview("m", "ok", userId = "me", username = "fabian")
        val anna = LastMessagePreview("m", "ok", userId = "anna", username = "anna")
        assertEquals(PreviewSender.Own, previewSender(oneToOne, own, "me"))
        assertEquals(PreviewSender.Own, previewSender(group, own, "me"))
        assertEquals(PreviewSender.Named("Anna"), previewSender(group, anna, "me"))
        assertEquals(PreviewSender.Hidden, previewSender(oneToOne, anna, "me"))
    }

    @Test
    fun `a sender who left a group is named by login name`() {
        val gone = LastMessagePreview("m", "ok", userId = "carl", username = "carl")
        assertEquals(PreviewSender.Named("carl"), previewSender(group, gone, "me"))
    }

    @Test
    fun `bodies - text, attachment kinds, unknown attachment, encrypted`() {
        assertEquals(PreviewBody.Text("first line"), previewBody(LastMessagePreview("m", "\nfirst line\nsecond")))
        assertEquals(PreviewBody.Attached(AttachmentKind.PHOTO), previewBody(LastMessagePreview("m", "", attachmentKind = AttachmentKind.PHOTO)))
        assertEquals(PreviewBody.SomeAttachment, previewBody(LastMessagePreview("m", "")))
        assertEquals(PreviewBody.Encrypted, previewBody(LastMessagePreview("m", "AAAA", encrypted = true)))
    }

    @Test
    fun `attachment kind from mime type`() {
        assertEquals(AttachmentKind.PHOTO, attachmentKindOf(listOf(att("image/jpeg"), att("video/mp4"))))
        assertEquals(AttachmentKind.VIDEO, attachmentKindOf(listOf(att("VIDEO/mp4"))))
        assertEquals(AttachmentKind.AUDIO, attachmentKindOf(listOf(att("audio/ogg"))))
        assertEquals(AttachmentKind.FILE, attachmentKindOf(listOf(att("application/pdf"))))
        assertNull(attachmentKindOf(emptyList()))
    }

    @Test
    fun `list time - today the time, earlier the date, in the user's locale and zone`() {
        val zone = ZoneId.of("Europe/Berlin")
        val now = Instant.parse("2026-11-19T15:00:00Z")
        assertEquals("10:42", formatListTime("2026-11-19T09:42:00Z", zone, now, Locale.GERMANY))
        assertEquals("18.11.26", formatListTime("2026-11-18T09:42:00Z", zone, now, Locale.GERMANY))
        // 23:30 UTC on the 18th is already the 19th in Berlin.
        assertEquals("00:30", formatListTime("2026-11-18T23:30:00+00:00", zone, now, Locale.GERMANY))
        assertEquals("", formatListTime("", zone, now, Locale.GERMANY))
        assertEquals("", formatListTime("garbage", zone, now, Locale.GERMANY))
    }

    @Test
    fun `sorted by last message, else creation, unknown last, ties keep order`() {
        val list = listOf(
            DmConversation(id = "none"),
            DmConversation(id = "old", lastMessage = LastMessagePreview("1", "a", createdAt = "2026-11-01T10:00:00Z")),
            DmConversation(id = "new-empty", createdAt = "2026-11-19T10:00:00Z"),
            DmConversation(id = "recent", lastMessage = LastMessagePreview("2", "b", createdAt = "2026-11-18T10:00:00.123456+00:00")),
            DmConversation(id = "none2"),
        )
        assertEquals(listOf("new-empty", "recent", "old", "none", "none2"), sortByActivity(list).map { it.id })
    }
}
