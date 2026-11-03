package io.wolftown.kaiku.ui.security

import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.hilt.navigation.compose.hiltViewModel
import io.wolftown.kaiku.R
import io.wolftown.kaiku.ui.shared.KeyScreenSizes

/**
 * L2-A5: the read-only security log, newest first, with a "Show older" button
 * (scrolling alone never loads more).
 */
@Composable
fun SecurityLogList(modifier: Modifier = Modifier, viewModel: SecurityLogViewModel = hiltViewModel()) {
    val ui by viewModel.ui.collectAsState()
    LaunchedEffect(Unit) { viewModel.load() }
    Column(modifier.verticalScroll(rememberScrollState()).padding(horizontal = 16.dp)) {
        if (ui.items.isEmpty() && !ui.loading) {
            Text(stringResource(R.string.security_log_empty), fontSize = 18.sp, color = MaterialTheme.colorScheme.onBackground,
                modifier = Modifier.padding(vertical = 16.dp))
        }
        ui.items.forEach { e ->
            Column(Modifier.padding(vertical = 10.dp)) {
                Text(stringResource(NoticeText.title(e.eventType), stringResource(R.string.security_subject_yours), e.actor?.username.orEmpty(),
                    e.authenticator?.label.orEmpty(), 1), fontSize = 18.sp, color = MaterialTheme.colorScheme.onBackground)
                EventLines(e)
            }
            HorizontalDivider()
        }
        ui.error?.let { Text(stringResource(it), fontSize = 18.sp, color = MaterialTheme.colorScheme.onBackground) }
        if (ui.loading) CircularProgressIndicator(Modifier.padding(16.dp))
        if (ui.hasMore && !ui.loading) {
            OutlinedButton(onClick = viewModel::showOlder, modifier = Modifier.fillMaxWidth().padding(vertical = 12.dp).heightIn(min = KeyScreenSizes.secondaryButton)) {
                Text(stringResource(R.string.security_log_show_older), fontSize = 18.sp, color = MaterialTheme.colorScheme.onBackground)
            }
        }
    }
}
