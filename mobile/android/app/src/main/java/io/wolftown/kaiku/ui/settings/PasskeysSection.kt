package io.wolftown.kaiku.ui.settings

import android.app.Activity
import android.content.Context
import android.content.ContextWrapper
import android.os.Build
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.Button
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import androidx.hilt.navigation.compose.hiltViewModel
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import io.wolftown.kaiku.R
import io.wolftown.kaiku.domain.model.Passkey

internal fun Context.findActivity(): Activity? {
    var c: Context? = this
    while (c is ContextWrapper) {
        if (c is Activity) return c
        c = c.baseContext
    }
    return null
}

/** "Passkeys" in Settings (C29); 48 dp targets. */
@Composable
fun PasskeysSection(viewModel: PasskeysViewModel = hiltViewModel()) {
    val state by viewModel.state.collectAsStateWithLifecycle()
    val activity = LocalContext.current.findActivity()
    var confirming by remember { mutableStateOf<String?>(null) }
    Column(modifier = Modifier.fillMaxWidth().padding(vertical = 8.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
        Text(stringResource(R.string.passkeys_settings_title), style = MaterialTheme.typography.titleMedium)
        if (!state.loading && state.passkeys.isEmpty()) {
            Text(stringResource(R.string.passkeys_settings_empty))
        }
        state.passkeys.forEach { p -> PasskeyRow(p, confirming == p.id, state.busy, { confirming = p.id }, { confirming = null }) {
            if (activity != null) viewModel.remove(activity, p)
            confirming = null
        } }
        if (state.canAdd && activity != null) {
            Button(
                onClick = { viewModel.add(activity, Build.MODEL ?: "Android") },
                enabled = !state.busy,
                modifier = Modifier.heightIn(min = 48.dp),
            ) { Text(stringResource(R.string.passkeys_settings_add)) }
        }
        state.errorRes?.let { Text(stringResource(it), color = MaterialTheme.colorScheme.error) }
    }
}

@Composable
private fun PasskeyRow(
    p: Passkey,
    confirming: Boolean,
    busy: Boolean,
    onAsk: () -> Unit,
    onCancel: () -> Unit,
    onRemove: () -> Unit,
) {
    Column(verticalArrangement = Arrangement.spacedBy(4.dp)) {
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            Text(p.label, style = MaterialTheme.typography.bodyLarge)
            if (p.backupState) Text(stringResource(R.string.passkeys_settings_synced))
        }
        when {
            p.isPending -> Text(stringResource(R.string.passkeys_settings_pending, p.readyAt.orEmpty()))
            p.lastUsedAt != null -> Text(stringResource(R.string.passkeys_settings_last_used, p.lastUsedAt))
            else -> Text(stringResource(R.string.passkeys_settings_never_used))
        }
        if (p.isActive) {
            if (confirming) {
                Text(stringResource(R.string.passkeys_settings_remove_confirm, p.label))
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    Button(onClick = onRemove, enabled = !busy, modifier = Modifier.heightIn(min = 48.dp)) {
                        Text(stringResource(R.string.passkeys_settings_remove_confirm_button))
                    }
                    OutlinedButton(onClick = onCancel, modifier = Modifier.heightIn(min = 48.dp)) {
                        Text(stringResource(R.string.passkeys_login_cancel))
                    }
                }
            } else {
                OutlinedButton(onClick = onAsk, enabled = !busy, modifier = Modifier.heightIn(min = 48.dp)) {
                    Text(stringResource(R.string.passkeys_settings_remove))
                }
            }
        }
    }
}
