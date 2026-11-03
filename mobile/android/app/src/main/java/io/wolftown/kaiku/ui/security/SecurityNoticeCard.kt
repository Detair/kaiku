package io.wolftown.kaiku.ui.security

import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Lock
import androidx.compose.material.icons.filled.Warning
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import io.wolftown.kaiku.R
import io.wolftown.kaiku.domain.model.EventAction
import io.wolftown.kaiku.domain.model.NoticeAction
import io.wolftown.kaiku.domain.model.SecurityEvent
import io.wolftown.kaiku.domain.model.SecurityNotice
import io.wolftown.kaiku.ui.auth.setup.parseInstant
import io.wolftown.kaiku.ui.auth.setup.readyText
import io.wolftown.kaiku.ui.shared.KeyScreenSizes
import io.wolftown.kaiku.ui.shared.currentFragmentActivity

/**
 * L2-A12: one Kaiku notice with its usable actions ("That wasn't me", Cancel,
 * Remove device, "I will help"). Severity is an icon plus text, never colour
 * alone; there is no long-press menu and no reactions.
 */
@Composable
fun SecurityNoticeCard(notice: SecurityNotice, actions: NoticeActionsViewModel, modifier: Modifier = Modifier, ownUserId: String? = null) {
    val ui by actions.ui.collectAsState()
    val activity = currentFragmentActivity()
    // "Your account" only when it is known to be: an unknown reader never makes another
    // account's notice (a contact's, an admin's) look like one's own.
    val subject = if (ownUserId != null && notice.subject.userId == ownUserId) {
        stringResource(R.string.security_subject_yours)
    } else {
        notice.subject.displayName.ifEmpty { notice.subject.username }
    }
    val newest = notice.events.firstOrNull()
    Card(
        modifier = modifier.fillMaxWidth().padding(horizontal = 12.dp, vertical = 6.dp),
        colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surfaceVariant),
    ) {
        Column(Modifier.padding(16.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Icon(
                    if (notice.severity == "high") Icons.Filled.Warning else Icons.Filled.Lock,
                    contentDescription = null,
                    tint = MaterialTheme.colorScheme.onSurfaceVariant,
                )
                Spacer(Modifier.width(8.dp))
                Text(
                    stringResource(NoticeText.title(notice.eventType), subject, newest?.actor?.username.orEmpty(),
                        newest?.authenticator?.label.orEmpty(), notice.count),
                    fontSize = 18.sp,
                    fontWeight = FontWeight.SemiBold,
                    color = MaterialTheme.colorScheme.onSurface,
                )
            }
            newest?.let { EventLines(it) }
            newest?.actions?.filter { it.action != NoticeAction.UNKNOWN }?.forEach { a ->
                OutlinedButton(
                    onClick = { activity?.let { actions.run(it, a, newest, notice) } },
                    modifier = Modifier.fillMaxWidth().padding(top = 8.dp).heightIn(min = KeyScreenSizes.otherAction),
                ) {
                    Text(actionLabel(a, notice), fontSize = 18.sp, color = MaterialTheme.colorScheme.onSurface)
                }
            }
        }
    }
    // One set of dialogs for the notice whose action runs, not one per card on screen.
    if (actions.activeNoticeId == notice.noticeId) ActionDialogs(ui, actions)
}

@Composable
internal fun EventLines(e: SecurityEvent) {
    parseInstant(e.createdAt)?.let { Text(readyText(it), fontSize = 16.sp, color = MaterialTheme.colorScheme.onSurfaceVariant) }
    e.client?.label?.takeIf { it.isNotEmpty() }?.let {
        Text(stringResource(R.string.security_event_client, it), fontSize = 16.sp, color = MaterialTheme.colorScheme.onSurfaceVariant)
    }
    e.place?.takeIf { it.isNotEmpty() }?.let {
        Text(stringResource(R.string.security_event_place, it), fontSize = 16.sp, color = MaterialTheme.colorScheme.onSurfaceVariant)
    }
}

@Composable
private fun actionLabel(a: EventAction, notice: SecurityNotice): String = when (a.action) {
    NoticeAction.REPUDIATE -> stringResource(R.string.security_action_repudiate)
    NoticeAction.CANCEL_REQUEST -> stringResource(R.string.security_action_cancel_request)
    NoticeAction.REMOVE_DEVICE -> stringResource(R.string.security_action_remove_device)
    NoticeAction.CONFIRM_CONTACT -> stringResource(R.string.security_action_confirm_contact, notice.subject.displayName.ifEmpty { notice.subject.username })
    NoticeAction.UNKNOWN -> ""
}

/** Warning first, part 2 when asked, results and errors; all dismissible, nothing gesture-only. */
@Composable
internal fun ActionDialogs(ui: NoticeActionUi, actions: NoticeActionsViewModel) {
    val activity = currentFragmentActivity()
    when (ui) {
        is NoticeActionUi.Warning -> AlertDialog(
            onDismissRequest = actions::dismiss,
            text = { Text(stringResource(ui.msg), fontSize = 18.sp) },
            confirmButton = { TextButton(onClick = { activity?.let(actions::confirmWarning) }) { Text(stringResource(R.string.security_warning_continue)) } },
            dismissButton = { TextButton(onClick = actions::dismiss) { Text(stringResource(R.string.common_cancel)) } },
        )
        NoticeActionUi.NeedsPart2 -> Part2FactorDialog(
            onSubmit = { f -> activity?.let { actions.submitPart2(it, f) } },
            onDismiss = actions::dismiss,
        )
        is NoticeActionUi.Result -> AlertDialog(
            onDismissRequest = actions::dismiss,
            text = { Text(stringResource(ui.msg), fontSize = 18.sp) },
            confirmButton = {
                // B12: a cancel that came too late offers to remove the device the request added (with a step-up).
                val device = ui.removeDevice
                if (device != null) {
                    TextButton(
                        onClick = { activity?.let(actions::removeCompletedDevice) },
                        modifier = Modifier.heightIn(min = KeyScreenSizes.otherAction),
                    ) {
                        Text(stringResource(R.string.security_remove_this_device, device.label))
                    }
                } else {
                    TextButton(onClick = actions::dismiss) { Text(stringResource(R.string.common_ok)) }
                }
            },
            dismissButton = if (ui.removeDevice != null) {
                { TextButton(onClick = actions::dismiss) { Text(stringResource(R.string.common_ok)) } }
            } else {
                null
            },
        )
        is NoticeActionUi.Error -> AlertDialog(
            onDismissRequest = actions::dismiss,
            text = { Text(stringResource(ui.msg), fontSize = 18.sp) },
            confirmButton = { TextButton(onClick = actions::dismiss) { Text(stringResource(R.string.common_ok)) } },
        )
        NoticeActionUi.Idle, NoticeActionUi.Working -> Unit
    }
}
