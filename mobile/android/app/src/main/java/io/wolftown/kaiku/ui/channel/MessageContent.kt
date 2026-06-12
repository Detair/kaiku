package io.wolftown.kaiku.ui.channel

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.LocalTextStyle
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextDecoration
import androidx.compose.ui.text.withStyle
import androidx.compose.ui.unit.dp
import io.wolftown.kaiku.domain.markdown.MarkdownParser

/**
 * Renders a chat message's content with inline markdown
 * (`**bold**`, `*italic*`, `~~strike~~`, `` `code` ``, `||spoiler||`).
 *
 * Spoilers render as a hidden block (background-filled, transparent text)
 * until tapped, matching the desktop click-to-reveal behavior.
 */
@Composable
fun MessageContent(
    content: String,
    modifier: Modifier = Modifier,
) {
    val spans = remember(content) { MarkdownParser.parse(content) }
    val hasSpoiler = remember(spans) { spans.any { it.spoiler } }

    // One reveal toggle for the whole message; tapping a hidden message
    // reveals all its spoilers (simple, predictable on touch).
    var revealed by remember(content) { mutableStateOf(false) }

    val codeBg = MaterialTheme.colorScheme.surfaceVariant
    val spoilerBg = MaterialTheme.colorScheme.onSurface

    val annotated: AnnotatedString = buildAnnotatedString {
        for (span in spans) {
            val hidden = span.spoiler && !revealed
            val style = SpanStyle(
                fontWeight = if (span.bold) FontWeight.Bold else null,
                fontStyle = if (span.italic) FontStyle.Italic else null,
                textDecoration = if (span.strikethrough) TextDecoration.LineThrough else null,
                fontFamily = if (span.code) FontFamily.Monospace else null,
                background = when {
                    hidden -> spoilerBg
                    span.code -> codeBg
                    else -> Color.Unspecified
                },
                color = if (hidden) Color.Transparent else Color.Unspecified,
            )
            withStyle(style) { append(span.text) }
        }
    }

    Text(
        text = annotated,
        style = LocalTextStyle.current,
        modifier = if (hasSpoiler && !revealed) {
            modifier.clickable { revealed = true }
        } else {
            modifier
        }.padding(top = 2.dp),
    )
}
