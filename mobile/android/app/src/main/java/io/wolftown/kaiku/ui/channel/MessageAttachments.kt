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
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.material3.TextButton
import androidx.compose.runtime.getValue
import androidx.compose.runtime.key
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.sizeIn
import androidx.compose.ui.platform.LocalDensity
import kotlin.math.ln
import kotlin.math.roundToInt
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
                ImagePreview(att, onOpen)
            } else {
                FileChip(att, onOpen)
            }
        }
    }
}

internal const val PREVIEW_MAX_DP = 240
internal const val PREVIEW_MIN_WIDTH_DP = 160
internal const val PREVIEW_MIN_HEIGHT_DP = 120

/** A size in the chat, in dp. */
internal data class PreviewTile(val widthDp: Int, val heightDp: Int)

/**
 * The size a loaded photo of [width] x [height] px is drawn at: its own shape,
 * fitted into [PREVIEW_MAX_DP] square and never larger than its own pixels at
 * [density] (px per dp). Null when the dimensions are unknown.
 */
internal fun photoSize(width: Int?, height: Int?, density: Float): PreviewTile? {
    if (width == null || height == null || width <= 0 || height <= 0) return null
    val dpPerPx = minOf(PREVIEW_MAX_DP.toDouble() / maxOf(width, height), 1.0 / density.coerceAtLeast(1f))
    return PreviewTile(
        (width * dpPerPx).roundToInt().coerceAtLeast(1),
        (height * dpPerPx).roundToInt().coerceAtLeast(1),
    )
}

/**
 * The tile shown while a photo loads or after it failed: the photo's size,
 * known before loading so the list does not jump, but at least
 * [PREVIEW_MIN_WIDTH_DP] x [PREVIEW_MIN_HEIGHT_DP] so the "couldn't load"
 * text and Retry have room.
 */
internal fun placeholderTile(photo: PreviewTile?): PreviewTile {
    if (photo == null) return PreviewTile(PREVIEW_MAX_DP, PREVIEW_MAX_DP * 2 / 3)
    return PreviewTile(
        photo.widthDp.coerceAtLeast(PREVIEW_MIN_WIDTH_DP),
        photo.heightDp.coerceAtLeast(PREVIEW_MIN_HEIGHT_DP),
    )
}

/** Where loading a photo stands (chat preview and viewer). */
internal enum class LoadState { LOADING, LOADED, FAILED }

/**
 * A photo in the chat (the small variant). While loading it holds a grey tile
 * of [placeholderTile]; loaded, it is drawn at [photoSize] without bands; when
 * loading fails it shows the file name, a short reason and Retry. The failed
 * tile grows with large fonts so Retry stays reachable.
 */
@Composable
private fun ImagePreview(att: Attachment, onOpen: (Attachment) -> Unit) {
    val photo = photoSize(att.width, att.height, LocalDensity.current.density)
    val placeholder = placeholderTile(photo)
    val url = resolveServerUrl(att.mediumUrl ?: att.url)
    // A new attempt number composes a new image, which starts the request again.
    var attempt by remember(url) { mutableIntStateOf(0) }
    var state by remember(url) { mutableStateOf(LoadState.LOADING) }
    val shape = RoundedCornerShape(8.dp)
    val tileColor = MaterialTheme.colorScheme.surfaceVariant
    when (state) {
        LoadState.FAILED -> Column(
            modifier = Modifier
                .padding(vertical = 2.dp)
                .width(placeholder.widthDp.dp)
                .heightIn(min = placeholder.heightDp.dp)
                .clip(shape)
                .background(tileColor)
                .padding(8.dp),
            verticalArrangement = Arrangement.Center,
            horizontalAlignment = Alignment.CenterHorizontally,
        ) {
            Text(
                text = att.filename,
                style = MaterialTheme.typography.labelMedium,
                color = MaterialTheme.colorScheme.onSurface,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
            Text(
                text = stringResource(R.string.chat_attachment_load_failed),
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                textAlign = TextAlign.Center,
            )
            TextButton(onClick = {
                state = LoadState.LOADING
                attempt++
            }) {
                Text(stringResource(R.string.common_retry))
            }
        }

        else -> {
            val loaded = state == LoadState.LOADED
            val boxModifier = when {
                !loaded -> Modifier.size(placeholder.widthDp.dp, placeholder.heightDp.dp).clip(shape).background(tileColor)
                photo != null -> Modifier.size(photo.widthDp.dp, photo.heightDp.dp).clip(shape)
                // Unknown dimensions: the photo's own shape, at most 240 dp.
                else -> Modifier.sizeIn(maxWidth = PREVIEW_MAX_DP.dp, maxHeight = PREVIEW_MAX_DP.dp).clip(shape)
            }
            val openLabel = stringResource(R.string.chat_attachment_view_photo)
            Box(modifier = Modifier.padding(vertical = 2.dp).then(boxModifier)) {
                key(attempt) {
                    AsyncImage(
                        model = url,
                        contentDescription = att.filename,
                        contentScale = ContentScale.Fit,
                        onLoading = { state = LoadState.LOADING },
                        onSuccess = { state = LoadState.LOADED },
                        onError = { state = LoadState.FAILED },
                        modifier = (if (loaded && photo == null) Modifier else Modifier.fillMaxSize())
                            .clickable(onClickLabel = openLabel, role = Role.Image) { onOpen(att) },
                    )
                }
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
