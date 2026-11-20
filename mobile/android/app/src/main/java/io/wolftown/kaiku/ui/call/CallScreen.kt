package io.wolftown.kaiku.ui.call

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawingPadding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.selection.toggleable
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Call
import androidx.compose.material.icons.filled.Close
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.FilledIconButton
import androidx.compose.material3.Surface
import androidx.compose.material3.Switch
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButtonDefaults
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.role
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import io.wolftown.kaiku.R

/**
 * The call screen. While a call rings it shows nothing but the caller's
 * name and Accept / Decline (it may be shown over the lock screen); otherwise
 * who, the state, the running time and Mute, Speaker, Hang up. Everything
 * scrolls, so the largest font size never hides a button.
 */
@Composable
fun CallScreen(
    ui: CallUi,
    micNeeded: Boolean,
    onAccept: () -> Unit,
    onDecline: () -> Unit,
    onHangUp: () -> Unit,
    onToggleMute: () -> Unit,
    onToggleSpeaker: () -> Unit,
    onOpenSettings: () -> Unit,
    onClose: () -> Unit,
) {
    Column(
        modifier = Modifier
            .fillMaxSize()
            .safeDrawingPadding()
            .verticalScroll(rememberScrollState())
            .padding(horizontal = 24.dp, vertical = 32.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.spacedBy(16.dp),
    ) {
        Spacer(Modifier.heightIn(min = 24.dp))
        Text(
            text = ui.name ?: stringResource(R.string.voice_call_unknown_person),
            style = MaterialTheme.typography.headlineLarge,
            color = MaterialTheme.colorScheme.onBackground,
            textAlign = TextAlign.Center,
            modifier = Modifier.fillMaxWidth(),
        )
        ui.status?.let {
            Text(
                text = stringResource(it),
                style = MaterialTheme.typography.titleMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                textAlign = TextAlign.Center,
                modifier = Modifier
                    .fillMaxWidth()
                    .semantics { liveRegion = LiveRegionMode.Polite },
            )
        }
        ui.durationSecs?.let {
            Text(
                text = CallUiMapper.formatDuration(it),
                style = MaterialTheme.typography.titleLarge,
                color = MaterialTheme.colorScheme.onBackground,
            )
        }
        if (micNeeded) {
            Text(
                text = stringResource(R.string.voice_call_mic_needed),
                style = MaterialTheme.typography.bodyLarge,
                color = MaterialTheme.colorScheme.onBackground,
                textAlign = TextAlign.Center,
                modifier = Modifier.fillMaxWidth(),
            )
            TextButton(onClick = onOpenSettings, modifier = Modifier.heightIn(min = 48.dp)) {
                Text(stringResource(R.string.voice_permission_open_settings))
            }
        }
        Spacer(Modifier.heightIn(min = 48.dp))
        when (ui.phase) {
            CallPhase.Ringing -> RingingActions(onAccept = onAccept, onDecline = onDecline)
            CallPhase.Calling, CallPhase.Connecting, CallPhase.Connected, CallPhase.Reconnecting ->
                InCallActions(ui, onHangUp = onHangUp, onToggleMute = onToggleMute, onToggleSpeaker = onToggleSpeaker)
            CallPhase.Ended, CallPhase.None ->
                Button(onClick = onClose, modifier = Modifier.heightIn(min = 56.dp).widthIn(min = 120.dp)) {
                    Text(stringResource(R.string.common_ok))
                }
        }
    }
}

@Composable
private fun RingingActions(onAccept: () -> Unit, onDecline: () -> Unit) {
    Row(
        modifier = Modifier.fillMaxWidth(),
        horizontalArrangement = Arrangement.SpaceEvenly,
    ) {
        RoundAction(
            label = stringResource(R.string.voice_call_decline),
            onClick = onDecline,
            container = MaterialTheme.colorScheme.errorContainer,
            content = MaterialTheme.colorScheme.onErrorContainer,
            icon = { Icon(Icons.Filled.Close, contentDescription = it, modifier = Modifier.size(36.dp)) },
        )
        RoundAction(
            label = stringResource(R.string.voice_call_accept),
            onClick = onAccept,
            container = MaterialTheme.colorScheme.primaryContainer,
            content = MaterialTheme.colorScheme.onPrimaryContainer,
            icon = { Icon(Icons.Filled.Call, contentDescription = it, modifier = Modifier.size(36.dp)) },
        )
    }
}

/** A whole-row switch, at least 56 dp high. */
@Composable
private fun ToggleRow(label: String, checked: Boolean, onToggle: () -> Unit, enabled: Boolean = true) {
    Surface(
        shape = MaterialTheme.shapes.medium,
        color = MaterialTheme.colorScheme.secondaryContainer,
        modifier = Modifier
            .fillMaxWidth()
            .heightIn(min = 56.dp)
            .toggleable(value = checked, enabled = enabled, role = Role.Switch, onValueChange = { onToggle() }),
    ) {
        Row(
            modifier = Modifier.padding(horizontal = 16.dp, vertical = 8.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text(
                label,
                style = MaterialTheme.typography.titleMedium,
                color = MaterialTheme.colorScheme.onSecondaryContainer,
                modifier = Modifier.weight(1f),
            )
            Switch(checked = checked, onCheckedChange = null, enabled = enabled)
        }
    }
}

/** A large round button with its label below; the label is the button's accessible name. */
@Composable
private fun RoundAction(
    label: String,
    onClick: () -> Unit,
    container: androidx.compose.ui.graphics.Color,
    content: androidx.compose.ui.graphics.Color,
    /** Gets [label] as its content description: the label below is only visual. */
    icon: @Composable (String) -> Unit,
) {
    Column(horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(8.dp)) {
        FilledIconButton(
            onClick = onClick,
            modifier = Modifier
                .size(80.dp)
                .semantics { role = Role.Button },
            colors = IconButtonDefaults.filledIconButtonColors(containerColor = container, contentColor = content),
        ) {
            icon(label)
        }
        Text(
            label,
            style = MaterialTheme.typography.titleMedium,
            color = MaterialTheme.colorScheme.onBackground,
            modifier = Modifier.clearAndSetSemantics {},
        )
    }
}

@Composable
private fun InCallActions(
    ui: CallUi,
    onHangUp: () -> Unit,
    onToggleMute: () -> Unit,
    onToggleSpeaker: () -> Unit,
) {
    Column(
        modifier = Modifier.fillMaxWidth(),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        // Switches: TalkBack reads "Mute, switch, on" (the label is the setting, the switch its state).
        ToggleRow(stringResource(R.string.voice_call_mute), checked = ui.muted, onToggle = onToggleMute)
        ToggleRow(stringResource(R.string.voice_call_speaker), checked = ui.speaker, onToggle = onToggleSpeaker, enabled = ui.speakerEnabled)
        Button(
            onClick = onHangUp,
            modifier = Modifier
                .fillMaxWidth()
                .heightIn(min = 64.dp),
            colors = ButtonDefaults.buttonColors(
                containerColor = MaterialTheme.colorScheme.errorContainer,
                contentColor = MaterialTheme.colorScheme.onErrorContainer,
            ),
        ) {
            Icon(Icons.Filled.Close, contentDescription = null)
            Spacer(Modifier.size(8.dp))
            Text(stringResource(R.string.voice_call_hang_up), style = MaterialTheme.typography.titleMedium)
        }
    }
}
