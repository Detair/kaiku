package io.wolftown.kaiku.domain

import io.wolftown.kaiku.domain.model.DmConversation
import io.wolftown.kaiku.domain.model.LastMessagePreview
import java.time.Instant
import java.time.OffsetDateTime
import java.time.ZoneId
import java.time.format.DateTimeFormatter
import java.time.format.FormatStyle
import java.util.Locale

/** The body part of a chat list preview. */
sealed interface PreviewBody {
    data class Text(val text: String) : PreviewBody

    /** A message without text: "Attachment", the same live and from the server's list (which has no attachment kind). */
    data object SomeAttachment : PreviewBody
    data object Encrypted : PreviewBody
}

/** Who wrote the last message, as the chat list names them. */
sealed interface PreviewSender {
    /** The signed-in user ("You: …"). */
    data object Own : PreviewSender
    data class Named(val name: String) : PreviewSender

    /** Not shown: a 1:1 conversation's other person (the row title already names them) or unknown. */
    data object Hidden : PreviewSender
}

fun previewBody(last: LastMessagePreview): PreviewBody = when {
    last.encrypted || isEncryptedEnvelope(last.content) -> PreviewBody.Encrypted
    last.content.isNotBlank() -> PreviewBody.Text(last.content.lineSequence().first { it.isNotBlank() }.trim())
    else -> PreviewBody.SomeAttachment
}

private val envelopeJson = kotlinx.serialization.json.Json { ignoreUnknownKeys = true }

/**
 * Whether [content] is an end-to-end encrypted message as clients send it:
 * a JSON object with `sender_key` and either `recipients` (one-to-one) or
 * `megolm_ciphertext` (group). The server's conversation list has no
 * "encrypted" flag, so the envelope is recognised by its shape, as the web
 * client does, and its ciphertext is never shown.
 */
fun isEncryptedEnvelope(content: String): Boolean {
    val text = content.trim()
    if (!text.startsWith("{") || !text.endsWith("}")) return false
    val obj = try {
        envelopeJson.parseToJsonElement(text) as? kotlinx.serialization.json.JsonObject
    } catch (_: Exception) {
        null
    } ?: return false
    return "sender_key" in obj && ("recipients" in obj || "megolm_ciphertext" in obj)
}

/**
 * The sender of [last] in [dm] seen by [me]: "You" for own messages; in a
 * group, the participant's display name (login name only if the participant
 * is unknown); in a 1:1 conversation nobody, since the title names them.
 */
fun previewSender(dm: DmConversation, last: LastMessagePreview, me: String?): PreviewSender {
    val sender = last.userId ?: return PreviewSender.Hidden
    if (sender == me) return PreviewSender.Own
    val group = dm.participants.count { it.userId != me } > 1
    if (!group) return PreviewSender.Hidden
    val name = dm.participants.find { it.userId == sender }?.displayName?.takeIf { it.isNotBlank() } ?: last.username
    return name?.let { PreviewSender.Named(it) } ?: PreviewSender.Hidden
}

/** Parses a server timestamp; null if blank or not ISO 8601 with offset. */
fun parseServerTime(iso: String?): Instant? {
    if (iso.isNullOrBlank()) return null
    return try {
        OffsetDateTime.parse(iso).toInstant()
    } catch (_: java.time.DateTimeException) {
        null
    }
}

/** When [dm] was last active: its last message, else its creation. */
fun lastActivity(dm: DmConversation): Instant? = parseServerTime(dm.lastMessage?.createdAt) ?: parseServerTime(dm.createdAt)

/** Most recently active first; conversations without a known time last; equal times keep their order. */
fun sortByActivity(conversations: List<DmConversation>): List<DmConversation> =
    conversations.sortedByDescending { lastActivity(it) ?: Instant.MIN }

/**
 * The chat list's time: the short time for today (in [zone]), the short date
 * otherwise, both in [locale]'s format. Empty for a missing time.
 */
fun formatListTime(
    iso: String?,
    zone: ZoneId,
    now: Instant,
    locale: Locale = Locale.getDefault(),
    /** The phone's 24-hour setting; null: the locale's own short time. */
    use24Hour: Boolean? = null,
): String {
    val moment = parseServerTime(iso)?.atZone(zone) ?: return ""
    val formatter = if (moment.toLocalDate() == now.atZone(zone).toLocalDate()) {
        when (use24Hour) {
            true -> DateTimeFormatter.ofPattern("HH:mm")
            false -> DateTimeFormatter.ofPattern("h:mm a")
            null -> DateTimeFormatter.ofLocalizedTime(FormatStyle.SHORT)
        }
    } else {
        DateTimeFormatter.ofLocalizedDate(FormatStyle.SHORT)
    }
    return formatter.withLocale(locale).format(moment)
}
