package io.wolftown.kaiku.ui.auth.setup

import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
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
import androidx.compose.ui.unit.sp
import androidx.hilt.navigation.compose.hiltViewModel
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.compose.LifecycleEventEffect
import io.wolftown.kaiku.R
import io.wolftown.kaiku.ui.shared.KeyBody
import io.wolftown.kaiku.ui.shared.KeyNotice
import io.wolftown.kaiku.ui.shared.KeyPrimaryButton
import io.wolftown.kaiku.ui.shared.KeyScreen
import io.wolftown.kaiku.ui.shared.KeySecondaryButton
import io.wolftown.kaiku.ui.shared.KeyTitle
import io.wolftown.kaiku.ui.shared.SecureScreen
import io.wolftown.kaiku.ui.shared.currentFragmentActivity
import java.time.Instant
import java.time.ZoneId
import java.time.format.DateTimeFormatter
import java.time.format.FormatStyle

/**
 * L2-A1: setting up this phone from a setup code or link (F5a/F5c). Every step
 * is inside [SecureScreen]. Ends with a full device-key session, or with the
 * pending wait for an existing account: then (and when the screen comes back
 * after process death with a stored setup) it switches to [SetupPendingScreen],
 * which keeps checking at every resume and every 15 minutes. Restored without
 * its setup code and with nothing stored, it offers [onRescan].
 */
@Composable
fun SetupFlowScreen(
    entry: SetupEntry?,
    onSignedIn: () -> Unit,
    onBack: (() -> Unit)?,
    viewModel: SetupFlowViewModel = hiltViewModel(),
    onRescan: (() -> Unit)? = null,
) {
    val ui by viewModel.ui.collectAsState()

    LaunchedEffect(entry) {
        if (!viewModel.started) {
            if (entry != null) viewModel.start(entry) else viewModel.resumeWithoutEntry()
        }
    }
    val u0 = ui
    if (u0 is SetupUi.Pending || u0 == SetupUi.Resume) {
        // L2-A13: the wait (and any reconciliation) continues on the pending screen, also while
        // this screen stays open.
        SetupPendingScreen(onSignedIn = onSignedIn, onEnded = { onBack?.invoke() })
        return
    }
    SetupFlowSteps(ui = u0, viewModel = viewModel, onSignedIn = onSignedIn, onBack = onBack, onRescan = onRescan)
}

@Composable
private fun SetupFlowSteps(
    ui: SetupUi,
    viewModel: SetupFlowViewModel,
    onSignedIn: () -> Unit,
    onBack: (() -> Unit)?,
    onRescan: (() -> Unit)?,
) {
    SecureScreen()
    val activity = currentFragmentActivity()
    LifecycleEventEffect(Lifecycle.Event.ON_RESUME) { viewModel.recheckScreenLock() }
    LaunchedEffect(ui) {
        when (ui) {
            SetupUi.SigningIn -> activity?.let(viewModel::signIn)
            SetupUi.Done -> onSignedIn()
            else -> Unit
        }
    }

    KeyScreen {
        when (val u = ui) {
            SetupUi.Opening -> Progress(stringResource(R.string.approval_checking))
            is SetupUi.ConfirmServer -> {
                KeyTitle(stringResource(R.string.setup_title))
                KeyBody(stringResource(R.string.setup_confirm_server, u.serverName, u.host))
                KeyPrimaryButton(stringResource(R.string.setup_confirm_server_yes), onClick = viewModel::confirmServer)
                KeySecondaryButton(stringResource(R.string.common_cancel), onClick = { viewModel.cancel() })
            }
            is SetupUi.ScreenLockNeeded -> ScreenLockGuide(onCheck = viewModel::recheckScreenLock, onCancel = viewModel::cancel)
            is SetupUi.ConfirmSubject -> {
                KeyTitle(stringResource(R.string.setup_confirm_subject, u.displayName, u.serverName))
                KeyBody(stringResource(R.string.setup_confirm_subject_account, u.username), secondary = true)
                KeyPrimaryButton(stringResource(R.string.setup_confirm_subject_yes), onClick = { activity?.let(viewModel::confirmSubject) })
                KeySecondaryButton(stringResource(R.string.common_cancel), onClick = viewModel::cancel)
            }
            is SetupUi.ShowCode -> SetupCode(u.code)
            SetupUi.Interrupted -> {
                KeyNotice(stringResource(R.string.setup_interrupted), warning = true)
                if (onRescan != null) KeyPrimaryButton(stringResource(R.string.scan_setup_title), onClick = onRescan)
                if (onBack != null) KeySecondaryButton(stringResource(R.string.common_back), onClick = onBack)
            }
            else -> SetupStatus(
                ui = u,
                onSignIn = { activity?.let(viewModel::signIn) },
                onRetry = viewModel::retry,
                onBack = onBack,
            )
        }
    }
}

/** States shared with [SetupPendingScreen]. */
@Composable
internal fun SetupStatus(ui: SetupUi, onSignIn: () -> Unit, onRetry: (() -> Unit)?, onBack: (() -> Unit)?) {
    when (ui) {
        SetupUi.WaitingForCode -> {
            KeyTitle(stringResource(R.string.setup_waiting_title))
            CircularProgressIndicator()
        }
        is SetupUi.Pending -> {
            KeyTitle(stringResource(R.string.setup_pending_title))
            KeyBody(stringResource(R.string.setup_pending_body, readyText(ui.readyAt)))
            KeyNotice(stringResource(R.string.setup_pending_hint), warning = false)
        }
        SetupUi.SigningIn, SetupUi.Done -> Progress(stringResource(R.string.setup_signing_in))
        SetupUi.SignInPaused -> KeyPrimaryButton(stringResource(R.string.setup_sign_in_again), onClick = onSignIn)
        is SetupUi.Ended -> {
            KeyNotice(if (ui.arg != null) stringResource(ui.msg, ui.arg) else stringResource(ui.msg), warning = true)
            // An unclear answer or no connection is tried again; anything else starts the setup over.
            val retryLabel = when (ui.msg) {
                R.string.setup_claim_unsure, R.string.security_network_error, R.string.errors_rate_limited -> R.string.common_try_again
                else -> R.string.setup_start_again
            }
            if (ui.canRetry && onRetry != null) KeyPrimaryButton(stringResource(retryLabel), onClick = onRetry)
            if (onBack != null) KeySecondaryButton(stringResource(R.string.common_back), onClick = onBack)
        }
        else -> Unit
    }
}

@Composable
private fun SetupCode(code: String) {
    val description = stringResource(R.string.setup_code_description, code)
    KeyTitle(stringResource(R.string.setup_show_code_title))
    Text(
        text = code,
        fontSize = 44.sp,
        fontWeight = FontWeight.Bold,
        fontFamily = FontFamily.Monospace,
        textAlign = TextAlign.Center,
        color = MaterialTheme.colorScheme.onBackground,
        modifier = Modifier.fillMaxWidth().semantics { contentDescription = description },
    )
    KeyBody(stringResource(R.string.setup_show_code_body))
    CircularProgressIndicator()
}

@Composable
private fun Progress(text: String) {
    CircularProgressIndicator()
    KeyBody(text)
}

internal fun readyText(at: Instant): String =
    DateTimeFormatter.ofLocalizedDateTime(FormatStyle.MEDIUM, FormatStyle.SHORT).format(at.atZone(ZoneId.systemDefault()))
