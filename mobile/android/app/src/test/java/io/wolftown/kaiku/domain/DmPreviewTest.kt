package io.wolftown.kaiku.domain

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
        assertEquals(PreviewBody.SomeAttachment, previewBody(LastMessagePreview("m", "")))
        assertEquals(PreviewBody.Encrypted, previewBody(LastMessagePreview("m", "AAAA", encrypted = true)))
    }

    @Test
    fun `list time follows the phone's 24-hour setting`() {
        val zone = ZoneId.of("Europe/Berlin")
        val now = Instant.parse("2026-11-19T15:00:00Z")
        assertEquals("14:05", formatListTime("2026-11-19T13:05:00Z", zone, now, Locale.US, use24Hour = true))
        val twelve = formatListTime("2026-11-19T13:05:00Z", zone, now, Locale.GERMANY, use24Hour = false)
        assertEquals(true, twelve.startsWith("2:05"))
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

    @Test
    fun `an encrypted message from the server list is recognised by its envelope`() {
        assertEquals(PreviewBody.Encrypted, previewBody(LastMessagePreview("m", """{"sender_key":"k","recipients":{"u":{"d":{"message_type":0,"ciphertext":"x"}}}}""")))
        assertEquals(PreviewBody.Encrypted, previewBody(LastMessagePreview("m", """{"sender_key":"k","room_id":"r","megolm_ciphertext":"x"}""")))
        // Text that merely looks like JSON stays text.
        assertEquals(PreviewBody.Text("""{"a":1}"""), previewBody(LastMessagePreview("m", """{"a":1}""")))
        assertEquals(PreviewBody.Text("{ hello"), previewBody(LastMessagePreview("m", "{ hello")))
    }

    @Test
    fun `a message without text previews as an attachment from either source`() {
        assertEquals(PreviewBody.SomeAttachment, previewBody(LastMessagePreview("m", "")))
    }
}
