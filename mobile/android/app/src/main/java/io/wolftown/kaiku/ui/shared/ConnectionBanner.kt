package io.wolftown.kaiku.ui.shared

import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import io.wolftown.kaiku.R
import io.wolftown.kaiku.data.ws.RealtimeStatus

/**
 * Thin strip shown while the realtime connection is not healthy.
 * Renders nothing for [RealtimeStatus.Connected] and [RealtimeStatus.Idle].
 */
@Composable
fun ConnectionBanner(status: RealtimeStatus, modifier: Modifier = Modifier) {
    val (text, offline) = when (status) {
        RealtimeStatus.Connected, RealtimeStatus.Idle -> return
        RealtimeStatus.Connecting, RealtimeStatus.Reconnecting ->
            stringResource(R.string.connection_reconnecting) to false
        RealtimeStatus.Offline -> stringResource(R.string.connection_offline) to true
    }
    Surface(
        modifier = modifier.fillMaxWidth(),
        color = if (offline) MaterialTheme.colorScheme.errorContainer else MaterialTheme.colorScheme.secondaryContainer,
        contentColor = if (offline) MaterialTheme.colorScheme.onErrorContainer else MaterialTheme.colorScheme.onSecondaryContainer
    ) {
        Text(
            text = text,
            style = MaterialTheme.typography.labelMedium,
            textAlign = TextAlign.Center,
            modifier = Modifier.padding(horizontal = 16.dp, vertical = 4.dp)
        )
    }
}
