package io.wolftown.kaiku.ui.settings

import android.content.pm.PackageManager
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.selection.toggleable
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.unit.dp
import io.wolftown.kaiku.R
import io.wolftown.kaiku.push.PushOffReason
import io.wolftown.kaiku.push.PushStatus
import io.wolftown.kaiku.push.PushStatusText

/** Notification status plus the UnifiedPush options; see [NotificationSettingsState] for what shows when. */
@Composable
fun NotificationSettingsSection(
    state: NotificationSettingsState,
    onPreferUnifiedPush: (Boolean) -> Unit,
    onPickDistributor: (String) -> Unit,
    onOpenSystemSettings: () -> Unit,
    onInstallNtfy: () -> Unit,
    modifier: Modifier = Modifier,
) {
    Column(modifier = modifier.fillMaxWidth()) {
        Text(
            text = stringResource(R.string.common_notifications),
            style = MaterialTheme.typography.titleSmall,
            color = MaterialTheme.colorScheme.primary,
            modifier = Modifier.padding(vertical = 12.dp),
        )

        Row(
            modifier = Modifier
                .fillMaxWidth()
                .padding(vertical = 8.dp),
            horizontalArrangement = Arrangement.SpaceBetween,
        ) {
            Text(
                text = stringResource(R.string.notif_settings_status_label),
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            Text(
                text = stringResource(PushStatusText.forStatus(state.status)),
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurface,
            )
        }

        if (state.showUnifiedPushSwitch) {
            Row(
                modifier = Modifier
                    .fillMaxWidth()
                    .toggleable(
                        value = state.preferUnifiedPush,
                        role = Role.Switch,
                        onValueChange = onPreferUnifiedPush,
                    )
                    .padding(vertical = 8.dp),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.SpaceBetween,
            ) {
                Column(modifier = Modifier.weight(1f)) {
                    Text(
                        text = stringResource(R.string.notif_settings_use_unifiedpush),
                        style = MaterialTheme.typography.bodyMedium,
                        color = MaterialTheme.colorScheme.onSurface,
                    )
                    Text(
                        text = stringResource(R.string.notif_settings_use_unifiedpush_hint),
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
                Switch(
                    checked = state.preferUnifiedPush,
                    onCheckedChange = null,
                )
            }
        }

        if (state.showDistributorPicker) {
            DistributorPicker(
                distributors = state.distributors,
                selected = state.selectedDistributor,
                onPick = onPickDistributor,
            )
        }

        val status = state.status
        if (status is PushStatus.Off && status.reason == PushOffReason.PERMISSION_DENIED) {
            OutlinedButton(
                onClick = onOpenSystemSettings,
                modifier = Modifier
                    .fillMaxWidth()
                    .padding(vertical = 4.dp),
            ) {
                Text(stringResource(R.string.notif_settings_open_system))
            }
        }
        if (status is PushStatus.Off && status.reason == PushOffReason.NO_TRANSPORT) {
            OutlinedButton(
                onClick = onInstallNtfy,
                modifier = Modifier
                    .fillMaxWidth()
                    .padding(vertical = 4.dp),
            ) {
                Text(stringResource(R.string.notif_settings_install_ntfy))
            }
        }
    }
}

@Composable
private fun DistributorPicker(
    distributors: List<String>,
    selected: String?,
    onPick: (String) -> Unit,
) {
    val packageManager = LocalContext.current.packageManager
    var expanded by remember { mutableStateOf(false) }
    val current = selected ?: distributors.firstOrNull()

    Row(
        modifier = Modifier
            .fillMaxWidth()
            .padding(vertical = 4.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.SpaceBetween,
    ) {
        Text(
            text = stringResource(R.string.notif_settings_distributor_label),
            style = MaterialTheme.typography.bodyMedium,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        Column {
            TextButton(onClick = { expanded = true }) {
                Text(current?.let { distributorLabel(packageManager, it) }.orEmpty())
            }
            DropdownMenu(expanded = expanded, onDismissRequest = { expanded = false }) {
                distributors.forEach { distributor ->
                    DropdownMenuItem(
                        text = { Text(distributorLabel(packageManager, distributor)) },
                        onClick = {
                            expanded = false
                            onPick(distributor)
                        },
                    )
                }
            }
        }
    }
}

/** The app's name when it can be resolved, else its package name. */
private fun distributorLabel(packageManager: PackageManager, packageName: String): String = try {
    packageManager.getApplicationLabel(packageManager.getApplicationInfo(packageName, 0)).toString()
} catch (_: Exception) {
    packageName
}
