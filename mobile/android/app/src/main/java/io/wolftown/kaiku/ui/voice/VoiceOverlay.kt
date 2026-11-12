package io.wolftown.kaiku.ui.voice

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Call
import androidx.compose.material3.*
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.unit.dp
import io.wolftown.kaiku.R
import io.wolftown.kaiku.data.voice.VoiceEndNotice

/**
 * Compact bar shown at the bottom of other screens while a voice call runs
 * (see [shouldShowVoiceOverlay]).
 *
 * Height: 56dp with accent background.
 * Shows: "Connected to #channel-name" ([channelName] null: "Connected to voice")
 * Actions: mute toggle, tap to return to the call, disconnect
 */
@Composable
fun VoiceOverlay(
    channelName: String?,
    isMuted: Boolean,
    reconnecting: Boolean = false,
    onMuteToggle: () -> Unit,
    onTap: () -> Unit,
    onDisconnect: () -> Unit
) {
    Surface(
        modifier = Modifier
            .fillMaxWidth()
            .height(56.dp)
            .clickable(onClickLabel = stringResource(R.string.voice_overlay_return), onClick = onTap),
        color = MaterialTheme.colorScheme.primaryContainer,
        tonalElevation = 4.dp
    ) {
        Row(
            modifier = Modifier
                .fillMaxSize()
                .padding(horizontal = 16.dp),
            verticalAlignment = Alignment.CenterVertically
        ) {
            // Channel info
            Column(
                modifier = Modifier.weight(1f)
            ) {
                Text(
                    text = if (reconnecting) {
                        stringResource(R.string.voice_call_reconnecting)
                    } else if (channelName != null) {
                        stringResource(R.string.voice_overlay_connected, channelName)
                    } else {
                        stringResource(R.string.voice_overlay_connected_unknown)
                    },
                    style = MaterialTheme.typography.labelLarge,
                    color = MaterialTheme.colorScheme.onPrimaryContainer,
                    maxLines = 1
                )
            }

            // Mute toggle
            val muteDescription = stringResource(R.string.voice_microphone)
            val muteState = stringResource(if (isMuted) R.string.voice_participants_muted else R.string.voice_microphone_on)
            IconButton(
                onClick = onMuteToggle,
                modifier = Modifier.semantics {
                    contentDescription = muteDescription
                    stateDescription = muteState
                }
            ) {
                Text(
                    text = if (isMuted) "🔇" else "🎤",
                    style = MaterialTheme.typography.titleMedium
                )
            }

            // Disconnect
            IconButton(onClick = onDisconnect) {
                Icon(
                    imageVector = Icons.Default.Call,
                    contentDescription = stringResource(R.string.voice_panel_disconnect),
                    tint = MaterialTheme.colorScheme.error
                )
            }
        }
    }
}

/**
 * One line saying why the call ended without the user leaving, with a
 * "Rejoin" action when the notice allows it. Shown on every screen.
 */
@Composable
fun VoiceEndNoticeBar(
    notice: VoiceEndNotice,
    onRejoin: () -> Unit,
    onDismiss: () -> Unit
) {
    Surface(
        modifier = Modifier.fillMaxWidth(),
        color = MaterialTheme.colorScheme.errorContainer,
        tonalElevation = 4.dp
    ) {
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .padding(horizontal = 16.dp, vertical = 4.dp),
            verticalAlignment = Alignment.CenterVertically
        ) {
            Text(
                text = stringResource(notice.message),
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onErrorContainer,
                modifier = Modifier
                    .weight(1f)
                    // Spoken when it appears: the call has ended without the user's doing.
                    .semantics { liveRegion = LiveRegionMode.Polite }
            )
            if (notice.canRejoin) {
                TextButton(onClick = onRejoin) { Text(stringResource(R.string.voice_call_rejoin)) }
            }
            TextButton(onClick = onDismiss) { Text(stringResource(R.string.common_dismiss)) }
        }
    }
}
