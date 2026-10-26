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
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.unit.dp
import io.wolftown.kaiku.R

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
                    text = if (channelName != null) {
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
