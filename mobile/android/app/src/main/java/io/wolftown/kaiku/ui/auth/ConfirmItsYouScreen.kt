package io.wolftown.kaiku.ui.auth

import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.height
import androidx.compose.material3.AlertDialog
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
import androidx.compose.ui.unit.dp
import androidx.hilt.navigation.compose.hiltViewModel
import io.wolftown.kaiku.R
import io.wolftown.kaiku.ui.shared.KeyBody
import io.wolftown.kaiku.ui.shared.KeyNotice
import io.wolftown.kaiku.ui.shared.KeyPrimaryButton
import io.wolftown.kaiku.ui.shared.KeyScreen
import io.wolftown.kaiku.ui.shared.KeySecondaryButton
import io.wolftown.kaiku.ui.shared.KeyTitle
import io.wolftown.kaiku.ui.shared.currentFragmentActivity

/**
 * L2-A11 "Confirm it's you": one large button that signs in with the phone's
 * screen lock, the account name, and "Use another account" (which removes this
 * phone as the key first). [renewal] is the B.4 prompt before the 90-day cap.
 */
@Composable
fun ConfirmItsYouScreen(
    onSignedIn: () -> Unit,
    onSignedOut: () -> Unit,
    renewal: Boolean = false,
    onLater: (() -> Unit)? = null,
    viewModel: ConfirmItsYouViewModel = hiltViewModel(),
) {
    val ui by viewModel.ui.collectAsState()
    val activity = currentFragmentActivity()
    var askRemove by remember { mutableStateOf(false) }

    LaunchedEffect(ui.done) {
        when (ui.done) {
            ConfirmDone.SIGNED_IN -> onSignedIn()
            ConfirmDone.SIGNED_OUT, ConfirmDone.PENDING -> onSignedOut()
            null -> Unit
        }
    }

    KeyScreen {
        KeyTitle(stringResource(if (renewal) R.string.security_confirm_renew else R.string.security_uv_title))
        ui.accountName?.let { KeyBody(stringResource(R.string.security_confirm_account, it)) }
        KeyBody(stringResource(R.string.security_confirm_body), secondary = true)
        ui.notice?.let { KeyNotice(stringResource(it), warning = true) }
        ui.error?.let { KeyNotice(stringResource(it), warning = true) }
        Spacer(Modifier.height(8.dp))
        if (ui.keyGone) {
            KeyPrimaryButton(stringResource(R.string.common_ok), onClick = viewModel::acknowledgeKeyGone)
        } else {
            KeyPrimaryButton(
                text = stringResource(R.string.security_confirm_button),
                onClick = { activity?.let(viewModel::confirm) },
                working = ui.working,
            )
            if (renewal && onLater != null) {
                KeySecondaryButton(stringResource(R.string.security_confirm_later), onClick = onLater, enabled = !ui.working)
            } else {
                KeySecondaryButton(stringResource(R.string.security_confirm_other_account), onClick = { askRemove = true }, enabled = !ui.working)
            }
        }
    }

    if (askRemove) {
        AlertDialog(
            onDismissRequest = { askRemove = false },
            title = { Text(stringResource(R.string.security_confirm_other_account)) },
            text = { Text(stringResource(R.string.security_confirm_other_account_explain)) },
            confirmButton = {
                TextButton(onClick = {
                    askRemove = false
                    activity?.let(viewModel::removeThisPhone)
                }) { Text(stringResource(R.string.security_sign_out_remove_phone)) }
            },
            dismissButton = {
                TextButton(onClick = { askRemove = false }) { Text(stringResource(R.string.common_cancel)) }
            },
        )
    }
}
