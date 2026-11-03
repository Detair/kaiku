package io.wolftown.kaiku.ui.approval

import android.content.Intent
import androidx.activity.compose.BackHandler
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.hilt.navigation.compose.hiltViewModel
import io.wolftown.kaiku.R
import io.wolftown.kaiku.ui.shared.CodeKeypad
import io.wolftown.kaiku.ui.shared.KeyBody
import io.wolftown.kaiku.ui.shared.KeyNotice
import io.wolftown.kaiku.ui.shared.KeyPrimaryButton
import io.wolftown.kaiku.ui.shared.KeyScreen
import io.wolftown.kaiku.ui.shared.KeySecondaryButton
import io.wolftown.kaiku.ui.shared.KeyTitle
import io.wolftown.kaiku.ui.shared.QrImage
import io.wolftown.kaiku.ui.shared.SecureScreen
import io.wolftown.kaiku.ui.shared.currentFragmentActivity
import java.time.Duration
import java.time.Instant

/**
 * "Add a phone" (F5a) and, with [mode] admin, "Set up a phone" (F5c): the setup
 * code as QR and link, then the keypad for the 6 digits the new phone shows.
 */
@Composable
fun AddPhoneScreen(
    mode: IssuerMode,
    onDone: () -> Unit,
    keypadHint: Int = R.string.issuer_type_code,
    viewModel: EnrollmentIssuerViewModel = hiltViewModel(),
) {
    SecureScreen()
    val ui by viewModel.ui.collectAsState()
    val activity = currentFragmentActivity()
    val context = LocalContext.current

    LaunchedEffect(Unit) {
        if (viewModel.ui.value == IssuerUi.Starting) activity?.let { viewModel.start(it, mode) }
    }
    BackHandler {
        if (ui !is IssuerUi.Done && ui !is IssuerUi.Ended) viewModel.cancel()
        onDone()
    }

    KeyScreen {
        KeyTitle(stringResource(R.string.issuer_title))
        when (val u = ui) {
            IssuerUi.Starting, IssuerUi.Confirming -> CircularProgressIndicator()
            is IssuerUi.ShowSetup -> {
                KeyBody(stringResource(R.string.issuer_show_setup))
                QrImage(u.setupUrl, stringResource(R.string.issuer_qr_description), Modifier.fillMaxWidth())
                u.expiresAt?.let {
                    val minutes = Duration.between(Instant.now(), it).toMinutes().coerceAtLeast(0)
                    KeyBody(stringResource(R.string.issuer_minutes_left, minutes.toInt()), secondary = true)
                }
                KeySecondaryButton(stringResource(R.string.issuer_share_link), onClick = {
                    val send = Intent(Intent.ACTION_SEND).setType("text/plain").putExtra(Intent.EXTRA_TEXT, u.setupUrl)
                    context.startActivity(Intent.createChooser(send, context.getString(R.string.issuer_share_chooser)))
                })
                KeyBody(stringResource(R.string.issuer_waiting), secondary = true)
                KeySecondaryButton(stringResource(R.string.common_cancel), onClick = { viewModel.cancel() })
            }
            is IssuerUi.TypeCode -> {
                KeyBody(stringResource(R.string.issuer_device_wants, u.label))
                KeyNotice(stringResource(keypadHint), warning = true)
                if (u.wrong) KeyNotice(stringResource(R.string.issuer_wrong_code, u.triesLeft), warning = true)
                CodeKeypad(
                    value = u.digits,
                    onValueChange = viewModel::onDigits,
                    onConfirm = { activity?.let(viewModel::confirmCode) },
                )
                KeySecondaryButton(stringResource(R.string.common_cancel), onClick = { viewModel.cancel() })
            }
            is IssuerUi.Done -> {
                KeyNotice(
                    if (u.pending != null) stringResource(R.string.admin_setup_done_existing, io.wolftown.kaiku.ui.auth.setup.readyText(u.pending))
                    else stringResource(if (mode is IssuerMode.Admin) R.string.admin_setup_done_new else R.string.issuer_done, u.label),
                    warning = false,
                )
                KeyPrimaryButton(stringResource(R.string.common_ok), onClick = onDone)
            }
            is IssuerUi.Ended -> {
                KeyNotice(stringResource(u.msg), warning = true)
                KeyPrimaryButton(stringResource(R.string.common_back), onClick = onDone)
            }
        }
    }
}
