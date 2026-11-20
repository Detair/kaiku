package io.wolftown.kaiku.ui.auth.setup

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
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
    /** A setup link waiting to be delivered (memory only); handed to the setup once, then [onLinkTaken]. */
    link: SetupEntry?,
    onSignedIn: () -> Unit,
    onBack: (() -> Unit)?,
    viewModel: SetupFlowViewModel = hiltViewModel(),
    onRescan: (() -> Unit)? = null,
    onLinkTaken: (SetupEntry) -> Unit = {},
) {
    val ui by viewModel.ui.collectAsState()
    val linkIgnored by viewModel.linkIgnored.collectAsState()
    val waiting by viewModel.waiting.collectAsState()
    val cannotAsk by viewModel.cannotAsk.collectAsState()
    val discardable by viewModel.discardable.collectAsState()

    // Taken only once composed, so a discarded composition never loses the memory-only code.
    LaunchedEffect(link) {
        deliverSetupLink(link, viewModel.started, viewModel::start, onLinkTaken, viewModel::resumeWithoutEntry)
    }
    val u0 = ui
    if (waiting) {
        // L2-A13: the wait (and any reconciliation) continues on the pending screen, also while
        // this screen stays open.
        SetupPendingScreen(onSignedIn = onSignedIn, onEnded = { onBack?.invoke() })
        return
    }
    SetupFlowSteps(
        ui = u0, viewModel = viewModel, linkIgnored = linkIgnored, cannotAsk = cannotAsk, discardable = discardable,
        onSignedIn = onSignedIn, onBack = onBack, onRescan = onRescan,
    )
}

