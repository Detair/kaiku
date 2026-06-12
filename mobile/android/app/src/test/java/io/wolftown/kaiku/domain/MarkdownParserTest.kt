package io.wolftown.kaiku.domain

import io.wolftown.kaiku.domain.markdown.MarkdownParser
import io.wolftown.kaiku.domain.markdown.MarkdownParser.Span
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class MarkdownParserTest {

    @Test
    fun plainText_isASingleUnstyledSpan() {
        assertEquals(listOf(Span("hello world")), MarkdownParser.parse("hello world"))
    }

    @Test
    fun bold_italic_strike_code_spoiler() {
        assertEquals(listOf(Span("x", bold = true)), MarkdownParser.parse("**x**"))
        assertEquals(listOf(Span("x", italic = true)), MarkdownParser.parse("*x*"))
        assertEquals(listOf(Span("x", strikethrough = true)), MarkdownParser.parse("~~x~~"))
        assertEquals(listOf(Span("x", code = true)), MarkdownParser.parse("`x`"))
        assertEquals(listOf(Span("x", spoiler = true)), MarkdownParser.parse("||x||"))
    }

    @Test
    fun mixedRun_splitsIntoStyledSegments() {
        val result = MarkdownParser.parse("a **b** c")
        assertEquals(
            listOf(Span("a "), Span("b", bold = true), Span(" c")),
            result,
        )
    }

    @Test
    fun nestedBoldItalic() {
        // **_x_** → bold + italic
        val result = MarkdownParser.parse("**_x_**")
        assertEquals(listOf(Span("x", bold = true, italic = true)), result)
    }

    @Test
    fun codeContentIsLiteral_notReparsed() {
        // markers inside code are NOT formatting
        val result = MarkdownParser.parse("`**not bold**`")
        assertEquals(listOf(Span("**not bold**", code = true)), result)
    }

    @Test
    fun spoilerContentIsLiteral() {
        val result = MarkdownParser.parse("||*secret*||")
        assertEquals(listOf(Span("*secret*", spoiler = true)), result)
    }

    @Test
    fun unmatchedDelimiter_isLiteral() {
        assertEquals(listOf(Span("2 * 3 = 6")), MarkdownParser.parse("2 * 3 = 6"))
        assertEquals(listOf(Span("a ** b")), MarkdownParser.parse("a ** b"))
    }

    @Test
    fun underscores_areLiteral_notItalic() {
        // snake_case / __init__ must not become italic
        assertEquals(listOf(Span("snake_case_name")), MarkdownParser.parse("snake_case_name"))
        assertEquals(listOf(Span("__init__")), MarkdownParser.parse("__init__"))
    }

    @Test
    fun emptyDelimiterPair_isLiteral() {
        // `**` immediately followed by `**` with nothing between
        assertEquals(listOf(Span("****")), MarkdownParser.parse("****"))
    }

    @Test
    fun doubleStarBeatsSingleStar() {
        // ensure ** is matched as bold, not two italics
        val result = MarkdownParser.parse("**bold** and *it*")
        assertEquals(
            listOf(Span("bold", bold = true), Span(" and "), Span("it", italic = true)),
            result,
        )
    }

    @Test
    fun emptyString_isEmptyList() {
        assertTrue(MarkdownParser.parse("").isEmpty())
    }

    @Test
    fun adjacentSameStyleMerges() {
        // two bold runs separated by re-parse should not fragment a word
        val result = MarkdownParser.parse("**ab**")
        assertEquals(1, result.size)
        assertEquals("ab", result[0].text)
    }
}
