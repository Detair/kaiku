package io.wolftown.kaiku.ui.security

import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.res.stringResource
import androidx.hilt.navigation.compose.hiltViewModel
import io.wolftown.kaiku.R
import io.wolftown.kaiku.ui.auth.setup.parseInstant
import io.wolftown.kaiku.ui.auth.setup.readyText
import io.wolftown.kaiku.ui.shared.KeyBody
import io.wolftown.kaiku.ui.shared.KeyNotice
import io.wolftown.kaiku.ui.shared.KeyPrimaryButton
import io.wolftown.kaiku.ui.shared.KeyScreen
import io.wolftown.kaiku.ui.shared.KeySecondaryButton
import io.wolftown.kaiku.ui.shared.KeyTitle
import io.wolftown.kaiku.ui.shared.currentFragmentActivity

/** L2-A10: the full-screen open-request prompt at app open, with a 64 dp "Cancel this request". */
@Composable
fun OpenRequestPrompt(onDone: () -> Unit, viewModel: OpenRequestViewModel = hiltViewModel()) {
    val pending by viewModel.pending.collectAsState()
    val message by viewModel.message.collectAsState()
    val working by viewModel.working.collectAsState()
    val activity = currentFragmentActivity()
    var askPart2 by remember { mutableStateOf(false) }
    LaunchedEffect(pending) { if (pending == null && !working) onDone() }
    val r = pending ?: return
    KeyScreen {
        val admin = r.startedBy
        KeyTitle(
            if (admin != null) stringResource(R.string.security_open_request_admin, admin.username)
            else stringResource(R.string.security_open_request_title),
        )
        parseInstant(r.readyAt)?.let { KeyBody(stringResource(R.string.security_open_request_ready_at, readyText(it))) }
        if (r.lostDevices.isNotEmpty()) {
            KeyBody(stringResource(R.string.security_open_request_lost, r.lostDevices.joinToString { it.label }))
        }
        message?.let { KeyNotice(stringResource(it), warning = true) }
        KeyPrimaryButton(
            stringResource(R.string.security_open_request_cancel),
            onClick = { if (r.cancel?.via == "part2") askPart2 = true else activity?.let { viewModel.cancel(it) } },
            working = working,
        )
        KeySecondaryButton(stringResource(R.string.security_open_request_ok), onClick = viewModel::dismiss, enabled = !working)
    }
    if (askPart2) {
        Part2FactorDialog(
            onSubmit = { f ->
                askPart2 = false
                activity?.let { viewModel.cancel(it, f) }
            },
            onDismiss = { askPart2 = false },
        )
    }
}
