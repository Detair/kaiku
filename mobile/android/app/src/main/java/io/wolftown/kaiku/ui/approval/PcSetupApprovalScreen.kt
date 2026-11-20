package io.wolftown.kaiku.ui.approval

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.hilt.navigation.compose.hiltViewModel
import io.wolftown.kaiku.R
import io.wolftown.kaiku.approval.PcSetupContext
import io.wolftown.kaiku.domain.ScanPayload
import io.wolftown.kaiku.ui.shared.KeyBody
import io.wolftown.kaiku.ui.shared.KeyNotice
import io.wolftown.kaiku.ui.shared.KeyPrimaryButton
import io.wolftown.kaiku.ui.shared.KeyScreen
import io.wolftown.kaiku.ui.shared.KeyScreenSizes
import io.wolftown.kaiku.ui.shared.KeySecondaryButton
import io.wolftown.kaiku.ui.shared.KeyTitle
import io.wolftown.kaiku.ui.shared.SecureScreen
import io.wolftown.kaiku.ui.shared.currentFragmentActivity
import java.time.Instant
import java.time.ZoneId
import java.time.format.DateTimeFormatter
import java.time.format.FormatStyle
import java.util.Locale

/**
 * "Set up this PC" on the key phone: what is granted, the client first, "Yes, continue", an equally
 * large "No" and a smaller "I didn't start this"; after the screen lock the six-digit code. The code
 * is not remembered across process death. Hardened with [SecureScreen].
 */
@Composable
fun PcSetupApprovalScreen(
    takePayload: () -> ScanPayload.PcSetup?,
    onRescan: () -> Unit,
    onDone: () -> Unit,
    viewModel: PcSetupApprovalViewModel = hiltViewModel(),
) {
    SecureScreen()
    val ui by viewModel.ui.collectAsState()
    val activity = currentFragmentActivity()

    LaunchedEffect(Unit) {
        if (!viewModel.started) {
            val payload = takePayload()
            if (payload != null) viewModel.start(payload) else onDone()
        }
    }
    // Back is always handled here: while a decision is being sent it is swallowed (leaving would
    // lose the code the answer brings back); on the code screen it asks first.
    BackHandler(enabled = true) {
        if (viewModel.requestBack()) {
            viewModel.back()
            onDone()
        }
    }
    val leaveAsked by viewModel.leaveAsked.collectAsState()
    if (leaveAsked) {
        AlertDialog(
            onDismissRequest = viewModel::stay,
            title = { Text(stringResource(R.string.pc_setup_leave_title)) },
            text = { Text(stringResource(R.string.pc_setup_leave_body)) },
            confirmButton = {
                TextButton(onClick = { viewModel.stay(); viewModel.back(); onDone() }) {
                    Text(stringResource(R.string.pc_setup_leave_yes))
                }
            },
            dismissButton = {
                TextButton(onClick = viewModel::stay) { Text(stringResource(R.string.pc_setup_leave_no)) }
            },
        )
    }

    KeyScreen {
        when (val u = ui) {
            PcSetupUi.Claiming, PcSetupUi.Working -> {
                CircularProgressIndicator()
                KeyBody(stringResource(R.string.approval_checking))
            }
            is PcSetupUi.Context -> ContextView(
                ctx = u.ctx,
                onYes = { activity?.let(viewModel::approve) },
                onNo = viewModel::deny,
                onReport = viewModel::askReport,
            )
            is PcSetupUi.ConfirmReport -> {
                KeyTitle(stringResource(R.string.pc_setup_end_session_title))
                KeyBody(stringResource(R.string.pc_setup_end_session_body))
                KeyPrimaryButton(stringResource(R.string.pc_setup_end_session_yes), onClick = { activity?.let(viewModel::confirmReport) })
                KeyPrimaryButton(stringResource(R.string.pc_setup_end_session_no), onClick = viewModel::cancelReport)
            }
            is PcSetupUi.Code -> CodeView(u.code)
            PcSetupUi.Entered -> {
                CircularProgressIndicator()
                KeyBody(stringResource(R.string.pc_setup_entered))
            }
            is PcSetupUi.Finished -> {
                KeyNotice(stringResource(u.msg), warning = !u.ok)
                KeyPrimaryButton(stringResource(R.string.common_ok), onClick = onDone)
            }
            is PcSetupUi.Error -> {
                KeyNotice(stringResource(u.msg), warning = true)
                if (u.rescan) KeyPrimaryButton(stringResource(R.string.scan_again), onClick = { viewModel.back(); onRescan() })
                KeySecondaryButton(stringResource(R.string.common_back), onClick = { viewModel.back(); onDone() })
            }
        }
    }
}

