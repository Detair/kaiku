package io.wolftown.kaiku.domain

import java.time.Instant
import java.time.OffsetDateTime
import java.time.ZoneId
import java.time.format.DateTimeFormatter
import java.time.format.FormatStyle
import java.util.Locale

/**
 * Formats a server timestamp ([iso], ISO 8601 with offset, usually UTC `Z`)
 * in [zone]: today's messages show the localized short time, older ones the
 * localized short date and time. "Today" is judged in [zone] against [now].
 * Returns an empty string for blank or unparseable input.
 */
fun formatMessageTime(
    iso: String,
    zone: ZoneId,
    now: Instant,
    locale: Locale = Locale.getDefault(),
): String {
    if (iso.isBlank()) return ""
    val moment = try {
        OffsetDateTime.parse(iso).toInstant().atZone(zone)
    } catch (_: java.time.DateTimeException) {
        return ""
    }
    val sameDay = moment.toLocalDate() == now.atZone(zone).toLocalDate()
    val formatter = if (sameDay) {
        DateTimeFormatter.ofLocalizedTime(FormatStyle.SHORT)
    } else {
        DateTimeFormatter.ofLocalizedDateTime(FormatStyle.SHORT)
    }
    return formatter.withLocale(locale).format(moment)
}
