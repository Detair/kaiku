package io.wolftown.kaiku.ui.channel

import androidx.compose.foundation.background
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.gestures.detectTransformGestures
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.safeDrawingPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Close
import androidx.compose.material.icons.filled.Share
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.SnackbarHost
import androidx.compose.material3.SnackbarHostState
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import androidx.compose.ui.window.Dialog
import androidx.compose.ui.window.DialogProperties
import coil3.compose.AsyncImage
import coil3.compose.LocalPlatformContext
import coil3.request.ImageRequest
import coil3.request.maxBitmapSize
import coil3.size.Precision
import coil3.size.Scale
import coil3.size.Size as CoilSize
import kotlin.math.roundToInt
import io.wolftown.kaiku.R
import io.wolftown.kaiku.domain.model.Attachment
import io.wolftown.kaiku.ui.shared.resolveServerUrl

private const val MAX_ZOOM = 5f
private const val DOUBLE_TAP_ZOOM = 2.5f

/**
 * Longest side the viewer decodes a photo at. Large enough that zooming shows
 * real detail, small enough for memory (about 64 MB at most) and for the GPU's
 * texture limit on common devices.
 */
internal const val MAX_VIEWER_DECODE_PX = 4096

internal data class DecodeSize(val width: Int, val height: Int)

/**
 * The size the viewer decodes a photo of [width] x [height] at: its own size,
 * scaled down so the longest side is at most [MAX_VIEWER_DECODE_PX]. Unknown
 * dimensions give the cap on both sides (only a bound; the photo is never scaled up).
 */
internal fun viewerDecodeSize(width: Int?, height: Int?): DecodeSize {
    if (width == null || height == null || width <= 0 || height <= 0) {
        return DecodeSize(MAX_VIEWER_DECODE_PX, MAX_VIEWER_DECODE_PX)
    }
    val longest = maxOf(width, height)
    if (longest <= MAX_VIEWER_DECODE_PX) return DecodeSize(width, height)
    val factor = MAX_VIEWER_DECODE_PX.toDouble() / longest
    return DecodeSize(
        (width * factor).roundToInt().coerceAtLeast(1),
        (height * factor).roundToInt().coerceAtLeast(1),
    )
}

/** Next zoom after a pinch; pure so the bounds can be unit tested. */
internal fun clampZoom(current: Float, factor: Float): Float = (current * factor).coerceIn(1f, MAX_ZOOM)

/**
 * Full-screen photo (AND-B-4): pinch or double-tap to zoom, drag to move while
 * zoomed; "Save" and "Share" in the bar. Back or the close button leaves.
 * "Save" is disabled while a save runs; its result is shown (and announced
 * politely) in [snackbarHostState] inside the viewer, not behind it.
 * Always on black, so the bar uses white text and icons (21:1).
 */
@Composable
fun ImageViewer(
    attachment: Attachment,
    busy: Boolean,
    snackbarHostState: SnackbarHostState,
    onSave: () -> Unit,
    onShare: () -> Unit,
    onDismiss: () -> Unit,
) {
    Dialog(
        onDismissRequest = onDismiss,
        properties = DialogProperties(usePlatformDefaultWidth = false, decorFitsSystemWindows = false),
    ) {
        var scale by remember { mutableFloatStateOf(1f) }
        var offset by remember { mutableStateOf(Offset.Zero) }

        Box(
            modifier = Modifier
                .fillMaxSize()
                .background(Color.Black)
        ) {
            // The original, decoded at full resolution (up to the cap) instead of
            // the screen size, so zooming in shows detail instead of pixels.
            val context = LocalPlatformContext.current
            val url = resolveServerUrl(attachment.url)
            val request = remember(url, attachment.width, attachment.height) {
                val target = viewerDecodeSize(attachment.width, attachment.height)
                ImageRequest.Builder(context)
                    .data(url)
                    .size(target.width, target.height)
                    .scale(Scale.FIT)
                    .precision(Precision.INEXACT) // never scale a smaller photo up
                    .maxBitmapSize(CoilSize(MAX_VIEWER_DECODE_PX, MAX_VIEWER_DECODE_PX))
                    .build()
            }
            AsyncImage(
                model = request,
                contentDescription = attachment.filename,
                contentScale = ContentScale.Fit,
                modifier = Modifier
                    .fillMaxSize()
                    .pointerInput(Unit) {
                        detectTapGestures(onDoubleTap = { tap ->
                            if (scale > 1f) {
                                scale = 1f
                                offset = Offset.Zero
                            } else {
                                scale = DOUBLE_TAP_ZOOM
                                offset = (Offset(size.width / 2f, size.height / 2f) - tap) * (DOUBLE_TAP_ZOOM - 1f)
                            }
                        })
                    }
                    .pointerInput(Unit) {
                        detectTransformGestures { _, pan, zoom, _ ->
                            scale = clampZoom(scale, zoom)
                            offset = if (scale == 1f) Offset.Zero else offset + pan
                        }
                    }
                    .graphicsLayer(
                        scaleX = scale,
                        scaleY = scale,
                        translationX = offset.x,
                        translationY = offset.y,
                    )
            )

            Row(
                modifier = Modifier
                    .fillMaxWidth()
                    .background(Color.Black.copy(alpha = 0.6f))
                    .safeDrawingPadding()
                    .padding(horizontal = 4.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                IconButton(onClick = onDismiss) {
                    Icon(Icons.Filled.Close, contentDescription = stringResource(R.string.common_close), tint = Color.White)
                }
                Box(modifier = Modifier.weight(1f))
                if (busy) {
                    CircularProgressIndicator(
                        modifier = Modifier.size(20.dp).padding(end = 4.dp),
                        color = Color.White,
                        strokeWidth = 2.dp,
                    )
                }
                TextButton(onClick = onSave, enabled = !busy) {
                    Text(stringResource(R.string.chat_attachment_save), color = Color.White)
                }
                IconButton(onClick = onShare, enabled = !busy) {
                    Icon(Icons.Filled.Share, contentDescription = stringResource(R.string.chat_attachment_share), tint = Color.White)
                }
            }

            // SnackbarHost is a polite live region, so TalkBack reads the result too.
            SnackbarHost(
                hostState = snackbarHostState,
                modifier = Modifier
                    .align(Alignment.BottomCenter)
                    .safeDrawingPadding(),
            )
        }
    }
}
