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
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.LinkAnnotation
import androidx.compose.ui.text.TextLinkStyles
import androidx.compose.ui.text.withLink
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
 *
 * Web links ([LinkFinder]) are underlined and open in the browser (AND-B-5);
 * links inside a hidden spoiler stay hidden and inert until it is revealed.
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
    val context = LocalContext.current
    val linkStyles = TextLinkStyles(
        style = SpanStyle(
            color = MaterialTheme.colorScheme.primary,
            textDecoration = TextDecoration.Underline,
        )
    )

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
            withStyle(style) {
                val links = if (hidden) emptyList() else LinkFinder.find(span.text)
                var at = 0
                for (link in links) {
                    append(span.text.substring(at, link.start))
                    withLink(
                        LinkAnnotation.Url(
                            url = link.url,
                            styles = linkStyles,
                            linkInteractionListener = { openWebLink(context, link.url) },
                        )
                    ) { append(span.text.substring(link.start, link.end)) }
                    at = link.end
                }
                append(span.text.substring(at))
            }
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

/** Opens an http(s) [url] in the browser; does nothing when no app can open it. */
internal fun openWebLink(context: android.content.Context, url: String) {
    val uri = android.net.Uri.parse(url)
    if (uri.scheme?.lowercase() !in setOf("http", "https")) return
    try {
        context.startActivity(
            android.content.Intent(android.content.Intent.ACTION_VIEW, uri)
                .addCategory(android.content.Intent.CATEGORY_BROWSABLE)
                .addFlags(android.content.Intent.FLAG_ACTIVITY_NEW_TASK)
        )
    } catch (_: android.content.ActivityNotFoundException) {
        // No browser installed.
    }
}

/** The text a person sees (markdown markers removed), for "Copy text". */
internal fun plainMessageText(content: String): String =
    MarkdownParser.parse(content).joinToString("") { it.text }
