package io.wolftown.kaiku.ui.channel

import androidx.annotation.StringRes
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.Spacer
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import io.wolftown.kaiku.R
import io.wolftown.kaiku.data.repository.PendingMessage

/** Why a pending message was not sent, as shown under it. */
@StringRes
internal fun pendingFailureRes(failure: PendingMessage.Failure?): Int = when (failure) {
    PendingMessage.Failure.TOO_LARGE -> R.string.chat_upload_too_large
    PendingMessage.Failure.UNSUPPORTED_TYPE -> R.string.chat_upload_unsupported_type
    PendingMessage.Failure.UPLOAD_FAILED -> R.string.chat_upload_failed
    PendingMessage.Failure.SEND_FAILED, null -> R.string.chat_send_failed
}

/** Retrying cannot help a file that is too large or of a refused type. */
internal fun canRetry(failure: PendingMessage.Failure?): Boolean =
    failure != PendingMessage.Failure.TOO_LARGE && failure != PendingMessage.Failure.UNSUPPORTED_TYPE

/**
 * A message of the user's own that the server has not confirmed (AND-B-3):
 * "Sending…" while it goes out, or the reason it was not sent with Retry and Delete.
 */
@Composable
fun PendingMessageItem(
    item: PendingMessage,
    onRetry: (localId: String) -> Unit,
    onDiscard: (localId: String) -> Unit,
    modifier: Modifier = Modifier,
) {
    // Aligned with the text of MessageItem (16 dp + 36 dp avatar + 8 dp).
    Column(
        modifier = modifier
            .fillMaxWidth()
            .padding(start = 60.dp, end = 16.dp, top = 4.dp, bottom = 4.dp)
    ) {
        if (item.text.isNotBlank()) {
            Text(
                text = item.text,
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurface,
            )
        }
        if (item.attachment != null) {
            Text(
                text = stringResource(R.string.chat_pending_attachment),
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
        when (item.state) {
            PendingMessage.State.SENDING -> Row(
                verticalAlignment = Alignment.CenterVertically,
                modifier = Modifier.padding(top = 2.dp),
            ) {
                CircularProgressIndicator(modifier = Modifier.size(12.dp), strokeWidth = 2.dp)
                Spacer(Modifier.width(6.dp))
                Text(
                    text = stringResource(R.string.chat_pending_sending),
                    style = MaterialTheme.typography.labelSmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }

            PendingMessage.State.FAILED -> Column(
                modifier = Modifier
                    .padding(top = 2.dp)
                    .semantics { liveRegion = LiveRegionMode.Polite },
            ) {
                Text(
                    text = stringResource(pendingFailureRes(item.failure)),
                    style = MaterialTheme.typography.labelMedium,
                    color = MaterialTheme.colorScheme.error,
                )
                Row(horizontalArrangement = Arrangement.Start) {
                    if (canRetry(item.failure)) {
                        TextButton(onClick = { onRetry(item.localId) }) {
                            Text(stringResource(R.string.common_retry))
                        }
                    }
                    TextButton(onClick = { onDiscard(item.localId) }) {
                        Text(stringResource(R.string.chat_pending_discard))
                    }
                }
            }
        }
    }
}