@Composable
private fun ContextView(ctx: PcSetupContext, onYes: () -> Unit, onNo: () -> Unit, onReport: () -> Unit) {
    KeyTitle(stringResource(R.string.pc_setup_question))
    Card(
        modifier = Modifier.fillMaxWidth(),
        colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surfaceVariant),
    ) {
        Column(Modifier.padding(16.dp)) {
            Text(
                text = ctx.clientDescription,
                fontSize = 22.sp,
                fontWeight = FontWeight.SemiBold,
                color = MaterialTheme.colorScheme.onSurface,
            )
            Field(stringResource(R.string.pc_setup_field_signed_in), signedInText(ctx))
            if (ctx.place.isNotBlank()) Field(stringResource(R.string.approval_field_place), ctx.place)
            Text(
                stringResource(R.string.pc_setup_effect),
                fontSize = 18.sp,
                color = MaterialTheme.colorScheme.onSurface,
                modifier = Modifier.padding(top = 12.dp),
            )
            Text(
                stringResource(R.string.pc_setup_only_if),
                fontSize = 18.sp,
                fontWeight = FontWeight.SemiBold,
                color = MaterialTheme.colorScheme.onSurface,
                modifier = Modifier.padding(top = 12.dp),
            )
        }
    }
    KeyPrimaryButton(stringResource(R.string.pc_setup_yes), onClick = onYes)
    // "No" is as large and as filled as "Yes".
    KeyPrimaryButton(stringResource(R.string.approval_no), onClick = onNo)
    TextButton(onClick = onReport, modifier = Modifier.fillMaxWidth().heightIn(min = KeyScreenSizes.secondaryButton)) {
        Text(stringResource(R.string.approval_not_me), fontSize = 18.sp, color = MaterialTheme.colorScheme.onBackground)
    }
}

/** Size and contrast as the setup code screen; the digits are read out one by one. */
@Composable
private fun CodeView(code: String) {
    val description = stringResource(R.string.pc_setup_code_description, code.toList().joinToString(" "))
    KeyTitle(stringResource(R.string.pc_setup_code_title))
    Text(
        text = code,
        fontSize = 44.sp,
        fontWeight = FontWeight.Bold,
        fontFamily = FontFamily.Monospace,
        textAlign = TextAlign.Center,
        color = MaterialTheme.colorScheme.onBackground,
        modifier = Modifier.fillMaxWidth().semantics { contentDescription = description },
    )
    KeyNotice(stringResource(R.string.pc_setup_code_warning), warning = true)
    CircularProgressIndicator()
}

@Composable
private fun Field(label: String, value: String) {
    Column(Modifier.padding(top = 12.dp)) {
        Text(label, fontSize = 16.sp, color = MaterialTheme.colorScheme.onSurfaceVariant)
        Text(value, fontSize = 18.sp, color = MaterialTheme.colorScheme.onSurface)
    }
}

/** When the PC's session began (signed Unix seconds) in the phone's zone and locale. */
internal fun signedInText(ctx: PcSetupContext, zone: ZoneId = ZoneId.systemDefault(), locale: Locale = Locale.getDefault()): String =
    DateTimeFormatter.ofLocalizedDateTime(FormatStyle.MEDIUM, FormatStyle.SHORT)
        .withLocale(locale)
        .format(Instant.ofEpochSecond(ctx.sessionCreatedAt).atZone(zone))
