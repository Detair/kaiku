package io.wolftown.kaiku.ui.approval

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
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
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.hilt.navigation.compose.hiltViewModel
import io.wolftown.kaiku.R
import io.wolftown.kaiku.approval.LoginContext
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

/**
 * The F2/F3 context screen on the key phone. Shows only the signed context,
 * client first, then asks "Are you signing in on a PC in front of you right
 * now?" with "Yes, sign in", an equally large "No" and a smaller "I didn't
 * start this". Hardened with [SecureScreen].
 */
@Composable
fun LoginApprovalScreen(
    takePayload: () -> ScanPayload.Login?,
    onRescan: () -> Unit,
    onDone: () -> Unit,
    viewModel: LoginApprovalViewModel = hiltViewModel(),
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
    BackHandler {
        viewModel.back()
        onDone()
    }

    KeyScreen {
        when (val u = ui) {
            LoginApprovalUi.Claiming, LoginApprovalUi.Working -> {
                CircularProgressIndicator()
                KeyBody(stringResource(R.string.approval_checking))
            }
            is LoginApprovalUi.Context -> ContextView(
                ctx = u.ctx,
                onYes = { activity?.let(viewModel::approve) },
                onNo = viewModel::deny,
                onReport = { activity?.let(viewModel::report) },
            )
            is LoginApprovalUi.Done -> {
                KeyNotice(stringResource(u.msg), warning = u.msg != R.string.approval_done)
                KeyPrimaryButton(stringResource(R.string.common_ok), onClick = onDone)
            }
            is LoginApprovalUi.Error -> {
                KeyNotice(stringResource(u.msg), warning = true)
                if (u.rescan) KeyPrimaryButton(stringResource(R.string.scan_again), onClick = onRescan)
                KeySecondaryButton(stringResource(R.string.common_back), onClick = onDone)
            }
        }
    }
}

@Composable
private fun ContextView(ctx: LoginContext, onYes: () -> Unit, onNo: () -> Unit, onReport: () -> Unit) {
    KeyTitle(stringResource(R.string.approval_question))
    Card(
        modifier = Modifier.fillMaxWidth(),
        colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surfaceVariant),
    ) {
        Column(Modifier.padding(16.dp)) {
            Text(
                text = if (ctx.clientLabel.isNotEmpty()) stringResource(clientLine(ctx), ctx.clientLabel) else stringResource(clientLine(ctx)),
                fontSize = 22.sp,
                fontWeight = FontWeight.SemiBold,
                color = MaterialTheme.colorScheme.onSurface,
            )
            Field(stringResource(R.string.approval_field_server), ctx.serverName)
            Field(stringResource(R.string.approval_field_account), ctx.username)
            Field(stringResource(R.string.approval_field_time), createdAtText(ctx))
            if (showPlace(ctx)) Field(stringResource(R.string.approval_field_place), ctx.place)
            variantLine(ctx)?.let {
                Text(it.let { r -> stringResource(r) }, fontSize = 18.sp, color = MaterialTheme.colorScheme.onSurface,
                    modifier = Modifier.padding(top = 12.dp))
            }
        }
    }
    KeyPrimaryButton(stringResource(R.string.approval_yes), onClick = onYes)
    // "No" is as large and as filled as "Yes" (F2: prominent).
    KeyPrimaryButton(stringResource(R.string.approval_no), onClick = onNo)
    TextButton(onClick = onReport, modifier = Modifier.fillMaxWidth().heightIn(min = KeyScreenSizes.secondaryButton)) {
        Text(stringResource(R.string.approval_not_me), fontSize = 18.sp, color = MaterialTheme.colorScheme.onBackground)
    }
}

@Composable
private fun Field(label: String, value: String) {
    Column(Modifier.padding(top = 12.dp)) {
        Text(label, fontSize = 16.sp, color = MaterialTheme.colorScheme.onSurfaceVariant)
        Text(value, fontSize = 18.sp, color = MaterialTheme.colorScheme.onSurface)
    }
}
