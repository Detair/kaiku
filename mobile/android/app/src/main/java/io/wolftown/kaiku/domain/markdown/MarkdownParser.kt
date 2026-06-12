package io.wolftown.kaiku.domain.markdown

/**
 * Minimal inline-markdown parser for chat messages (Android beta).
 *
 * Matches the subset the desktop client renders for inline formatting:
 * `**bold**`, `*italic*` / `_italic_`, `~~strikethrough~~`, `` `code` ``,
 * and `||spoiler||`. Block constructs (headings, lists, fenced code) are
 * out of scope here — messages are short and inline-formatted in practice.
 *
 * Pure and dependency-free so it can be unit-tested without a Compose or
 * Android runtime. Rendering lives in the Compose layer (see MessageItem).
 *
 * Robustness: unmatched delimiters are emitted as literal text rather than
 * swallowed, so a stray `*` never eats the rest of a message.
 */
object MarkdownParser {

    /** Styling flags carried by a run of text. `code` content is literal. */
    data class Span(
        val text: String,
        val bold: Boolean = false,
        val italic: Boolean = false,
        val strikethrough: Boolean = false,
        val code: Boolean = false,
        val spoiler: Boolean = false,
    )

    private data class Delimiter(
        val marker: String,
        val literal: Boolean, // content is not re-parsed (code, spoiler)
        val apply: (Span) -> Span,
    )

    // Order matters: longer / literal markers are tried first so `**` wins
    // over `*` and `` ` `` / `||` capture literal content.
    private val delimiters = listOf(
        Delimiter("`", literal = true) { it.copy(code = true) },
        Delimiter("||", literal = true) { it.copy(spoiler = true) },
        Delimiter("**", literal = false) { it.copy(bold = true) },
        Delimiter("~~", literal = false) { it.copy(strikethrough = true) },
        Delimiter("*", literal = false) { it.copy(italic = true) },
        // NB: `_` is deliberately NOT an italic delimiter. Intraword
        // underscores (`snake_case`, `__init__`) are far more common in a
        // dev/gaming chat than `_italic_`, and matching them produces false
        // italics. Use `*italic*`. (GFM-style word-boundary `_` handling can
        // be added later if needed.)
    )

    fun parse(input: String): List<Span> {
        val out = ArrayList<Span>()
        parseInto(input, Span(""), out)
        return mergeAdjacent(out)
    }

    /** Parse `input`, applying the accumulated `base` style, into `out`. */
    private fun parseInto(input: String, base: Span, out: MutableList<Span>) {
        var i = 0
        val literal = StringBuilder()

        fun flushLiteral() {
            if (literal.isNotEmpty()) {
                out.add(base.copy(text = literal.toString()))
                literal.setLength(0)
            }
        }

        while (i < input.length) {
            val delim = delimiters.firstOrNull { input.startsWith(it.marker, i) }
            if (delim == null) {
                literal.append(input[i])
                i++
                continue
            }
            val contentStart = i + delim.marker.length
            val close = input.indexOf(delim.marker, contentStart)
            // No closing marker, or empty content → treat the marker literally.
            if (close == -1 || close == contentStart) {
                literal.append(delim.marker)
                i = contentStart
                continue
            }
            flushLiteral()
            val content = input.substring(contentStart, close)
            val styled = delim.apply(base)
            if (delim.literal) {
                out.add(styled.copy(text = content))
            } else {
                parseInto(content, styled, out)
            }
            i = close + delim.marker.length
        }
        flushLiteral()
    }

    /** Collapse consecutive spans that share identical styling. */
    private fun mergeAdjacent(spans: List<Span>): List<Span> {
        val merged = ArrayList<Span>(spans.size)
        for (s in spans) {
            if (s.text.isEmpty()) continue
            val last = merged.lastOrNull()
            if (last != null && last.copy(text = "") == s.copy(text = "")) {
                merged[merged.size - 1] = last.copy(text = last.text + s.text)
            } else {
                merged.add(s)
            }
        }
        return merged
    }
}