@Composable
private fun SetupFlowSteps(
    ui: SetupUi,
    viewModel: SetupFlowViewModel,
    linkIgnored: Boolean,
    cannotAsk: Boolean,
    discardable: Boolean,
    onSignedIn: () -> Unit,
    onBack: (() -> Unit)?,
    onRescan: (() -> Unit)?,
) {
    SecureScreen()
    val activity = currentFragmentActivity()
    LifecycleEventEffect(Lifecycle.Event.ON_RESUME) { viewModel.recheckScreenLock() }
    // Back never drops the setup silently. A question or the code asks first, the progress before
    // a question is cancelled as with its button, the claim on its way, a running prompt or sign-in
    // keep the screen; ended steps leave as usual.
    var askLeave by remember { mutableStateOf(false) }
    val back = setupBackAction(ui)
    BackHandler(enabled = back != SetupBack.LEAVE) {
        when (back) {
            SetupBack.CANCEL -> viewModel.cancel()
            SetupBack.ASK -> askLeave = true
            SetupBack.STAY, SetupBack.LEAVE -> Unit
        }
    }
    if (askLeave && back == SetupBack.ASK) {
        SetupLeaveDialog(
            atCode = ui is SetupUi.ShowCode || ui == SetupUi.WaitingForCode,
            onStay = { askLeave = false },
            onStop = {
                askLeave = false
                viewModel.cancel()
            },
        )
    }
    LaunchedEffect(ui) {
        when (ui) {
            SetupUi.SigningIn -> activity?.let(viewModel::signIn)
            SetupUi.Done -> onSignedIn()
            else -> Unit
        }
    }

    KeyScreen {
        if (linkIgnored) KeyNotice(stringResource(R.string.setup_link_ignored), warning = false)
        when (val u = ui) {
            SetupUi.Opening, SetupUi.Claiming -> Progress(stringResource(R.string.approval_checking))
            is SetupUi.ConfirmServer -> {
                KeyTitle(stringResource(R.string.setup_title))
                KeyBody(stringResource(R.string.setup_confirm_server, u.serverName, u.host))
                KeyPrimaryButton(stringResource(R.string.setup_confirm_server_yes), onClick = viewModel::confirmServer)
                KeySecondaryButton(stringResource(R.string.common_cancel), onClick = { viewModel.cancel() })
            }
            is SetupUi.ScreenLockNeeded -> ScreenLockGuide(onCheck = viewModel::recheckScreenLock, onCancel = viewModel::cancel)
            is SetupUi.ConfirmSubject -> {
                // The signed account name and the pinned server first; unsigned names only as a hint.
                KeyTitle(stringResource(R.string.setup_confirm_subject, u.username, u.host))
                KeyBody(
                    stringResource(R.string.setup_confirm_subject_shown, u.displayName.ifEmpty { u.username }, u.serverName),
                    secondary = true,
                )
                KeyPrimaryButton(stringResource(R.string.setup_confirm_subject_yes), onClick = { activity?.let(viewModel::confirmSubject) })
                KeySecondaryButton(stringResource(R.string.common_cancel), onClick = viewModel::cancel)
            }
            is SetupUi.ShowCode -> SetupCode(u.code)
            SetupUi.ClaimWaiting -> {
                KeyNotice(stringResource(R.string.setup_claim_waiting), warning = false)
                KeyPrimaryButton(stringResource(R.string.common_try_again), onClick = viewModel::retry)
                if (onBack != null) KeySecondaryButton(stringResource(R.string.common_back), onClick = onBack)
                KeyBody(stringResource(R.string.security_clear_storage_hint), secondary = true)
            }
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
        UnfinishedSetupHelp(cannotAsk = cannotAsk, discardable = discardable, onDiscard = viewModel::discard)
    }
}

/**
 * Under a stored setup whose server could not be asked: the last resort in plain words, and, once
 * the setup's own time is long past, "Discard this unfinished setup" behind a question that says
 * what it means.
 */
@Composable
internal fun UnfinishedSetupHelp(cannotAsk: Boolean, discardable: Boolean, onDiscard: () -> Unit) {
    if (!cannotAsk) return
    var ask by remember { mutableStateOf(false) }
    KeyBody(stringResource(R.string.setup_cannot_ask_hint), secondary = true)
    if (discardable) KeySecondaryButton(stringResource(R.string.setup_discard), onClick = { ask = true })
    KeyBody(stringResource(R.string.security_clear_storage_hint), secondary = true)
    if (ask && discardable) {
        AlertDialog(
            onDismissRequest = { ask = false },
            title = { Text(stringResource(R.string.setup_discard_title)) },
            text = { Text(stringResource(R.string.setup_discard_body)) },
            confirmButton = {
                TextButton(onClick = {
                    ask = false
                    onDiscard()
                }) { Text(stringResource(R.string.setup_discard_confirm)) }
            },
            dismissButton = {
                TextButton(onClick = { ask = false }) { Text(stringResource(R.string.setup_discard_keep)) }
            },
        )
    }
}

/**
 * What the setup screen does with the link waiting for it, each time that link changes: a link
 * is handed to the setup once and then marked taken (one delivery, one start; the setup itself
 * decides whether it is used). Without a link, a screen that was handed nothing looks at what the
 * phone has stored.
 */
internal fun deliverSetupLink(
    link: SetupEntry?,
    started: Boolean,
    start: (SetupEntry) -> Unit,
    taken: (SetupEntry) -> Unit,
    resumeWithoutEntry: () -> Unit,
) {
    if (link != null) {
        start(link)
        taken(link)
    } else if (!started) {
        resumeWithoutEntry()
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
        SetupUi.SignInPaused -> {
            // The button alone did not say what is left to do.
            KeyBody(stringResource(R.string.setup_sign_in_paused))
            KeyPrimaryButton(stringResource(R.string.setup_sign_in_again), onClick = onSignIn)
        }
        SetupUi.SignInTimedOut -> {
            KeyNotice(stringResource(R.string.security_took_too_long), warning = true)
            KeyPrimaryButton(stringResource(R.string.setup_sign_in_again), onClick = onSignIn)
        }
        is SetupUi.Ended -> {
            KeyNotice(if (ui.arg != null) stringResource(ui.msg, ui.arg) else stringResource(ui.msg), warning = true)
            // An unclear answer or no connection is tried again; anything else starts the setup over.
            val retryLabel = when (ui.msg) {
                R.string.setup_claim_unsure, R.string.security_network_error, R.string.errors_rate_limited -> R.string.common_try_again
                R.string.setup_stopped_after_code -> R.string.setup_leave_stay
                else -> R.string.setup_start_again
            }
            if (ui.canRetry && onRetry != null) KeyPrimaryButton(stringResource(retryLabel), onClick = onRetry)
            // Back leads to the sign-in, which for a key phone is "Confirm it's you" (its screen lock).
            if (ui.offerSignIn && onBack != null) {
                KeyPrimaryButton(stringResource(R.string.setup_device_in_use_sign_in), onClick = onBack)
            } else if (onBack != null) {
                KeySecondaryButton(stringResource(R.string.common_back), onClick = onBack)
            }
        }
        else -> Unit
    }
}

/**
 * "Stop setting up this phone?" for Back on a live setup step, shared by the setup and the
 * waiting screen. [atCode]: the code is shown or awaited (otherwise a question is open).
 */
@Composable
internal fun SetupLeaveDialog(atCode: Boolean, onStay: () -> Unit, onStop: () -> Unit) {
    AlertDialog(
        onDismissRequest = onStay,
        title = { Text(stringResource(R.string.setup_leave_title)) },
        text = { Text(stringResource(if (atCode) R.string.setup_leave_body else R.string.setup_leave_body_question)) },
        confirmButton = {
            TextButton(onClick = onStay) {
                Text(stringResource(if (atCode) R.string.setup_leave_stay else R.string.setup_leave_continue))
            }
        },
        dismissButton = {
            TextButton(onClick = onStop) { Text(stringResource(R.string.setup_leave_stop)) }
        },
    )
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
