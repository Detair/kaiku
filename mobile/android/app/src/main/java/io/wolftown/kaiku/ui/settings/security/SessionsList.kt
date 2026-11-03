package io.wolftown.kaiku.ui.settings.security

import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Lock
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import io.wolftown.kaiku.R
import io.wolftown.kaiku.domain.model.SessionItem
import io.wolftown.kaiku.ui.auth.setup.parseInstant
import io.wolftown.kaiku.ui.auth.setup.readyText
import io.wolftown.kaiku.ui.shared.KeyScreenSizes

/** Sessions with their assurance (icon + text), client and last use; a key device's row removes the device. */
@Composable
fun SessionsList(sessions: List<SessionItem>, readOnly: Boolean, onEnd: (SessionItem) -> Unit) {
    sessions.forEach { s ->
        Row(Modifier.fillMaxWidth().heightIn(min = KeyScreenSizes.secondaryButton).padding(vertical = 4.dp), verticalAlignment = Alignment.CenterVertically) {
            Column(Modifier.weight(1f)) {
                Text(s.device.ifEmpty { s.clientClass }, fontSize = 18.sp, color = MaterialTheme.colorScheme.onBackground)
                Row(verticalAlignment = Alignment.CenterVertically) {
                    if (s.assurance == "full") {
                        Icon(Icons.Filled.Lock, contentDescription = null, tint = MaterialTheme.colorScheme.onSurfaceVariant, modifier = Modifier.padding(end = 4.dp))
                    }
                    Text(
                        stringResource(if (s.assurance == "full") R.string.security_session_full else R.string.security_session_probation),
                        fontSize = 16.sp,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
                parseInstant(s.lastUsedAt)?.let {
                    Text(stringResource(R.string.security_session_last_used, readyText(it)), fontSize = 16.sp, color = MaterialTheme.colorScheme.onSurfaceVariant)
                }
            }
            if (!s.isCurrent && !readOnly) {
                TextButton(onClick = { onEnd(s) }) {
                    Text(
                        stringResource(if (s.authenticator?.type == "key_device") R.string.security_session_remove_device else R.string.security_session_end),
                        fontSize = 18.sp,
                        color = MaterialTheme.colorScheme.onBackground,
                    )
                }
            }
        }
    }
}
