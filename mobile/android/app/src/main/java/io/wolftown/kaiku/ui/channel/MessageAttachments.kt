package io.wolftown.kaiku.ui.channel

import android.text.format.Formatter
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.PlayArrow
import androidx.compose.material3.Icon
import androidx.compose.ui.Alignment
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import io.wolftown.kaiku.R
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import coil3.compose.AsyncImage
import io.wolftown.kaiku.domain.model.Attachment
import io.wolftown.kaiku.ui.shared.resolveServerUrl
import kotlin.math.ln
import kotlin.math.pow

/**
 * Renders a message's attachments: images as inline thumbnails, everything
 * else as a compact file chip with name and size. Tapping one calls [onOpen]
 * (photo viewer, or the system app for videos and files; AND-B-4).
 */
@Composable
fun MessageAttachments(
    attachments: List<Attachment>,
    modifier: Modifier = Modifier,
    onOpen: (Attachment) -> Unit = {},
) {
    if (attachments.isEmpty()) return
    Column(modifier = modifier.padding(top = 4.dp)) {
        for (att in attachments) {
            if (att.isImage()) {
                val openLabel = stringResource(R.string.chat_attachment_view_photo)
                AsyncImage(
                    model = resolveServerUrl(att.mediumUrl ?: att.url),
                    contentDescription = att.filename,
                    contentScale = ContentScale.Fit,
                    modifier = Modifier
                        .padding(vertical = 2.dp)
                        .widthIn(max = 240.dp)
                        .heightIn(max = 240.dp)
                        .clip(RoundedCornerShape(8.dp))
                        .clickable(onClickLabel = openLabel, role = Role.Image) { onOpen(att) },
                )
            } else {
                FileChip(att, onOpen)
            }
        }
    }
}

@Composable
private fun FileChip(att: Attachment, onOpen: (Attachment) -> Unit) {
    val context = LocalContext.current
    val openLabel = stringResource(R.string.chat_attachment_open)
    Row(
        modifier = Modifier
            .padding(vertical = 2.dp)
            .clip(RoundedCornerShape(8.dp))
            .background(MaterialTheme.colorScheme.surfaceVariant)
            .clickable(onClickLabel = openLabel, role = Role.Button) { onOpen(att) }
            .padding(horizontal = 12.dp, vertical = 8.dp)
            .widthIn(max = 280.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        if (att.isVideo()) {
            Icon(
                Icons.Filled.PlayArrow,
                contentDescription = null,
                tint = MaterialTheme.colorScheme.onSurface,
                modifier = Modifier.size(24.dp),
            )
            Spacer(Modifier.width(8.dp))
        }
        Column {
            Text(
                text = att.filename,
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurface,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
            Text(
                // In the device's language and number format (AND-B-25).
                text = Formatter.formatShortFileSize(context, att.size.coerceAtLeast(0)),
                style = MaterialTheme.typography.labelSmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
    }
}

/** True for videos (shown with a play symbol, opened with the system player). */
fun Attachment.isVideo(): Boolean = mimeType.startsWith("video/", ignoreCase = true)

/** True when the attachment is an image we can render inline. */
fun Attachment.isImage(): Boolean = mimeType.startsWith("image/", ignoreCase = true)

/**
 * Format a byte count as a short human-readable string (e.g. "1.5 MB").
 * Pure helper, unit-tested. Uses binary units (1024) like most file UIs.
 */
fun humanReadableSize(bytes: Long): String {
    if (bytes < 0) return "0 B"
    if (bytes < 1024) return "$bytes B"
    val units = arrayOf("KB", "MB", "GB", "TB", "PB")
    val exp = (ln(bytes.toDouble()) / ln(1024.0)).toInt().coerceIn(1, units.size)
    val value = bytes / 1024.0.pow(exp.toDouble())
    // One decimal, trimming a trailing .0
    val rounded = (value * 10).toLong() / 10.0
    val text = if (rounded % 1.0 == 0.0) rounded.toLong().toString() else rounded.toString()
    return "$text ${units[exp - 1]}"
}
