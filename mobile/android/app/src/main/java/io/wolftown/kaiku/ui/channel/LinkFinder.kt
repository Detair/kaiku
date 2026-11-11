package io.wolftown.kaiku.ui.channel

/** A web link found in message text: where it is, and the address to open. */
data class FoundLink(val start: Int, val end: Int, val url: String)

/**
 * Finds web links in plain message text (AND-B-5). Only `http://`, `https://`
 * and `www.` addresses count; `www.` opens as https. Other schemes
 * (`javascript:`, `intent:`, `file:` …) are never turned into links.
 * Trailing punctuation of the sentence and an unmatched closing bracket are
 * not part of the link.
 */
object LinkFinder {
    private val CANDIDATE = Regex("""(?i)\b(?:https?://|www\.)[^\s<>"]+""")
    private const val TRAILING = ".,;:!?'\"*_~"

    fun find(text: String): List<FoundLink> = CANDIDATE.findAll(text).mapNotNull { match ->
        var end = match.range.last + 1
        val start = match.range.first
        while (end > start) {
            val c = text[end - 1]
            val drop = when {
                c in TRAILING -> true
                c == ')' -> text.substring(start, end).count { it == '(' } < text.substring(start, end).count { it == ')' }
                c == ']' -> text.substring(start, end).count { it == '[' } < text.substring(start, end).count { it == ']' }
                else -> false
            }
            if (!drop) break
            end--
        }
        val raw = text.substring(start, end)
        val url = if (raw.startsWith("www.", ignoreCase = true)) "https://$raw" else raw
        // A bare scheme or "www." alone is not a link.
        val host = url.substringAfter("://").substringBefore('/')
        if (host.isEmpty() || host.equals("www.", ignoreCase = true) || !host.contains('.')) null
        else FoundLink(start, end, url)
    }.toList()
}
