package io.wolftown.kaiku.ui.voice

import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.viewinterop.AndroidView
import io.wolftown.kaiku.R
import io.wolftown.kaiku.data.ws.ScreenShareInfo
import org.webrtc.EglBase
import org.webrtc.RendererCommon
import org.webrtc.SurfaceViewRenderer
import org.webrtc.VideoTrack

/**
 * Renders a WebRTC video track (screen share) using [SurfaceViewRenderer].
 *
 * Wraps the native Android view in Compose via [AndroidView] and manages
 * the sink lifecycle (add on update, remove + release on dispose).
 *
 * Shows:
 * - The video content from the remote screen share
 * - An overlay with username, source label, and quality badge
 * - Layer quality selector chips (Auto, High, Medium, Low)
 *
 * Tapping the view toggles the overlay controls visibility.
 */
@Composable
fun ScreenShareView(
    videoTrack: VideoTrack?,
    screenShareInfo: ScreenShareInfo,
    eglContext: EglBase.Context,
    currentLayer: String,
    onLayerChange: (String) -> Unit,
    onToggleFullscreen: () -> Unit,
    modifier: Modifier = Modifier
) {
    var showControls by remember { mutableStateOf(true) }

    Box(
        modifier = modifier
            .clip(RoundedCornerShape(8.dp))
            .background(Color.Black)
            .clickable {
                // Toggle overlay controls visibility
                showControls = !showControls
            }
    ) {
        // Video renderer
        if (videoTrack != null) {
            // Track the current video track to properly manage sink lifecycle
            var currentSinkTrack by remember { mutableStateOf<VideoTrack?>(null) }

            AndroidView(
                factory = { ctx ->
                    SurfaceViewRenderer(ctx).apply {
                        init(eglContext, null)
                        setMirror(false)
                        setScalingType(RendererCommon.ScalingType.SCALE_ASPECT_FIT)
                        setEnableHardwareScaler(true)
                    }
                },
                update = { renderer ->
                    // Remove old sink if the track changed
                    if (currentSinkTrack != null && currentSinkTrack != videoTrack) {
                        try {
                            currentSinkTrack?.removeSink(renderer)
                        } catch (_: Exception) {
                            // Track may already be disposed
                        }
                    }
                    // Add new track as sink
                    if (currentSinkTrack != videoTrack) {
                        videoTrack.addSink(renderer)
                        currentSinkTrack = videoTrack
                    }
                },
                onRelease = { renderer ->
                    try {
                        currentSinkTrack?.removeSink(renderer)
                    } catch (_: Exception) {
                        // Track may already be disposed
                    }
                    renderer.release()
                },
                modifier = Modifier.fillMaxSize()
            )
        } else {
            // No video track yet — show loading placeholder
            Box(
                modifier = Modifier.fillMaxSize(),
                contentAlignment = Alignment.Center
            ) {
                Column(horizontalAlignment = Alignment.CenterHorizontally) {
                    CircularProgressIndicator(
                        modifier = Modifier.size(32.dp),
                        color = MaterialTheme.colorScheme.onSurface.copy(alpha = 0.6f)
                    )
                    Spacer(modifier = Modifier.height(8.dp))
                    Text(
                        text = stringResource(R.string.voice_screen_share_connecting),
                        style = MaterialTheme.typography.bodySmall,
                        color = Color.White.copy(alpha = 0.7f)
                    )
                }
            }
        }

        // Overlay controls
        AnimatedVisibility(
            visible = showControls,
            enter = fadeIn(),
            exit = fadeOut(),
            modifier = Modifier.fillMaxSize()
        ) {
            Box(modifier = Modifier.fillMaxSize()) {
                // Top info bar
                Row(
                    modifier = Modifier
                        .fillMaxWidth()
                        .background(Color.Black.copy(alpha = 0.5f))
                        .padding(horizontal = 12.dp, vertical = 6.dp)
                        .align(Alignment.TopStart),
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.SpaceBetween
                ) {
                    // Username and source label
                    Column(modifier = Modifier.weight(1f)) {
                        Text(
                            text = screenShareInfo.username.ifEmpty { stringResource(R.string.voice_unknown_user) },
                            style = MaterialTheme.typography.labelLarge,
                            color = Color.White,
                            maxLines = 1,
                            overflow = TextOverflow.Ellipsis
                        )
                        if (screenShareInfo.sourceLabel.isNotEmpty()) {
                            Text(
                                text = screenShareInfo.sourceLabel,
                                style = MaterialTheme.typography.labelSmall,
                                color = Color.White.copy(alpha = 0.7f),
                                maxLines = 1,
                                overflow = TextOverflow.Ellipsis
                            )
                        }
                    }

                    // Quality badge
                    Surface(
                        shape = RoundedCornerShape(4.dp),
                        color = qualityBadgeColor(screenShareInfo.quality),
                        modifier = Modifier.padding(start = 8.dp)
                    ) {
                        Text(
                            text = screenShareInfo.quality.uppercase(),
                            style = MaterialTheme.typography.labelSmall,
                            color = Color.White,
                            modifier = Modifier.padding(horizontal = 6.dp, vertical = 2.dp)
                        )
                    }
                }

                // Bottom layer selector + fullscreen
                Row(
                    modifier = Modifier
                        .fillMaxWidth()
                        .background(Color.Black.copy(alpha = 0.5f))
                        .padding(horizontal = 8.dp, vertical = 4.dp)
                        .align(Alignment.BottomStart),
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.SpaceBetween
                ) {
                    // Layer quality chips
                    Row(horizontalArrangement = Arrangement.spacedBy(4.dp)) {
                        layerOptions.forEach { (labelRes, value) ->
                            LayerChip(
                                label = stringResource(labelRes),
                                isSelected = currentLayer == value,
                                onClick = { onLayerChange(value) }
                            )
                        }
                    }

                    // Fullscreen button
                    TextButton(
                        onClick = onToggleFullscreen,
                        contentPadding = PaddingValues(horizontal = 8.dp, vertical = 0.dp)
                    ) {
                        Text(
                            text = stringResource(R.string.voice_screen_share_fullscreen),
                            style = MaterialTheme.typography.labelSmall,
                            color = Color.White
                        )
                    }
                }
            }
        }
    }
}

/**
 * A small chip for selecting the simulcast layer quality.
 */
@Composable
private fun LayerChip(
    label: String,
    isSelected: Boolean,
    onClick: () -> Unit
) {
    Surface(
        shape = RoundedCornerShape(12.dp),
        color = if (isSelected) {
            MaterialTheme.colorScheme.primary
        } else {
            Color.White.copy(alpha = 0.2f)
        },
        modifier = Modifier.clickable(onClick = onClick)
    ) {
        Text(
            text = label,
            style = MaterialTheme.typography.labelSmall,
            color = Color.White,
            modifier = Modifier.padding(horizontal = 8.dp, vertical = 4.dp)
        )
    }
}

/** Ordered list of layer options displayed as chips. */
private val layerOptions = listOf(
    R.string.voice_screen_share_layer_auto to "auto",
    R.string.voice_screen_share_layer_high to "high",
    R.string.voice_screen_share_layer_medium to "medium",
    R.string.voice_screen_share_layer_low to "low"
)

/** Returns a color for the quality badge based on the quality string. */
private fun qualityBadgeColor(quality: String): Color = when (quality.lowercase()) {
    "high" -> Color(0xFF4CAF50)    // Green
    "medium" -> Color(0xFFFFC107)  // Amber
    "low" -> Color(0xFFFF5722)     // Deep orange
    else -> Color(0xFF607D8B)      // Blue grey for "auto" or unknown
}
